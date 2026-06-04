use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Body;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::header::{
    AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, LOCATION, SET_COOKIE,
};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use futures::{SinkExt, StreamExt};
use psychevo_gateway_protocol as wire;
use psychevo_runtime::command_registry::{
    AvailableSlashCommand, CommandArgumentKind, CommandCapability,
    available_slash_commands_for_surface,
};
use psychevo_runtime::{
    AgentBackendConfig, AgentDiscoveryOptions, AgentEntrypoint, ClarifyAnswer, ClarifyResponse,
    ClarifyResult, Error, ListSkillsOptions, Message as RuntimeMessage, PermissionApprovalDecision,
    PermissionApprovalOutcome, PermissionMode, RunMode, RunOptions, SessionArtifactKind,
    SessionExportFormat, SessionExportIncludeSet, SessionExportOptions, SessionSummary,
    SkillDiscoveryOptions, StateRuntime, UserContentBlock, canonicalize_workdir, discover_agents,
    discover_skills, list_agents_value, list_skills_value_with_options, load_agent_backend_configs,
    render_session_export, resolve_agent_definition, valid_agent_name,
    view_agent_value_with_catalog,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    BackendKind, Gateway, GatewayActivity, GatewayBackendInfo, GatewayEvent, GatewayEventSink,
    GatewayInputPart, GatewaySource, GatewaySourceLifetime, GatewayThread, GatewayThreadSelector,
    GatewayTurnResult, PermissionDecision, SourceKey, gateway_now_ms,
};

const HISTORY_SOURCES: &[&str] = &["run", "tui", "web", "peer_agent"];
const MAX_COMPLETION_ITEMS: usize = 50;
const MAX_FILE_COMPLETION_ITEMS: usize = 80;
const MAX_FILE_COMPLETION_DEPTH: usize = 8;

#[derive(Debug, Clone)]
pub struct GatewayWebServerConfig {
    pub gateway: Gateway,
    pub home: PathBuf,
    pub workdir: PathBuf,
    pub config_path: Option<PathBuf>,
    pub inherited_env: BTreeMap<String, String>,
    pub static_dir: Option<PathBuf>,
    pub bind_addr: SocketAddr,
    pub token: String,
    pub managed_state_path: Option<PathBuf>,
}

impl GatewayWebServerConfig {
    pub fn new(
        gateway: Gateway,
        home: PathBuf,
        workdir: PathBuf,
        config_path: Option<PathBuf>,
        inherited_env: BTreeMap<String, String>,
        static_dir: PathBuf,
    ) -> Self {
        Self {
            gateway,
            home,
            workdir,
            config_path,
            inherited_env,
            static_dir: Some(static_dir),
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            token: Uuid::now_v7().to_string(),
            managed_state_path: None,
        }
    }

    pub fn headless(
        gateway: Gateway,
        home: PathBuf,
        workdir: PathBuf,
        config_path: Option<PathBuf>,
        inherited_env: BTreeMap<String, String>,
        token: String,
    ) -> Self {
        Self {
            gateway,
            home,
            workdir,
            config_path,
            inherited_env,
            static_dir: None,
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            token,
            managed_state_path: None,
        }
    }
}

pub struct BoundGatewayWebServer {
    listener: TcpListener,
    app: Router,
    local_addr: SocketAddr,
    token: String,
}

impl BoundGatewayWebServer {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.local_addr)
    }

    pub async fn run(self) -> psychevo_runtime::Result<()> {
        axum::serve(self.listener, self.app.into_make_service()).await?;
        Ok(())
    }
}

pub async fn bind_gateway_web_server(
    config: GatewayWebServerConfig,
) -> psychevo_runtime::Result<BoundGatewayWebServer> {
    let listener = TcpListener::bind(config.bind_addr).await?;
    let local_addr = listener.local_addr()?;
    let token = config.token.clone();
    if let Some(path) = &config.managed_state_path {
        write_managed_state(path, local_addr, &config)?;
    }
    let state = WebState::new(config);
    let app = Router::new()
        .route("/readyz", get(readyz))
        .route("/health", get(readyz))
        .route("/_gateway/launch", post(create_launch))
        .route("/_gateway/launch/{launch_id}", get(consume_launch))
        .route("/ws", get(ws_handler))
        .route(
            "/download/session/{session_id}/{kind}",
            get(download_session),
        )
        .fallback(get(static_asset))
        .with_state(state);
    Ok(BoundGatewayWebServer {
        listener,
        app,
        local_addr,
        token,
    })
}

fn write_managed_state(
    path: &Path,
    local_addr: SocketAddr,
    config: &GatewayWebServerConfig,
) -> psychevo_runtime::Result<()> {
    let executable = executable_fingerprint(&std::env::current_exe()?)?;
    let state = wire::ManagedServerState {
        pid: std::process::id(),
        base_url: format!("http://{local_addr}"),
        readyz_url: format!("http://{local_addr}/readyz"),
        started_at_ms: now_ms(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        executable_path: Some(executable.path),
        executable_modified_ms: Some(executable.modified_ms),
        executable_size: Some(executable.size),
        executable_inode: executable.inode,
        static_dir: config
            .static_dir
            .as_deref()
            .map(canonical_path_string)
            .transpose()?,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(&state)?)?;
    Ok(())
}

struct ExecutableFingerprint {
    path: String,
    modified_ms: i64,
    size: u64,
    inode: Option<u64>,
}

fn executable_fingerprint(path: &Path) -> psychevo_runtime::Result<ExecutableFingerprint> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let metadata = std::fs::metadata(&path)?;
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default();
    Ok(ExecutableFingerprint {
        path: path.display().to_string(),
        modified_ms,
        size: metadata.len(),
        inode: executable_inode(&metadata),
    })
}

fn canonical_path_string(path: &Path) -> psychevo_runtime::Result<String> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    Ok(path.display().to_string())
}

#[cfg(unix)]
fn executable_inode(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;

    Some(metadata.ino())
}

