use std::env;
#[cfg(feature = "native-runtime")]
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(feature = "native-runtime")]
use std::time::Duration;

#[cfg(feature = "native-runtime")]
use std::collections::HashMap;
#[cfg(feature = "native-runtime")]
use std::process::Command;
#[cfg(feature = "native-runtime")]
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

#[cfg(feature = "native-runtime")]
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
#[cfg(feature = "native-runtime")]
use tauri::{AppHandle, Emitter, Manager, State};
#[cfg(feature = "native-runtime")]
use tokio::sync::{Mutex as AsyncMutex, mpsc, watch};
#[cfg(feature = "native-runtime")]
use tokio::time::timeout;
#[cfg(feature = "native-runtime")]
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
#[cfg(feature = "native-runtime")]
use tokio_tungstenite::tungstenite::protocol::Message;

#[cfg(feature = "native-runtime")]
const GATEWAY_BRIDGE_QUEUE_CAPACITY: usize = 128;

type DesktopError = Box<dyn std::error::Error + Send + Sync>;

#[cfg(feature = "native-runtime")]
pub(crate) struct GatewayBridge {
    entries: Mutex<HashMap<String, GatewayBridgeEntry>>,
    next_generation: AtomicU64,
}

#[cfg(feature = "native-runtime")]
struct GatewayBridgeEntry {
    sender: mpsc::Sender<String>,
    cancel: watch::Sender<bool>,
    owner_window: String,
    generation: u64,
}

#[cfg(feature = "native-runtime")]
impl Default for GatewayBridge {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(1),
        }
    }
}

#[cfg(feature = "native-runtime")]
#[derive(Default)]
pub(crate) struct ManagedGatewayResolver {
    managed: AsyncMutex<Option<ManagedGateway>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedGateway {
    base_url: String,
    token: String,
}

#[cfg(feature = "native-runtime")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GatewayStartOutput {
    base_url: String,
    ok: bool,
}

#[cfg(feature = "native-runtime")]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewayEndpoint {
    http_base: String,
    ws_url: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadSessionRequest {
    filename: Option<String>,
    format: Option<String>,
    include: Option<Vec<String>>,
    kind: String,
    thread_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadSessionResult {
    content: Vec<u8>,
    content_type: String,
    filename: String,
}

#[cfg(feature = "native-runtime")]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayBridgeEvent {
    connection_id: String,
    generation: u64,
    message: String,
}

#[cfg(feature = "native-runtime")]
#[tauri::command]
pub(crate) async fn gateway_connect(
    app: AppHandle,
    resolver: State<'_, ManagedGatewayResolver>,
    state: State<'_, GatewayBridge>,
    connection_id: String,
    owner_window: String,
) -> Result<u64, String> {
    let managed = resolve_managed_gateway(resolver.inner())
        .await
        .map_err(|err| err.to_string())?;
    let request = websocket_request(&managed).map_err(|err| err.to_string())?;
    let (socket, _) = connect_async(request)
        .await
        .map_err(|err| err.to_string())?;
    let (mut write, mut read) = socket.split();
    let (sender, mut receiver) = mpsc::channel::<String>(GATEWAY_BRIDGE_QUEUE_CAPACITY);
    let (cancel, mut writer_cancel) = watch::channel(false);
    let mut reader_cancel = cancel.subscribe();
    let generation = state.next_generation.fetch_add(1, Ordering::Relaxed);
    let replaced = state
        .entries
        .lock()
        .map_err(|_| "Gateway bridge lock poisoned".to_string())?
        .insert(
            connection_id.clone(),
            GatewayBridgeEntry {
                sender,
                cancel,
                owner_window,
                generation,
            },
        );
    if let Some(replaced) = replaced {
        let _ = replaced.cancel.send(true);
    }
    #[cfg(feature = "wdio-test")]
    crate::startup_trace::record_bridge_connected(&connection_id);

    let writer_app = app.clone();
    let writer_connection_id = connection_id.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                changed = writer_cancel.changed() => {
                    if changed.is_err() || *writer_cancel.borrow() {
                        return;
                    }
                }
                message = receiver.recv() => {
                    let Some(message) = message else {
                        return;
                    };
                    if let Err(error) = write.send(Message::Text(message.into())).await {
                        if remove_bridge_generation(
                            &writer_app,
                            &writer_connection_id,
                            generation,
                        ) {
                            let _ = writer_app.emit(
                                "gateway-disconnect",
                                GatewayBridgeEvent {
                                    connection_id: writer_connection_id,
                                    generation,
                                    message: error.to_string(),
                                },
                            );
                        }
                        return;
                    }
                }
            }
        }
    });

    let read_connection_id = connection_id.clone();
    tauri::async_runtime::spawn(async move {
        let disconnect_message = loop {
            let message = tokio::select! {
                changed = reader_cancel.changed() => {
                    if changed.is_err() || *reader_cancel.borrow() {
                        return;
                    }
                    continue;
                }
                message = read.next() => message,
            };
            let Some(message) = message else {
                break "Gateway WebSocket closed".to_string();
            };
            match message {
                Ok(Message::Text(text)) => {
                    let _ = app.emit(
                        "gateway-message",
                        GatewayBridgeEvent {
                            connection_id: read_connection_id.clone(),
                            generation,
                            message: text.to_string(),
                        },
                    );
                }
                Ok(Message::Close(_)) => break "Gateway WebSocket closed".to_string(),
                Ok(_) => {}
                Err(err) => break err.to_string(),
            }
        };
        if remove_bridge_generation(&app, &read_connection_id, generation) {
            let _ = app.emit(
                "gateway-disconnect",
                GatewayBridgeEvent {
                    connection_id: read_connection_id,
                    generation,
                    message: disconnect_message,
                },
            );
        }
    });

    Ok(generation)
}

