#![cfg_attr(not(feature = "native-runtime"), allow(dead_code))]

mod capture;

use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
#[cfg(feature = "native-runtime")]
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "native-runtime")]
use std::collections::HashMap;
#[cfg(feature = "native-runtime")]
use std::process::Command;
#[cfg(feature = "native-runtime")]
use std::sync::Mutex;

#[cfg(feature = "native-runtime")]
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(feature = "native-runtime")]
use tauri::{AppHandle, Emitter, Manager, State};
#[cfg(feature = "native-runtime")]
use tokio::sync::{Mutex as AsyncMutex, mpsc};
#[cfg(feature = "native-runtime")]
use tokio::time::timeout;
#[cfg(feature = "native-runtime")]
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
#[cfg(feature = "native-runtime")]
use tokio_tungstenite::tungstenite::protocol::Message;

use capture::DesktopCaptureFacade;

const DESKTOP_CWD_ENV: &str = "PSYCHEVO_DESKTOP_CWD";

type DesktopError = Box<dyn std::error::Error + Send + Sync>;

#[cfg(feature = "native-runtime")]
#[derive(Default)]
struct GatewayBridge {
    senders: Mutex<HashMap<String, mpsc::UnboundedSender<String>>>,
}

#[cfg(feature = "native-runtime")]
#[derive(Default)]
struct ManagedGatewayResolver {
    managed: AsyncMutex<Option<ManagedGateway>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedGateway {
    base_url: String,
    token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedServerState {
    base_url: String,
    executable_inode: Option<u64>,
    executable_modified_ms: Option<i64>,
    executable_path: Option<String>,
    executable_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExecutableFingerprint {
    inode: Option<u64>,
    modified_ms: i64,
    path: String,
    size: u64,
}

#[cfg(feature = "native-runtime")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GatewayStartOutput {
    base_url: String,
    ok: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Rect {
    height: f64,
    width: f64,
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct FloatingActivation {
    activation_id: String,
    anchor: Option<Rect>,
    attachments: Vec<Value>,
    cwd: String,
}

#[cfg(feature = "native-runtime")]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayEndpoint {
    http_base: String,
    ws_url: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct DownloadSessionRequest {
    filename: Option<String>,
    format: Option<String>,
    include: Option<Vec<String>>,
    kind: String,
    thread_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadSessionResult {
    content: Vec<u8>,
    content_type: String,
    filename: String,
}

#[cfg(feature = "native-runtime")]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayBridgeEvent {
    connection_id: String,
    message: String,
}

#[cfg(feature = "native-runtime")]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayBridgeBroadcastEvent {
    message: String,
    origin_connection_id: String,
}

#[cfg(feature = "native-runtime")]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenWorkbenchThreadEvent {
    thread_id: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum CapabilityFailureReason {
    Unsupported,
    Unavailable,
    PermissionDenied,
    Canceled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CapabilityFailure {
    capability: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    ok: bool,
    reason: CapabilityFailureReason,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CapabilitySuccess<T> {
    ok: bool,
    value: T,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
enum CapabilityResult<T> {
    Success(CapabilitySuccess<T>),
    Failure(CapabilityFailure),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegionCapture {
    data_url: String,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopPlatformCapabilities {
    capture: CaptureCapabilities,
    display_variables: Vec<String>,
    os: &'static str,
    session: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureCapabilities {
    pointer: CapabilitySnapshot,
    portal_screenshot: CapabilitySnapshot,
    region_screenshot: CapabilitySnapshot,
    selection: CapabilitySnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CapabilitySnapshot {
    available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<CapabilityFailureReason>,
}

#[cfg(feature = "native-runtime")]
#[tauri::command]
async fn gateway_connect(
    app: AppHandle,
    resolver: State<'_, ManagedGatewayResolver>,
    state: State<'_, GatewayBridge>,
    connection_id: String,
) -> Result<(), String> {
    let managed = resolve_managed_gateway(resolver.inner())
        .await
        .map_err(|err| err.to_string())?;
    let request = websocket_request(&managed).map_err(|err| err.to_string())?;
    let (socket, _) = connect_async(request)
        .await
        .map_err(|err| err.to_string())?;
    let (mut write, mut read) = socket.split();
    let (sender, mut receiver) = mpsc::unbounded_channel::<String>();
    state
        .senders
        .lock()
        .map_err(|_| "Gateway bridge lock poisoned".to_string())?
        .insert(connection_id.clone(), sender);

    tauri::async_runtime::spawn(async move {
        while let Some(message) = receiver.recv().await {
            if write.send(Message::Text(message.into())).await.is_err() {
                break;
            }
        }
    });

    let read_connection_id = connection_id.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    let message = text.to_string();
                    let _ = app.emit(
                        "gateway-message",
                        GatewayBridgeEvent {
                            connection_id: read_connection_id.clone(),
                            message: message.clone(),
                        },
                    );
                    if should_broadcast_gateway_notification(&message) {
                        let _ = app.emit(
                            "gateway-broadcast",
                            GatewayBridgeBroadcastEvent {
                                message,
                                origin_connection_id: read_connection_id.clone(),
                            },
                        );
                    }
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(err) => {
                    let _ = app.emit(
                        "gateway-disconnect",
                        GatewayBridgeEvent {
                            connection_id: read_connection_id.clone(),
                            message: err.to_string(),
                        },
                    );
                    return;
                }
            }
        }
        let _ = app.emit(
            "gateway-disconnect",
            GatewayBridgeEvent {
                connection_id: read_connection_id,
                message: "Gateway WebSocket closed".to_string(),
            },
        );
    });

    Ok(())
}

#[cfg(feature = "native-runtime")]
#[tauri::command]
async fn gateway_send(
    state: State<'_, GatewayBridge>,
    connection_id: String,
    message: String,
) -> Result<(), String> {
    let guard = state
        .senders
        .lock()
        .map_err(|_| "Gateway bridge lock poisoned".to_string())?;
    let sender = guard
        .get(&connection_id)
        .ok_or_else(|| "Gateway bridge is not connected".to_string())?;
    sender
        .send(message)
        .map_err(|_| "Gateway bridge is closed".to_string())
}

#[cfg(feature = "native-runtime")]
#[tauri::command]
async fn gateway_disconnect(
    state: State<'_, GatewayBridge>,
    connection_id: String,
) -> Result<(), String> {
    state
        .senders
        .lock()
        .map_err(|_| "Gateway bridge lock poisoned".to_string())?
        .remove(&connection_id);
    Ok(())
}

#[cfg(feature = "native-runtime")]
#[tauri::command]
async fn gateway_endpoint(
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
async fn open_thread_in_workbench(app: AppHandle, thread_id: String) -> Result<(), String> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Err("thread id is empty".to_string());
    }
    let window = app
        .get_webview_window("workbench")
        .ok_or_else(|| "Workbench window is unavailable".to_string())?;
    window.unminimize().map_err(|err| err.to_string())?;
    window.show().map_err(|err| err.to_string())?;
    window.set_focus().map_err(|err| err.to_string())?;
    window
        .emit(
            "desktop-open-thread",
            OpenWorkbenchThreadEvent {
                thread_id: thread_id.to_string(),
            },
        )
        .map_err(|err| err.to_string())
}

#[cfg(feature = "native-runtime")]
#[tauri::command]
async fn download_session_artifact(
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

#[cfg_attr(feature = "native-runtime", tauri::command)]
async fn desktop_fallback_cwd() -> Result<String, String> {
    Ok(desktop_fallback_cwd_value())
}

#[cfg_attr(feature = "native-runtime", tauri::command)]
async fn desktop_platform_capabilities() -> Result<DesktopPlatformCapabilities, String> {
    Ok(platform_capabilities())
}

#[cfg_attr(feature = "native-runtime", tauri::command)]
async fn floating_initial_activation() -> Result<FloatingActivation, String> {
    Ok(current_activation())
}

#[cfg_attr(feature = "native-runtime", tauri::command)]
async fn floating_capture_selection() -> Result<FloatingActivation, String> {
    Ok(current_activation())
}

#[cfg_attr(feature = "native-runtime", tauri::command)]
async fn floating_capture_region(bounds: Rect) -> Result<CapabilityResult<RegionCapture>, String> {
    Ok(capture_region(bounds))
}

#[cfg_attr(feature = "native-runtime", tauri::command)]
async fn floating_begin_region_picker() -> Result<CapabilityResult<Option<Rect>>, String> {
    Ok(DesktopCaptureFacade::detect().begin_region_picker())
}

#[cfg(all(
    feature = "native-runtime",
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(feature = "wdio-test")]
    let builder = builder
        .plugin(tauri_plugin_wdio_webdriver::init())
        .plugin(tauri_plugin_wdio::init());

    builder
        .manage(GatewayBridge::default())
        .manage(ManagedGatewayResolver::default())
        .invoke_handler(tauri::generate_handler![
            desktop_fallback_cwd,
            desktop_platform_capabilities,
            floating_begin_region_picker,
            floating_capture_region,
            floating_capture_selection,
            floating_initial_activation,
            download_session_artifact,
            gateway_connect,
            gateway_disconnect,
            gateway_endpoint,
            gateway_send,
            open_thread_in_workbench
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Psychevo Desktop");
}

#[cfg(all(
    not(feature = "native-runtime"),
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
pub fn run() {
    panic!("Psychevo Desktop was built without the native-runtime feature");
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn run() {
    panic!("Psychevo Desktop is supported only on macOS, Windows, and Linux");
}

fn fake_activation() -> FloatingActivation {
    let text = env::var("PSYCHEVO_FLOATING_TEXT")
        .ok()
        .filter(|text| !text.trim().is_empty());
    activation_from_selection(
        text.as_deref(),
        env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
        now_ms(),
    )
}

fn current_activation() -> FloatingActivation {
    if env::var("PSYCHEVO_FLOATING_TEXT").is_ok_and(|text| !text.trim().is_empty()) {
        return fake_activation();
    }
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let now = now_ms();
    match DesktopCaptureFacade::detect().selection() {
        CapabilityResult::Success(selection) => activation_from_capture(
            selection.value.text.as_deref(),
            selection.value.source_app.as_deref(),
            selection.value.anchor,
            cwd,
            now,
        ),
        CapabilityResult::Failure(_) => activation_from_capture(None, None, None, cwd, now),
    }
}

fn activation_from_selection(
    text: Option<&str>,
    cwd: PathBuf,
    activation_ms: u128,
) -> FloatingActivation {
    activation_from_capture(
        text,
        Some("Floating fallback"),
        Some(Rect {
            x: 260.0,
            y: 24.0,
            width: 280.0,
            height: 28.0,
        }),
        cwd,
        activation_ms,
    )
}

fn activation_from_capture(
    text: Option<&str>,
    source_app: Option<&str>,
    anchor: Option<Rect>,
    cwd: PathBuf,
    activation_ms: u128,
) -> FloatingActivation {
    let attachments = text
        .as_ref()
        .map(|text| {
            vec![json!({
                "id": format!("selection:{activation_ms}"),
                "kind": "textSelection",
                "name": "Selected text",
                "preview": preview_text(text),
                "sourceApp": source_app.unwrap_or("Native selection"),
                "text": text,
                "visibleToModel": true
            })]
        })
        .unwrap_or_default();
    FloatingActivation {
        activation_id: format!("capsule-{activation_ms}"),
        anchor,
        attachments,
        cwd: cwd.display().to_string(),
    }
}

fn capture_region(bounds: Rect) -> CapabilityResult<RegionCapture> {
    DesktopCaptureFacade::detect().capture_region(bounds)
}

fn platform_capabilities() -> DesktopPlatformCapabilities {
    DesktopCaptureFacade::detect().platform_capabilities()
}

fn desktop_fallback_cwd_value() -> String {
    desktop_fallback_cwd_from_env(
        env::var(DESKTOP_CWD_ENV).ok().as_deref(),
        env::current_dir().ok(),
    )
}

fn desktop_fallback_cwd_from_env(explicit: Option<&str>, fallback: Option<PathBuf>) -> String {
    if let Some(value) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return value.to_string();
    }
    fallback
        .unwrap_or_else(|| PathBuf::from("/"))
        .display()
        .to_string()
}

fn desktop_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

fn linux_session() -> &'static str {
    if desktop_os() != "linux" {
        return "unknown";
    }
    detect_linux_session(
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
        env::var("WAYLAND_DISPLAY").ok().as_deref(),
        env::var("DISPLAY").ok().as_deref(),
    )
}

fn detect_linux_session(
    xdg_session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> &'static str {
    match xdg_session_type.map(|value| value.to_ascii_lowercase()) {
        Some(value) if value == "wayland" => "wayland",
        Some(value) if value == "x11" => "x11",
        _ if wayland_display.is_some_and(|value| !value.trim().is_empty()) => "wayland",
        _ if display.is_some_and(|value| !value.trim().is_empty()) => "x11",
        _ => "unknown",
    }
}

fn observed_display_variables() -> Vec<String> {
    [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_SESSION_TYPE",
        "XDG_CURRENT_DESKTOP",
        "DESKTOP_SESSION",
        "WSL_DISTRO_NAME",
        "WSL_INTEROP",
    ]
    .into_iter()
    .filter(|name| env::var(name).is_ok_and(|value| !value.trim().is_empty()))
    .map(str::to_string)
    .collect()
}

fn platform_capture_unavailable_message() -> String {
    match (desktop_os(), linux_session()) {
        ("linux", "x11") => {
            "Linux X11 capture adapter is detected but native selection/screenshot capture is not available in this build.".to_string()
        }
        ("linux", "wayland") => {
            "Linux Wayland capture requires portal/AT-SPI support that is not available in this deterministic build.".to_string()
        }
        ("linux", _) => "Linux display capture is unavailable because no supported X11 or Wayland session was detected.".to_string(),
        ("macos", _) => "macOS selection and screenshot capture require native permissions and are not available in this deterministic build.".to_string(),
        ("windows", _) => "Windows selection and screenshot capture require native foreground capture support and are not available in this deterministic build.".to_string(),
        _ => "Native capture is unavailable on this platform.".to_string(),
    }
}

fn capability_success<T>(value: T) -> CapabilityResult<T> {
    CapabilityResult::Success(CapabilitySuccess { ok: true, value })
}

fn capability_failure<T>(
    capability: &'static str,
    reason: CapabilityFailureReason,
    message: impl Into<String>,
) -> CapabilityResult<T> {
    CapabilityResult::Failure(CapabilityFailure {
        capability,
        message: Some(message.into()),
        ok: false,
        reason,
    })
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

fn should_broadcast_gateway_notification(message: &str) -> bool {
    let Ok(Value::Object(frame)) = serde_json::from_str::<Value>(message) else {
        return false;
    };
    if frame.contains_key("id") {
        return false;
    }
    if frame.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return false;
    }
    matches!(
        frame.get("method").and_then(Value::as_str),
        Some("gateway/event" | "turn/result" | "turn/error")
    )
}

#[cfg(feature = "native-runtime")]
async fn resolve_managed_gateway(
    resolver: &ManagedGatewayResolver,
) -> Result<ManagedGateway, DesktopError> {
    let mut managed_guard = resolver.managed.lock().await;
    if let Some(managed) = managed_guard.as_ref() {
        if ensure_managed_gateway_healthy(managed).await.is_ok() {
            return Ok(managed.clone());
        }
        *managed_guard = None;
    }
    let managed = resolve_managed_gateway_uncached().await?;
    *managed_guard = Some(managed.clone());
    Ok(managed)
}

#[cfg(feature = "native-runtime")]
async fn resolve_managed_gateway_uncached() -> Result<ManagedGateway, DesktopError> {
    if let (Ok(base_url), Ok(token)) = (
        env::var("PSYCHEVO_GATEWAY_BASE_URL"),
        env::var("PSYCHEVO_GATEWAY_TOKEN"),
    ) && !base_url.trim().is_empty()
        && !token.trim().is_empty()
    {
        let managed = ManagedGateway { base_url, token };
        ensure_managed_gateway_healthy(&managed)
            .await
            .map_err(|err| format!("configured Gateway endpoint is unavailable: {err}"))?;
        return Ok(managed);
    }

    let home = psychevo_home()?;
    let expected_executable = managed_gateway_executable_fingerprint()?;
    if let Ok((managed, state)) = read_managed_gateway_state(&home) {
        if managed_gateway_stale_reason(&state, &expected_executable).is_none()
            && ensure_managed_gateway_healthy(&managed).await.is_ok()
        {
            return Ok(managed);
        }
    }
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
    UsePersisted,
}

#[cfg(test)]
fn managed_gateway_decision(
    explicit_healthy: Option<bool>,
    persisted_healthy: Option<bool>,
    persisted_stale: bool,
) -> Result<ManagedGatewayDecision, &'static str> {
    if let Some(healthy) = explicit_healthy {
        return if healthy {
            Ok(ManagedGatewayDecision::UseExplicit)
        } else {
            Err("configured Gateway endpoint is unavailable")
        };
    }
    if persisted_healthy == Some(true) && !persisted_stale {
        return Ok(ManagedGatewayDecision::UsePersisted);
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

#[cfg(test)]
fn read_managed_gateway(home: &Path) -> Result<ManagedGateway, DesktopError> {
    read_managed_gateway_state(home).map(|(managed, _state)| managed)
}

fn read_managed_gateway_state(
    home: &Path,
) -> Result<(ManagedGateway, ManagedServerState), DesktopError> {
    let gateway_dir = home.join("gateway");
    let state: ManagedServerState =
        serde_json::from_str(&fs::read_to_string(gateway_dir.join("server.json"))?)?;
    let token = fs::read_to_string(gateway_dir.join("token"))?
        .trim()
        .to_string();
    if token.is_empty() {
        return Err("managed Gateway token is empty".into());
    }
    Ok((
        ManagedGateway {
            base_url: state.base_url.clone(),
            token,
        },
        state,
    ))
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

fn managed_gateway_executable_fingerprint() -> Result<ExecutableFingerprint, DesktopError> {
    executable_fingerprint(&managed_gateway_executable()?)
}

fn managed_gateway_executable() -> Result<PathBuf, DesktopError> {
    if let Ok(pevo) = env::var("PSYCHEVO_PEVO_BIN")
        && !pevo.trim().is_empty()
    {
        return Ok(PathBuf::from(pevo));
    }
    resolve_executable_on_path("pevo")
}

fn executable_fingerprint(path: &Path) -> Result<ExecutableFingerprint, DesktopError> {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let metadata = fs::metadata(&path)?;
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default();
    Ok(ExecutableFingerprint {
        inode: executable_inode(&metadata),
        modified_ms,
        path: path.display().to_string(),
        size: metadata.len(),
    })
}

fn managed_gateway_stale_reason(
    state: &ManagedServerState,
    expected: &ExecutableFingerprint,
) -> Option<&'static str> {
    let Some(state_executable) = state_executable_fingerprint(state) else {
        return Some("missing_executable_fingerprint");
    };
    if &state_executable != expected {
        return Some("executable_fingerprint_mismatch");
    }
    None
}

fn state_executable_fingerprint(state: &ManagedServerState) -> Option<ExecutableFingerprint> {
    Some(ExecutableFingerprint {
        inode: state.executable_inode,
        modified_ms: state.executable_modified_ms?,
        path: state.executable_path.clone()?,
        size: state.executable_size?,
    })
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

#[cfg(unix)]
fn executable_inode(metadata: &fs::Metadata) -> Option<u64> {
    Some(metadata.ino())
}

#[cfg(not(unix))]
fn executable_inode(_metadata: &fs::Metadata) -> Option<u64> {
    None
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

fn preview_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 80 {
        return normalized;
    }
    format!("{}...", normalized.chars().take(77).collect::<String>())
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_text_collapses_whitespace_and_truncates() {
        let text = "hello\nworld ".repeat(12);
        let preview = preview_text(&text);
        assert!(preview.len() <= 83, "{preview}");
        assert!(!preview.contains('\n'));
        assert!(preview.ends_with("..."));
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
    fn gateway_bridge_broadcasts_only_thread_notifications() {
        assert!(should_broadcast_gateway_notification(
            r#"{"jsonrpc":"2.0","method":"gateway/event","params":{"type":"activityChanged","threadId":"thread-1","activity":{"running":false,"activeTurnId":null,"queuedTurns":0}}}"#
        ));
        assert!(should_broadcast_gateway_notification(
            r#"{"jsonrpc":"2.0","method":"turn/result","params":{"thread":{"id":"thread-1"},"turn":{"id":"turn-1"}}}"#
        ));
        assert!(should_broadcast_gateway_notification(
            r#"{"jsonrpc":"2.0","method":"turn/error","params":{"message":"failed"}}"#
        ));
        assert!(!should_broadcast_gateway_notification(
            r#"{"jsonrpc":"2.0","id":"1","result":{"accepted":true}}"#
        ));
        assert!(!should_broadcast_gateway_notification(
            r#"{"jsonrpc":"2.0","id":"1","method":"turn/start","params":{}}"#
        ));
        assert!(!should_broadcast_gateway_notification(
            r#"{"jsonrpc":"2.0","method":"terminal/output","params":{}}"#
        ));
    }

    #[test]
    fn read_managed_gateway_reads_state_and_token() {
        let temp = tempfile_dir();
        let gateway = temp.join("gateway");
        fs::create_dir_all(&gateway).expect("gateway dir");
        fs::write(
            gateway.join("server.json"),
            r#"{"baseUrl":"http://127.0.0.1:58080"}"#,
        )
        .expect("state");
        fs::write(gateway.join("token"), "token\n").expect("token");

        let managed = read_managed_gateway(&temp).expect("managed gateway");

        assert_eq!(
            managed,
            ManagedGateway {
                base_url: "http://127.0.0.1:58080".to_string(),
                token: "token".to_string(),
            }
        );
        let _ = fs::remove_dir_all(temp);
    }

    #[test]
    fn managed_gateway_decision_reuses_healthy_persisted_state() {
        assert_eq!(
            managed_gateway_decision(None, Some(true), false).expect("decision"),
            ManagedGatewayDecision::UsePersisted
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
    fn managed_gateway_stale_reason_rejects_mismatched_executable_fingerprint() {
        let state = managed_server_state_with_executable(executable_fingerprint_fixture(
            "/old/pevo",
            10,
            100,
            Some(1),
        ));
        let expected = executable_fingerprint_fixture("/new/pevo", 20, 200, Some(2));

        assert_eq!(
            managed_gateway_stale_reason(&state, &expected),
            Some("executable_fingerprint_mismatch")
        );
    }

    #[test]
    fn managed_gateway_stale_reason_rejects_missing_executable_fingerprint() {
        let state = ManagedServerState {
            base_url: "http://127.0.0.1:58080".to_string(),
            executable_inode: None,
            executable_modified_ms: None,
            executable_path: None,
            executable_size: None,
        };
        let expected = executable_fingerprint_fixture("/new/pevo", 20, 200, Some(2));

        assert_eq!(
            managed_gateway_stale_reason(&state, &expected),
            Some("missing_executable_fingerprint")
        );
    }

    #[test]
    fn managed_gateway_stale_reason_accepts_matching_executable_fingerprint() {
        let expected = executable_fingerprint_fixture("/current/pevo", 20, 200, Some(2));
        let state = managed_server_state_with_executable(expected.clone());

        assert_eq!(managed_gateway_stale_reason(&state, &expected), None);
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

    fn executable_fingerprint_fixture(
        path: &str,
        modified_ms: i64,
        size: u64,
        inode: Option<u64>,
    ) -> ExecutableFingerprint {
        ExecutableFingerprint {
            inode,
            modified_ms,
            path: path.to_string(),
            size,
        }
    }

    fn managed_server_state_with_executable(
        executable: ExecutableFingerprint,
    ) -> ManagedServerState {
        ManagedServerState {
            base_url: "http://127.0.0.1:58080".to_string(),
            executable_inode: executable.inode,
            executable_modified_ms: Some(executable.modified_ms),
            executable_path: Some(executable.path),
            executable_size: Some(executable.size),
        }
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

    #[test]
    fn fallback_activation_maps_selected_text_to_visible_attachment() {
        let activation = activation_from_selection(
            Some("hello\nfloating capsule"),
            PathBuf::from("/tmp/workspace"),
            42,
        );

        assert_eq!(activation.activation_id, "capsule-42");
        assert_eq!(activation.cwd, "/tmp/workspace");
        assert_eq!(activation.attachments.len(), 1);
        assert_eq!(activation.attachments[0]["kind"], "textSelection");
        assert_eq!(activation.attachments[0]["id"], "selection:42");
        assert_eq!(
            activation.attachments[0]["preview"],
            "hello floating capsule"
        );
        assert_eq!(activation.attachments[0]["text"], "hello\nfloating capsule");
        assert_eq!(activation.attachments[0]["visibleToModel"], true);
    }

    #[test]
    fn linux_session_detection_prefers_explicit_session_type() {
        assert_eq!(
            detect_linux_session(Some("wayland"), None, Some(":0")),
            "wayland"
        );
        assert_eq!(
            detect_linux_session(Some("x11"), Some("wayland-0"), None),
            "x11"
        );
        assert_eq!(
            detect_linux_session(None, Some("wayland-0"), Some(":0")),
            "wayland"
        );
        assert_eq!(detect_linux_session(None, None, Some(":0")), "x11");
        assert_eq!(detect_linux_session(None, None, None), "unknown");
    }

    #[test]
    fn platform_capability_results_serialize_reason_taxonomy() {
        let failed: CapabilityResult<()> = capability_failure(
            "floating.captureRegion",
            CapabilityFailureReason::PermissionDenied,
            "Screen capture permission was denied.",
        );
        assert_eq!(
            serde_json::to_value(failed).expect("json"),
            json!({
                "capability": "floating.captureRegion",
                "message": "Screen capture permission was denied.",
                "ok": false,
                "reason": "permissionDenied"
            })
        );
    }

    #[test]
    fn fake_region_capture_uses_data_url_when_supplied() {
        // SAFETY: the test serially sets and removes a process environment
        // variable before any thread observes this helper.
        unsafe {
            env::set_var(
                "PSYCHEVO_FLOATING_REGION_DATA_URL",
                "data:image/png;base64,AA==",
            );
        }
        let capture = capture_region(Rect {
            x: 0.0,
            y: 0.0,
            width: 123.0,
            height: 45.0,
        });
        unsafe {
            env::remove_var("PSYCHEVO_FLOATING_REGION_DATA_URL");
        }
        assert_eq!(
            serde_json::to_value(capture).expect("json"),
            json!({
                "ok": true,
                "value": {
                    "dataUrl": "data:image/png;base64,AA==",
                    "name": "floating-region-123x45.png"
                }
            })
        );
    }

    #[test]
    fn desktop_fallback_cwd_prefers_explicit_env_value() {
        assert_eq!(
            desktop_fallback_cwd_from_env(
                Some(" /tmp/psychevo-desktop-workspace "),
                Some(PathBuf::from("/tmp/process-cwd"))
            ),
            "/tmp/psychevo-desktop-workspace"
        );
    }

    #[test]
    fn desktop_fallback_cwd_uses_process_cwd_without_env_value() {
        assert_eq!(
            desktop_fallback_cwd_from_env(Some(" "), Some(PathBuf::from("/tmp/process-cwd"))),
            "/tmp/process-cwd"
        );
    }

    fn tempfile_dir() -> PathBuf {
        let path = env::temp_dir().join(format!("psychevo-desktop-test-{}", now_ms()));
        fs::create_dir_all(&path).expect("temp dir");
        path
    }
}
