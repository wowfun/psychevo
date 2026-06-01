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
use psychevo_runtime::{
    ClarifyAnswer, ClarifyResponse, ClarifyResult, Error, Message as RuntimeMessage,
    PermissionApprovalDecision, PermissionApprovalOutcome, PermissionMode, RunMode, RunOptions,
    SessionArtifactKind, SessionExportFormat, SessionExportIncludeSet, SessionExportOptions,
    SessionSummary, StateRuntime, TimelineDebugEventRecord as RuntimeTimelineDebugEventRecord,
    TimelineItemKind as RuntimeTimelineItemKind, TimelineItemRecord as RuntimeTimelineItemRecord,
    TimelineItemStatus as RuntimeTimelineItemStatus, UserContentBlock, canonicalize_workdir,
    render_session_export,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    Gateway, GatewayActivity, GatewayEvent, GatewayEventSink, GatewayInputPart, GatewaySource,
    GatewaySourceLifetime, GatewayThread, GatewayThreadSelector, GatewayTurnResult,
    PermissionDecision, SourceKey, TimelineItem, gateway_now_ms,
};

const HISTORY_SOURCES: &[&str] = &["run", "tui", "web"];

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
        write_managed_state(path, local_addr)?;
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

fn write_managed_state(path: &Path, local_addr: SocketAddr) -> psychevo_runtime::Result<()> {
    let state = wire::ManagedServerState {
        pid: std::process::id(),
        base_url: format!("http://{local_addr}"),
        readyz_url: format!("http://{local_addr}/readyz"),
        started_at_ms: now_ms(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(&state)?)?;
    Ok(())
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
            return StatusCode::NOT_FOUND.into_response();
        };
        entry
    };
    if entry.expires_at_ms < now_ms() || entry.open_token != query.open_token {
        return StatusCode::UNAUTHORIZED.into_response();
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
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    response
        .headers_mut()
        .insert(LOCATION, HeaderValue::from_static("/"));
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
                "sessions": sessions.into_iter().take(limit).map(session_summary_value).collect::<Vec<_>>(),
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
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let input = params.input_parts()?;
            let mut options = state.run_options(scope.workdir.clone(), params.thread_id.clone());
            options.model = params.model;
            options.reasoning_effort = params.reasoning_effort;
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
                        thread_id: params.thread_id,
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
        "source/reset" => {
            let params = request.required_params::<wire::SourceResetParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            start_empty_thread(&state, &scope)
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
        "settings/read" => {
            let params = request.params::<wire::SettingsReadParams>()?;
            let workdir = resolve_workdir_filter(&state, &auth, params.workdir)?;
            Ok(json!({
            "workdir": workdir,
            "memoryResources": {"mode": "status_only", "available": true},
            "secrets": {"frontendPersistence": "disabled"}
            }))
        }
        "debug/events" => {
            let params = request.required_params::<wire::DebugEventsParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            let events = state
                .inner
                .state
                .store()
                .load_timeline_debug_events(&params.thread_id, params.limit.unwrap_or(200))?
                .into_iter()
                .map(debug_event_from_record)
                .collect::<Vec<_>>();
            Ok(json!({ "events": events }))
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
    let activity = state.activity(&scope.source, None);
    if activity.running {
        return Err(Error::Message(
            "cannot switch threads while a turn is running".to_string(),
        ));
    }
    state.inner.gateway.reset_source(&scope.source, thread_id)?;
    Ok(())
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

async fn static_asset(State(state): State<WebState>, uri: axum::http::Uri) -> impl IntoResponse {
    let Some(static_dir) = &state.inner.static_dir else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = uri.path().trim_start_matches('/');
    let candidate = if path.is_empty() {
        static_dir.join("index.html")
    } else {
        static_dir.join(path)
    };
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
    let items = match thread_id {
        Some(thread_id) => state
            .inner
            .state
            .store()
            .load_timeline_items(thread_id)?
            .into_iter()
            .map(timeline_item_from_record)
            .collect::<Vec<_>>(),
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
        "items": items,
        "activity": activity,
        "pendingPermissions": pending_permissions,
        "pendingClarifies": pending_clarifies,
    }))
}

fn timeline_item_from_record(record: RuntimeTimelineItemRecord) -> TimelineItem {
    TimelineItem {
        id: record.item_id,
        thread_id: record.session_id,
        turn_id: record.turn_id,
        sequence: record.item_seq,
        kind: timeline_item_kind(record.kind),
        status: timeline_item_status(record.status),
        source: record.source,
        title: record.title,
        body: record.body_text,
        preview: record.preview_text,
        detail: record.detail_text,
        artifact_ids: record.artifact_ids,
        metadata: record.metadata,
        created_at_ms: record.created_at_ms,
        updated_at_ms: record.updated_at_ms,
    }
}

fn debug_event_from_record(record: RuntimeTimelineDebugEventRecord) -> wire::TimelineDebugEvent {
    wire::TimelineDebugEvent {
        id: record.id,
        thread_id: record.session_id,
        turn_id: record.turn_id,
        event_type: record.event_type,
        source: record.source,
        scope: record.scope,
        status: record.status,
        summary: record.summary,
        payload: record.payload,
        created_at_ms: record.created_at_ms,
    }
}

fn timeline_item_kind(kind: RuntimeTimelineItemKind) -> wire::TimelineItemKind {
    match kind {
        RuntimeTimelineItemKind::Prompt => wire::TimelineItemKind::Prompt,
        RuntimeTimelineItemKind::Assistant => wire::TimelineItemKind::Assistant,
        RuntimeTimelineItemKind::Reasoning => wire::TimelineItemKind::Reasoning,
        RuntimeTimelineItemKind::Tool => wire::TimelineItemKind::Tool,
        RuntimeTimelineItemKind::Shell => wire::TimelineItemKind::Shell,
        RuntimeTimelineItemKind::File => wire::TimelineItemKind::File,
        RuntimeTimelineItemKind::Web => wire::TimelineItemKind::Web,
        RuntimeTimelineItemKind::Mcp => wire::TimelineItemKind::Mcp,
        RuntimeTimelineItemKind::Clarify => wire::TimelineItemKind::Clarify,
        RuntimeTimelineItemKind::Permission => wire::TimelineItemKind::Permission,
        RuntimeTimelineItemKind::Skill => wire::TimelineItemKind::Skill,
        RuntimeTimelineItemKind::Agent => wire::TimelineItemKind::Agent,
        RuntimeTimelineItemKind::Mailbox => wire::TimelineItemKind::Mailbox,
        RuntimeTimelineItemKind::Status => wire::TimelineItemKind::Status,
        RuntimeTimelineItemKind::Diff => wire::TimelineItemKind::Diff,
        RuntimeTimelineItemKind::Artifact => wire::TimelineItemKind::Artifact,
    }
}

fn timeline_item_status(status: RuntimeTimelineItemStatus) -> wire::TimelineItemStatus {
    match status {
        RuntimeTimelineItemStatus::Pending => wire::TimelineItemStatus::Pending,
        RuntimeTimelineItemStatus::Running => wire::TimelineItemStatus::Running,
        RuntimeTimelineItemStatus::Completed => wire::TimelineItemStatus::Completed,
        RuntimeTimelineItemStatus::Failed => wire::TimelineItemStatus::Failed,
        RuntimeTimelineItemStatus::Cancelled => wire::TimelineItemStatus::Cancelled,
        RuntimeTimelineItemStatus::NeedsInput => wire::TimelineItemStatus::NeedsInput,
        RuntimeTimelineItemStatus::Info => wire::TimelineItemStatus::Info,
    }
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
        .map(session_summary_value)
        .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))
}

fn session_summary_value(summary: SessionSummary) -> Value {
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
        }
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
}