#[cfg(feature = "native-runtime")]
#[tauri::command]
pub(crate) async fn gateway_send(
    state: State<'_, GatewayBridge>,
    connection_id: String,
    generation: u64,
    message: String,
) -> Result<(), String> {
    let guard = state
        .entries
        .lock()
        .map_err(|_| "Gateway bridge lock poisoned".to_string())?;
    let entry = guard
        .get(&connection_id)
        .ok_or_else(|| "Gateway bridge is not connected".to_string())?;
    if entry.generation != generation {
        return Err("Gateway bridge generation is stale".to_string());
    }
    let sender = entry.sender.clone();
    drop(guard);
    match sender.try_send(message) {
        Ok(()) => Ok(()),
        Err(mpsc::error::TrySendError::Full(_)) => {
            let _ = remove_bridge_entry(state.inner(), &connection_id, Some(generation));
            Err("Gateway bridge sender is overloaded".to_string())
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            let _ = remove_bridge_entry(state.inner(), &connection_id, Some(generation));
            Err("Gateway bridge is closed".to_string())
        }
    }
}

#[cfg(feature = "native-runtime")]
#[tauri::command]
pub(crate) async fn gateway_disconnect(
    state: State<'_, GatewayBridge>,
    connection_id: String,
    generation: u64,
) -> Result<(), String> {
    remove_bridge_entry(state.inner(), &connection_id, Some(generation))
        .map_err(|_| "Gateway bridge lock poisoned".to_string())?;
    Ok(())
}

#[cfg(feature = "native-runtime")]
fn remove_bridge_entry(
    state: &GatewayBridge,
    connection_id: &str,
    generation: Option<u64>,
) -> Result<bool, ()> {
    let mut entries = state.entries.lock().map_err(|_| ())?;
    if let Some(expected) = generation
        && entries
            .get(connection_id)
            .is_some_and(|entry| entry.generation != expected)
    {
        return Ok(false);
    }
    let Some(entry) = entries.remove(connection_id) else {
        return Ok(false);
    };
    let _ = entry.cancel.send(true);
    Ok(true)
}

#[cfg(feature = "native-runtime")]
fn remove_bridge_generation(app: &AppHandle, connection_id: &str, generation: u64) -> bool {
    let state = app.state::<GatewayBridge>();
    remove_bridge_entry(state.inner(), connection_id, Some(generation)).unwrap_or(false)
}