#[cfg(not(unix))]
fn executable_inode(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[derive(Clone)]
struct WebState {
    inner: Arc<WebStateInner>,
}

struct WebStateInner {
    gateway: Gateway,
    state: StateRuntime,
    home: PathBuf,
    workdir: PathBuf,
    config_path: Option<PathBuf>,
    inherited_env: BTreeMap<String, String>,
    static_dir: Option<PathBuf>,
    token: String,
    source: GatewaySource,
    launches: Mutex<HashMap<String, LaunchEntry>>,
    browser_sessions: Mutex<HashMap<String, BrowserSession>>,
    pending_permissions: Mutex<HashMap<String, PendingPermissionView>>,
    pending_clarifies: Mutex<HashMap<String, PendingClarifyView>>,
}

#[derive(Debug, Clone)]
struct BrowserSession {
    workdir: PathBuf,
    source: GatewaySource,
}

#[derive(Debug, Clone)]
struct LaunchEntry {
    open_token: String,
    expires_at_ms: i64,
    workdir: PathBuf,
    source: GatewaySource,
}

#[derive(Debug, Clone)]
enum AuthContext {
    Bearer,
    Browser(BrowserSession),
}

impl AuthContext {
    fn is_bearer(&self) -> bool {
        matches!(self, Self::Bearer)
    }
}

impl WebState {
    fn new(config: GatewayWebServerConfig) -> Self {
        let state = config.gateway.state().clone();
        let source = workdir_source(&config.workdir);
        Self {
            inner: Arc::new(WebStateInner {
                gateway: config.gateway,
                state,
                home: config.home,
                workdir: config.workdir,
                config_path: config.config_path,
                inherited_env: config.inherited_env,
                static_dir: config.static_dir,
                token: config.token,
                source,
                launches: Mutex::new(HashMap::new()),
                browser_sessions: Mutex::new(HashMap::new()),
                pending_permissions: Mutex::new(HashMap::new()),
                pending_clarifies: Mutex::new(HashMap::new()),
            }),
        }
    }

    fn auth_from_headers(&self, headers: &HeaderMap) -> Option<AuthContext> {
        if bearer_token(headers).is_some_and(|token| token == self.inner.token) {
            return Some(AuthContext::Bearer);
        }
        let cookie = headers
            .get(COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(session_cookie_value)?;
        self.inner
            .browser_sessions
            .lock()
            .expect("web browser sessions poisoned")
            .get(cookie)
            .cloned()
            .map(AuthContext::Browser)
    }

    fn selector(&self, source: &GatewaySource) -> GatewayThreadSelector {
        GatewayThreadSelector::source(source.source_key())
    }

    fn activity(&self, source: &GatewaySource, thread_id: Option<&str>) -> GatewayActivity {
        match thread_id {
            Some(thread_id) => self
                .inner
                .gateway
                .activity_for_selector(GatewayThreadSelector::thread_id(thread_id)),
            None => self
                .inner
                .gateway
                .activity_for_selector(self.selector(source)),
        }
    }

    fn run_options(&self, workdir: PathBuf, thread_id: Option<String>) -> RunOptions {
        RunOptions {
            state: self.inner.state.clone(),
            workdir,
            snapshot_root: Some(self.inner.home.join("snapshots")),
            session: thread_id,
            continue_latest: false,
            prompt: String::new(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: true,
            prompt_display: None,
            max_context_messages: None,
            config_path: self.inner.config_path.clone(),
            project_context_override: None,
            model: None,
            reasoning_effort: None,
            include_reasoning: false,
            mode: RunMode::Default,
            permission_mode: Some(PermissionMode::Default),
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: true,
            inherited_env: Some(self.inner.inherited_env.clone()),
            agent: None,
            no_agents: false,
            no_skills: false,
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
        }
    }

    fn record_event(&self, event: &GatewayEvent) {
        match event {
            GatewayEvent::PermissionRequested {
                request_id,
                tool_name,
                reason,
                ..
            } => {
                self.inner
                    .pending_permissions
                    .lock()
                    .expect("web pending permissions poisoned")
                    .insert(
                        request_id.clone(),
                        PendingPermissionView {
                            request_id: request_id.clone(),
                            tool_name: tool_name.clone(),
                            reason: reason.clone(),
                        },
                    );
            }
            GatewayEvent::PermissionResolved { request_id, .. } => {
                self.inner
                    .pending_permissions
                    .lock()
                    .expect("web pending permissions poisoned")
                    .remove(request_id);
            }
            GatewayEvent::ClarifyRequested { request_id, raw } => {
                self.inner
                    .pending_clarifies
                    .lock()
                    .expect("web pending clarifies poisoned")
                    .insert(
                        request_id.clone(),
                        PendingClarifyView {
                            request_id: request_id.clone(),
                            raw: raw.clone(),
                        },
                    );
            }
            GatewayEvent::ClarifyResolved { request_id, .. } => {
                self.inner
                    .pending_clarifies
                    .lock()
                    .expect("web pending clarifies poisoned")
                    .remove(request_id);
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingPermissionView {
    request_id: String,
    tool_name: String,
    reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingClarifyView {
    request_id: String,
    raw: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentListParams {
    scope: Option<wire::GatewayRequestScope>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentReadParams {
    name: String,
    scope: Option<wire::GatewayRequestScope>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentBackendRefInput {
    #[serde(rename = "ref")]
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentWriteParams {
    name: String,
    description: String,
    #[serde(default)]
    instructions: String,
    #[serde(default)]
    backend: Option<AgentBackendRefInput>,
    #[serde(default)]
    entrypoints: Vec<String>,
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default, rename = "mcpServers")]
    mcp_servers: Vec<String>,
    scope: Option<wire::GatewayRequestScope>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentDeleteParams {
    name: String,
    scope: Option<wire::GatewayRequestScope>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackendListParams {
    scope: Option<wire::GatewayRequestScope>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackendDoctorParams {
    id: String,
    scope: Option<wire::GatewayRequestScope>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandListParams {
    thread_id: Option<String>,
    scope: Option<wire::GatewayRequestScope>,
}

async fn readyz() -> impl IntoResponse {
    Json(wire::ReadyzResult {
        ok: true,
        server: "psychevo-gateway".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchQuery {
    open_token: String,
}

async fn ws_handler(
    State(state): State<WebState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let Some(auth) = state.auth_from_headers(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    ws.on_upgrade(move |socket| handle_socket(socket, state, auth))
}

async fn create_launch(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(params): Json<wire::CreateLaunchParams>,
) -> impl IntoResponse {
    if !state
        .auth_from_headers(&headers)
        .is_some_and(|auth| auth.is_bearer())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let workdir = match canonicalize_workdir(Path::new(&params.workdir)) {
        Ok(workdir) => workdir,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": err.to_string()}})),
            )
                .into_response();
        }
    };
    let source = source_from_input(
        params.source,
        &workdir,
        wire::GatewaySourceLifetime::Persistent,
    );
    let launch_id = Uuid::now_v7().to_string();
    let open_token = Uuid::now_v7().to_string();
    let expires_at_ms = now_ms() + 30_000;
    state
        .inner
        .launches
        .lock()
        .expect("web launches poisoned")
        .insert(
            launch_id.clone(),
            LaunchEntry {
                open_token: open_token.clone(),
                expires_at_ms,
                workdir,
                source,
            },
        );
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("127.0.0.1");
    let open_url = format!("http://{host}/_gateway/launch/{launch_id}?openToken={open_token}");
    Json(wire::CreateLaunchResult {
        launch_id,
        expires_at_ms,
        open_url,
    })
    .into_response()
}

async fn consume_launch(
    State(state): State<WebState>,
    AxumPath(launch_id): AxumPath<String>,
    Query(query): Query<LaunchQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let entry = {
        let mut launches = state.inner.launches.lock().expect("web launches poisoned");
        let Some(entry) = launches.remove(&launch_id) else {
            if state.auth_from_headers(&headers).is_some() {
                return shell_redirect().into_response();
            }
            return launch_expired_page(StatusCode::NOT_FOUND).into_response();
        };
        entry
    };
    if entry.expires_at_ms < now_ms() || entry.open_token != query.open_token {
        if state.auth_from_headers(&headers).is_some() {
            return shell_redirect().into_response();
        }
        return launch_expired_page(StatusCode::UNAUTHORIZED).into_response();
    }
    let session_id = Uuid::now_v7().to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("web browser sessions poisoned")
        .insert(
            session_id.clone(),
            BrowserSession {
                workdir: entry.workdir,
                source: entry.source,
            },
        );
    let mut response = shell_redirect();
    let secure = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|proto| proto == "https");
    let cookie = if secure {
        format!("psychevo_gateway_session={session_id}; Path=/; HttpOnly; SameSite=Lax; Secure")
    } else {
        format!("psychevo_gateway_session={session_id}; Path=/; HttpOnly; SameSite=Lax")
    };
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response.into_response()
}

fn shell_redirect() -> Response<Body> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    response
        .headers_mut()
        .insert(LOCATION, HeaderValue::from_static("/"));
    response
}

async fn handle_socket(socket: WebSocket, state: WebState, auth: AuthContext) {
    let (mut sender, mut receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            if sender.send(WsMessage::Text(message.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(message) = receiver.next().await {
        let Ok(message) = message else {
            break;
        };
        match message {
            WsMessage::Text(text) => {
                let response = handle_rpc_text(&state, &auth, out_tx.clone(), text.as_str()).await;
                if let Some(response) = response {
                    let _ = out_tx.send(response);
                }
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }
    drop(out_tx);
    let _ = writer.await;
}

async fn handle_rpc_text(
    state: &WebState,
    auth: &AuthContext,
    out_tx: mpsc::UnboundedSender<String>,
    text: &str,
) -> Option<String> {
    let request = match serde_json::from_str::<RpcRequest>(text) {
        Ok(request) => request,
        Err(err) => {
            return Some(rpc_error(
                Value::Null,
                -32700,
                format!("invalid json: {err}"),
            ));
        }
    };
    if request.jsonrpc != wire::JSONRPC_VERSION {
        return Some(rpc_error(
            request.id.clone().unwrap_or(Value::Null),
            -32600,
            "invalid JSON-RPC version".to_string(),
        ));
    }
    let id = request.id.clone()?;
    let result = handle_rpc(state.clone(), auth.clone(), out_tx, request).await;
    Some(match result {
        Ok(value) => rpc_result(id, value),
        Err(err) => rpc_error(id, -32000, err.to_string()),
    })
}

async fn handle_rpc(
    state: WebState,
    auth: AuthContext,
    out_tx: mpsc::UnboundedSender<String>,
    request: RpcRequest,
) -> psychevo_runtime::Result<Value> {
    match request.method.as_str() {
        "initialize" => {
            let scope = default_resolved_scope(&state, &auth)?;
            Ok(json!({
            "server": "psychevo-gateway",
            "version": env!("CARGO_PKG_VERSION"),
            "cwd": scope.workdir,
            "scope": scope.to_wire_scope(),
            "source": scope.source,
            "capabilities": {
                "threads": true,
                "turns": true,
                "historyManagement": true,
                "downloads": true,
                "settingsWrite": "structured",
                "memoryResources": "status_only"
            }
            }))
        }
        "thread/start" => {
            let params = request.required_params::<wire::ThreadStartParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            start_empty_thread(&state, &scope)
        }
        "thread/resume" => {
            let params = request.params::<wire::ThreadResumeParams>()?;
            let thread_id = match params.thread_id {
                Some(thread_id) => {
                    authorize_thread(&state, &auth, &thread_id)?;
                    if let Some(scope) = params.scope {
                        let scope = resolve_required_scope(&state, &auth, scope)?;
                        bind_source_to_thread(&state, &scope, &thread_id)?;
                    }
                    Some(thread_id)
                }
                None => {
                    let scope = resolve_optional_scope(&state, &auth, params.scope)?;
                    state.inner.gateway.resolve_source_thread(&scope.source)?
                }
            };
            let scope = default_resolved_scope(&state, &auth)?;
            thread_snapshot(&state, &scope, thread_id.as_deref())
        }
        "thread/read" => {
            let params = request.required_params::<wire::ThreadReadParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            let scope = default_resolved_scope(&state, &auth)?;
            thread_snapshot(&state, &scope, Some(&params.thread_id))
        }
        "thread/list" => {
            let params = request.params::<wire::ThreadListParams>()?;
            let limit = params.limit.unwrap_or(50).clamp(1, 200);
            let workdir = resolve_workdir_filter(&state, &auth, params.workdir)?;
            let store = state.inner.state.store();
            let sessions = if params.archived.unwrap_or(false) {
                store.list_archived_sessions_for_workdir_with_sources(&workdir, HISTORY_SOURCES)?
            } else {
                store.list_sessions_for_workdir_with_sources(&workdir, HISTORY_SOURCES)?
            };
            Ok(json!({
                "sessions": sessions.into_iter().take(limit).map(|session| session_summary_value(&state, session)).collect::<Vec<_>>(),
            }))
        }
        "thread/rename" => {
            let params = request.required_params::<wire::ThreadRenameParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            state
                .inner
                .state
                .store()
                .set_session_title(&params.thread_id, &params.title)?;
            Ok(json!({"session": session_summary_by_id(&state, &params.thread_id)?}))
        }
        "thread/archive" => {
            let params = request.required_params::<wire::ThreadIdParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            guard_session_mutation(&state, &auth, &params.thread_id, true)?;
            state
                .inner
                .state
                .store()
                .archive_session(&params.thread_id)?;
            Ok(json!({"session": session_summary_by_id(&state, &params.thread_id)?}))
        }
        "thread/restore" => {
            let params = request.required_params::<wire::ThreadIdParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            guard_session_mutation(&state, &auth, &params.thread_id, true)?;
            state
                .inner
                .state
                .store()
                .restore_session(&params.thread_id)?;
            Ok(json!({"session": session_summary_by_id(&state, &params.thread_id)?}))
        }
        "thread/delete" => {
            let params = request.required_params::<wire::ThreadIdParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            guard_session_mutation(&state, &auth, &params.thread_id, false)?;
            state
                .inner
                .state
                .store()
                .delete_session(&params.thread_id)?;
            Ok(json!({"deleted": true, "threadId": params.thread_id}))
        }
        "turn/start" => {
            let params = request.required_params::<wire::TurnStartParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            let thread_id = match params.thread_id.clone() {
                Some(thread_id) => {
                    authorize_thread(&state, &auth, &thread_id)?;
                    thread_id
                }
                None => ensure_source_thread_for_turn(&state, &scope)?,
            };
            if state
                .inner
                .gateway
                .resolve_source_thread(&scope.source)?
                .as_deref()
                != Some(thread_id.as_str())
            {
                bind_source_to_thread(&state, &scope, &thread_id)?;
            }
            let input = params.input_parts()?;
            let mut options = state.run_options(scope.workdir.clone(), Some(thread_id.clone()));
            options.model = params.model;
            options.reasoning_effort = params.reasoning_effort;
            options.agent = params.agent_name.clone();
            apply_mentions_to_run_options(&mut options, &params.mentions);
            let source = scope.source.clone();
            let event_state = state.clone();
            let event_tx = out_tx.clone();
            let event_sink: GatewayEventSink = Arc::new(move |event| {
                event_state.record_event(&event);
                let _ = event_tx.send(rpc_notification("gateway/event", json!(event)));
            });
            let gateway = state.inner.gateway.clone();
            tokio::spawn(async move {
                let result = gateway
                    .send_turn(crate::SendTurnRequest {
                        thread_id: Some(thread_id),
                        source: Some(source),
                        reset_source_binding: false,
                        input,
                        options,
                        runtime_source: Some("web".to_string()),
                        continue_sources: vec![
                            "run".to_string(),
                            "tui".to_string(),
                            "web".to_string(),
                        ],
                        stream: None,
                        event_sink: Some(event_sink.clone()),
                        control_handle: None,
                        control: None,
                        lineage: None,
                    })
                    .await;
                let notification = match result {
                    Ok(result) => {
                        rpc_notification("turn/result", gateway_turn_result_value(result))
                    }
                    Err(err) => rpc_notification("turn/error", json!({"message": err.to_string()})),
                };
                let _ = out_tx.send(notification);
            });
            Ok(json!({"accepted": true}))
        }
        "turn/steer" => {
            let params = request.required_params::<wire::TurnSteerParams>()?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let message = RuntimeMessage::User {
                content: vec![UserContentBlock::text(params.text)],
                timestamp_ms: gateway_now_ms(),
            };
            let selector = selector_from_thread_or_default(&state, &auth, params.thread_id)?;
            let accepted =
                state
                    .inner
                    .gateway
                    .steer_turn(selector, Some(&params.expected_turn_id), message);
            Ok(json!({"accepted": accepted.is_some()}))
        }
        "turn/interrupt" => {
            let params = request.params::<wire::TurnInterruptParams>()?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let selector = if let Some(thread_id) = params.thread_id {
                GatewayThreadSelector::thread_id(thread_id)
            } else if let Some(source_key) = params.source_key {
                GatewayThreadSelector::source(source_key)
            } else {
                let scope = default_resolved_scope(&state, &auth)?;
                state.selector(&scope.source)
            };
            let interrupted = state.inner.gateway.interrupt_turn(selector.clone());
            let cleared = state.inner.gateway.clear_queue(selector);
            Ok(json!({"interrupted": interrupted, "cleared": cleared}))
        }
        "completion/list" => {
            let params = request.required_params::<wire::CompletionListParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            completion_list_value(&state, &scope, params)
        }
        "source/reset" => {
            let params = request.required_params::<wire::SourceResetParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            reset_source_to_empty_thread(&state, &scope)
        }
        "permission/respond" => {
            let params = request.required_params::<wire::PermissionRespondParams>()?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let decision = permission_decision(params.decision);
            let selector = selector_from_thread_or_default(&state, &auth, params.thread_id)?;
            let accepted =
                state
                    .inner
                    .gateway
                    .submit_permission(selector, &params.request_id, decision);
            Ok(json!({"accepted": accepted}))
        }
        "clarify/respond" => {
            let params = request.required_params::<wire::ClarifyRespondParams>()?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let result = if params.cancel.unwrap_or(false) {
                ClarifyResult::Cancelled
            } else {
                ClarifyResult::Answered(ClarifyResponse {
                    answers: params
                        .answers
                        .unwrap_or_default()
                        .into_iter()
                        .map(|answers| ClarifyAnswer { answers })
                        .collect(),
                })
            };
            let selector = selector_from_thread_or_default(&state, &auth, params.thread_id)?;
            let accepted = state
                .inner
                .gateway
                .submit_clarify(selector, &params.request_id, result);
            Ok(json!({"accepted": accepted}))
        }
        "agent/list" => {
            let params = request.params::<AgentListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let catalog = discover_gateway_agents(&state, &scope)?;
            Ok(list_agents_value(&catalog))
        }
        "agent/read" => {
            let params = request.required_params::<AgentReadParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let catalog = discover_gateway_agents(&state, &scope)?;
            let agent = resolve_agent_definition(
                &catalog,
                &params.name,
                &scope.workdir,
                &state.inner.inherited_env,
            )?;
            Ok(view_agent_value_with_catalog(&agent, Some(&catalog)))
        }
        "agent/write" => {
            let params = request.required_params::<AgentWriteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            write_project_agent_definition(&scope.workdir, params)
        }
        "agent/delete" => {
            let params = request.required_params::<AgentDeleteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            delete_project_agent_definition(&scope.workdir, &params.name)
        }
        "backend/list" => {
            let params = request.params::<BackendListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let backends = load_agent_backend_configs(
                &state.inner.home,
                &scope.workdir,
                &state.inner.inherited_env,
            )?;
            Ok(json!({
                "backends": backends.values().map(backend_value).collect::<Vec<_>>()
            }))
        }
        "backend/doctor" => {
            let params = request.required_params::<BackendDoctorParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let backends = load_agent_backend_configs(
                &state.inner.home,
                &scope.workdir,
                &state.inner.inherited_env,
            )?;
            let backend = backends
                .get(&params.id)
                .ok_or_else(|| Error::Message(format!("unknown backend: {}", params.id)))?;
            Ok(backend_doctor_value(backend, &state.inner.inherited_env)?)
        }
        "command/list" => {
            let params = request.params::<CommandListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let active_turn = if let Some(thread_id) = params.thread_id.as_deref() {
                authorize_thread(&state, &auth, thread_id)?;
                state.activity(&scope.source, Some(thread_id)).running
            } else {
                state.activity(&scope.source, None).running
            };
            Ok(command_list_value(active_turn))
        }
        "command/execute" => {
            let params = request.required_params::<wire::CommandExecuteParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            command_execute_value(&state, &scope, params)
        }
        "settings/read" => {
            let params = request.params::<wire::SettingsReadParams>()?;
            let workdir = resolve_workdir_filter(&state, &auth, params.workdir)?;
            Ok(json!({
            "workdir": workdir,
            "memoryResources": {"mode": "status_only", "available": true},
            "secrets": {"frontendPersistence": "disabled"}
            }))
        }
        method => Err(Error::Message(format!("method not found: {method}"))),
    }
}

#[derive(Debug, Clone)]
struct ResolvedScope {
    workdir: PathBuf,
    source: GatewaySource,
}

impl ResolvedScope {
    fn to_wire_scope(&self) -> wire::GatewayRequestScope {
        wire::GatewayRequestScope {
            workdir: self.workdir.display().to_string(),
            source: wire::GatewaySourceInput {
                kind: self.source.kind.clone(),
                raw_id: Some(self.source.raw_id.clone()),
                lifetime: Some(self.source.lifetime),
                raw_identity: self.source.raw_identity.clone(),
                visible_name: self.source.visible_name.clone(),
            },
        }
    }
}

fn start_empty_thread(state: &WebState, scope: &ResolvedScope) -> psychevo_runtime::Result<Value> {
    let session_id = create_empty_thread_session(state, scope)?;
    state.inner.gateway.bind_source_thread(
        &scope.source,
        &session_id,
        &GatewayBackendInfo {
            kind: BackendKind::Psychevo,
            native_id: Some(session_id.clone()),
        },
        Some(json!({"reason": "thread_start"})),
    )?;
    thread_snapshot(state, scope, Some(&session_id))
}

fn ensure_source_thread_for_turn(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<String> {
    if let Some(thread_id) = state.inner.gateway.resolve_source_thread(&scope.source)? {
        return Ok(thread_id);
    }
    let session_id = create_empty_thread_session(state, scope)?;
    state.inner.gateway.bind_source_thread(
        &scope.source,
        &session_id,
        &GatewayBackendInfo {
            kind: BackendKind::Psychevo,
            native_id: Some(session_id.clone()),
        },
        Some(json!({"reason": "turn_start"})),
    )?;
    Ok(session_id)
}

fn create_empty_thread_session(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<String> {
    state.inner.state.store().create_session_with_metadata(
        &scope.workdir,
        &scope.source.kind,
        "pending",
        "pending",
        None,
    )
}

fn reset_source_to_empty_thread(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<Value> {
    let session_id = state.inner.state.store().create_session_with_metadata(
        &scope.workdir,
        &scope.source.kind,
        "pending",
        "pending",
        Some(json!({"source_reset": true})),
    )?;
    state
        .inner
        .gateway
        .reset_source(&scope.source, &session_id)?;
    thread_snapshot(state, scope, Some(&session_id))
}

fn bind_source_to_thread(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: &str,
) -> psychevo_runtime::Result<()> {
    if let Some(bound) = state.inner.gateway.resolve_source_thread(&scope.source)?
        && bound == thread_id
    {
        return Ok(());
    }
    state.inner.gateway.bind_source_thread(
        &scope.source,
        thread_id,
        &gateway_backend_info_for_thread(state, thread_id)?,
        Some(json!({"reason": "thread_resume"})),
    )?;
    Ok(())
}

fn gateway_backend_info_for_thread(
    state: &WebState,
    thread_id: &str,
) -> psychevo_runtime::Result<GatewayBackendInfo> {
    let store = state.inner.state.store();
    let summary = store
        .session_summary(thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
    if summary.source == "peer_agent" {
        let native_id = store
            .session_metadata(thread_id)?
            .and_then(|metadata| metadata.get("peer_agent").cloned())
            .and_then(|peer| {
                peer.get("nativeSessionId")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .or_else(|| Some(thread_id.to_string()));
        Ok(GatewayBackendInfo {
            kind: BackendKind::PeerAgent,
            native_id,
        })
    } else {
        Ok(GatewayBackendInfo {
            kind: BackendKind::Psychevo,
            native_id: Some(thread_id.to_string()),
        })
    }
}

fn default_resolved_scope(
    state: &WebState,
    auth: &AuthContext,
) -> psychevo_runtime::Result<ResolvedScope> {
    match auth {
        AuthContext::Bearer => Ok(ResolvedScope {
            workdir: state.inner.workdir.clone(),
            source: state.inner.source.clone(),
        }),
        AuthContext::Browser(session) => Ok(ResolvedScope {
            workdir: session.workdir.clone(),
            source: session.source.clone(),
        }),
    }
}

fn resolve_optional_scope(
    state: &WebState,
    auth: &AuthContext,
    scope: Option<wire::GatewayRequestScope>,
) -> psychevo_runtime::Result<ResolvedScope> {
    match scope {
        Some(scope) => resolve_required_scope(state, auth, scope),
        None => default_resolved_scope(state, auth),
    }
}

fn resolve_required_scope(
    _state: &WebState,
    auth: &AuthContext,
    scope: wire::GatewayRequestScope,
) -> psychevo_runtime::Result<ResolvedScope> {
    let workdir = canonicalize_workdir(Path::new(&scope.workdir))?;
    authorize_workdir(auth, &workdir)?;
    Ok(ResolvedScope {
        source: source_from_input(
            Some(scope.source),
            &workdir,
            wire::GatewaySourceLifetime::Persistent,
        ),
        workdir,
    })
}

fn resolve_workdir_filter(
    state: &WebState,
    auth: &AuthContext,
    workdir: Option<String>,
) -> psychevo_runtime::Result<PathBuf> {
    let workdir = match workdir {
        Some(workdir) => canonicalize_workdir(Path::new(&workdir))?,
        None => default_resolved_scope(state, auth)?.workdir,
    };
    authorize_workdir(auth, &workdir)?;
    Ok(workdir)
}

fn discover_gateway_agents(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<psychevo_runtime::AgentCatalog> {
    discover_agents(&AgentDiscoveryOptions {
        home: state.inner.home.clone(),
        workdir: scope.workdir.clone(),
        env: state.inner.inherited_env.clone(),
        explicit_inputs: Vec::new(),
        no_agents: false,
    })
}

fn discover_gateway_skills(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<psychevo_runtime::SkillCatalog> {
    discover_skills(&SkillDiscoveryOptions {
        home: state.inner.home.clone(),
        workdir: scope.workdir.clone(),
        config_path: state.inner.config_path.clone(),
        env: state.inner.inherited_env.clone(),
        explicit_inputs: Vec::new(),
        no_skills: false,
    })
}

#[derive(Debug, Clone)]
struct CompletionToken {
    sigil: char,
    query: String,
    start: usize,
    end: usize,
}

fn completion_list_value(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::CompletionListParams,
) -> psychevo_runtime::Result<Value> {
    let Some(token) = active_completion_token(&params.text, params.cursor) else {
        return Ok(serde_json::to_value(wire::CompletionListResult {
            items: Vec::new(),
            replacement: None,
        })?);
    };
    let query = token.query.to_ascii_lowercase();
    let mut items = match token.sigil {
        '/' => slash_completion_items(state, scope, params.thread_id.as_deref(), &query),
        '$' => dollar_completion_items(state, scope, &query)?,
        '@' => file_completion_items(&scope.workdir, &query)?,
        _ => Vec::new(),
    };
    items.truncate(MAX_COMPLETION_ITEMS);
    Ok(serde_json::to_value(wire::CompletionListResult {
        items,
        replacement: Some(wire::CompletionReplacement {
            start: token.start,
            end: token.end,
        }),
    })?)
}

fn active_completion_token(text: &str, cursor: usize) -> Option<CompletionToken> {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let prefix = &text[..cursor];
    for (idx, ch) in prefix.char_indices().rev() {
        if ch.is_whitespace() {
            return None;
        }
        if !matches!(ch, '/' | '$' | '@') {
            continue;
        }
        if ch == '/' {
            let line_prefix = prefix[..idx].rsplit('\n').next().unwrap_or_default();
            if !line_prefix.trim().is_empty() {
                continue;
            }
        }
        let query = prefix[idx + ch.len_utf8()..].to_string();
        return Some(CompletionToken {
            sigil: ch,
            query,
            start: idx,
            end: cursor,
        });
    }
    None
}

fn slash_completion_items(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
    query: &str,
) -> Vec<wire::CompletionItem> {
    let active_turn = thread_id
        .map(|thread_id| state.activity(&scope.source, Some(thread_id)).running)
        .unwrap_or_else(|| state.activity(&scope.source, None).running);
    let available = available_slash_commands_for_surface(
        &gateway_command_capabilities(),
        active_turn,
        &[],
        MAX_COMPLETION_ITEMS,
    );
    available
        .commands
        .into_iter()
        .filter(|command| command_matches(command, query))
        .map(|command| wire::CompletionItem {
            id: format!("command:{}", command.name),
            sigil: "/".to_string(),
            label: format!("/{}", command.name),
            insert_text: format!("/{}", command.name),
            kind: "command".to_string(),
            detail: Some(command.summary.to_string()),
            target: None,
            sort_text: Some(format!("command:{}", command.name)),
        })
        .collect()
}

fn command_matches(command: &AvailableSlashCommand, query: &str) -> bool {
    query.is_empty()
        || command.name.contains(query)
        || command.aliases.iter().any(|alias| alias.contains(query))
        || command.summary.to_ascii_lowercase().contains(query)
}

fn dollar_completion_items(
    state: &WebState,
    scope: &ResolvedScope,
    query: &str,
) -> psychevo_runtime::Result<Vec<wire::CompletionItem>> {
    let mut items = Vec::new();
    let skill_catalog = discover_gateway_skills(state, scope)?;
    let skills = list_skills_value_with_options(
        &skill_catalog,
        &ListSkillsOptions {
            detail: true,
            enabled_only: true,
            ..ListSkillsOptions::default()
        },
    );
    if let Some(skills) = skills.get("skills").and_then(Value::as_array) {
        for skill in skills {
            let Some(name) = skill.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !completion_name_matches(
                name,
                skill.get("description").and_then(Value::as_str),
                query,
            ) {
                continue;
            }
            let path = skill
                .get("location")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            items.push(wire::CompletionItem {
                id: format!("skill:{name}"),
                sigil: "$".to_string(),
                label: format!("${name}"),
                insert_text: format!("${name}"),
                kind: "skill".to_string(),
                detail: skill
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                target: Some(wire::GatewayMentionTarget::Skill {
                    name: name.to_string(),
                    path,
                }),
                sort_text: Some(completion_sort_text(
                    query,
                    name,
                    skill.get("description").and_then(Value::as_str),
                    "skill",
                )),
            });
        }
    }

    let agent_catalog = discover_gateway_agents(state, scope)?;
    for agent in agent_catalog.agents {
        if !completion_name_matches(&agent.name, Some(&agent.description), query) {
            continue;
        }
        let name = agent.name.clone();
        let description = agent.description.clone();
        let sort_text = completion_sort_text(query, &name, Some(&description), "agent");
        let entrypoints = agent
            .entrypoints
            .iter()
            .map(|entrypoint| (*entrypoint).as_str().to_string())
            .collect::<Vec<_>>();
        items.push(wire::CompletionItem {
            id: format!("agent:{name}"),
            sigil: "$".to_string(),
            label: format!("${name}"),
            insert_text: format!("${name}"),
            kind: "agent".to_string(),
            detail: Some(description),
            target: Some(wire::GatewayMentionTarget::Agent {
                name,
                source: Some(agent.source.as_str().to_string()),
                entrypoints,
                backend_ref: agent.backend.map(|backend| backend.name),
            }),
            sort_text: Some(sort_text),
        });
    }
    items.sort_by(|left, right| {
        left.sort_text
            .cmp(&right.sort_text)
            .then(left.label.cmp(&right.label))
    });
    Ok(items)
}

fn completion_name_matches(name: &str, description: Option<&str>, query: &str) -> bool {
    query.is_empty()
        || name.to_ascii_lowercase().contains(query)
        || description.is_some_and(|description| description.to_ascii_lowercase().contains(query))
}

fn completion_sort_text(query: &str, name: &str, description: Option<&str>, kind: &str) -> String {
    let name_lower = name.to_ascii_lowercase();
    let description_lower = description.map(str::to_ascii_lowercase).unwrap_or_default();
    let rank = if query.is_empty() {
        2
    } else if name_lower == query {
        0
    } else if name_lower.starts_with(query) {
        1
    } else if name_lower
        .split(['-', '_', '/', '.'])
        .any(|part| part.starts_with(query))
    {
        2
    } else if name_lower.contains(query) {
        3
    } else if description_lower.contains(query) {
        4
    } else {
        9
    };
    format!("{rank}:{kind}:{name_lower}")
}

fn file_completion_items(
    workdir: &Path,
    query: &str,
) -> psychevo_runtime::Result<Vec<wire::CompletionItem>> {
    let mut items = Vec::new();
    collect_file_completion_items(workdir, workdir, query, 0, &mut items);
    items.sort_by(|left, right| left.label.cmp(&right.label));
    items.truncate(MAX_FILE_COMPLETION_ITEMS);
    Ok(items)
}

fn collect_file_completion_items(
    root: &Path,
    dir: &Path,
    query: &str,
    depth: usize,
    items: &mut Vec<wire::CompletionItem>,
) {
    if depth > MAX_FILE_COMPLETION_DEPTH || items.len() >= MAX_FILE_COMPLETION_ITEMS {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if items.len() >= MAX_FILE_COMPLETION_ITEMS {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if should_skip_completion_path(&name) {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let is_dir = path.is_dir();
        let label = if is_dir {
            format!("@{relative}/")
        } else {
            format!("@{relative}")
        };
        if query.is_empty() || relative.to_ascii_lowercase().contains(query) {
            items.push(wire::CompletionItem {
                id: format!("file:{relative}"),
                sigil: "@".to_string(),
                label: label.clone(),
                insert_text: label,
                kind: if is_dir { "directory" } else { "file" }.to_string(),
                detail: Some(relative.clone()),
                target: Some(wire::GatewayMentionTarget::File {
                    path: path.display().to_string(),
                    relative_path: relative.clone(),
                }),
                sort_text: Some(relative.clone()),
            });
        }
        if is_dir {
            collect_file_completion_items(root, &path, query, depth + 1, items);
        }
    }
}

fn should_skip_completion_path(name: &str) -> bool {
    matches!(name, ".git" | ".local" | "target" | "node_modules")
}

fn command_execute_value(
    _state: &WebState,
    _scope: &ResolvedScope,
    params: wire::CommandExecuteParams,
) -> psychevo_runtime::Result<Value> {
    let raw = params.command.trim().to_string();
    let thread_id = params.thread_id.clone();
    let command = raw.trim_start_matches('/');
    let mut parts = command.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or_default();
    let args = parts.next().unwrap_or_default().trim();
    let result = match name {
        "" => wire::CommandExecuteResult {
            accepted: false,
            command: raw.to_string(),
            message: Some("empty command".to_string()),
            action: None,
        },
        "new" | "clear" => wire::CommandExecuteResult {
            accepted: true,
            command: name.to_string(),
            message: None,
            action: Some(json!({"type": "threadStart"})),
        },
        "sessions" | "history" => wire::CommandExecuteResult {
            accepted: true,
            command: name.to_string(),
            message: None,
            action: Some(json!({"type": "showPanel", "panel": "history"})),
        },
        "archive" => wire::CommandExecuteResult {
            accepted: true,
            command: name.to_string(),
            message: None,
            action: Some(json!({"type": "threadArchive", "threadId": thread_id.clone()})),
        },
        "delete" => wire::CommandExecuteResult {
            accepted: true,
            command: name.to_string(),
            message: None,
            action: Some(json!({"type": "threadDelete", "threadId": thread_id.clone()})),
        },
        "stop" | "cancel" | "interrupt" => wire::CommandExecuteResult {
            accepted: true,
            command: name.to_string(),
            message: None,
            action: Some(json!({"type": "turnInterrupt", "threadId": thread_id.clone()})),
        },
        "queue" => wire::CommandExecuteResult {
            accepted: true,
            command: name.to_string(),
            message: None,
            action: Some(json!({"type": "queuePrompt", "text": args})),
        },
        "export" => wire::CommandExecuteResult {
            accepted: true,
            command: name.to_string(),
            message: None,
            action: Some(
                json!({"type": "downloadSession", "kind": "export", "threadId": thread_id.clone()}),
            ),
        },
        "share" => wire::CommandExecuteResult {
            accepted: true,
            command: name.to_string(),
            message: None,
            action: Some(
                json!({"type": "downloadSession", "kind": "share", "threadId": thread_id.clone()}),
            ),
        },
        "agents" => wire::CommandExecuteResult {
            accepted: true,
            command: name.to_string(),
            message: None,
            action: Some(json!({"type": "showPanel", "panel": "agents"})),
        },
        "status" => wire::CommandExecuteResult {
            accepted: true,
            command: name.to_string(),
            message: None,
            action: Some(json!({"type": "showPanel", "panel": "status"})),
        },
        "help" | "commands" => wire::CommandExecuteResult {
            accepted: true,
            command: name.to_string(),
            message: None,
            action: Some(json!({"type": "showPanel", "panel": "commands"})),
        },
        _ => wire::CommandExecuteResult {
            accepted: false,
            command: name.to_string(),
            message: Some(format!("unsupported command: /{name}")),
            action: None,
        },
    };
    Ok(serde_json::to_value(result)?)
}

fn write_project_agent_definition(
    workdir: &Path,
    params: AgentWriteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.name) {
        return Err(Error::Message(format!(
            "invalid agent name: {}",
            params.name
        )));
    }
    let description = params.description.trim();
    if description.is_empty() {
        return Err(Error::Message(
            "agent description must be non-empty".to_string(),
        ));
    }
    if let Some(backend) = &params.backend
        && !valid_agent_name(&backend.name)
    {
        return Err(Error::Message(format!(
            "invalid backend ref: {}",
            backend.name
        )));
    }
    let mut entrypoints = Vec::new();
    for entrypoint in &params.entrypoints {
        let parsed = AgentEntrypoint::parse(entrypoint).ok_or_else(|| {
            Error::Message(format!(
                "agent entrypoint `{entrypoint}` must be peer or subagent"
            ))
        })?;
        entrypoints.push(parsed.as_str().to_string());
    }
    let path = project_agent_definition_path(workdir, &params.name);
    let mut frontmatter = serde_yaml::Mapping::new();
    frontmatter.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(params.name.clone()),
    );
    frontmatter.insert(
        serde_yaml::Value::String("description".to_string()),
        serde_yaml::Value::String(description.to_string()),
    );
    if let Some(backend) = params.backend {
        let mut backend_value = serde_yaml::Mapping::new();
        backend_value.insert(
            serde_yaml::Value::String("ref".to_string()),
            serde_yaml::Value::String(backend.name),
        );
        frontmatter.insert(
            serde_yaml::Value::String("backend".to_string()),
            serde_yaml::Value::Mapping(backend_value),
        );
    }
    if !entrypoints.is_empty() {
        frontmatter.insert(
            serde_yaml::Value::String("entrypoints".to_string()),
            serde_yaml::Value::Sequence(
                entrypoints
                    .into_iter()
                    .map(serde_yaml::Value::String)
                    .collect(),
            ),
        );
    }
    if !params.tools.is_empty() {
        frontmatter.insert(
            serde_yaml::Value::String("tools".to_string()),
            serde_yaml::Value::Sequence(
                params
                    .tools
                    .into_iter()
                    .filter(|tool| !tool.trim().is_empty())
                    .map(|tool| serde_yaml::Value::String(tool.trim().to_string()))
                    .collect(),
            ),
        );
    }
    if !params.mcp_servers.is_empty() {
        frontmatter.insert(
            serde_yaml::Value::String("mcpServers".to_string()),
            serde_yaml::Value::Sequence(
                params
                    .mcp_servers
                    .into_iter()
                    .filter(|server| !server.trim().is_empty())
                    .map(|server| serde_yaml::Value::String(server.trim().to_string()))
                    .collect(),
            ),
        );
    }
    let frontmatter = serde_yaml::to_string(&frontmatter)?;
    let body = params.instructions.trim();
    let text = if body.is_empty() {
        format!("---\n{frontmatter}---\n")
    } else {
        format!("---\n{frontmatter}---\n{body}\n")
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, text)?;
    Ok(json!({
        "written": true,
        "name": params.name,
        "path": path,
    }))
}

fn delete_project_agent_definition(workdir: &Path, name: &str) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(name) {
        return Err(Error::Message(format!("invalid agent name: {name}")));
    }
    let path = project_agent_definition_path(workdir, name);
    let deleted = match std::fs::remove_file(&path) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => return Err(err.into()),
    };
    Ok(json!({
        "deleted": deleted,
        "name": name,
        "path": path,
    }))
}

fn project_agent_definition_path(workdir: &Path, name: &str) -> PathBuf {
    workdir
        .join(".psychevo")
        .join("agents")
        .join(format!("{name}.md"))
}

fn backend_value(backend: &AgentBackendConfig) -> Value {
    json!({
        "id": backend.id,
        "kind": backend.kind.as_str(),
        "enabled": backend.enabled,
        "label": backend.label,
        "description": backend.description,
        "command": backend.command,
        "args": backend.args,
        "cwd": backend.cwd,
        "entrypoints": backend.entrypoints,
        "clientCapabilities": backend.client_capabilities,
        "mcpServers": backend.mcp_servers,
        "envKeys": backend.env.keys().cloned().collect::<Vec<_>>(),
        "diagnostics": backend_diagnostics(backend),
    })
}

fn backend_diagnostics(backend: &AgentBackendConfig) -> Vec<Value> {
    let mut diagnostics = Vec::new();
    if !backend.enabled {
        diagnostics.push(json!({"kind": "disabled", "message": "backend is disabled"}));
    }
    if backend
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        diagnostics.push(json!({
            "kind": "missing_description",
            "message": "backend will not generate an agent without a description"
        }));
    }
    if backend.command.is_none() {
        diagnostics.push(json!({
            "kind": "missing_command",
            "message": "backend command is required for execution"
        }));
    }
    diagnostics
}

fn backend_doctor_value(
    backend: &AgentBackendConfig,
    env: &BTreeMap<String, String>,
) -> psychevo_runtime::Result<Value> {
    let mut checks = Vec::new();
    checks.push(json!({
        "name": "enabled",
        "ok": backend.enabled,
        "message": if backend.enabled { "backend enabled" } else { "backend disabled" },
    }));
    checks.push(json!({
        "name": "description",
        "ok": backend.description.as_deref().is_some_and(|value| !value.trim().is_empty()),
        "message": if backend.description.as_deref().is_some_and(|value| !value.trim().is_empty()) {
            "description configured"
        } else {
            "description missing; generated agent will be hidden"
        },
    }));
    let command_check = match backend.command.as_deref() {
        Some(command) => match resolve_command_path(command, env) {
            Some(path) => json!({
                "name": "command",
                "ok": true,
                "message": "command resolved",
                "path": path,
            }),
            None => json!({
                "name": "command",
                "ok": false,
                "message": "command was not found on PATH or as a configured path",
            }),
        },
        None => json!({
            "name": "command",
            "ok": false,
            "message": "command missing",
        }),
    };
    checks.push(command_check);
    let ok = checks
        .iter()
        .all(|check| check.get("ok").and_then(Value::as_bool).unwrap_or(false));
    Ok(json!({
        "id": backend.id,
        "kind": backend.kind.as_str(),
        "ok": ok,
        "checks": checks,
    }))
}

fn resolve_command_path(command: &str, env: &BTreeMap<String, String>) -> Option<PathBuf> {
    let command_path = PathBuf::from(command);
    if command_path.components().count() > 1 {
        return command_path.is_file().then_some(command_path);
    }
    let path_var = env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok())?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(command))
        .find(|path| path.is_file())
}

fn command_list_value(active_turn: bool) -> Value {
    let available = available_slash_commands_for_surface(
        &gateway_command_capabilities(),
        active_turn,
        &[],
        256,
    );
    json!({
        "commands": available.commands.iter().map(command_value).collect::<Vec<_>>(),
        "hiddenDynamic": available.hidden_dynamic,
    })
}

fn command_value(command: &AvailableSlashCommand) -> Value {
    json!({
        "name": command.name,
        "slash": format!("/{}", command.name),
        "usage": command.usage,
        "summary": command.summary,
        "aliases": command.aliases,
        "argumentKind": command_argument_kind(command.argument_kind),
        "source": "core",
    })
}

fn command_argument_kind(kind: CommandArgumentKind) -> &'static str {
    match kind {
        CommandArgumentKind::None => "none",
        CommandArgumentKind::RequiredValue => "required_value",
        CommandArgumentKind::OptionalValue => "optional_value",
        CommandArgumentKind::FixedEnumValue => "fixed_enum_value",
        CommandArgumentKind::FreeFormTrailingText => "free_form_trailing_text",
        CommandArgumentKind::DynamicSuffixOptionalText => "dynamic_suffix_optional_text",
    }
}

fn gateway_command_capabilities() -> Vec<CommandCapability> {
    vec![
        CommandCapability::Picker,
        CommandCapability::ActiveTurnControl,
        CommandCapability::Queue,
        CommandCapability::SessionSwitch,
        CommandCapability::ArtifactWrite,
        CommandCapability::WorkspaceDiff,
        CommandCapability::ConfigWrite,
        CommandCapability::PolicyWrite,
        CommandCapability::SkillStateWrite,
    ]
}

fn authorize_workdir(auth: &AuthContext, workdir: &Path) -> psychevo_runtime::Result<()> {
    match auth {
        AuthContext::Bearer => Ok(()),
        AuthContext::Browser(session) if session.workdir == workdir => Ok(()),
        AuthContext::Browser(_) => Err(Error::Message(
            "browser session is not authorized for this workdir".to_string(),
        )),
    }
}

fn authorize_thread(
    state: &WebState,
    auth: &AuthContext,
    thread_id: &str,
) -> psychevo_runtime::Result<()> {
    if matches!(auth, AuthContext::Bearer) {
        return Ok(());
    }
    let Some(summary) = state.inner.state.store().session_summary(thread_id)? else {
        return Err(Error::Message(format!("session not found: {thread_id}")));
    };
    authorize_workdir(auth, Path::new(&summary.workdir))
}

fn selector_from_thread_or_default(
    state: &WebState,
    auth: &AuthContext,
    thread_id: Option<String>,
) -> psychevo_runtime::Result<GatewayThreadSelector> {
    if let Some(thread_id) = thread_id {
        return Ok(GatewayThreadSelector::thread_id(thread_id));
    }
    let scope = default_resolved_scope(state, auth)?;
    Ok(state.selector(&scope.source))
}

fn source_from_input(
    input: Option<wire::GatewaySourceInput>,
    workdir: &Path,
    default_lifetime: wire::GatewaySourceLifetime,
) -> GatewaySource {
    let canonical = workdir.to_string_lossy().to_string();
    let hash = stable_hash_hex(&canonical);
    let display = workdir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workdir")
        .to_string();
    let input = input.unwrap_or(wire::GatewaySourceInput {
        kind: "web".to_string(),
        raw_id: None,
        lifetime: Some(default_lifetime),
        raw_identity: None,
        visible_name: None,
    });
    let raw_id = input.raw_id.unwrap_or_else(|| format!("workdir:{hash}"));
    let mut source = GatewaySource::new(input.kind, raw_id);
    source.lifetime = input.lifetime.unwrap_or(default_lifetime);
    source.visible_name = input.visible_name.or(Some(display.clone()));
    let source_kind = source.kind.clone();
    let source_raw_id = source.raw_id.clone();
    let source_lifetime = source.lifetime;
    source.raw_identity = Some(input.raw_identity.unwrap_or_else(|| {
        json!({
            "kind": source_kind,
            "rawId": source_raw_id,
            "workdirHash": hash,
            "displayName": display,
            "lifetime": source_lifetime,
        })
    }));
    source
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

fn session_cookie_value(cookie_header: &str) -> Option<&str> {
    cookie_header.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == "psychevo_gateway_session").then_some(value)
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as i64
}

fn apply_mentions_to_run_options(options: &mut RunOptions, mentions: &[wire::GatewayMention]) {
    for mention in mentions {
        let wire::GatewayMentionTarget::Skill { name, path } = &mention.target else {
            continue;
        };
        let input = path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .unwrap_or(name)
            .to_string();
        if !options
            .skill_inputs
            .iter()
            .any(|existing| existing == &input)
        {
            options.skill_inputs.push(input);
        }
    }
}

trait TurnStartInputExt {
    fn input_parts(&self) -> psychevo_runtime::Result<Vec<GatewayInputPart>>;
}

impl TurnStartInputExt for wire::TurnStartParams {
    fn input_parts(&self) -> psychevo_runtime::Result<Vec<GatewayInputPart>> {
        let mut input = self.input.clone();
        if let Some(text) = &self.text
            && !text.trim().is_empty()
        {
            input.push(GatewayInputPart::Text { text: text.clone() });
        }
        if input.is_empty() {
            return Err(Error::Message("turn/start requires input".to_string()));
        }
        Ok(input)
    }
}

async fn download_session(
    State(state): State<WebState>,
    headers: HeaderMap,
    AxumPath((session_id, kind)): AxumPath<(String, String)>,
) -> impl IntoResponse {
    let Some(auth) = state.auth_from_headers(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if let Err(err) = authorize_thread(&state, &auth, &session_id) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": {"message": err.to_string()}})),
        )
            .into_response();
    }
    match render_download(&state, &session_id, &kind) {
        Ok(response) => response.into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": err.to_string()}})),
        )
            .into_response(),
    }
}

fn render_download(
    state: &WebState,
    session_id: &str,
    kind: &str,
) -> psychevo_runtime::Result<Response<Body>> {
    let artifact_kind = match kind {
        "export" => SessionArtifactKind::Export,
        "share" => SessionArtifactKind::Share,
        value => return Err(Error::Message(format!("unknown download kind: {value}"))),
    };
    let artifact = render_session_export(
        state.inner.state.store(),
        session_id,
        SessionExportOptions {
            format: SessionExportFormat::Markdown,
            include: SessionExportIncludeSet::default_for(artifact_kind),
            artifact_kind,
        },
    )?;
    let filename = format!("{kind}-{session_id}.md");
    let mut response = Response::new(Body::from(artifact.content));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    response.headers_mut().insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    Ok(response)
}

async fn static_asset(
    State(state): State<WebState>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> impl IntoResponse {
    let Some(static_dir) = &state.inner.static_dir else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = uri.path().trim_start_matches('/');
    let candidate = if path.is_empty() {
        static_dir.join("index.html")
    } else {
        static_dir.join(path)
    };
    let serves_shell = path.is_empty() || path == "index.html" || !candidate.is_file();
    if serves_shell && state.auth_from_headers(&headers).is_none() {
        return launch_required_page().into_response();
    }
    let path = if candidate.is_file() {
        candidate
    } else {
        static_dir.join("index.html")
    };
    match std::fs::read(&path) {
        Ok(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            response.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static(content_type_for_path(&path)),
            );
            response.into_response()
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            "Workbench assets not found. Run `pnpm --filter @psychevo/workbench build` or pass --static-dir.",
        )
            .into_response(),
    }
}

fn launch_required_page() -> Response<Body> {
    let body = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>pevo launch required</title>
    <style>
      :root { color-scheme: light dark; font-family: ui-sans-serif, system-ui, sans-serif; }
      body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: Canvas; color: CanvasText; }
      main { max-width: 560px; padding: 32px; line-height: 1.5; }
      h1 { margin: 0 0 12px; font-size: 24px; }
      p { margin: 0 0 14px; }
      code { padding: 2px 6px; border: 1px solid color-mix(in srgb, CanvasText 18%, transparent); border-radius: 6px; }
    </style>
  </head>
  <body>
    <main>
      <h1>pevo launch required</h1>
      <p>This local Workbench URL needs a browser-session cookie created by the launch flow.</p>
      <p>Run <code>pevo web</code>, or run <code>pevo web --print-url</code> and open the returned <code>openUrl</code>.</p>
      <p>Do not open the managed <code>baseUrl</code> directly.</p>
    </main>
  </body>
</html>"#;
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::UNAUTHORIZED;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

fn launch_expired_page(status: StatusCode) -> Response<Body> {
    let body = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>pevo launch link expired</title>
    <style>
      :root { color-scheme: light dark; font-family: ui-sans-serif, system-ui, sans-serif; }
      body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: Canvas; color: CanvasText; }
      main { max-width: 560px; padding: 32px; line-height: 1.5; }
      h1 { margin: 0 0 12px; font-size: 24px; }
      p { margin: 0 0 14px; }
      code { padding: 2px 6px; border: 1px solid color-mix(in srgb, CanvasText 18%, transparent); border-radius: 6px; }
    </style>
  </head>
  <body>
    <main>
      <h1>pevo launch link expired</h1>
      <p>This <code>openUrl</code> was already used, expired, or opened in a browser without the launch cookie.</p>
      <p>Run <code>pevo web</code>, or run <code>pevo web --print-url</code> and open the new <code>openUrl</code>.</p>
      <p>If the Workbench already launched in this browser, open the clean local URL shown as <code>baseUrl</code>.</p>
    </main>
  </body>
</html>"#;
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

fn thread_snapshot(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    let thread = thread_id.map(|thread_id| GatewayThread {
        id: thread_id.to_string(),
        backend: crate::GatewayBackendInfo {
            kind: crate::BackendKind::Psychevo,
            native_id: Some(thread_id.to_string()),
        },
        source_key: Some(scope.source.source_key()),
    });
    let entries = match thread_id {
        Some(thread_id) => state.inner.gateway.thread_transcript(thread_id)?,
        None => Vec::new(),
    };
    let pending_permissions = state
        .inner
        .pending_permissions
        .lock()
        .expect("web pending permissions poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let pending_clarifies = state
        .inner
        .pending_clarifies
        .lock()
        .expect("web pending clarifies poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let activity = state.activity(&scope.source, thread_id);
    Ok(json!({
        "source": scope.source,
        "thread": thread,
        "entries": entries,
        "activity": activity,
        "pendingPermissions": pending_permissions,
        "pendingClarifies": pending_clarifies,
    }))
}

fn guard_session_mutation(
    state: &WebState,
    auth: &AuthContext,
    session_id: &str,
    allow_current_idle: bool,
) -> psychevo_runtime::Result<()> {
    let scope = default_resolved_scope(state, auth)?;
    let activity = state.activity(&scope.source, Some(session_id));
    if activity.running {
        return Err(Error::Message(
            "running session cannot be archived, restored, or deleted".to_string(),
        ));
    }
    if !allow_current_idle
        && let Some(bound) = state.inner.gateway.resolve_source_thread(&scope.source)?
        && bound == session_id
    {
        return Err(Error::Message(
            "current bound session cannot be deleted; reset the source first".to_string(),
        ));
    }
    Ok(())
}

fn session_summary_by_id(state: &WebState, session_id: &str) -> psychevo_runtime::Result<Value> {
    state
        .inner
        .state
        .store()
        .session_summary(session_id)?
        .map(|summary| session_summary_value(state, summary))
        .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))
}

fn session_summary_value(state: &WebState, summary: SessionSummary) -> Value {
    let activity = state
        .inner
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(&summary.id));
    json!({
        "id": summary.id,
        "source": summary.source,
        "workdir": summary.workdir,
        "model": summary.model,
        "provider": summary.provider,
        "startedAtMs": summary.started_at_ms,
        "updatedAtMs": summary.updated_at_ms,
        "endedAtMs": summary.ended_at_ms,
        "endReason": summary.end_reason,
        "archivedAtMs": summary.archived_at_ms,
        "messageCount": summary.message_count,
        "toolCallCount": summary.tool_call_count,
        "activity": activity,
        "title": summary.title,
    })
}

fn gateway_turn_result_value(result: GatewayTurnResult) -> Value {
    json!({
        "thread": result.thread,
        "turn": result.turn,
        "result": {
            "sessionId": result.result.session_id,
            "outcome": result.result.outcome.as_str(),
            "finalAnswer": result.result.final_answer,
            "toolFailures": result.result.tool_failures,
            "provider": result.result.provider,
            "model": result.result.model,
        },
        "committedEntries": result.committed_entries,
    })
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

impl RpcRequest {
    fn params<T>(&self) -> psychevo_runtime::Result<T>
    where
        T: Default + for<'de> Deserialize<'de>,
    {
        match &self.params {
            Some(params) => Ok(serde_json::from_value(params.clone())?),
            None => Ok(T::default()),
        }
    }

    fn required_params<T: for<'de> Deserialize<'de>>(&self) -> psychevo_runtime::Result<T> {
        let params = self
            .params
            .clone()
            .ok_or_else(|| Error::Message(format!("{} requires params", self.method)))?;
        Ok(serde_json::from_value(params)?)
    }
}

fn permission_decision(decision: PermissionDecision) -> PermissionApprovalDecision {
    PermissionApprovalDecision {
        outcome: match decision {
            PermissionDecision::AllowOnce => PermissionApprovalOutcome::AllowOnce,
            PermissionDecision::AllowSession => PermissionApprovalOutcome::AllowSession,
            PermissionDecision::AllowAlways => PermissionApprovalOutcome::AllowAlways,
            PermissionDecision::Deny => PermissionApprovalOutcome::Deny,
        },
    }
}

fn rpc_result(id: Value, result: Value) -> String {
    serde_json::to_string(&json!({"jsonrpc": wire::JSONRPC_VERSION, "id": id, "result": result}))
        .expect("json rpc result serializes")
}

fn rpc_error(id: Value, code: i64, message: String) -> String {
    serde_json::to_string(&json!({
        "jsonrpc": wire::JSONRPC_VERSION,
        "id": id,
        "error": {"code": code, "message": message}
    }))
    .expect("json rpc error serializes")
}

fn rpc_notification(method: &str, params: Value) -> String {
    serde_json::to_string(
        &json!({"jsonrpc": wire::JSONRPC_VERSION, "method": method, "params": params}),
    )
    .expect("json rpc notification serializes")
}

fn workdir_source(workdir: &Path) -> GatewaySource {
    source_from_input(None, workdir, GatewaySourceLifetime::Persistent)
}

fn stable_hash_hex(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn content_type_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "json" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[allow(dead_code)]
fn _source_key_value(source_key: SourceKey) -> Value {
    json!({"sourceKey": source_key.0})
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use std::collections::BTreeMap;

    fn web_state() -> (tempfile::TempDir, WebState) {
        let temp = tempfile::tempdir().expect("tempdir");
        let workdir = temp.path().join("work");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = Gateway::new(state);
        let config = GatewayWebServerConfig::new(
            gateway,
            temp.path().join("home"),
            workdir,
            None,
            BTreeMap::new(),
            temp.path().join("static"),
        );
        (temp, WebState::new(config))
    }

    fn web_state_with_static() -> (tempfile::TempDir, WebState) {
        let (temp, state) = web_state();
        let static_dir = temp.path().join("static");
        std::fs::create_dir_all(&static_dir).expect("static dir");
        std::fs::write(
            static_dir.join("index.html"),
            "<!doctype html><title>workbench</title>",
        )
        .expect("index");
        (temp, state)
    }

    async fn response_text(response: Response<Body>) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8(bytes.to_vec()).expect("utf8")
    }

    #[test]
    fn start_empty_thread_binds_web_source() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

        let snapshot = start_empty_thread(&state, &scope).expect("snapshot");
        let thread_id = snapshot
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .expect("thread id");

        assert_eq!(
            state
                .inner
                .gateway
                .resolve_source_thread(&state.inner.source)
                .expect("source lookup")
                .as_deref(),
            Some(thread_id)
        );
    }

    #[test]
    fn start_empty_thread_keeps_previous_history_active() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

        let first = start_empty_thread(&state, &scope).expect("first");
        let first_id = first["thread"]["id"]
            .as_str()
            .expect("first id")
            .to_string();
        let second = start_empty_thread(&state, &scope).expect("second");
        let second_id = second["thread"]["id"]
            .as_str()
            .expect("second id")
            .to_string();

        let active_ids = state
            .inner
            .state
            .store()
            .list_sessions_for_workdir_with_sources(&state.inner.workdir, HISTORY_SOURCES)
            .expect("active sessions")
            .into_iter()
            .map(|session| session.id)
            .collect::<Vec<_>>();

        assert!(active_ids.contains(&first_id));
        assert!(active_ids.contains(&second_id));
    }

    #[test]
    fn reset_source_to_empty_thread_archives_previous_binding() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

        let first = start_empty_thread(&state, &scope).expect("first");
        let first_id = first["thread"]["id"]
            .as_str()
            .expect("first id")
            .to_string();
        reset_source_to_empty_thread(&state, &scope).expect("reset");

        assert!(
            state
                .inner
                .state
                .store()
                .session_summary(&first_id)
                .expect("first summary")
                .expect("first exists")
                .archived_at_ms
                .is_some()
        );
    }

    #[test]
    fn bind_source_to_thread_rebinds_existing_session() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");

        bind_source_to_thread(&state, &scope, &session_id).expect("bind");

        assert_eq!(
            state
                .inner
                .gateway
                .resolve_source_thread(&state.inner.source)
                .expect("source lookup")
                .as_deref(),
            Some(session_id.as_str())
        );
    }

    #[test]
    fn thread_snapshot_projects_visible_entries_for_history_session_with_messages() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        state
            .inner
            .state
            .store()
            .append_message(
                &session_id,
                &RuntimeMessage::User {
                    content: vec![UserContentBlock::text("hello history")],
                    timestamp_ms: 1,
                },
            )
            .expect("append user");
        state
            .inner
            .state
            .store()
            .append_message(
                &session_id,
                &RuntimeMessage::Assistant {
                    content: vec![psychevo_runtime::AssistantBlock::Text {
                        text: "hello from assistant".to_string(),
                    }],
                    timestamp_ms: 2,
                    finish_reason: Some("stop".to_string()),
                    outcome: psychevo_runtime::Outcome::Normal,
                    model: Some("fake-model".to_string()),
                    provider: Some("fake-provider".to_string()),
                },
            )
            .expect("append assistant");
        let summary = state
            .inner
            .state
            .store()
            .session_summary(&session_id)
            .expect("summary")
            .expect("session exists");
        assert!(summary.message_count > 0);

        let snapshot = thread_snapshot(&state, &scope, Some(&session_id)).expect("snapshot");
        let entries = snapshot["entries"].as_array().expect("entries array");

        assert_eq!(entries.len(), 2, "{snapshot:#}");
        assert_eq!(entries[0]["blocks"][0]["body"], "hello history");
        assert_eq!(entries[1]["blocks"][0]["body"], "hello from assistant");
    }

    #[test]
    fn bind_source_to_thread_keeps_previous_history_active() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let first = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("first");
        let second = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("second");

        bind_source_to_thread(&state, &scope, &first).expect("bind first");
        bind_source_to_thread(&state, &scope, &second).expect("bind second");

        assert!(
            state
                .inner
                .state
                .store()
                .session_summary(&first)
                .expect("first summary")
                .expect("first exists")
                .archived_at_ms
                .is_none()
        );
    }

    #[test]
    fn active_completion_token_keeps_at_paths_with_slashes() {
        let token = active_completion_token("@src/ma", 7).expect("token");

        assert_eq!(token.sigil, '@');
        assert_eq!(token.query, "src/ma");
        assert_eq!(token.start, 0);
        assert_eq!(token.end, 7);
    }

    #[tokio::test]
    async fn agent_and_backend_rpc_list_generated_peer_backend() {
        let (_temp, state) = web_state();
        std::fs::create_dir_all(&state.inner.home).expect("home");
        std::fs::write(
            state.inner.home.join("config.toml"),
            r#"[agents.backends.cursor]
kind = "acp"
description = "Cursor ACP coding agent."
command = "cursor-agent"
"#,
        )
        .expect("config");
        let (tx, _rx) = mpsc::unbounded_channel();

        let backends = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "backend/list".to_string(),
                params: None,
            },
        )
        .await
        .expect("backend/list");
        assert_eq!(backends["backends"][0]["id"], "cursor");

        let agents = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "agent/list".to_string(),
                params: None,
            },
        )
        .await
        .expect("agent/list");
        let cursor = agents["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .find(|agent| agent["name"] == "cursor")
            .expect("cursor agent");
        assert_eq!(cursor["generated"], true);
        assert_eq!(cursor["backend"]["ref"], "cursor");
    }

    #[tokio::test]
    async fn completion_list_returns_workdir_files() {
        let (_temp, state) = web_state();
        let src = state.inner.workdir.join("src");
        std::fs::create_dir_all(&src).expect("src");
        std::fs::write(src.join("main.rs"), "fn main() {}\n").expect("main");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "completion/list".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "text": "@src/ma",
                    "cursor": 7
                })),
            },
        )
        .await
        .expect("completion/list");

        let labels = result["items"]
            .as_array()
            .expect("items")
            .iter()
            .filter_map(|item| item["label"].as_str())
            .collect::<Vec<_>>();
        assert!(labels.contains(&"@src/main.rs"));
    }

    #[tokio::test]
    async fn completion_list_ranks_dollar_prefix_matches_first() {
        let (_temp, state) = web_state();
        write_project_skill(&state, "x-daily", "Fetch X daily posts.");
        write_project_skill(&state, "explore", "Explore code and X references.");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "completion/list".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "text": "$x",
                    "cursor": 2
                })),
            },
        )
        .await
        .expect("completion/list");

        let labels = result["items"]
            .as_array()
            .expect("items")
            .iter()
            .filter_map(|item| item["label"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels.first().copied(), Some("$x-daily"));
        assert!(labels.contains(&"$explore"), "{labels:?}");
    }

    #[tokio::test]
    async fn command_execute_opens_web_utility_panels() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        for (command, panel) in [
            ("/status", "status"),
            ("/help", "commands"),
            ("/commands", "commands"),
            ("/agents", "agents"),
        ] {
            let result = handle_rpc(
                state.clone(),
                AuthContext::Bearer,
                tx.clone(),
                RpcRequest {
                    jsonrpc: wire::JSONRPC_VERSION.to_string(),
                    id: Some(json!("1")),
                    method: "command/execute".to_string(),
                    params: Some(json!({
                        "scope": scope,
                        "command": command,
                        "threadId": null
                    })),
                },
            )
            .await
            .expect("command/execute");

            assert_eq!(result["accepted"], true, "{command}: {result:?}");
            assert_eq!(result["action"]["type"], "showPanel");
            assert_eq!(result["action"]["panel"], panel);
        }
    }

    #[tokio::test]
    async fn agent_write_rpc_creates_project_backend_ref_shadow() {
        let (_temp, state) = web_state();
        let (tx, _rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "agent/write".to_string(),
                params: Some(json!({
                    "name": "cursor-reviewer",
                    "description": "Review with Cursor",
                    "backend": {"ref": "cursor"},
                    "entrypoints": ["subagent"],
                    "instructions": "Return concise findings."
                })),
            },
        )
        .await
        .expect("agent/write");
        let path = result["path"].as_str().expect("path");
        let text = std::fs::read_to_string(path).expect("agent file");
        assert!(text.contains("cursor-reviewer"));
        assert!(text.contains("ref: cursor"));
        assert!(text.contains("subagent"));
    }

    #[tokio::test]
    async fn static_shell_without_browser_session_returns_launch_required_page() {
        let (_temp, state) = web_state_with_static();

        let response = static_asset(
            State(state),
            HeaderMap::new(),
            axum::http::Uri::from_static("/"),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = response_text(response).await;
        assert!(body.contains("pevo launch required"), "{body}");
        assert!(body.contains("pevo web --print-url"), "{body}");
        assert!(!body.contains("<title>workbench</title>"), "{body}");
    }

    #[tokio::test]
    async fn static_shell_with_browser_session_serves_workbench_index() {
        let (_temp, state) = web_state_with_static();
        let session_id = "session-test".to_string();
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .insert(
                session_id.clone(),
                BrowserSession {
                    workdir: state.inner.workdir.clone(),
                    source: state.inner.source.clone(),
                },
            );
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("psychevo_gateway_session={session_id}"))
                .expect("cookie"),
        );

        let response = static_asset(State(state), headers, axum::http::Uri::from_static("/"))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_text(response).await;
        assert!(body.contains("<title>workbench</title>"), "{body}");
    }

    #[tokio::test]
    async fn consumed_launch_without_browser_session_returns_recovery_page() {
        let (_temp, state) = web_state_with_static();

        let response = consume_launch(
            State(state),
            AxumPath("missing-launch".to_string()),
            Query(LaunchQuery {
                open_token: "used-token".to_string(),
            }),
            HeaderMap::new(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_text(response).await;
        assert!(body.contains("pevo launch link expired"), "{body}");
        assert!(body.contains("pevo web --print-url"), "{body}");
    }

    #[tokio::test]
    async fn consumed_launch_with_browser_session_redirects_to_clean_shell() {
        let (_temp, state) = web_state_with_static();
        let session_id = "session-test".to_string();
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .insert(
                session_id.clone(),
                BrowserSession {
                    workdir: state.inner.workdir.clone(),
                    source: state.inner.source.clone(),
                },
            );
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("psychevo_gateway_session={session_id}"))
                .expect("cookie"),
        );

        let response = consume_launch(
            State(state),
            AxumPath("missing-launch".to_string()),
            Query(LaunchQuery {
                open_token: "used-token".to_string(),
            }),
            headers,
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/")
        );
    }

    fn write_project_skill(state: &WebState, name: &str, description: &str) {
        let dir = state
            .inner
            .workdir
            .join(".psychevo")
            .join("skills")
            .join(name);
        std::fs::create_dir_all(&dir).expect("skill dir");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description:?}\n---\n\nUse this skill.\n"),
        )
        .expect("skill");
    }
}