#[cfg(feature = "native-runtime")]
pub(crate) fn remove_bridge_window(state: &GatewayBridge, owner_window: &str) {
    let Ok(mut entries) = state.entries.lock() else {
        return;
    };
    let connection_ids = entries
        .iter()
        .filter(|(_, entry)| entry.owner_window == owner_window)
        .map(|(connection_id, _)| connection_id.clone())
        .collect::<Vec<_>>();
    for connection_id in connection_ids {
        if let Some(entry) = entries.remove(&connection_id) {
            let _ = entry.cancel.send(true);
        }
    }
}

#[cfg(feature = "native-runtime")]
#[tauri::command]
pub(crate) async fn gateway_endpoint(
    resolver: State<'_, ManagedGatewayResolver>,
) -> Result<GatewayEndpoint, String> {
    let managed = resolve_managed_gateway(resolver.inner())
        .await
        .map_err(|err| err.to_string())?;
    let mut ws_url = managed.base_url.trim_end_matches('/').to_string();
    if let Some(rest) = ws_url.strip_prefix("https://") {
        ws_url = format!("wss://{rest}");
    } else if let Some(rest) = ws_url.strip_prefix("http://") {
        ws_url = format!("ws://{rest}");
    }
    ws_url.push_str("/ws");
    Ok(GatewayEndpoint {
        http_base: managed.base_url,
        ws_url,
    })
}

#[cfg(feature = "native-runtime")]
#[tauri::command]
pub(crate) async fn download_session_artifact(
    resolver: State<'_, ManagedGatewayResolver>,
    request: DownloadSessionRequest,
) -> Result<DownloadSessionResult, String> {
    let managed = resolve_managed_gateway(resolver.inner())
        .await
        .map_err(|err| err.to_string())?;
    download_session_artifact_with_managed(&managed, &request)
        .await
        .map_err(|err| err.to_string())
}

fn websocket_request(
    managed: &ManagedGateway,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, DesktopError> {
    let mut url = managed.base_url.trim_end_matches('/').to_string();
    if let Some(rest) = url.strip_prefix("https://") {
        url = format!("wss://{rest}");
    } else if let Some(rest) = url.strip_prefix("http://") {
        url = format!("ws://{rest}");
    }
    url.push_str("/ws");
    let mut request = url.into_client_request()?;
    request
        .headers_mut()
        .insert(AUTHORIZATION, format!("Bearer {}", managed.token).parse()?);
    Ok(request)
}

#[cfg(feature = "native-runtime")]
async fn resolve_managed_gateway(
    resolver: &ManagedGatewayResolver,
) -> Result<ManagedGateway, DesktopError> {
    let mut managed_guard = resolver.managed.lock().await;
    if let Some(managed) = managed_guard.as_ref() {
        if ensure_managed_gateway_healthy(managed).await.is_ok() {
            #[cfg(feature = "wdio-test")]
            crate::startup_trace::record_managed_gateway_ready();
            return Ok(managed.clone());
        }
        *managed_guard = None;
    }
    let managed = resolve_managed_gateway_uncached().await?;
    #[cfg(feature = "wdio-test")]
    crate::startup_trace::record_managed_gateway_ready();
    *managed_guard = Some(managed.clone());
    Ok(managed)
}

#[cfg(feature = "native-runtime")]
async fn resolve_managed_gateway_uncached() -> Result<ManagedGateway, DesktopError> {
    let explicit_base = env::var("PSYCHEVO_GATEWAY_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let explicit_token = env::var("PSYCHEVO_GATEWAY_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty());
    match (explicit_base, explicit_token) {
        (Some(base_url), Some(token)) => {
            let managed = ManagedGateway { base_url, token };
            ensure_managed_gateway_healthy(&managed)
                .await
                .map_err(|err| format!("configured Gateway endpoint is unavailable: {err}"))?;
            return Ok(managed);
        }
        (None, None) => {}
        _ => {
            return Err(
                "PSYCHEVO_GATEWAY_BASE_URL and PSYCHEVO_GATEWAY_TOKEN must be set together".into(),
            );
        }
    }

    let home = psychevo_home()?;
    let managed = start_managed_gateway(&home)?;
    ensure_managed_gateway_healthy(&managed)
        .await
        .map_err(|err| format!("started managed Gateway is unavailable: {err}"))?;
    Ok(managed)
}

#[cfg(feature = "native-runtime")]
async fn ensure_managed_gateway_healthy(managed: &ManagedGateway) -> Result<(), String> {
    let request = websocket_request(managed).map_err(|err| err.to_string())?;
    let (socket, _) = timeout(Duration::from_secs(2), connect_async(request))
        .await
        .map_err(|_| "managed Gateway health check timed out".to_string())?
        .map_err(|err| format!("managed Gateway health check failed: {err}"))?;
    drop(socket);
    Ok(())
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedGatewayDecision {
    StartManaged,
    UseExplicit,
}

#[cfg(test)]
fn managed_gateway_decision(
    explicit_healthy: Option<bool>,
    _persisted_healthy: Option<bool>,
    _persisted_stale: bool,
) -> Result<ManagedGatewayDecision, &'static str> {
    if let Some(healthy) = explicit_healthy {
        return if healthy {
            Ok(ManagedGatewayDecision::UseExplicit)
        } else {
            Err("configured Gateway endpoint is unavailable")
        };
    }
    Ok(ManagedGatewayDecision::StartManaged)
}

#[cfg(feature = "native-runtime")]
async fn download_session_artifact_with_managed(
    managed: &ManagedGateway,
    request: &DownloadSessionRequest,
) -> Result<DownloadSessionResult, DesktopError> {
    let url = download_session_url(&managed.base_url, request)?;
    let response = reqwest::Client::new()
        .get(url)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", managed.token),
        )
        .send()
        .await?
        .error_for_status()?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let fallback_filename = default_download_filename(request);
    let filename = response
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(content_disposition_filename)
        .unwrap_or(fallback_filename);
    let content = response.bytes().await?.to_vec();
    Ok(DownloadSessionResult {
        content,
        content_type,
        filename,
    })
}

fn download_session_url(
    base_url: &str,
    request: &DownloadSessionRequest,
) -> Result<String, DesktopError> {
    let thread_id = request.thread_id.trim();
    if thread_id.is_empty() {
        return Err("download session thread id is empty".into());
    }
    if !matches!(request.kind.as_str(), "export" | "share") {
        return Err(format!("unsupported download session kind: {}", request.kind).into());
    }
    let mut url = format!(
        "{}/download/session/{}/{}",
        base_url.trim_end_matches('/'),
        percent_encode_component(thread_id),
        percent_encode_component(&request.kind)
    );
    let mut query = Vec::new();
    if let Some(format) = request
        .format
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        query.push(format!("format={}", percent_encode_component(format)));
    }
    if let Some(include) = request.include.as_ref() {
        let include = include
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !include.is_empty() {
            query.push(format!(
                "include={}",
                percent_encode_component(&include.join(","))
            ));
        }
    }
    if let Some(filename) = request
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        query.push(format!("filename={}", percent_encode_component(filename)));
    }
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    Ok(url)
}

fn default_download_filename(request: &DownloadSessionRequest) -> String {
    let extension = if request.format.as_deref() == Some("json") {
        "json"
    } else {
        "md"
    };
    format!(
        "{}-{}.{}",
        safe_filename_component(&request.thread_id, "session"),
        safe_filename_component(&request.kind, "artifact"),
        extension
    )
}

fn content_disposition_filename(header: &str) -> Option<String> {
    for part in header.split(';') {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("filename") {
            continue;
        }
        let filename = unquote_header_value(value.trim());
        return safe_download_filename(&filename);
    }
    None
}

fn unquote_header_value(value: &str) -> String {
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return value.to_string();
    }
    let mut output = String::new();
    let mut escaped = false;
    for ch in value[1..value.len() - 1].chars() {
        if escaped {
            output.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            output.push(ch);
        }
    }
    output
}

fn safe_download_filename(value: &str) -> Option<String> {
    let filename = value.trim();
    if filename.is_empty()
        || filename == "."
        || filename == ".."
        || filename
            .chars()
            .any(|ch| ch == '/' || ch == '\\' || ch == '\0' || ch.is_control())
    {
        return None;
    }
    Some(filename.to_string())
}

fn safe_filename_component(value: &str, fallback: &str) -> String {
    let mut sanitized = value
        .trim()
        .chars()
        .map(|ch| match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect::<String>();
    sanitized = sanitized.trim_matches('.').to_string();
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

fn percent_encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(*byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(feature = "native-runtime")]
fn start_managed_gateway(home: &Path) -> Result<ManagedGateway, DesktopError> {
    let pevo = managed_gateway_executable()?;
    let output = Command::new(pevo)
        .args(["gateway", "start"])
        .env("PSYCHEVO_HOME", home)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "pevo gateway start failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let parsed: GatewayStartOutput = serde_json::from_slice(&output.stdout)?;
    if !parsed.ok {
        return Err("pevo gateway start returned ok=false".into());
    }
    let token = fs::read_to_string(home.join("gateway").join("token"))?
        .trim()
        .to_string();
    Ok(ManagedGateway {
        base_url: parsed.base_url,
        token,
    })
}

fn managed_gateway_executable() -> Result<PathBuf, DesktopError> {
    if let Ok(pevo) = env::var("PSYCHEVO_PEVO_BIN")
        && !pevo.trim().is_empty()
    {
        return Ok(PathBuf::from(pevo));
    }
    resolve_executable_on_path("pevo")
}

fn resolve_executable_on_path(name: &str) -> Result<PathBuf, DesktopError> {
    let requested = PathBuf::from(name);
    if requested.is_absolute() || name.contains('/') || name.contains('\\') {
        return Ok(requested);
    }
    let path_env = env::var_os("PATH")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "PATH is not set"))?;
    for dir in env::split_paths(&path_env) {
        for candidate in executable_candidates(&dir, name) {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{name} executable was not found on PATH"),
    )
    .into())
}

fn executable_candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut candidates = vec![dir.join(name)];
    if cfg!(windows) {
        let pathext = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        candidates.extend(
            pathext
                .split(';')
                .map(str::trim)
                .filter(|extension| !extension.is_empty())
                .map(|extension| dir.join(format!("{name}{extension}"))),
        );
    }
    candidates
}

#[cfg(feature = "native-runtime")]
fn psychevo_home() -> Result<PathBuf, DesktopError> {
    if let Ok(home) = env::var("PSYCHEVO_HOME")
        && !home.trim().is_empty()
    {
        return Ok(PathBuf::from(home));
    }
    let home = env::var("HOME")?;
    Ok(PathBuf::from(home).join(".psychevo"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "native-runtime")]
    #[test]
    fn bridge_generation_cleanup_cannot_remove_a_replacement() {
        let bridge = GatewayBridge::default();
        let (sender, _receiver) = mpsc::channel(GATEWAY_BRIDGE_QUEUE_CAPACITY);
        let (cancel, _cancelled) = watch::channel(false);
        bridge.entries.lock().expect("bridge entries").insert(
            "workbench:one".to_string(),
            GatewayBridgeEntry {
                sender,
                cancel,
                owner_window: "workbench".to_string(),
                generation: 2,
            },
        );

        assert!(!remove_bridge_entry(&bridge, "workbench:one", Some(1)).unwrap());
        assert!(bridge.entries.lock().unwrap().contains_key("workbench:one"));
        assert!(remove_bridge_entry(&bridge, "workbench:one", Some(2)).unwrap());
        assert!(!bridge.entries.lock().unwrap().contains_key("workbench:one"));
    }

    #[cfg(feature = "native-runtime")]
    #[test]
    fn bridge_window_cleanup_cancels_only_the_owned_entries() {
        let bridge = GatewayBridge::default();
        for (connection_id, owner_window, generation) in [
            ("workbench:one", "workbench", 1),
            ("floating:one", "floating", 2),
        ] {
            let (sender, _receiver) = mpsc::channel(GATEWAY_BRIDGE_QUEUE_CAPACITY);
            let (cancel, _cancelled) = watch::channel(false);
            bridge.entries.lock().unwrap().insert(
                connection_id.to_string(),
                GatewayBridgeEntry {
                    sender,
                    cancel,
                    owner_window: owner_window.to_string(),
                    generation,
                },
            );
        }

        remove_bridge_window(&bridge, "workbench");

        let entries = bridge.entries.lock().unwrap();
        assert!(!entries.contains_key("workbench:one"));
        assert!(entries.contains_key("floating:one"));
    }

    #[cfg(feature = "native-runtime")]
    #[tokio::test]
    async fn bridge_sender_has_a_fixed_frame_capacity() {
        let (sender, _receiver) = mpsc::channel(GATEWAY_BRIDGE_QUEUE_CAPACITY);
        for index in 0..GATEWAY_BRIDGE_QUEUE_CAPACITY {
            sender
                .try_send(format!("frame-{index}"))
                .expect("within capacity");
        }
        assert!(matches!(
            sender.try_send("overflow".to_string()),
            Err(mpsc::error::TrySendError::Full(_))
        ));
    }

    #[test]
    fn websocket_request_uses_bearer_without_exposing_token_in_url() {
        let request = websocket_request(&ManagedGateway {
            base_url: "http://127.0.0.1:58080".to_string(),
            token: "secret-token".to_string(),
        })
        .expect("request");
        assert_eq!(request.uri().to_string(), "ws://127.0.0.1:58080/ws");
        assert_eq!(
            request
                .headers()
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer secret-token")
        );
        assert!(!request.uri().to_string().contains("secret-token"));
    }

    #[test]
    fn managed_gateway_decision_delegates_persisted_state_to_cli() {
        assert_eq!(
            managed_gateway_decision(None, Some(true), false).expect("decision"),
            ManagedGatewayDecision::StartManaged
        );
    }

    #[test]
    fn managed_gateway_decision_starts_when_persisted_state_is_unhealthy() {
        assert_eq!(
            managed_gateway_decision(None, Some(false), false).expect("decision"),
            ManagedGatewayDecision::StartManaged
        );
    }

    #[test]
    fn managed_gateway_decision_starts_when_persisted_state_is_stale() {
        assert_eq!(
            managed_gateway_decision(None, Some(true), true).expect("decision"),
            ManagedGatewayDecision::StartManaged
        );
    }

    #[test]
    fn managed_gateway_decision_rejects_unhealthy_env_override() {
        assert_eq!(
            managed_gateway_decision(Some(true), None, true).expect("decision"),
            ManagedGatewayDecision::UseExplicit
        );
        assert_eq!(
            managed_gateway_decision(Some(false), Some(true), false).expect_err("error"),
            "configured Gateway endpoint is unavailable"
        );
    }

    #[test]
    fn download_session_url_preserves_format_include_and_filename_options() {
        let request = DownloadSessionRequest {
            filename: Some("provider response.json".to_string()),
            format: Some("json".to_string()),
            include: Some(vec![
                "last-provider-request".to_string(),
                "last-provider-response".to_string(),
            ]),
            kind: "export".to_string(),
            thread_id: "thread/id".to_string(),
        };

        assert_eq!(
            download_session_url("http://127.0.0.1:58080/", &request).expect("url"),
            "http://127.0.0.1:58080/download/session/thread%2Fid/export?format=json&include=last-provider-request%2Clast-provider-response&filename=provider%20response.json"
        );
    }

    #[test]
    fn content_disposition_filename_parsing_falls_back_for_absent_or_invalid_values() {
        let request = DownloadSessionRequest {
            filename: None,
            format: Some("json".to_string()),
            include: None,
            kind: "share".to_string(),
            thread_id: "session-1".to_string(),
        };

        assert_eq!(
            content_disposition_filename("attachment; filename=\"session.json\""),
            Some("session.json".to_string())
        );
        assert_eq!(
            content_disposition_filename("attachment; filename=\"../session.json\""),
            None
        );
        assert_eq!(
            content_disposition_filename("attachment; filename=\"\""),
            None
        );
        assert_eq!(content_disposition_filename("attachment"), None);
        assert_eq!(default_download_filename(&request), "session-1-share.json");
        assert_eq!(
            default_download_filename(&DownloadSessionRequest {
                filename: None,
                format: None,
                include: None,
                kind: "export".to_string(),
                thread_id: "../unsafe/session".to_string(),
            }),
            "_unsafe_session-export.md"
        );
    }
}
