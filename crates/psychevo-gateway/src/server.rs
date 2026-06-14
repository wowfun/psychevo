use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{Error as IoError, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
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
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures::{SinkExt, StreamExt};
use psychevo_gateway_protocol as wire;
use psychevo_runtime::command_registry::{
    AvailableSlashCommand, CommandArgumentKind, CommandCapability, CommandPresentation,
    DynamicSlashCommand, SlashCommandAction, SlashCommandEffect, SlashCommandParse,
    SlashCommandSurface, available_slash_commands_for_surface, command_presentation,
    dynamic_slash_command_effect, parse_slash_command_line, skill_prompt_marker,
    slash_invocation_effect,
};
use psychevo_runtime::{
    AgentBackendConfig, AgentCatalog, AgentDefinition, AgentDiagnostic, AgentDiscoveryOptions,
    AgentEntrypoint, AgentRunRecord, ClarifyAnswer, ClarifyResponse, ClarifyResult, ConfigScope,
    ContextOptions, Error, ListSkillsOptions, LoadedMainAgent, MAX_AGENT_SPAWN_DEPTH_CAP,
    Message as RuntimeMessage, PermissionApprovalDecision, PermissionApprovalOutcome,
    PermissionMode, RunMode, RunOptions, SESSION_MAIN_AGENT_METADATA_KEY, SessionArtifactKind,
    SessionExportFormat, SessionExportIncludeSet, SessionExportOptions, SessionSummary,
    SessionTraceReadOptions, SessionUndoOptions, SessionUsageOptions, SkillDiscoveryOptions,
    StateRuntime, UserContentBlock, UserShellContextOptions, WorkspaceDiffFile,
    WorkspaceDiffFileStatus, agent_spawn_paused, agent_status_records, canonicalize_workdir,
    collect_workspace_diff, configured_models, context_snapshot, discover_agents, discover_skills,
    format_context_total_value, format_context_total_value_parts, list_skill_bundles,
    list_skills_value_with_options, load_agent_backend_configs, main_agent_default_metadata,
    main_agent_from_session_metadata, main_agent_metadata, redo_session, remove_config_value,
    render_session_export, resolve_agent_definition, resolve_workspace_root,
    selected_configured_model, session_usage_summary, set_config_value, undo_session,
    valid_agent_name,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    ACP_PEER_METADATA_KEY, BackendKind, Gateway, GatewayActivity, GatewayBackendInfo, GatewayEvent,
    GatewayEventSink, GatewayInputPart, GatewayShellResult, GatewaySource, GatewaySourceLifetime,
    GatewayThread, GatewayThreadSelector, GatewayTurnResult, PermissionDecision, SendShellRequest,
    SourceKey, TranscriptEntry, TranscriptEntryRole, gateway_now_ms,
};

const INTERNAL_SESSION_SOURCES: &[&str] = &["tui-side"];
const MAX_COMPLETION_ITEMS: usize = 50;
const MAX_FILE_COMPLETION_ITEMS: usize = 80;
const MAX_FILE_COMPLETION_DEPTH: usize = 8;
const MAX_WORKSPACE_FILE_ITEMS: usize = 1_500;
const MAX_WORKSPACE_TEXT_FILE_BYTES: usize = 1024 * 1024;
#[derive(Debug, Clone)]
pub struct GatewayWebServerConfig {
    pub gateway: Gateway,
    pub home: PathBuf,
    pub workdir: PathBuf,
    pub config_path: Option<PathBuf>,
    pub inherited_env: BTreeMap<String, String>,
    pub static_dir: Option<PathBuf>,
    pub bind_addr: SocketAddr,
    pub bind_port_fallbacks: u16,
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
            bind_port_fallbacks: 0,
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
            bind_port_fallbacks: 0,
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
    let listener = bind_tcp_listener(config.bind_addr, config.bind_port_fallbacks).await?;
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

async fn bind_tcp_listener(
    bind_addr: SocketAddr,
    bind_port_fallbacks: u16,
) -> std::io::Result<TcpListener> {
    let max_offset = if bind_addr.port() == 0 {
        0
    } else {
        bind_port_fallbacks
    };
    let mut last_addr_in_use = None;
    for offset in 0..=max_offset {
        let Some(port) = bind_addr.port().checked_add(offset) else {
            break;
        };
        let candidate = SocketAddr::new(bind_addr.ip(), port);
        match TcpListener::bind(candidate).await {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == ErrorKind::AddrInUse && offset < max_offset => {
                last_addr_in_use = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_addr_in_use.unwrap_or_else(|| {
        IoError::new(
            ErrorKind::InvalidInput,
            "managed gateway bind fallback range overflowed",
        )
    }))
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
    terminals: TerminalManager,
    review: WorkspaceReviewState,
    pending_permissions: Mutex<HashMap<String, PendingPermissionView>>,
    pending_clarifies: Mutex<HashMap<String, PendingClarifyView>>,
}

#[derive(Clone, Default)]
struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, TerminalSession>>>,
}

#[derive(Clone, Default)]
struct WorkspaceReviewState {
    inner: Arc<Mutex<WorkspaceReviewInner>>,
}

#[derive(Default)]
struct WorkspaceReviewInner {
    pending: HashMap<String, PendingReviewTurn>,
    groups: Vec<WorkspaceReviewGroup>,
}

#[derive(Clone)]
struct PendingReviewTurn {
    thread_id: Option<String>,
    workdir: PathBuf,
    baseline: WorkspaceBaseline,
    created_at_ms: i64,
}

#[derive(Clone, Default)]
struct WorkspaceBaseline {
    files: HashMap<String, ReviewBaseline>,
}

#[derive(Clone)]
struct WorkspaceReviewGroup {
    turn_id: String,
    thread_id: Option<String>,
    workdir: PathBuf,
    created_at_ms: i64,
    completed_at_ms: i64,
    files: Vec<WorkspaceReviewFile>,
}

#[derive(Clone)]
struct WorkspaceReviewFile {
    path: String,
    status: wire::WorkspaceDiffFileStatusView,
    binary: bool,
    unreadable: bool,
    review_status: wire::WorkspaceChangeReviewStatusView,
    baseline: ReviewBaseline,
    post_revision: String,
    message: Option<String>,
}

#[derive(Clone)]
enum ReviewBaseline {
    Text { content: String },
    Absent,
    Unsupported { reason: String },
}

impl ReviewBaseline {
    fn can_reject(&self) -> bool {
        !matches!(self, Self::Unsupported { .. })
    }

    fn message(&self) -> Option<String> {
        match self {
            Self::Unsupported { reason } => Some(reason.clone()),
            _ => None,
        }
    }
}

impl WorkspaceReviewState {
    fn begin_turn(&self, turn_id: &str, thread_id: Option<String>, workdir: &Path) {
        let baseline = capture_workspace_baseline(workdir);
        let mut inner = self.inner.lock().expect("workspace review state poisoned");
        inner
            .pending
            .entry(turn_id.to_string())
            .or_insert_with(|| PendingReviewTurn {
                thread_id,
                workdir: workdir.to_path_buf(),
                baseline,
                created_at_ms: now_ms(),
            });
    }

    fn complete_turn(&self, turn_id: &str) {
        let pending = {
            let mut inner = self.inner.lock().expect("workspace review state poisoned");
            inner.pending.remove(turn_id)
        };
        let Some(pending) = pending else {
            return;
        };
        let files = build_review_files(&pending.workdir, &pending.baseline).unwrap_or_default();
        if files.is_empty() {
            return;
        }
        let mut inner = self.inner.lock().expect("workspace review state poisoned");
        inner.groups.retain(|group| group.turn_id != turn_id);
        inner.groups.insert(
            0,
            WorkspaceReviewGroup {
                turn_id: turn_id.to_string(),
                thread_id: pending.thread_id,
                workdir: pending.workdir,
                created_at_ms: pending.created_at_ms,
                completed_at_ms: now_ms(),
                files,
            },
        );
        inner.groups.truncate(40);
    }

    fn changes_for_scope(&self, scope: &ResolvedScope) -> wire::WorkspaceChangesResult {
        let inner = self.inner.lock().expect("workspace review state poisoned");
        wire::WorkspaceChangesResult {
            groups: inner
                .groups
                .iter()
                .filter(|group| group.workdir == scope.workdir)
                .map(review_group_to_wire)
                .collect(),
        }
    }

    fn accept(
        &self,
        scope: &ResolvedScope,
        turn_id: &str,
        path: &str,
    ) -> psychevo_runtime::Result<wire::WorkspaceChangeMutationResult> {
        let path = normalize_workspace_path(path);
        let mut accepted = false;
        {
            let mut inner = self.inner.lock().expect("workspace review state poisoned");
            if let Some(file) = inner
                .groups
                .iter_mut()
                .find(|group| group.workdir == scope.workdir && group.turn_id == turn_id)
                .and_then(|group| group.files.iter_mut().find(|file| file.path == path))
            {
                file.review_status = wire::WorkspaceChangeReviewStatusView::Accepted;
                file.message = None;
                accepted = true;
            }
        }
        Ok(wire::WorkspaceChangeMutationResult {
            accepted,
            changes: self.changes_for_scope(scope),
        })
    }

    fn reject(
        &self,
        scope: &ResolvedScope,
        turn_id: &str,
        path: &str,
    ) -> psychevo_runtime::Result<wire::WorkspaceChangeMutationResult> {
        let path = normalize_workspace_path(path);
        let file = {
            let inner = self.inner.lock().expect("workspace review state poisoned");
            inner
                .groups
                .iter()
                .find(|group| group.workdir == scope.workdir && group.turn_id == turn_id)
                .and_then(|group| group.files.iter().find(|file| file.path == path))
                .cloned()
        };
        let Some(file) = file else {
            return Ok(wire::WorkspaceChangeMutationResult {
                accepted: false,
                changes: self.changes_for_scope(scope),
            });
        };
        if !file.baseline.can_reject() {
            return Ok(wire::WorkspaceChangeMutationResult {
                accepted: false,
                changes: self.changes_for_scope(scope),
            });
        }
        let current_revision = workspace_path_revision(&scope.workdir, &path)?;
        if current_revision != file.post_revision {
            self.mark_conflict(scope, turn_id, &path, "File changed after this turn.");
            return Ok(wire::WorkspaceChangeMutationResult {
                accepted: false,
                changes: self.changes_for_scope(scope),
            });
        }
        restore_review_baseline(&scope.workdir, &path, &file.baseline)?;
        {
            let mut inner = self.inner.lock().expect("workspace review state poisoned");
            if let Some(file) = inner
                .groups
                .iter_mut()
                .find(|group| group.workdir == scope.workdir && group.turn_id == turn_id)
                .and_then(|group| group.files.iter_mut().find(|file| file.path == path))
            {
                file.review_status = wire::WorkspaceChangeReviewStatusView::Rejected;
                file.message = None;
            }
        }
        Ok(wire::WorkspaceChangeMutationResult {
            accepted: true,
            changes: self.changes_for_scope(scope),
        })
    }

    fn mark_conflict(&self, scope: &ResolvedScope, turn_id: &str, path: &str, message: &str) {
        let mut inner = self.inner.lock().expect("workspace review state poisoned");
        if let Some(file) = inner
            .groups
            .iter_mut()
            .find(|group| group.workdir == scope.workdir && group.turn_id == turn_id)
            .and_then(|group| group.files.iter_mut().find(|file| file.path == path))
        {
            file.review_status = wire::WorkspaceChangeReviewStatusView::Conflict;
            file.message = Some(message.to_string());
        }
    }
}

#[derive(Clone)]
struct TerminalSession {
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl TerminalManager {
    fn start(
        &self,
        scope: &ResolvedScope,
        params: wire::TerminalStartParams,
        inherited_env: &BTreeMap<String, String>,
        out_tx: mpsc::UnboundedSender<String>,
    ) -> psychevo_runtime::Result<wire::TerminalStartResult> {
        let cwd = resolve_terminal_cwd(&scope.workdir, params.cwd.as_deref())?;
        let rows = params.rows.clamp(4, 200);
        let cols = params.cols.clamp(20, 400);
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| Error::Message(err.to_string()))?;
        let shell = default_terminal_shell(inherited_env);
        let mut command = portable_pty::CommandBuilder::new(shell);
        command.cwd(cwd.as_os_str());
        command.env("TERM", "xterm-256color");
        for (key, value) in inherited_env {
            command.env(key, value);
        }
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| Error::Message(err.to_string()))?;
        let pid = child.process_id();
        drop(pair.slave);
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| Error::Message(err.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| Error::Message(err.to_string()))?;
        let terminal_id = Uuid::now_v7().to_string();
        let child = Arc::new(Mutex::new(child));
        let session = TerminalSession {
            child: Arc::clone(&child),
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
        };
        self.sessions
            .lock()
            .expect("web terminal sessions poisoned")
            .insert(terminal_id.clone(), session);
        spawn_terminal_reader(terminal_id.clone(), reader, out_tx.clone());
        spawn_terminal_waiter(
            terminal_id.clone(),
            Arc::clone(&child),
            self.clone(),
            out_tx,
        );
        Ok(wire::TerminalStartResult {
            terminal_id,
            cwd: cwd.display().to_string(),
            pid,
        })
    }

    fn write(
        &self,
        params: wire::TerminalWriteParams,
    ) -> psychevo_runtime::Result<wire::TerminalMutationResult> {
        let bytes = BASE64_STANDARD
            .decode(params.data_base64.as_bytes())
            .map_err(|err| Error::Message(format!("invalid terminal data: {err}")))?;
        let session = self.session(&params.terminal_id)?;
        let mut writer = session
            .writer
            .lock()
            .map_err(|_| Error::Message("terminal writer is unavailable".to_string()))?;
        writer.write_all(&bytes)?;
        writer.flush()?;
        Ok(wire::TerminalMutationResult { accepted: true })
    }

    fn resize(
        &self,
        params: wire::TerminalResizeParams,
    ) -> psychevo_runtime::Result<wire::TerminalMutationResult> {
        let session = self.session(&params.terminal_id)?;
        let master = session
            .master
            .lock()
            .map_err(|_| Error::Message("terminal pty is unavailable".to_string()))?;
        master
            .resize(portable_pty::PtySize {
                rows: params.rows.clamp(4, 200),
                cols: params.cols.clamp(20, 400),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| Error::Message(err.to_string()))?;
        Ok(wire::TerminalMutationResult { accepted: true })
    }

    fn terminate(
        &self,
        params: wire::TerminalTerminateParams,
        out_tx: mpsc::UnboundedSender<String>,
    ) -> psychevo_runtime::Result<wire::TerminalMutationResult> {
        let Some(session) = self
            .sessions
            .lock()
            .expect("web terminal sessions poisoned")
            .remove(&params.terminal_id)
        else {
            return Ok(wire::TerminalMutationResult { accepted: false });
        };
        if let Ok(mut child) = session.child.lock() {
            let _ = child.kill();
        }
        let _ = out_tx.send(rpc_notification(
            "terminal/exited",
            serde_json::to_value(wire::TerminalExitedPayload {
                terminal_id: params.terminal_id,
                exit_code: None,
                reason: "terminated".to_string(),
            })?,
        ));
        Ok(wire::TerminalMutationResult { accepted: true })
    }

    fn session(&self, terminal_id: &str) -> psychevo_runtime::Result<TerminalSession> {
        self.sessions
            .lock()
            .expect("web terminal sessions poisoned")
            .get(terminal_id)
            .cloned()
            .ok_or_else(|| Error::Message(format!("unknown terminal: {terminal_id}")))
    }

    fn remove(&self, terminal_id: &str) -> bool {
        self.sessions
            .lock()
            .expect("web terminal sessions poisoned")
            .remove(terminal_id)
            .is_some()
    }
}

fn spawn_terminal_reader(
    terminal_id: String,
    mut reader: Box<dyn Read + Send>,
    out_tx: mpsc::UnboundedSender<String>,
) {
    thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let payload = wire::TerminalOutputPayload {
                        terminal_id: terminal_id.clone(),
                        stream: "stdout".to_string(),
                        data_base64: BASE64_STANDARD.encode(&chunk[..n]),
                    };
                    if let Ok(value) = serde_json::to_value(payload) {
                        let _ = out_tx.send(rpc_notification("terminal/output", value));
                    }
                }
                Err(err) if err.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });
}

fn spawn_terminal_waiter(
    terminal_id: String,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    manager: TerminalManager,
    out_tx: mpsc::UnboundedSender<String>,
) {
    thread::spawn(move || {
        loop {
            let status = {
                let Ok(mut child) = child.lock() else {
                    return;
                };
                child.try_wait()
            };
            match status {
                Ok(Some(status)) => {
                    if manager.remove(&terminal_id) {
                        let _ = out_tx.send(rpc_notification(
                            "terminal/exited",
                            json!({
                                "terminalId": terminal_id,
                                "exitCode": status.exit_code() as i32,
                                "reason": status.signal().unwrap_or("exited")
                            }),
                        ));
                    }
                    return;
                }
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(err) => {
                    if manager.remove(&terminal_id) {
                        let _ = out_tx.send(rpc_notification(
                            "terminal/exited",
                            json!({
                                "terminalId": terminal_id,
                                "exitCode": null,
                                "reason": err.to_string()
                            }),
                        ));
                    }
                    return;
                }
            }
        }
    });
}

fn resolve_terminal_cwd(root: &Path, cwd: Option<&str>) -> psychevo_runtime::Result<PathBuf> {
    let Some(cwd) = cwd.map(str::trim).filter(|cwd| !cwd.is_empty()) else {
        return Ok(root.to_path_buf());
    };
    if cwd.contains('\0') {
        return Err(Error::Message("terminal cwd is invalid".to_string()));
    }
    let raw = Path::new(cwd);
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        root.join(raw)
    };
    let canonical = canonicalize_workdir(&candidate)?;
    if !canonical.starts_with(root) {
        return Err(Error::Message(
            "terminal cwd is outside the workspace".to_string(),
        ));
    }
    Ok(canonical)
}

fn default_terminal_shell(inherited_env: &BTreeMap<String, String>) -> String {
    inherited_env
        .get("SHELL")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .or_else(|| std::env::var("SHELL").ok())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| inherited_env.get("COMSPEC").cloned())
        .or_else(|| std::env::var("COMSPEC").ok())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "cmd.exe".to_string()
            } else {
                "/bin/sh".to_string()
            }
        })
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
    Browser { session_id: String },
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
                terminals: TerminalManager::default(),
                review: WorkspaceReviewState::default(),
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
            .map(|_| AuthContext::Browser {
                session_id: cookie.to_string(),
            })
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

    fn record_review_event(&self, event: &GatewayEvent, workdir: &Path) {
        match event {
            GatewayEvent::TurnStarted {
                thread_id, turn_id, ..
            } => {
                self.inner
                    .review
                    .begin_turn(turn_id, thread_id.clone(), workdir);
            }
            GatewayEvent::TurnCompleted { turn_id, .. } => {
                self.inner.review.complete_turn(turn_id);
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
            "profile": gateway_profile_value(&state),
            "capabilities": {
                "threads": true,
                "turns": true,
                "historyManagement": true,
                "downloads": true,
                "settingsWrite": "structured",
                "workspaceCreate": true,
                "memoryResources": "status_only"
            }
            }))
        }
        "thread/start" => {
            let params = request.required_params::<wire::ThreadStartParams>()?;
            let scope = resolve_start_scope(&state, &auth, params.scope.clone())?;
            state.inner.gateway.clear_source_binding(&scope.source)?;
            let snapshot_scope = detached_draft_scope(&scope, &auth);
            update_browser_session_scope(&state, &auth, &snapshot_scope);
            thread_snapshot(&state, &snapshot_scope, None)
        }
        "thread/resume" => {
            let params = request.params::<wire::ThreadResumeParams>()?;
            let (thread_id, scope) = match params.thread_id {
                Some(thread_id) => {
                    authorize_thread(&state, &auth, &thread_id)?;
                    let scope = resolved_scope_for_thread(&state, &thread_id)?;
                    bind_source_to_thread(&state, &scope, &thread_id)?;
                    update_browser_session_scope(&state, &auth, &scope);
                    (Some(thread_id), scope)
                }
                None => {
                    let scope = resolve_optional_scope(&state, &auth, params.scope)?;
                    let thread_id = state.inner.gateway.resolve_source_thread(&scope.source)?;
                    (thread_id, scope)
                }
            };
            thread_snapshot(&state, &scope, thread_id.as_deref())
        }
        "thread/read" => {
            let params = request.required_params::<wire::ThreadReadParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            let scope = resolved_scope_for_thread(&state, &params.thread_id)?;
            thread_snapshot(&state, &scope, Some(&params.thread_id))
        }
        "thread/trace" => {
            let params = request.required_params::<wire::ThreadTraceParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            let runtime_state = state.inner.state.clone();
            let result = tokio::task::spawn_blocking(move || {
                runtime_state.read_session_trace(
                    &params.thread_id,
                    SessionTraceReadOptions {
                        after_seq: params.after_seq,
                        limit: params.limit,
                    },
                )
            })
            .await
            .map_err(|err| Error::Message(format!("thread trace read task failed: {err}")))?;
            Ok(serde_json::to_value(result)?)
        }
        "thread/list" => {
            let params = request.params::<wire::ThreadListParams>()?;
            let limit = params.limit.unwrap_or(50).clamp(1, 200);
            let workdir = resolve_session_workdir_filter(&state, &auth, params.workdir)?;
            let store = state.inner.state.store();
            let sessions = if params.archived.unwrap_or(false) {
                match workdir.as_ref() {
                    Some(workdir) => {
                        store.list_archived_sessions_for_workdir_with_sources(workdir, &[])?
                    }
                    None => store.list_archived_sessions_with_sources(&[])?,
                }
            } else {
                match workdir.as_ref() {
                    Some(workdir) => store.list_sessions_for_workdir_with_sources(workdir, &[])?,
                    None => store.list_sessions_with_sources(&[])?,
                }
            };
            Ok(json!({
                "sessions": sessions
                    .into_iter()
                    .filter(|session| human_visible_session(&state, session))
                    .take(limit)
                    .map(|session| session_summary_value(&state, session))
                    .collect::<psychevo_runtime::Result<Vec<_>>>()?,
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
            state.inner.state.delete_session(&params.thread_id)?;
            Ok(json!({"deleted": true, "threadId": params.thread_id}))
        }
        "turn/start" => {
            let params = request.required_params::<wire::TurnStartParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            let input = params.input_parts()?;
            let thread_id = match params.thread_id.clone() {
                Some(thread_id) => {
                    authorize_thread(&state, &auth, &thread_id)?;
                    Some(thread_id)
                }
                None => state.inner.gateway.resolve_source_thread(&scope.source)?,
            };
            if state
                .inner
                .gateway
                .resolve_source_thread(&scope.source)?
                .as_deref()
                != thread_id.as_deref()
                && let Some(thread_id) = thread_id.as_deref()
            {
                bind_source_to_thread(&state, &scope, thread_id)?;
            }
            let mut options = state.run_options(scope.workdir.clone(), thread_id.clone());
            options.model = params.model;
            options.reasoning_effort = params.reasoning_effort;
            if let Some(mode) = params.mode.as_deref() {
                options.mode = RunMode::parse(mode)
                    .ok_or_else(|| Error::Message(format!("unknown mode: {mode}")))?;
            }
            if let Some(permission_mode) = params.permission_mode.as_deref() {
                options.permission_mode =
                    Some(PermissionMode::parse(permission_mode).ok_or_else(|| {
                        Error::Message(format!("unknown permission mode: {permission_mode}"))
                    })?);
            }
            options.agent = params.agent_name.clone();
            apply_mentions_to_run_options(&mut options, &params.mentions);
            let source = scope.source.clone();
            let event_state = state.clone();
            let review_workdir = scope.workdir.clone();
            let event_tx = out_tx.clone();
            let event_sink: GatewayEventSink = Arc::new(move |event| {
                event_state.record_event(&event);
                event_state.record_review_event(&event, &review_workdir);
                let _ = event_tx.send(rpc_notification("gateway/event", json!(event)));
            });
            let gateway = state.inner.gateway.clone();
            let bind_source = workdir_source(&scope.workdir);
            tokio::spawn(async move {
                let result = gateway
                    .send_turn(crate::SendTurnRequest {
                        thread_id,
                        source: Some(source),
                        bind_source: Some(bind_source),
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
        "workspace/files" => {
            let params = request.required_params::<wire::WorkspaceFilesParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            workspace_files_value(&scope)
        }
        "workspace/file/read" => {
            let params = request.required_params::<wire::WorkspaceFileReadParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            workspace_file_read_value(&scope, &params.path)
        }
        "workspace/file/write" => {
            let params = request.required_params::<wire::WorkspaceFileWriteParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            workspace_file_write_value(&scope, params)
        }
        "workspace/diff" => {
            let params = request.required_params::<wire::WorkspaceDiffParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            workspace_diff_value(&scope, params.path.as_deref())
        }
        "workspace/changes" => {
            let params = request.required_params::<wire::WorkspaceChangesParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            Ok(serde_json::to_value(
                state.inner.review.changes_for_scope(&scope),
            )?)
        }
        "workspace/change/accept" => {
            let params = request.required_params::<wire::WorkspaceChangeFileParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            Ok(serde_json::to_value(state.inner.review.accept(
                &scope,
                &params.turn_id,
                &params.path,
            )?)?)
        }
        "workspace/change/reject" => {
            let params = request.required_params::<wire::WorkspaceChangeFileParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            Ok(serde_json::to_value(state.inner.review.reject(
                &scope,
                &params.turn_id,
                &params.path,
            )?)?)
        }
        "workspace/create" => {
            let params = request.required_params::<wire::WorkspaceCreateParams>()?;
            workspace_create_value(&state, &auth, params)
        }
        "context/read" => {
            let params = request.required_params::<wire::ContextReadParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            context_read_value(&state, &scope, params.thread_id.as_deref())
        }
        "observability/read" => {
            let params = request.required_params::<wire::ObservabilityReadParams>()?;
            let requested_scope = resolve_required_scope(&state, &auth, params.scope)?;
            let (scope, thread_id) = match params.thread_id {
                Some(thread_id) => {
                    authorize_thread(&state, &auth, &thread_id)?;
                    (
                        resolved_scope_for_thread(&state, &thread_id)?,
                        Some(thread_id),
                    )
                }
                None => (requested_scope, None),
            };
            observability_read_value(&state, &scope, thread_id.as_deref())
        }
        "source/reset" => {
            let params = request.required_params::<wire::SourceResetParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            reset_source_to_empty(&state, &scope)
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
            let params = request.params::<wire::AgentListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let catalog = discover_gateway_agents(&state, &scope)?;
            Ok(serde_json::to_value(agent_list_result(&catalog))?)
        }
        "agent/read" => {
            let params = request.required_params::<wire::AgentReadParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let catalog = discover_gateway_agents(&state, &scope)?;
            let agent = resolve_agent_definition(
                &catalog,
                &params.name,
                &scope.workdir,
                &state.inner.inherited_env,
            )?;
            Ok(serde_json::to_value(agent_read_result(&agent))?)
        }
        "agent/write" => {
            let params = request.required_params::<wire::AgentWriteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            write_project_agent_definition(&scope.workdir, params)
        }
        "agent/delete" => {
            let params = request.required_params::<wire::AgentDeleteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            delete_project_agent_definition(&scope.workdir, &params.name)
        }
        "agent/status" => {
            let params = request.params::<wire::AgentStatusParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            if let Some(thread_id) = params.thread_id.as_deref() {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let source_thread_id = if params.thread_id.is_some() || params.all.unwrap_or(false) {
                None
            } else {
                state.inner.gateway.resolve_source_thread(&scope.source)?
            };
            let thread_id = params.thread_id.as_deref().or(source_thread_id.as_deref());
            Ok(serde_json::to_value(agent_status_result(
                Some(state.inner.state.store()),
                thread_id,
                params.all.unwrap_or(false),
            ))?)
        }
        "backend/list" => {
            let params = request.params::<wire::BackendListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let backends = load_agent_backend_configs(
                &state.inner.home,
                &scope.workdir,
                &state.inner.inherited_env,
            )?;
            Ok(serde_json::to_value(wire::BackendListResult {
                backends: backend_values_for_scope(&state, &scope, &backends)?,
            })?)
        }
        "backend/doctor" => {
            let params = request.required_params::<wire::BackendDoctorParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let backends = load_agent_backend_configs(
                &state.inner.home,
                &scope.workdir,
                &state.inner.inherited_env,
            )?;
            let backend = backends
                .get(&params.id)
                .ok_or_else(|| Error::Message(format!("unknown backend: {}", params.id)))?;
            Ok(serde_json::to_value(backend_doctor_value(
                backend,
                &state.inner.inherited_env,
            )?)?)
        }
        "backend/write" => {
            let params = request.required_params::<wire::BackendWriteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            write_backend_config(&state, &scope, params)
        }
        "backend/delete" => {
            let params = request.required_params::<wire::BackendDeleteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            delete_backend_config(&state, &scope, params)
        }
        "command/list" => {
            let params = request.params::<wire::CommandListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let active_turn = if let Some(thread_id) = params.thread_id.as_deref() {
                authorize_thread(&state, &auth, thread_id)?;
                state.activity(&scope.source, Some(thread_id)).running
            } else {
                state.activity(&scope.source, None).running
            };
            command_list_value(&state, &scope, active_turn)
        }
        "command/execute" => {
            let params = request.required_params::<wire::CommandExecuteParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            command_execute_value(&state, &scope, params)
        }
        "shell/start" => {
            let params = request.required_params::<wire::ShellStartParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            let command = params.command.trim().to_string();
            if command.is_empty() {
                return Ok(serde_json::to_value(wire::ShellStartResult {
                    accepted: false,
                    thread_id: params.thread_id,
                    message: Some(
                        "shell mode: type !<command> to run a local shell command".to_string(),
                    ),
                })?);
            }
            let thread_id = match params.thread_id.clone() {
                Some(thread_id) => {
                    authorize_thread(&state, &auth, &thread_id)?;
                    Some(thread_id)
                }
                None => state.inner.gateway.resolve_source_thread(&scope.source)?,
            };
            if state
                .inner
                .gateway
                .resolve_source_thread(&scope.source)?
                .as_deref()
                != thread_id.as_deref()
                && let Some(thread_id) = thread_id.as_deref()
            {
                bind_source_to_thread(&state, &scope, thread_id)?;
            }
            let event_state = state.clone();
            let event_tx = out_tx.clone();
            let event_sink: GatewayEventSink = Arc::new(move |event| {
                event_state.record_event(&event);
                let _ = event_tx.send(rpc_notification("gateway/event", json!(event)));
            });
            let context = user_shell_context_options(&state, &scope, thread_id.clone());
            let gateway = state.inner.gateway.clone();
            let source = scope.source.clone();
            let bind_source = workdir_source(&scope.workdir);
            let workdir = scope.workdir.clone();
            let result_thread_id = thread_id.clone();
            tokio::spawn(async move {
                let result = gateway
                    .send_shell(SendShellRequest {
                        thread_id: result_thread_id.clone(),
                        source: Some(source),
                        bind_source: Some(bind_source),
                        workdir,
                        command,
                        context,
                        stream: None,
                        event_sink: Some(event_sink),
                        lineage: Some(json!({"reason": "shell_start"})),
                    })
                    .await;
                let notification = match result {
                    Ok(result) => {
                        rpc_notification("shell/result", gateway_shell_result_value(result))
                    }
                    Err(err) => rpc_notification(
                        "shell/error",
                        json!({"message": err.to_string(), "threadId": result_thread_id}),
                    ),
                };
                let _ = out_tx.send(notification);
            });
            Ok(serde_json::to_value(wire::ShellStartResult {
                accepted: true,
                thread_id,
                message: None,
            })?)
        }
        "terminal/start" => {
            let params = request.required_params::<wire::TerminalStartParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(state.inner.terminals.start(
                &scope,
                params,
                &state.inner.inherited_env,
                out_tx,
            )?)?)
        }
        "terminal/write" => {
            let params = request.required_params::<wire::TerminalWriteParams>()?;
            Ok(serde_json::to_value(state.inner.terminals.write(params)?)?)
        }
        "terminal/resize" => {
            let params = request.required_params::<wire::TerminalResizeParams>()?;
            Ok(serde_json::to_value(state.inner.terminals.resize(params)?)?)
        }
        "terminal/terminate" => {
            let params = request.required_params::<wire::TerminalTerminateParams>()?;
            Ok(serde_json::to_value(
                state.inner.terminals.terminate(params, out_tx)?,
            )?)
        }
        "settings/read" => {
            let params = request.params::<wire::SettingsReadParams>()?;
            let (workdir, thread_id) = if let Some(thread_id) = params.thread_id {
                authorize_thread(&state, &auth, &thread_id)?;
                let summary = state
                    .inner
                    .state
                    .store()
                    .session_summary(&thread_id)?
                    .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
                (PathBuf::from(summary.workdir), Some(thread_id))
            } else {
                (resolve_workdir_filter(&state, &auth, params.workdir)?, None)
            };
            settings_read_value(&state, &workdir, thread_id.as_deref())
        }
        "settings/update" => {
            let params = request.required_params::<wire::SettingsUpdateParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            update_session_agent_setting(
                &state,
                &scope,
                &params.thread_id,
                params.agent.as_deref(),
            )?;
            settings_read_value(&state, &scope.workdir, Some(&params.thread_id))
        }
        method => Err(Error::Message(format!("method not found: {method}"))),
    }
}

fn gateway_profile_value(state: &WebState) -> Value {
    let name = state
        .inner
        .inherited_env
        .get("PSYCHEVO_PROFILE")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("default");
    json!({
        "name": name,
        "home": state.inner.home.display().to_string(),
        "default": name == "default",
    })
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

fn detached_draft_scope(scope: &ResolvedScope, auth: &AuthContext) -> ResolvedScope {
    if !matches!(auth, AuthContext::Browser { .. }) {
        return scope.clone();
    }
    let mut source = scope.source.clone();
    source.raw_id = format!("{}:draft:{}", source.raw_id, Uuid::now_v7());
    source.visible_name = source
        .visible_name
        .clone()
        .or_else(|| Some("Web draft".to_string()));
    source.raw_identity = Some(json!({
        "kind": source.kind.clone(),
        "rawId": source.raw_id.clone(),
        "canonicalRawId": scope.source.raw_id.clone(),
        "workdir": scope.workdir.display().to_string(),
        "draft": true,
    }));
    ResolvedScope {
        workdir: scope.workdir.clone(),
        source,
    }
}

#[cfg(test)]
fn start_empty_source(state: &WebState, scope: &ResolvedScope) -> psychevo_runtime::Result<Value> {
    state.inner.gateway.clear_source_binding(&scope.source)?;
    thread_snapshot(state, scope, None)
}

fn reset_source_to_empty(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<Value> {
    state.inner.gateway.reset_source_to_empty(&scope.source)?;
    thread_snapshot(state, scope, None)
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

fn user_shell_context_options(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<String>,
) -> UserShellContextOptions {
    UserShellContextOptions {
        state: state.inner.state.clone(),
        session: thread_id,
        continue_latest: false,
        source: scope.source.kind.clone(),
        continue_sources: Vec::new(),
        config_path: state.inner.config_path.clone(),
        model: None,
        reasoning_effort: None,
        mode: RunMode::Default,
        inherited_env: Some(state.inner.inherited_env.clone()),
    }
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
            .and_then(|metadata| metadata.get(ACP_PEER_METADATA_KEY).cloned())
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
        AuthContext::Browser { .. } => {
            let session = current_browser_session(state, auth)?;
            Ok(ResolvedScope {
                workdir: session.workdir.clone(),
                source: session.source.clone(),
            })
        }
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
    state: &WebState,
    auth: &AuthContext,
    scope: wire::GatewayRequestScope,
) -> psychevo_runtime::Result<ResolvedScope> {
    let workdir = canonicalize_workdir(Path::new(&scope.workdir))?;
    authorize_workdir(state, auth, &workdir)?;
    Ok(ResolvedScope {
        source: source_from_input(
            Some(scope.source),
            &workdir,
            wire::GatewaySourceLifetime::Persistent,
        ),
        workdir,
    })
}

fn resolve_start_scope(
    state: &WebState,
    auth: &AuthContext,
    scope: wire::GatewayRequestScope,
) -> psychevo_runtime::Result<ResolvedScope> {
    let workdir = canonicalize_workdir(Path::new(&scope.workdir))?;
    authorize_start_workdir(state, auth, &workdir)?;
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
    authorize_workdir(state, auth, &workdir)?;
    Ok(workdir)
}

fn resolve_session_workdir_filter(
    state: &WebState,
    auth: &AuthContext,
    workdir: Option<String>,
) -> psychevo_runtime::Result<Option<PathBuf>> {
    let Some(workdir) = workdir else {
        return Ok(None);
    };
    let workdir = canonicalize_workdir(Path::new(&workdir))?;
    authorize_workdir(state, auth, &workdir)?;
    Ok(Some(workdir))
}

fn resolved_scope_for_thread(
    state: &WebState,
    thread_id: &str,
) -> psychevo_runtime::Result<ResolvedScope> {
    let summary = state
        .inner
        .state
        .store()
        .session_summary(thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
    let workdir = PathBuf::from(summary.workdir);
    Ok(ResolvedScope {
        source: workdir_source(&workdir),
        workdir,
    })
}

fn update_browser_session_scope(state: &WebState, auth: &AuthContext, scope: &ResolvedScope) {
    let AuthContext::Browser { session_id, .. } = auth else {
        return;
    };
    state
        .inner
        .browser_sessions
        .lock()
        .expect("web browser sessions poisoned")
        .insert(
            session_id.clone(),
            BrowserSession {
                workdir: scope.workdir.clone(),
                source: scope.source.clone(),
            },
        );
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

fn dynamic_slash_commands(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<Vec<DynamicSlashCommand>> {
    let mut commands = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for bundle in list_skill_bundles(&state.inner.home, &scope.workdir)? {
        if seen.insert(bundle.slug.clone()) {
            commands.push(DynamicSlashCommand {
                name: bundle.slug.clone(),
                summary: bundle.description,
                prompt: skill_prompt_marker(&bundle.slug, ""),
            });
        }
    }
    for skill in discover_gateway_skills(state, scope)?.skills {
        if skill.disable_model_invocation || !skill.supported_on_current_platform {
            continue;
        }
        if seen.insert(skill.name.clone()) {
            commands.push(DynamicSlashCommand {
                name: skill.name.clone(),
                summary: skill.description,
                prompt: skill_prompt_marker(&skill.name, ""),
            });
        }
    }
    Ok(commands)
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
        '/' => slash_completion_items(state, scope, params.thread_id.as_deref(), &query)?,
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
) -> psychevo_runtime::Result<Vec<wire::CompletionItem>> {
    let active_turn = thread_id
        .map(|thread_id| state.activity(&scope.source, Some(thread_id)).running)
        .unwrap_or_else(|| state.activity(&scope.source, None).running);
    let dynamic = dynamic_slash_commands(state, scope)?;
    let available = available_slash_commands_for_surface(
        &gateway_command_capabilities(),
        active_turn,
        &dynamic,
        MAX_COMPLETION_ITEMS,
    );
    Ok(available
        .commands
        .into_iter()
        .filter(web_desktop_command_visible)
        .filter(|command| command_matches(command, query))
        .map(|command| wire::CompletionItem {
            id: format!("command:{}", command.name),
            sigil: "/".to_string(),
            label: format!("/{}", command.name),
            insert_text: format!("/{}", command.name),
            kind: "command".to_string(),
            detail: Some(command_completion_detail(&command)),
            target: None,
            sort_text: Some(format!("command:{}", command.name)),
        })
        .collect())
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

fn workspace_files_value(scope: &ResolvedScope) -> psychevo_runtime::Result<Value> {
    let mut entries = Vec::new();
    let mut truncated = false;
    collect_workspace_file_entries(
        &scope.workdir,
        &scope.workdir,
        0,
        &mut entries,
        &mut truncated,
    );
    Ok(serde_json::to_value(wire::WorkspaceFilesResult {
        root: scope.workdir.display().to_string(),
        entries,
        truncated,
    })?)
}

fn workspace_create_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::WorkspaceCreateParams,
) -> psychevo_runtime::Result<Value> {
    let dir_name = workspace_dir_name(&params.name)?;
    let options = state.run_options(state.inner.workdir.clone(), None);
    let root = canonicalize_workdir(&resolve_workspace_root(&options, &state.inner.workdir)?)?;
    let workdir = canonicalize_workdir(&root.join(&dir_name))?;
    if !workdir.starts_with(&root) {
        return Err(Error::Message(
            "workspace path is outside the configured workspace root".to_string(),
        ));
    }
    let scope = ResolvedScope {
        source: workdir_source(&workdir),
        workdir,
    };
    update_browser_session_scope(state, auth, &scope);
    Ok(serde_json::to_value(wire::WorkspaceCreateResult {
        workdir: scope.workdir.display().to_string(),
        scope: scope.to_wire_scope(),
    })?)
}

fn workspace_dir_name(input: &str) -> psychevo_runtime::Result<String> {
    let name = input.trim();
    if name.is_empty() {
        return Err(Error::Message(
            "workspace name must not be empty".to_string(),
        ));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(Error::Message(
            "workspace name must be a single directory name".to_string(),
        ));
    }
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(name.to_string()),
        _ => Err(Error::Message(
            "workspace name must be a single directory name".to_string(),
        )),
    }
}

fn collect_workspace_file_entries(
    root: &Path,
    dir: &Path,
    depth: usize,
    entries: &mut Vec<wire::WorkspaceFileEntry>,
    truncated: &mut bool,
) {
    if depth > MAX_FILE_COMPLETION_DEPTH || entries.len() >= MAX_WORKSPACE_FILE_ITEMS {
        *truncated = true;
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    let mut children = read_dir.flatten().collect::<Vec<_>>();
    children.sort_by_key(|entry| {
        let dir_rank = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            0
        } else {
            1
        };
        (
            dir_rank,
            entry.file_name().to_string_lossy().to_ascii_lowercase(),
        )
    });
    for entry in children {
        if entries.len() >= MAX_WORKSPACE_FILE_ITEMS {
            *truncated = true;
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if should_skip_completion_path(&name) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let is_dir = file_type.is_dir();
        if !is_dir && !file_type.is_file() {
            continue;
        }
        entries.push(wire::WorkspaceFileEntry {
            path: relative,
            name,
            kind: if is_dir {
                wire::WorkspaceFileKind::Directory
            } else {
                wire::WorkspaceFileKind::File
            },
            depth,
        });
        if is_dir {
            collect_workspace_file_entries(root, &path, depth + 1, entries, truncated);
        }
    }
}

fn workspace_file_read_value(scope: &ResolvedScope, path: &str) -> psychevo_runtime::Result<Value> {
    let resolved = resolve_workspace_relative_path(&scope.workdir, path)?;
    let display_path =
        path_from_root(&scope.workdir, &resolved).unwrap_or_else(|| normalize_workspace_path(path));
    let snapshot = match read_workspace_text_snapshot(&resolved) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return Ok(serde_json::to_value(wire::WorkspaceFileReadResult {
                path: display_path,
                content: None,
                truncated: false,
                binary: false,
                editable: false,
                editable_reason: Some(err.to_string()),
                size_bytes: 0,
                revision: "unreadable".to_string(),
                line_ending: None,
                unreadable: Some(err.to_string()),
            })?);
        }
    };
    let editable_reason = workspace_editable_reason(&snapshot);
    Ok(serde_json::to_value(wire::WorkspaceFileReadResult {
        path: display_path,
        content: snapshot.content,
        truncated: snapshot.truncated,
        binary: snapshot.binary,
        editable: editable_reason.is_none(),
        editable_reason,
        size_bytes: snapshot.size_bytes,
        revision: snapshot.revision,
        line_ending: snapshot.line_ending,
        unreadable: None,
    })?)
}

fn workspace_file_write_value(
    scope: &ResolvedScope,
    params: wire::WorkspaceFileWriteParams,
) -> psychevo_runtime::Result<Value> {
    if params.content.len() > MAX_WORKSPACE_TEXT_FILE_BYTES {
        return Err(Error::Message(
            "workspace file is larger than 1 MB".to_string(),
        ));
    }
    if params.content.as_bytes().contains(&0) {
        return Err(Error::Message(
            "workspace file content must be text".to_string(),
        ));
    }
    let resolved = resolve_workspace_write_path(&scope.workdir, &params.path)?;
    let path = path_from_root(&scope.workdir, &resolved)
        .unwrap_or_else(|| normalize_workspace_path(&params.path));
    let current_revision = workspace_path_revision(&scope.workdir, &path)?;
    if !params.force
        && let Some(expected) = params.expected_revision.as_deref()
        && expected != current_revision
    {
        return Err(Error::Message("workspace file changed on disk".to_string()));
    }
    std::fs::write(&resolved, params.content.as_bytes())?;
    let revision = workspace_path_revision(&scope.workdir, &path)?;
    Ok(serde_json::to_value(wire::WorkspaceFileWriteResult {
        path,
        revision,
        size_bytes: params.content.len(),
        line_ending: detect_line_ending(&params.content),
    })?)
}

fn resolve_workspace_relative_path(root: &Path, path: &str) -> psychevo_runtime::Result<PathBuf> {
    let raw = Path::new(path);
    if raw.is_absolute() || path.contains('\0') {
        return Err(Error::Message(
            "workspace path must be relative".to_string(),
        ));
    }
    let normalized = normalize_workspace_path(path);
    if normalized.is_empty() || normalized.starts_with("../") || normalized == ".." {
        return Err(Error::Message(
            "workspace path must be relative".to_string(),
        ));
    }
    let candidate = root.join(&normalized);
    let canonical = candidate.canonicalize()?;
    if !canonical.starts_with(root) {
        return Err(Error::Message(
            "workspace path is outside the workspace".to_string(),
        ));
    }
    Ok(canonical)
}

fn resolve_workspace_write_path(root: &Path, path: &str) -> psychevo_runtime::Result<PathBuf> {
    let raw = Path::new(path);
    if raw.is_absolute() || path.contains('\0') {
        return Err(Error::Message(
            "workspace path must be relative".to_string(),
        ));
    }
    let normalized = normalize_workspace_path(path);
    if normalized.is_empty() || normalized.starts_with("../") || normalized == ".." {
        return Err(Error::Message(
            "workspace path must be relative".to_string(),
        ));
    }
    let candidate = root.join(&normalized);
    if candidate.exists() {
        return resolve_workspace_relative_path(root, &normalized);
    }
    let parent = candidate
        .parent()
        .ok_or_else(|| Error::Message("workspace file parent is unavailable".to_string()))?;
    let canonical_parent = parent.canonicalize()?;
    if !canonical_parent.starts_with(root) {
        return Err(Error::Message(
            "workspace path is outside the workspace".to_string(),
        ));
    }
    Ok(candidate)
}

fn normalize_workspace_path(path: &str) -> String {
    path.trim()
        .trim_start_matches('/')
        .replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn path_from_root(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
}

struct WorkspaceTextSnapshot {
    content: Option<String>,
    truncated: bool,
    binary: bool,
    size_bytes: usize,
    revision: String,
    line_ending: Option<String>,
}

fn read_workspace_text_snapshot(path: &Path) -> psychevo_runtime::Result<WorkspaceTextSnapshot> {
    let metadata = std::fs::metadata(path)?;
    let size_bytes = metadata.len() as usize;
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_WORKSPACE_TEXT_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > MAX_WORKSPACE_TEXT_FILE_BYTES;
    if truncated {
        bytes.truncate(MAX_WORKSPACE_TEXT_FILE_BYTES);
    }
    let binary = bytes.contains(&0) || std::str::from_utf8(&bytes).is_err();
    let content = if binary {
        None
    } else {
        Some(String::from_utf8_lossy(&bytes).into_owned())
    };
    let line_ending = content.as_deref().and_then(detect_line_ending);
    Ok(WorkspaceTextSnapshot {
        content,
        truncated,
        binary,
        size_bytes,
        revision: revision_for_bytes(&bytes, Some(size_bytes)),
        line_ending,
    })
}

fn workspace_editable_reason(snapshot: &WorkspaceTextSnapshot) -> Option<String> {
    if snapshot.binary {
        Some("Binary files cannot be edited in Workbench.".to_string())
    } else if snapshot.truncated || snapshot.size_bytes > MAX_WORKSPACE_TEXT_FILE_BYTES {
        Some("Files larger than 1 MB cannot be edited in Workbench.".to_string())
    } else {
        None
    }
}

fn workspace_path_revision(root: &Path, path: &str) -> psychevo_runtime::Result<String> {
    let resolved = resolve_workspace_write_path(root, path)?;
    if !resolved.exists() {
        return Ok("missing".to_string());
    }
    let bytes = std::fs::read(&resolved)?;
    Ok(revision_for_bytes(&bytes, Some(bytes.len())))
}

fn revision_for_bytes(bytes: &[u8], full_size: Option<usize>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    full_size.unwrap_or(bytes.len()).hash(&mut hasher);
    format!(
        "r{:016x}:{}",
        hasher.finish(),
        full_size.unwrap_or(bytes.len())
    )
}

fn detect_line_ending(content: &str) -> Option<String> {
    if content.contains("\r\n") {
        Some("crlf".to_string())
    } else if content.contains('\n') {
        Some("lf".to_string())
    } else {
        None
    }
}

fn workspace_diff_value(
    scope: &ResolvedScope,
    path: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(workspace_diff_result(scope, path)?)?)
}

fn workspace_diff_result(
    scope: &ResolvedScope,
    path: Option<&str>,
) -> psychevo_runtime::Result<wire::WorkspaceDiffResult> {
    let diff = collect_workspace_diff(&scope.workdir)?;
    let selected = path
        .map(|path| {
            let raw = Path::new(path);
            if raw.is_absolute() || path.contains('\0') {
                return Err(Error::Message(
                    "workspace diff path must be relative".to_string(),
                ));
            }
            Ok(normalize_workspace_path(path))
        })
        .transpose()?
        .filter(|path| !path.is_empty());
    let files = diff
        .files
        .iter()
        .filter(|file| {
            selected
                .as_deref()
                .is_none_or(|selected| file.path == selected)
        })
        .map(|file| wire::WorkspaceDiffFileView {
            path: file.path.clone(),
            status: workspace_diff_status(file.status),
            binary: file.binary,
            unreadable: file.unreadable,
            placeholder: file.placeholder.clone(),
        })
        .collect::<Vec<_>>();
    let unified_diff = if let Some(selected) = selected.as_deref() {
        extract_unified_diff_for_path(&diff.unified_diff, selected).unwrap_or_else(|| {
            diff.files
                .iter()
                .find(|file| file.path == selected)
                .and_then(|file| file.placeholder.clone())
                .unwrap_or_default()
        })
    } else {
        diff.unified_diff
    };
    Ok(wire::WorkspaceDiffResult {
        is_git_repo: diff.is_git_repo,
        files,
        unified_diff,
        truncation: wire::WorkspaceDiffTruncationView {
            truncated: diff.truncation.truncated,
            max_bytes: diff.truncation.max_bytes,
            max_lines: diff.truncation.max_lines,
            omitted_bytes: diff.truncation.omitted_bytes,
            omitted_lines: diff.truncation.omitted_lines,
        },
        selected_path: selected,
    })
}

fn workspace_diff_status(status: WorkspaceDiffFileStatus) -> wire::WorkspaceDiffFileStatusView {
    match status {
        WorkspaceDiffFileStatus::Modified => wire::WorkspaceDiffFileStatusView::Modified,
        WorkspaceDiffFileStatus::Added => wire::WorkspaceDiffFileStatusView::Added,
        WorkspaceDiffFileStatus::Deleted => wire::WorkspaceDiffFileStatusView::Deleted,
        WorkspaceDiffFileStatus::Untracked => wire::WorkspaceDiffFileStatusView::Untracked,
        WorkspaceDiffFileStatus::Binary => wire::WorkspaceDiffFileStatusView::Binary,
        WorkspaceDiffFileStatus::Unreadable => wire::WorkspaceDiffFileStatusView::Unreadable,
    }
}

fn capture_workspace_baseline(workdir: &Path) -> WorkspaceBaseline {
    let mut baseline = WorkspaceBaseline::default();
    if let Ok(diff) = collect_workspace_diff(workdir) {
        for file in diff.files {
            baseline
                .files
                .insert(file.path.clone(), baseline_from_pre_turn_file(&file));
        }
    }
    baseline
}

fn baseline_from_pre_turn_file(file: &WorkspaceDiffFile) -> ReviewBaseline {
    if file.binary {
        return ReviewBaseline::Unsupported {
            reason: "Binary baseline cannot be restored.".to_string(),
        };
    }
    if file.unreadable {
        return ReviewBaseline::Unsupported {
            reason: "Unreadable baseline cannot be restored.".to_string(),
        };
    }
    if matches!(file.status, WorkspaceDiffFileStatus::Deleted) {
        return ReviewBaseline::Absent;
    }
    file.new_text
        .as_ref()
        .map(|content| ReviewBaseline::Text {
            content: content.clone(),
        })
        .unwrap_or_else(|| ReviewBaseline::Unsupported {
            reason: "Baseline content is unavailable.".to_string(),
        })
}

fn build_review_files(
    workdir: &Path,
    baseline: &WorkspaceBaseline,
) -> psychevo_runtime::Result<Vec<WorkspaceReviewFile>> {
    let diff = collect_workspace_diff(workdir)?;
    let mut post_by_path = HashMap::new();
    let mut candidates = HashSet::new();
    for file in diff.files {
        candidates.insert(file.path.clone());
        post_by_path.insert(file.path.clone(), file);
    }
    for path in baseline.files.keys() {
        candidates.insert(path.clone());
    }
    let mut files = Vec::new();
    for path in candidates {
        let pre = baseline.files.get(&path);
        if let Some(pre) = pre
            && baseline_matches_current(workdir, &path, pre)?
        {
            continue;
        }
        let post = post_by_path.get(&path);
        let review_baseline = pre
            .cloned()
            .or_else(|| post.and_then(baseline_from_post_diff))
            .unwrap_or_else(|| ReviewBaseline::Unsupported {
                reason: "Turn-start baseline is unavailable.".to_string(),
            });
        if post.is_none() && matches!(review_baseline, ReviewBaseline::Unsupported { .. }) {
            continue;
        }
        let post_revision = workspace_path_revision(workdir, &path)?;
        let status = post
            .map(|file| workspace_diff_status(file.status))
            .unwrap_or(wire::WorkspaceDiffFileStatusView::Modified);
        let binary = post.is_some_and(|file| file.binary);
        let unreadable = post.is_some_and(|file| file.unreadable);
        let message = review_baseline.message();
        files.push(WorkspaceReviewFile {
            path,
            status,
            binary,
            unreadable,
            review_status: wire::WorkspaceChangeReviewStatusView::Pending,
            baseline: review_baseline,
            post_revision,
            message,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn baseline_from_post_diff(file: &WorkspaceDiffFile) -> Option<ReviewBaseline> {
    if file.binary {
        return Some(ReviewBaseline::Unsupported {
            reason: "Binary baseline cannot be restored.".to_string(),
        });
    }
    if file.unreadable {
        return Some(ReviewBaseline::Unsupported {
            reason: "Unreadable baseline cannot be restored.".to_string(),
        });
    }
    match file.status {
        WorkspaceDiffFileStatus::Added | WorkspaceDiffFileStatus::Untracked => {
            Some(ReviewBaseline::Absent)
        }
        WorkspaceDiffFileStatus::Deleted | WorkspaceDiffFileStatus::Modified => file
            .old_text
            .as_ref()
            .map(|content| ReviewBaseline::Text {
                content: content.clone(),
            })
            .or_else(|| {
                Some(ReviewBaseline::Unsupported {
                    reason: "Baseline content is unavailable.".to_string(),
                })
            }),
        WorkspaceDiffFileStatus::Binary | WorkspaceDiffFileStatus::Unreadable => {
            Some(ReviewBaseline::Unsupported {
                reason: "Baseline content is unavailable.".to_string(),
            })
        }
    }
}

fn baseline_matches_current(
    workdir: &Path,
    path: &str,
    baseline: &ReviewBaseline,
) -> psychevo_runtime::Result<bool> {
    let resolved = resolve_workspace_write_path(workdir, path)?;
    match baseline {
        ReviewBaseline::Text { content } => {
            if !resolved.exists() {
                return Ok(false);
            }
            let bytes = std::fs::read(&resolved)?;
            Ok(bytes == content.as_bytes())
        }
        ReviewBaseline::Absent => Ok(!resolved.exists()),
        ReviewBaseline::Unsupported { .. } => Ok(false),
    }
}

fn restore_review_baseline(
    workdir: &Path,
    path: &str,
    baseline: &ReviewBaseline,
) -> psychevo_runtime::Result<()> {
    let resolved = resolve_workspace_write_path(workdir, path)?;
    match baseline {
        ReviewBaseline::Text { content } => {
            std::fs::write(resolved, content.as_bytes())?;
            Ok(())
        }
        ReviewBaseline::Absent => match std::fs::remove_file(&resolved) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        },
        ReviewBaseline::Unsupported { reason } => Err(Error::Message(reason.clone())),
    }
}

fn review_group_to_wire(group: &WorkspaceReviewGroup) -> wire::WorkspaceChangeGroupView {
    wire::WorkspaceChangeGroupView {
        turn_id: group.turn_id.clone(),
        thread_id: group.thread_id.clone(),
        created_at_ms: group.created_at_ms,
        completed_at_ms: group.completed_at_ms,
        files: group.files.iter().map(review_file_to_wire).collect(),
    }
}

fn review_file_to_wire(file: &WorkspaceReviewFile) -> wire::WorkspaceChangeFileView {
    wire::WorkspaceChangeFileView {
        path: file.path.clone(),
        status: file.status,
        binary: file.binary,
        unreadable: file.unreadable,
        review_status: file.review_status,
        can_reject: file.baseline.can_reject(),
        message: file.message.clone().or_else(|| file.baseline.message()),
    }
}

fn extract_unified_diff_for_path(diff: &str, path: &str) -> Option<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    for line in diff.split_inclusive('\n') {
        if line.starts_with("diff --git ") && !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks.into_iter().find(|block| {
        let header = block.lines().next().unwrap_or_default();
        diff_header_matches_path(header, path)
            || block.lines().take(6).any(|line| {
                line.strip_prefix("+++ b/")
                    .is_some_and(|candidate| candidate == path)
                    || line
                        .strip_prefix("--- a/")
                        .is_some_and(|candidate| candidate == path)
            })
    })
}

fn diff_header_matches_path(header: &str, path: &str) -> bool {
    header.contains(&format!(" a/{path} "))
        || header.ends_with(&format!(" a/{path}"))
        || header.contains(&format!(" b/{path} "))
        || header.ends_with(&format!(" b/{path}"))
}

fn context_read_value(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(context_read_result(
        state, scope, thread_id,
    )?)?)
}

fn context_read_result(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<wire::ContextReadResult> {
    let thread_id = match thread_id {
        Some(thread_id) => Some(thread_id.to_string()),
        None => state.inner.gateway.resolve_source_thread(&scope.source)?,
    };
    let Some(thread_id) = thread_id else {
        return Ok(context_unavailable("No active session"));
    };
    let snapshot = match context_snapshot(ContextOptions {
        state: state.inner.state.clone(),
        workdir: scope.workdir.clone(),
        session: thread_id,
        config_path: state.inner.config_path.clone(),
        inherited_env: Some(state.inner.inherited_env.clone()),
    }) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return Ok(context_unavailable(&err.to_string()));
        }
    };
    let categories = snapshot
        .categories
        .iter()
        .filter(|(id, _)| id.as_str() != "free_space")
        .map(|(id, category)| wire::ContextUsageCategoryView {
            id: id.clone(),
            label: category.label.clone(),
            tokens: category.tokens,
            estimated: category.estimated,
            status: category.status.clone(),
            percent: category.percent,
            details: Some(category.details.clone()),
        })
        .collect::<Vec<_>>();
    Ok(wire::ContextReadResult {
        available: true,
        label: format_context_total_value(&snapshot),
        status: snapshot.status,
        used_tokens: snapshot.total.tokens,
        context_limit: snapshot.context_limit,
        percent: snapshot.total.percent,
        categories,
        advice: snapshot
            .advice
            .into_iter()
            .map(|advice| advice.message)
            .collect(),
    })
}

fn observability_read_value(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    let resolved_thread_id = match thread_id {
        Some(thread_id) => Some(thread_id.to_string()),
        None => state.inner.gateway.resolve_source_thread(&scope.source)?,
    };
    let metadata = match resolved_thread_id.as_deref() {
        Some(session_id) => state.inner.state.store().session_metadata(session_id)?,
        None => None,
    };
    let peer_usage = metadata.as_ref().and_then(acp_peer_usage_update);
    let context = match peer_usage.and_then(acp_peer_context_read_result) {
        Some(context) => context,
        None => context_read_result(state, scope, resolved_thread_id.as_deref())?,
    };
    let usage = match resolved_thread_id {
        Some(session_id) => {
            let summary = session_usage_summary(SessionUsageOptions {
                state: state.inner.state.clone(),
                session_id,
            })?;
            let mut view = wire::SessionUsageSummaryView {
                available: true,
                session_id: Some(summary.session_id),
                provider: Some(summary.provider),
                model: Some(summary.model),
                message_count: summary.message_count,
                assistant_message_count: summary.assistant_message_count,
                context_input_tokens: summary.context_input_tokens,
                billable_input_tokens: summary.billable_input_tokens,
                billable_output_tokens: summary.billable_output_tokens,
                reasoning_tokens: summary.reasoning_tokens,
                cache_read_tokens: summary.cache_read_tokens,
                cache_write_tokens: summary.cache_write_tokens,
                reported_total_tokens: summary.reported_total_tokens,
                estimated_cost_nanodollars: summary.estimated_cost_nanodollars,
                unknown_pricing_count: summary.unknown_pricing_count,
                cache_read_percent: summary.cache_read_percent,
            };
            apply_acp_peer_usage_to_summary(&mut view, peer_usage);
            view
        }
        None => usage_unavailable(),
    };
    Ok(serde_json::to_value(wire::ObservabilityReadResult {
        context,
        usage,
    })?)
}

fn acp_peer_usage_update(metadata: &Value) -> Option<&Value> {
    metadata.get(ACP_PEER_METADATA_KEY)?.get("usageUpdate")
}

fn acp_peer_context_read_result(usage: &Value) -> Option<wire::ContextReadResult> {
    let used = usage_u64_field(usage, "used")?;
    let size = usage_u64_field(usage, "size")?;
    let percent = (size > 0).then(|| (used as f64 / size as f64) * 100.0);
    Some(wire::ContextReadResult {
        available: true,
        label: format_context_total_value_parts(used, false, Some(size), percent),
        status: "reported by ACP peer".to_string(),
        used_tokens: used,
        context_limit: Some(size),
        percent,
        categories: Vec::new(),
        advice: Vec::new(),
    })
}

fn apply_acp_peer_usage_to_summary(
    usage: &mut wire::SessionUsageSummaryView,
    peer_usage: Option<&Value>,
) {
    let Some(peer_usage) = peer_usage else {
        return;
    };
    if let Some(used) = usage_u64_field(peer_usage, "used") {
        if usage.reported_total_tokens == 0 {
            usage.reported_total_tokens = used;
        }
        if usage.context_input_tokens == 0 {
            usage.context_input_tokens = used;
        }
    }
    if usage.estimated_cost_nanodollars == 0
        && let Some(cost) = acp_peer_usage_cost_nanodollars(peer_usage)
    {
        usage.estimated_cost_nanodollars = cost;
    }
}

fn usage_u64_field(value: &Value, field: &str) -> Option<u64> {
    value.get(field).and_then(|value| {
        value.as_u64().or_else(|| {
            value
                .as_f64()
                .filter(|number| *number >= 0.0)
                .map(|number| number as u64)
        })
    })
}

fn acp_peer_usage_cost_nanodollars(usage: &Value) -> Option<i64> {
    let cost = usage.get("cost")?;
    let amount = cost.get("amount").and_then(Value::as_f64)?;
    let currency = cost
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or("USD");
    if !currency.eq_ignore_ascii_case("USD") || amount < 0.0 {
        return None;
    }
    Some((amount * 1_000_000_000.0).round() as i64)
}

fn context_unavailable(label: &str) -> wire::ContextReadResult {
    wire::ContextReadResult {
        available: false,
        label: label.to_string(),
        status: "unavailable".to_string(),
        used_tokens: 0,
        context_limit: None,
        percent: None,
        categories: Vec::new(),
        advice: Vec::new(),
    }
}

fn usage_unavailable() -> wire::SessionUsageSummaryView {
    wire::SessionUsageSummaryView {
        available: false,
        session_id: None,
        provider: None,
        model: None,
        message_count: 0,
        assistant_message_count: 0,
        context_input_tokens: 0,
        billable_input_tokens: 0,
        billable_output_tokens: 0,
        reasoning_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reported_total_tokens: 0,
        estimated_cost_nanodollars: 0,
        unknown_pricing_count: 0,
        cache_read_percent: None,
    }
}

fn settings_read_value(
    state: &WebState,
    workdir: &Path,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    let controls = workbench_controls_value(state, workdir, thread_id)?;
    let project = workbench_project_value(workdir);
    Ok(json!({
        "workdir": workdir,
        "project": project,
        "memoryResources": {"mode": "status_only", "available": true},
        "secrets": {"frontendPersistence": "disabled"},
        "controls": controls
    }))
}

fn workbench_controls_value(
    state: &WebState,
    workdir: &Path,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<wire::WorkbenchControlsView> {
    let options = state.run_options(workdir.to_path_buf(), None);
    let agent = session_control_agent(state, thread_id)?;
    let selected = selected_configured_model(&options).ok().flatten();
    let configured = configured_models(&options).unwrap_or_default();
    Ok(wire::WorkbenchControlsView {
        permission_mode: PermissionMode::Default.as_str().to_string(),
        mode: RunMode::Default.as_str().to_string(),
        agent,
        model: selected
            .as_ref()
            .map(|model| format!("{}/{}", model.provider, model.model)),
        variant: selected
            .as_ref()
            .and_then(|model| model.reasoning_effort.clone())
            .or_else(|| Some("none".to_string())),
        permission_mode_options: ["default", "acceptEdits", "dontAsk", "bypassPermissions"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        mode_options: ["default", "plan"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        model_options: configured
            .into_iter()
            .map(|model| format!("{}/{}", model.provider, model.model))
            .collect(),
        variant_options: ["none", "minimal", "low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .map(str::to_string)
            .collect(),
    })
}

fn session_control_agent(
    state: &WebState,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Option<String>> {
    let Some(thread_id) = thread_id else {
        return Ok(None);
    };
    let metadata = state.inner.state.store().session_metadata(thread_id)?;
    Ok(match main_agent_from_session_metadata(metadata.as_ref()) {
        LoadedMainAgent::Agent(agent) => Some(agent),
        LoadedMainAgent::Default | LoadedMainAgent::Missing => None,
    })
}

fn update_session_agent_setting(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: &str,
    input: Option<&str>,
) -> psychevo_runtime::Result<()> {
    let summary = state
        .inner
        .state
        .store()
        .session_summary(thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
    if Path::new(&summary.workdir) != scope.workdir.as_path() {
        return Err(Error::Message(format!(
            "session {thread_id} does not belong to {}",
            scope.workdir.display()
        )));
    }
    let Some(input) = input else {
        state.inner.state.store().set_session_metadata_field(
            thread_id,
            SESSION_MAIN_AGENT_METADATA_KEY,
            Some(main_agent_default_metadata()),
        )?;
        return Ok(());
    };
    let input = input.trim();
    if input.is_empty() {
        return Err(Error::Message(
            "settings/update agent must be null or a concrete agent".to_string(),
        ));
    }
    let catalog = discover_gateway_agents(state, scope)?;
    if catalog.shadowed_agents.iter().any(|agent| {
        agent
            .file_path
            .as_ref()
            .is_some_and(|path| path.to_string_lossy() == input)
    }) {
        return Err(Error::Message(format!(
            "shadowed agent definitions cannot be used as main: {input}"
        )));
    }
    let agent =
        resolve_agent_definition(&catalog, input, &scope.workdir, &state.inner.inherited_env)?;
    state.inner.state.store().set_session_metadata_field(
        thread_id,
        SESSION_MAIN_AGENT_METADATA_KEY,
        Some(main_agent_metadata(
            input,
            &agent.name,
            agent.source,
            agent.file_path.as_ref(),
        )),
    )?;
    Ok(())
}

fn workbench_project_value(workdir: &Path) -> wire::WorkbenchProjectView {
    wire::WorkbenchProjectView {
        path: workdir.display().to_string(),
        display_path: display_workdir(workdir),
        branch: current_git_branch(workdir),
    }
}

fn display_workdir(workdir: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = home
        && let Ok(relative) = workdir.strip_prefix(&home)
    {
        let relative = relative.to_string_lossy();
        return if relative.is_empty() {
            "~".to_string()
        } else {
            format!("~/{}", relative.replace('\\', "/"))
        };
    }
    workdir.to_string_lossy().replace('\\', "/")
}

fn current_git_branch(workdir: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(workdir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}

fn command_execute_value(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::CommandExecuteParams,
) -> psychevo_runtime::Result<Value> {
    let raw = params.command.trim().to_string();
    let thread_id = params.thread_id.clone();
    if raw.is_empty() {
        return Ok(serde_json::to_value(command_rejected_unknown(
            &raw,
            Some("empty command".to_string()),
            None,
        ))?);
    }
    let active_turn = thread_id
        .as_deref()
        .map(|thread_id| state.activity(&scope.source, Some(thread_id)).running)
        .unwrap_or_else(|| state.activity(&scope.source, None).running);
    let dynamic = dynamic_slash_commands(state, scope)?;
    let result = match parse_slash_command_line(&raw) {
        SlashCommandParse::Known(invocation) => {
            let action = invocation.spec.action;
            if !web_desktop_action_visible(action) {
                command_unsupported(
                    &raw,
                    action,
                    web_desktop_unavailable_message(invocation.spec.canonical, action),
                )
            } else if active_turn
                && matches!(action, SlashCommandAction::Undo | SlashCommandAction::Redo)
            {
                let command_name = invocation.spec.canonical;
                command_known_result(
                    &raw,
                    action,
                    true,
                    Some(format!(
                        "interrupt requested; run {command_name} again after the turn settles"
                    )),
                    Some(json!({"type": "turnInterrupt", "threadId": thread_id})),
                )
            } else {
                match slash_invocation_effect(
                    &invocation,
                    &gateway_command_capabilities(),
                    SlashCommandSurface::WebDesktop,
                    active_turn,
                ) {
                    Ok(effect) => {
                        command_result_from_effect(state, scope, &raw, action, effect, thread_id)?
                    }
                    Err(message) => command_unsupported(&raw, action, message),
                }
            }
        }
        SlashCommandParse::Unknown {
            original,
            command,
            args,
        } => {
            if let Some(effect) = dynamic_slash_command_effect(&command, &args, &dynamic) {
                command_result_from_effect(
                    state,
                    scope,
                    &raw,
                    SlashCommandAction::SkillInvoke,
                    effect,
                    thread_id,
                )?
            } else {
                command_rejected_unknown(
                    &command,
                    None,
                    Some(json!({"type": "passThroughPrompt", "text": original})),
                )
            }
        }
        SlashCommandParse::NotSlash => command_rejected_unknown(
            &raw,
            None,
            Some(json!({"type": "passThroughPrompt", "text": raw})),
        ),
    };
    Ok(serde_json::to_value(result)?)
}

fn command_result_from_effect(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    effect: SlashCommandEffect,
    thread_id: Option<String>,
) -> psychevo_runtime::Result<wire::CommandExecuteResult> {
    match effect {
        SlashCommandEffect::LocalText => match action {
            SlashCommandAction::Help => Ok(command_action(
                raw,
                action,
                json!({"type": "showPanel", "panel": "commands"}),
            )),
            SlashCommandAction::Status
            | SlashCommandAction::Usage
            | SlashCommandAction::Context => Ok(command_action(
                raw,
                action,
                json!({"type": "showPanel", "panel": "status"}),
            )),
            _ => Ok(command_accepted_message(raw, action, None)),
        },
        SlashCommandEffect::PassThroughPrompt(text) => Ok(command_action(
            raw,
            action,
            json!({"type": "passThroughPrompt", "text": text}),
        )),
        SlashCommandEffect::SubmitPrompt(text) => Ok(command_action(
            raw,
            action,
            json!({"type": "submitPrompt", "text": text, "displayText": raw}),
        )),
        SlashCommandEffect::Steer(text) => Ok(command_action(
            raw,
            action,
            json!({"type": "steerPrompt", "text": text}),
        )),
        SlashCommandEffect::Queue(text) => Ok(command_action(
            raw,
            action,
            json!({"type": "queuePrompt", "text": text, "displayText": raw}),
        )),
        SlashCommandEffect::PendingCancel => Ok(command_action(
            raw,
            action,
            json!({"type": "turnInterrupt", "threadId": thread_id}),
        )),
        SlashCommandEffect::NewSession => {
            Ok(command_action(raw, action, json!({"type": "threadStart"})))
        }
        SlashCommandEffect::SessionsList => Ok(command_action(
            raw,
            action,
            json!({"type": "showPanel", "panel": "history"}),
        )),
        SlashCommandEffect::ResumeSession { .. } => Ok(command_action(
            raw,
            action,
            json!({"type": "showPanel", "panel": "history"}),
        )),
        SlashCommandEffect::Agents => Ok(command_action(
            raw,
            action,
            json!({"type": "showPanel", "panel": "agents"}),
        )),
        SlashCommandEffect::Export { .. } => Ok(command_action(
            raw,
            action,
            json!({"type": "downloadSession", "kind": "export", "threadId": thread_id}),
        )),
        SlashCommandEffect::Share { .. } => Ok(command_action(
            raw,
            action,
            json!({"type": "downloadSession", "kind": "share", "threadId": thread_id}),
        )),
        SlashCommandEffect::Fork(prompt) => Ok(command_action(
            raw,
            action,
            json!({"type": "submitPrompt", "text": prompt, "displayText": raw}),
        )),
        SlashCommandEffect::Compact { instructions } => Ok(command_action(
            raw,
            action,
            json!({"type": "submitPrompt", "text": compact_prompt_text(instructions), "displayText": raw}),
        )),
        SlashCommandEffect::Diff => {
            let diff = workspace_diff_result(scope, None)?;
            Ok(command_action(
                raw,
                action,
                json!({"type": "workspaceDiff", "diff": diff}),
            ))
        }
        SlashCommandEffect::SandboxShow => {
            let options = state.run_options(scope.workdir.clone(), thread_id.clone());
            let status = psychevo_runtime::sandbox_status_text(&options, RunMode::Default)?;
            Ok(command_accepted_message(raw, action, Some(status)))
        }
        SlashCommandEffect::Undo => Ok(command_session_undo(state, scope, raw, action, thread_id)),
        SlashCommandEffect::Redo => Ok(command_session_redo(state, scope, raw, action, thread_id)),
        SlashCommandEffect::Unsupported(message) => Ok(command_unsupported(raw, action, message)),
        SlashCommandEffect::ShowModel
        | SlashCommandEffect::SetModel { .. }
        | SlashCommandEffect::SetVariant(_)
        | SlashCommandEffect::SetMode(_)
        | SlashCommandEffect::PermissionsShow
        | SlashCommandEffect::PermissionAdd { .. }
        | SlashCommandEffect::PermissionRemove { .. }
        | SlashCommandEffect::ToolsShow
        | SlashCommandEffect::ToolsetSet { .. }
        | SlashCommandEffect::Rename(_)
        | SlashCommandEffect::Skills { .. }
        | SlashCommandEffect::Bundles { .. }
        | SlashCommandEffect::Curator { .. } => Ok(command_unsupported(
            raw,
            action,
            web_desktop_unavailable_message(raw.split_whitespace().next().unwrap_or(raw), action),
        )),
    }
}

fn command_session_undo(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    thread_id: Option<String>,
) -> wire::CommandExecuteResult {
    let options = match command_session_undo_options(state, scope, thread_id, "undo") {
        Ok(options) => options,
        Err(message) => return command_unsupported(raw, action, message),
    };
    match undo_session(options) {
        Ok(result) => command_known_result(
            raw,
            action,
            true,
            Some(format!(
                "undone {} messages; prompt restored",
                result.reverted_messages
            )),
            Some(json!({
                "type": "sessionUndo",
                "threadId": result.session_id,
                "prompt": result.prompt,
                "revertedMessages": result.reverted_messages
            })),
        ),
        Err(err) => command_unsupported(raw, action, err.to_string()),
    }
}

fn command_session_redo(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    thread_id: Option<String>,
) -> wire::CommandExecuteResult {
    let options = match command_session_undo_options(state, scope, thread_id, "redo") {
        Ok(options) => options,
        Err(message) => return command_unsupported(raw, action, message),
    };
    match redo_session(options) {
        Ok(result) => {
            let suffix = if result.complete {
                "complete"
            } else {
                "partial"
            };
            command_known_result(
                raw,
                action,
                true,
                Some(format!(
                    "redone {} messages; {suffix}",
                    result.restored_messages
                )),
                Some(json!({
                    "type": "sessionRedo",
                    "threadId": result.session_id,
                    "restoredMessages": result.restored_messages,
                    "complete": result.complete
                })),
            )
        }
        Err(err) => command_unsupported(raw, action, err.to_string()),
    }
}

fn command_session_undo_options(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<String>,
    verb: &str,
) -> std::result::Result<SessionUndoOptions, String> {
    let Some(thread_id) = thread_id else {
        return Err(format!("no current session to {verb}"));
    };
    let summary = state
        .inner
        .state
        .store()
        .session_summary(&thread_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("session not found: {thread_id}"))?;
    if Path::new(&summary.workdir) != scope.workdir.as_path() {
        return Err(format!(
            "session {thread_id} does not belong to {}",
            scope.workdir.display()
        ));
    }
    Ok(SessionUndoOptions {
        state: state.inner.state.clone(),
        workdir: scope.workdir.clone(),
        snapshot_root: state.inner.home.join("snapshots"),
        session_id: thread_id,
    })
}

fn command_action(
    raw: &str,
    slash_action: SlashCommandAction,
    action: Value,
) -> wire::CommandExecuteResult {
    command_known_result(raw, slash_action, true, None, Some(action))
}

fn command_accepted_message(
    raw: &str,
    slash_action: SlashCommandAction,
    message: Option<String>,
) -> wire::CommandExecuteResult {
    command_known_result(raw, slash_action, true, message, None)
}

fn command_unsupported(
    raw: &str,
    slash_action: SlashCommandAction,
    message: String,
) -> wire::CommandExecuteResult {
    command_known_result(raw, slash_action, false, Some(message), None)
}

fn command_known_result(
    raw: &str,
    slash_action: SlashCommandAction,
    accepted: bool,
    message: Option<String>,
    action: Option<Value>,
) -> wire::CommandExecuteResult {
    let presentation = command_presentation(slash_action);
    wire::CommandExecuteResult {
        accepted,
        command: raw.to_string(),
        known: Some(true),
        presentation_kind: Some(presentation.kind.as_str().to_string()),
        feedback_anchor: Some(presentation.feedback_anchor.as_str().to_string()),
        alternate_action: command_alternate_action(presentation),
        message,
        action,
    }
}

fn command_rejected_unknown(
    raw: &str,
    message: Option<String>,
    action: Option<Value>,
) -> wire::CommandExecuteResult {
    wire::CommandExecuteResult {
        accepted: false,
        command: raw.to_string(),
        known: Some(false),
        presentation_kind: None,
        feedback_anchor: None,
        alternate_action: None,
        message,
        action,
    }
}

fn web_desktop_unavailable_message(command: &str, action: SlashCommandAction) -> String {
    let command = command.split_whitespace().next().unwrap_or(command);
    match action {
        SlashCommandAction::ModelShow
        | SlashCommandAction::VariantSet
        | SlashCommandAction::ModeSet => {
            format!("{command} is managed by the Workbench model controls.")
        }
        SlashCommandAction::Image => {
            format!("{command} is managed by the Workbench attachment control.")
        }
        SlashCommandAction::Permissions => {
            format!("{command} is managed by Workbench status controls.")
        }
        SlashCommandAction::Agents => {
            format!("{command} is managed by the Workbench agent selector and Settings Agents.")
        }
        SlashCommandAction::Sessions | SlashCommandAction::Resume => {
            format!("{command} is managed by Workbench history.")
        }
        SlashCommandAction::Tools
        | SlashCommandAction::Skills
        | SlashCommandAction::Bundles
        | SlashCommandAction::Curator => {
            format!("{command} is managed by Workbench panels.")
        }
        _ => format!("{command} is not available in Web/Desktop."),
    }
}

fn compact_prompt_text(instructions: Option<String>) -> String {
    match instructions {
        Some(instructions) if !instructions.trim().is_empty() => {
            format!(
                "Compact this session with these instructions:\n\n{}",
                instructions.trim()
            )
        }
        _ => "Compact this session.".to_string(),
    }
}

fn write_project_agent_definition(
    workdir: &Path,
    params: wire::AgentWriteParams,
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
    Ok(serde_json::to_value(wire::AgentWriteResult {
        written: true,
        name: params.name,
        path: path.display().to_string(),
    })?)
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
    Ok(serde_json::to_value(wire::AgentDeleteResult {
        deleted,
        name: name.to_string(),
        path: path.display().to_string(),
    })?)
}

fn project_agent_definition_path(workdir: &Path, name: &str) -> PathBuf {
    workdir
        .join(".psychevo")
        .join("agents")
        .join(format!("{name}.md"))
}

fn agent_list_result(catalog: &AgentCatalog) -> wire::AgentListResult {
    wire::AgentListResult {
        agents: catalog.agents.iter().map(agent_definition_view).collect(),
        shadowed_agents: catalog
            .shadowed_agents
            .iter()
            .map(agent_definition_view)
            .collect(),
        diagnostics: catalog
            .diagnostics
            .iter()
            .map(agent_diagnostic_view)
            .collect(),
    }
}

fn agent_read_result(agent: &AgentDefinition) -> wire::AgentReadResult {
    wire::AgentReadResult {
        agent: agent_definition_view(agent),
        instructions: agent.instructions.clone(),
    }
}

fn agent_definition_view(agent: &AgentDefinition) -> wire::AgentDefinitionView {
    wire::AgentDefinitionView {
        name: agent.name.clone(),
        description: agent.description.clone(),
        source: agent.source.as_str().to_string(),
        generated: matches!(agent.source, psychevo_runtime::AgentSource::Generated),
        path: agent
            .file_path
            .as_ref()
            .map(|path| path.display().to_string()),
        backend: agent
            .backend
            .as_ref()
            .map(|backend| wire::AgentBackendRefView {
                name: backend.name.clone(),
            }),
        entrypoints: agent
            .entrypoints
            .iter()
            .map(|entrypoint| entrypoint.as_str().to_string())
            .collect(),
        diagnostics: agent
            .diagnostics
            .iter()
            .map(agent_diagnostic_view)
            .collect(),
    }
}

fn agent_diagnostic_view(diagnostic: &AgentDiagnostic) -> wire::AgentDiagnosticView {
    wire::AgentDiagnosticView {
        kind: diagnostic.kind.clone(),
        message: diagnostic.message.clone(),
        path: diagnostic
            .path
            .as_ref()
            .map(|path| path.display().to_string()),
    }
}

fn agent_status_result(
    store: Option<&psychevo_runtime::SqliteStore>,
    parent_session_id: Option<&str>,
    all: bool,
) -> wire::AgentStatusResult {
    wire::AgentStatusResult {
        agents: agent_status_records(store, parent_session_id, all)
            .iter()
            .map(agent_run_view)
            .collect(),
        control: wire::AgentStatusControlView {
            spawning_paused: agent_spawn_paused(),
            max_spawn_depth_cap: MAX_AGENT_SPAWN_DEPTH_CAP,
            concurrency_cap: None,
        },
    }
}

fn agent_run_view(record: &AgentRunRecord) -> wire::AgentRunView {
    wire::AgentRunView {
        id: record.id.clone(),
        task_name: record.task_name.clone(),
        agent_name: record.agent_name.clone(),
        task: record.task.clone(),
        parent_session_id: record.parent_session_id.clone(),
        child_session_id: record.child_session_id.clone(),
        role: record.role.as_str().to_string(),
        background: record.background,
        status: record.status.as_str().to_string(),
        edge_status: record.edge_status.map(|status| status.as_str().to_string()),
        started_at_ms: record.started_at_ms,
        ended_at_ms: record.ended_at_ms,
        outcome: record.outcome.clone(),
        final_answer: record.final_answer.clone(),
        error: record.error.clone(),
        effective_max_spawn_depth: record.effective_max_spawn_depth,
    }
}

fn backend_value_with_sources(
    backend: &AgentBackendConfig,
    source_targets: Vec<wire::BackendConfigTarget>,
) -> wire::BackendConfigView {
    wire::BackendConfigView {
        id: backend.id.clone(),
        kind: backend.kind.as_str().to_string(),
        enabled: backend.enabled,
        label: backend.label.clone(),
        description: backend.description.clone(),
        command: backend.command.clone(),
        args: backend.args.clone(),
        cwd: backend.cwd.clone(),
        entrypoints: backend
            .entrypoints
            .iter()
            .map(|entrypoint| entrypoint.as_str().to_string())
            .collect(),
        client_capabilities: backend.client_capabilities.iter().cloned().collect(),
        mcp_servers: backend.mcp_servers.iter().cloned().collect(),
        env_keys: backend.env.keys().cloned().collect(),
        source_targets,
        diagnostics: backend_diagnostics(backend),
    }
}

fn backend_values_for_scope(
    state: &WebState,
    scope: &ResolvedScope,
    backends: &BTreeMap<String, AgentBackendConfig>,
) -> psychevo_runtime::Result<Vec<wire::BackendConfigView>> {
    backends
        .values()
        .map(|backend| {
            Ok(backend_value_with_sources(
                backend,
                backend_source_targets(state, scope, &backend.id)?,
            ))
        })
        .collect()
}

fn write_backend_config(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::BackendWriteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.id) {
        return Err(Error::Message(format!("invalid backend id: {}", params.id)));
    }
    ensure_profile_config_for_backend_write(state, scope, params.target)?;
    let existing_backends = load_agent_backend_configs(
        &state.inner.home,
        &scope.workdir,
        &state.inner.inherited_env,
    )?;
    let value = backend_config_json(&params, existing_backends.get(&params.id))?;
    let target = params.target;
    let config_dir = backend_config_dir(state, scope, target)?;
    let result = set_config_value(config_dir, &format!("agents.backends.{}", params.id), value)?;
    let backends = load_agent_backend_configs(
        &state.inner.home,
        &scope.workdir,
        &state.inner.inherited_env,
    )?;
    let backend = backends
        .get(&params.id)
        .ok_or_else(|| Error::Message(format!("backend write did not reload: {}", params.id)))?;
    Ok(serde_json::to_value(wire::BackendWriteResult {
        written: true,
        changed: result.changed,
        path: result.path.display().to_string(),
        target,
        backend: backend_value_with_sources(
            backend,
            backend_source_targets(state, scope, &backend.id)?,
        ),
    })?)
}

fn delete_backend_config(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::BackendDeleteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.id) {
        return Err(Error::Message(format!("invalid backend id: {}", params.id)));
    }
    let target = params.target;
    let config_dir = backend_config_dir(state, scope, target)?;
    let result = remove_config_value(config_dir, &format!("agents.backends.{}", params.id))?;
    Ok(serde_json::to_value(wire::BackendDeleteResult {
        deleted: result.changed,
        changed: result.changed,
        id: params.id,
        path: result.path.display().to_string(),
        target,
    })?)
}

fn backend_config_dir(
    state: &WebState,
    scope: &ResolvedScope,
    target: wire::BackendConfigTarget,
) -> psychevo_runtime::Result<PathBuf> {
    match target {
        wire::BackendConfigTarget::Project => Ok(scope.workdir.join(".psychevo")),
        wire::BackendConfigTarget::Profile => Ok(active_profile_config_dir(state, scope)),
    }
}

fn ensure_profile_config_for_backend_write(
    state: &WebState,
    scope: &ResolvedScope,
    target: wire::BackendConfigTarget,
) -> psychevo_runtime::Result<()> {
    if target != wire::BackendConfigTarget::Profile
        || !state
            .inner
            .inherited_env
            .get("PSYCHEVO_CONFIG")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    {
        return Ok(());
    }
    let config_path = active_profile_config_dir(state, scope).join("config.toml");
    if config_path.exists() {
        return Ok(());
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(config_path, "")?;
    Ok(())
}

fn active_profile_config_dir(state: &WebState, scope: &ResolvedScope) -> PathBuf {
    state
        .inner
        .inherited_env
        .get("PSYCHEVO_CONFIG")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .and_then(|value| {
            let path = resolve_gateway_env_path(value, state, scope);
            path.parent().map(Path::to_path_buf)
        })
        .unwrap_or_else(|| state.inner.home.clone())
}

fn resolve_gateway_env_path(value: &str, state: &WebState, scope: &ResolvedScope) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        let home = state
            .inner
            .inherited_env
            .get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| state.inner.home.clone());
        return home.join(rest);
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        scope.workdir.join(path)
    }
}

fn backend_config_json(
    params: &wire::BackendWriteParams,
    existing: Option<&AgentBackendConfig>,
) -> psychevo_runtime::Result<Value> {
    let entrypoints = if params.entrypoints.is_empty() {
        vec!["peer".to_string(), "subagent".to_string()]
    } else {
        validate_backend_entrypoints(&params.entrypoints)?
    };
    let client_capabilities = if params.client_capabilities.is_empty() {
        vec![
            "fs.read".to_string(),
            "fs.write".to_string(),
            "terminal".to_string(),
        ]
    } else {
        validate_backend_client_capabilities(&params.client_capabilities)?
    };
    let args = trimmed_string_list(&params.args);
    let mcp_servers = trimmed_string_list(&params.mcp_servers);
    let env = if params.env.is_empty() {
        existing
            .map(|backend| backend.env.clone())
            .unwrap_or_default()
    } else {
        params
            .env
            .iter()
            .filter_map(|(key, value)| {
                let key = key.trim();
                if key.is_empty() {
                    None
                } else {
                    Some((key.to_string(), value.to_string()))
                }
            })
            .collect::<BTreeMap<_, _>>()
    };
    let label = params
        .label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cwd = params
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("invocation");
    let mut object = serde_json::Map::new();
    object.insert("kind".to_string(), json!("acp"));
    object.insert("enabled".to_string(), json!(params.enabled.unwrap_or(true)));
    if let Some(label) = label {
        object.insert("label".to_string(), json!(label));
    }
    if let Some(description) = params
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("description".to_string(), json!(description));
    }
    if let Some(command) = params
        .command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("command".to_string(), json!(command));
    }
    object.insert("args".to_string(), json!(args));
    object.insert("env".to_string(), json!(env));
    object.insert("cwd".to_string(), json!(cwd));
    object.insert("entrypoints".to_string(), json!(entrypoints));
    object.insert(
        "client_capabilities".to_string(),
        json!(client_capabilities),
    );
    object.insert("mcp_servers".to_string(), json!(mcp_servers));
    Ok(Value::Object(object))
}

fn validate_backend_entrypoints(values: &[String]) -> psychevo_runtime::Result<Vec<String>> {
    let mut entrypoints = Vec::new();
    for value in values {
        let value = value.trim();
        let entrypoint = AgentEntrypoint::parse(value).ok_or_else(|| {
            Error::Message(format!(
                "backend entrypoint `{value}` must be peer or subagent"
            ))
        })?;
        let entrypoint = entrypoint.as_str().to_string();
        if !entrypoints.contains(&entrypoint) {
            entrypoints.push(entrypoint);
        }
    }
    if entrypoints.is_empty() {
        return Err(Error::Message(
            "backend entrypoints must include peer or subagent".to_string(),
        ));
    }
    Ok(entrypoints)
}

fn validate_backend_client_capabilities(
    values: &[String],
) -> psychevo_runtime::Result<Vec<String>> {
    let mut capabilities = Vec::new();
    for value in values {
        let value = value.trim();
        if !matches!(value, "fs.read" | "fs.write" | "terminal") {
            return Err(Error::Message(format!(
                "backend client capability `{value}` must be fs.read, fs.write, or terminal"
            )));
        }
        if !capabilities.iter().any(|capability| capability == value) {
            capabilities.push(value.to_string());
        }
    }
    Ok(capabilities)
}

fn trimmed_string_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn backend_source_targets(
    state: &WebState,
    scope: &ResolvedScope,
    id: &str,
) -> psychevo_runtime::Result<Vec<wire::BackendConfigTarget>> {
    let mut targets = Vec::new();
    if backend_exists_in_scope(state, scope, id, ConfigScope::Global)? {
        targets.push(wire::BackendConfigTarget::Profile);
    }
    if backend_exists_in_scope(state, scope, id, ConfigScope::Local)? {
        targets.push(wire::BackendConfigTarget::Project);
    }
    Ok(targets)
}

fn backend_exists_in_scope(
    state: &WebState,
    scope: &ResolvedScope,
    id: &str,
    config_scope: ConfigScope,
) -> psychevo_runtime::Result<bool> {
    let config_dir = match config_scope {
        ConfigScope::Global => active_profile_config_dir(state, scope),
        ConfigScope::Local => scope.workdir.join(".psychevo"),
        ConfigScope::Effective => {
            return Err(Error::Config(
                "backend source target checks require a concrete config scope".to_string(),
            ));
        }
    };
    backend_exists_in_config_dir(&config_dir, id)
}

fn backend_exists_in_config_dir(config_dir: &Path, id: &str) -> psychevo_runtime::Result<bool> {
    let config_path = config_dir.join("config.toml");
    if !config_path.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&config_path)?;
    let parsed: toml::Value = toml::from_str(&text)
        .map_err(|err| Error::Config(format!("{}: {err}", config_path.display())))?;
    Ok(parsed
        .get("agents")
        .and_then(|value| value.get("backends"))
        .and_then(|value| value.get(id))
        .is_some())
}

fn backend_diagnostics(backend: &AgentBackendConfig) -> Vec<wire::BackendDiagnosticView> {
    let mut diagnostics = Vec::new();
    if !backend.enabled {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "disabled".to_string(),
            message: "backend is disabled".to_string(),
        });
    }
    if backend.command.is_none() {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "missing_command".to_string(),
            message: "backend command is required for execution".to_string(),
        });
    }
    diagnostics
}

fn backend_doctor_value(
    backend: &AgentBackendConfig,
    env: &BTreeMap<String, String>,
) -> psychevo_runtime::Result<wire::BackendDoctorResult> {
    let mut checks = Vec::new();
    checks.push(wire::BackendDoctorCheck {
        name: "enabled".to_string(),
        ok: backend.enabled,
        message: if backend.enabled {
            "backend enabled"
        } else {
            "backend disabled"
        }
        .to_string(),
        path: None,
    });
    checks.push(wire::BackendDoctorCheck {
        name: "description".to_string(),
        ok: true,
        message: if backend
            .description
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            "description configured"
        } else {
            "description optional; using backend label"
        }
        .to_string(),
        path: None,
    });
    let command_check = match backend.command.as_deref() {
        Some(command) => match resolve_command_path(command, env) {
            Some(path) => wire::BackendDoctorCheck {
                name: "command".to_string(),
                ok: true,
                message: "command resolved".to_string(),
                path: Some(path.display().to_string()),
            },
            None => wire::BackendDoctorCheck {
                name: "command".to_string(),
                ok: false,
                message: "command was not found on PATH or as a configured path".to_string(),
                path: None,
            },
        },
        None => wire::BackendDoctorCheck {
            name: "command".to_string(),
            ok: false,
            message: "command missing".to_string(),
            path: None,
        },
    };
    checks.push(command_check);
    let ok = checks.iter().all(|check| check.ok);
    Ok(wire::BackendDoctorResult {
        id: backend.id.clone(),
        kind: backend.kind.as_str().to_string(),
        ok,
        checks,
    })
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

fn command_list_value(
    state: &WebState,
    scope: &ResolvedScope,
    active_turn: bool,
) -> psychevo_runtime::Result<Value> {
    let dynamic = dynamic_slash_commands(state, scope)?;
    let dynamic_names = dynamic
        .iter()
        .map(|command| command.name.trim_start_matches('/').to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let available = available_slash_commands_for_surface(
        &gateway_command_capabilities(),
        active_turn,
        &dynamic,
        256,
    );
    Ok(serde_json::to_value(wire::CommandListResult {
        commands: available
            .commands
            .iter()
            .filter(|command| web_desktop_command_visible(command))
            .map(|command| command_value(command, &dynamic_names))
            .collect(),
        hidden_dynamic: available.hidden_dynamic,
    })?)
}

fn command_value(
    command: &AvailableSlashCommand,
    dynamic_names: &std::collections::BTreeSet<String>,
) -> wire::CommandListItem {
    let presentation = command.presentation;
    wire::CommandListItem {
        name: command.name.clone(),
        slash: format!("/{}", command.name),
        usage: command.usage.clone(),
        summary: command.summary.clone(),
        aliases: command
            .aliases
            .iter()
            .map(|alias| alias.trim_start_matches('/').to_string())
            .collect(),
        argument_kind: command_argument_kind(command.argument_kind).to_string(),
        source: if dynamic_names.contains(&command.name) {
            "dynamic".to_string()
        } else {
            "core".to_string()
        },
        presentation_kind: Some(presentation.kind.as_str().to_string()),
        destination: Some(presentation.destination.as_str().to_string()),
        feedback_anchor: Some(presentation.feedback_anchor.as_str().to_string()),
        alternate_action: command_alternate_action(presentation),
    }
}

fn web_desktop_command_visible(command: &AvailableSlashCommand) -> bool {
    matches!(
        command.action,
        SlashCommandAction::Help
            | SlashCommandAction::Status
            | SlashCommandAction::New
            | SlashCommandAction::Sessions
            | SlashCommandAction::Resume
            | SlashCommandAction::Usage
            | SlashCommandAction::Context
            | SlashCommandAction::Diff
            | SlashCommandAction::Steer
            | SlashCommandAction::Queue
            | SlashCommandAction::Pending
            | SlashCommandAction::Sandbox
            | SlashCommandAction::Fork
            | SlashCommandAction::Compact
            | SlashCommandAction::Export
            | SlashCommandAction::Share
            | SlashCommandAction::Undo
            | SlashCommandAction::Redo
            | SlashCommandAction::SkillInvoke
    )
}

fn web_desktop_action_visible(action: SlashCommandAction) -> bool {
    matches!(
        action,
        SlashCommandAction::Help
            | SlashCommandAction::Status
            | SlashCommandAction::New
            | SlashCommandAction::Sessions
            | SlashCommandAction::Resume
            | SlashCommandAction::Usage
            | SlashCommandAction::Context
            | SlashCommandAction::Diff
            | SlashCommandAction::Steer
            | SlashCommandAction::Queue
            | SlashCommandAction::Pending
            | SlashCommandAction::Sandbox
            | SlashCommandAction::Fork
            | SlashCommandAction::Compact
            | SlashCommandAction::Export
            | SlashCommandAction::Share
            | SlashCommandAction::Undo
            | SlashCommandAction::Redo
            | SlashCommandAction::SkillInvoke
    )
}

fn command_completion_detail(command: &AvailableSlashCommand) -> String {
    let destination = match command.presentation.destination.as_str() {
        "commands" => "Panel",
        "history" => "History",
        "agents" => "Agents",
        "status" => "Status",
        "preview" => "Preview",
        "composer" => "Prompt",
        "download" => "Download",
        _ => "Command",
    };
    format!("{destination} - {}", command.summary)
}

fn command_alternate_action(
    presentation: CommandPresentation,
) -> Option<wire::CommandAlternateAction> {
    presentation
        .alternate_action
        .map(|action| wire::CommandAlternateAction {
            action_type: action.action_type.as_str().to_string(),
            target: action.target.to_string(),
            label: action.label.to_string(),
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
        CommandCapability::SessionRevert,
        CommandCapability::ArtifactWrite,
        CommandCapability::WorkspaceDiff,
        CommandCapability::ConfigWrite,
        CommandCapability::PolicyWrite,
        CommandCapability::SkillStateWrite,
    ]
}

fn current_browser_session(
    state: &WebState,
    auth: &AuthContext,
) -> psychevo_runtime::Result<BrowserSession> {
    let AuthContext::Browser { session_id } = auth else {
        return Err(Error::Message(
            "browser session is required for this operation".to_string(),
        ));
    };
    state
        .inner
        .browser_sessions
        .lock()
        .expect("web browser sessions poisoned")
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::Message("browser session is no longer active".to_string()))
}

fn authorize_workdir(
    state: &WebState,
    auth: &AuthContext,
    workdir: &Path,
) -> psychevo_runtime::Result<()> {
    match auth {
        AuthContext::Bearer => Ok(()),
        AuthContext::Browser { .. } if current_browser_session(state, auth)?.workdir == workdir => {
            Ok(())
        }
        AuthContext::Browser { .. } => Err(Error::Message(
            "browser session is not authorized for this workdir".to_string(),
        )),
    }
}

fn authorize_start_workdir(
    state: &WebState,
    auth: &AuthContext,
    workdir: &Path,
) -> psychevo_runtime::Result<()> {
    match auth {
        AuthContext::Bearer => Ok(()),
        AuthContext::Browser { .. } if current_browser_session(state, auth)?.workdir == workdir => {
            Ok(())
        }
        AuthContext::Browser { .. } if browser_known_session_project(state, workdir)? => Ok(()),
        AuthContext::Browser { .. } => Err(Error::Message(
            "browser session is not authorized for this workdir".to_string(),
        )),
    }
}

fn browser_known_session_project(
    state: &WebState,
    workdir: &Path,
) -> psychevo_runtime::Result<bool> {
    let store = state.inner.state.store();
    let active = store.list_sessions_for_workdir_with_sources(workdir, &[])?;
    if active
        .iter()
        .any(|session| human_visible_session(state, session))
    {
        return Ok(true);
    }
    let archived = store.list_archived_sessions_for_workdir_with_sources(workdir, &[])?;
    Ok(archived
        .iter()
        .any(|session| human_visible_session(state, session)))
}

fn authorize_thread(
    state: &WebState,
    auth: &AuthContext,
    thread_id: &str,
) -> psychevo_runtime::Result<()> {
    if matches!(auth, AuthContext::Bearer) {
        return Ok(());
    }
    if state
        .inner
        .state
        .store()
        .session_summary(thread_id)?
        .is_none()
    {
        return Err(Error::Message(format!("session not found: {thread_id}")));
    }
    Ok(())
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
        "scope": scope.to_wire_scope(),
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
        .transpose()?
        .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))
}

fn human_visible_session(_state: &WebState, summary: &SessionSummary) -> bool {
    if summary.parent_session_id.is_some() {
        return false;
    }
    if INTERNAL_SESSION_SOURCES.contains(&summary.source.as_str()) {
        return false;
    }
    true
}

fn session_summary_value(
    state: &WebState,
    summary: SessionSummary,
) -> psychevo_runtime::Result<Value> {
    let activity = state
        .inner
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(&summary.id));
    let entries = state.inner.gateway.thread_transcript(&summary.id)?;
    let preview = session_preview(&entries);
    let display_title = summary
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| preview.clone())
        .unwrap_or_else(|| short_thread_id(&summary.id));
    let project = session_project_value(&summary.workdir);
    Ok(json!({
        "id": summary.id,
        "workdir": summary.workdir,
        "project": project,
        "model": summary.model,
        "provider": summary.provider,
        "startedAtMs": summary.started_at_ms,
        "updatedAtMs": summary.updated_at_ms,
        "endedAtMs": summary.ended_at_ms,
        "endReason": summary.end_reason,
        "archivedAtMs": summary.archived_at_ms,
        "messageCount": summary.message_count,
        "toolCallCount": summary.tool_call_count,
        "visibleEntryCount": entries.len(),
        "activity": activity,
        "title": summary.title,
        "displayTitle": display_title,
        "preview": preview,
    }))
}

fn session_project_value(workdir: &str) -> Value {
    let path = PathBuf::from(workdir);
    json!({
        "workdir": workdir,
        "label": project_label(&path),
        "displayPath": display_workdir(&path),
    })
}

fn project_label(workdir: &Path) -> String {
    workdir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("workdir")
        .to_string()
}

fn session_preview(entries: &[TranscriptEntry]) -> Option<String> {
    entries
        .iter()
        .find(|entry| entry.role == TranscriptEntryRole::User)
        .and_then(entry_preview)
        .or_else(|| entries.iter().find_map(entry_preview))
}

fn entry_preview(entry: &TranscriptEntry) -> Option<String> {
    entry
        .blocks
        .iter()
        .filter_map(|block| block.preview.as_deref().or(block.body.as_deref()))
        .map(compact_display_text)
        .find(|text| !text.is_empty())
}

fn compact_display_text(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 120;
    if collapsed.chars().count() <= MAX_CHARS {
        return collapsed;
    }
    let mut out = collapsed.chars().take(MAX_CHARS - 1).collect::<String>();
    out.push('…');
    out
}

fn short_thread_id(id: &str) -> String {
    id.chars().take(8).collect()
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

fn gateway_shell_result_value(result: GatewayShellResult) -> Value {
    json!({
        "thread": result.thread,
        "command": result.result.command,
        "outcome": result.result.outcome.as_str(),
        "toolFailures": result.result.tool_failures,
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
    use std::ffi::OsStr;

    fn web_state() -> (tempfile::TempDir, WebState) {
        web_state_with_env(BTreeMap::new())
    }

    fn web_state_with_env(
        inherited_env: BTreeMap<String, String>,
    ) -> (tempfile::TempDir, WebState) {
        let temp = tempfile::tempdir().expect("tempdir");
        let workdir = temp.path().join("work");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let mut env = BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                home.to_string_lossy().to_string(),
            ),
        ]);
        env.extend(inherited_env);
        let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = Gateway::new(state);
        let config = GatewayWebServerConfig::new(
            gateway,
            home,
            workdir,
            None,
            env,
            temp.path().join("static"),
        );
        (temp, WebState::new(config))
    }

    fn write_agent_definition(dir: &Path, name: &str, description: &str) -> PathBuf {
        std::fs::create_dir_all(dir).expect("agent dir");
        let path = dir.join(format!("{name}.md"));
        std::fs::write(
            &path,
            format!("---\ndescription: {description:?}\n---\n\nUse this agent.\n"),
        )
        .expect("agent definition");
        path
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

    fn append_accounted_assistant(
        state: &WebState,
        session_id: &str,
        context_tokens: u64,
        cache_read_tokens: u64,
    ) {
        state
            .inner
            .state
            .store()
            .append_message_with_metrics(
                session_id,
                &RuntimeMessage::Assistant {
                    content: vec![psychevo_runtime::AssistantBlock::Text {
                        text: "done".to_string(),
                    }],
                    timestamp_ms: 1,
                    finish_reason: Some("stop".to_string()),
                    outcome: psychevo_ai::Outcome::Normal,
                    model: Some("fake-model".to_string()),
                    provider: Some("fake-provider".to_string()),
                },
                Some(json!({
                    "input_tokens": context_tokens,
                    "total_tokens": context_tokens,
                    "cached_tokens": cache_read_tokens,
                })),
                None,
            )
            .expect("assistant");
    }

    async fn response_text(response: Response<Body>) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8(bytes.to_vec()).expect("utf8")
    }

    async fn occupied_port_with_free_successor() -> TcpListener {
        for _ in 0..100 {
            let occupied = TcpListener::bind("127.0.0.1:0").await.expect("occupy port");
            let port = occupied.local_addr().expect("occupied addr").port();
            let Some(next_port) = port.checked_add(1) else {
                continue;
            };
            if let Ok(probe) = TcpListener::bind(("127.0.0.1", next_port)).await {
                drop(probe);
                return occupied;
            }
        }
        panic!("could not find adjacent free loopback ports");
    }

    #[tokio::test]
    async fn bind_gateway_web_server_falls_back_from_used_port() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workdir = temp.path().join("work");
        let static_dir = temp.path().join("static");
        std::fs::create_dir_all(&workdir).expect("workdir");
        std::fs::create_dir_all(&static_dir).expect("static dir");
        let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = Gateway::new(state);
        let occupied = occupied_port_with_free_successor().await;
        let occupied_addr = occupied.local_addr().expect("occupied addr");
        let mut config = GatewayWebServerConfig::new(
            gateway,
            temp.path().join("home"),
            workdir,
            None,
            BTreeMap::new(),
            static_dir,
        );
        config.bind_addr = occupied_addr;
        config.bind_port_fallbacks = 1;

        let bound = bind_gateway_web_server(config).await.expect("bind");

        assert_eq!(bound.local_addr().ip(), occupied_addr.ip());
        assert_eq!(bound.local_addr().port(), occupied_addr.port() + 1);
    }

    #[tokio::test]
    async fn initialize_reports_current_profile() {
        let mut env = BTreeMap::new();
        env.insert("PSYCHEVO_PROFILE".to_string(), "coder".to_string());
        let (temp, state) = web_state_with_env(env);
        let home = temp.path().join("home").display().to_string();
        let (out_tx, _out_rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            out_tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "initialize".to_string(),
                params: None,
            },
        )
        .await
        .expect("initialize");

        assert_eq!(value["profile"]["name"], "coder");
        assert_eq!(value["profile"]["home"], home);
        assert_eq!(value["profile"]["default"], false);
    }

    #[tokio::test]
    async fn observability_read_returns_active_session_usage() {
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
        append_accounted_assistant(&state, &session_id, 200, 50);
        bind_source_to_thread(&state, &scope, &session_id).expect("bind");
        let (tx, _rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "observability/read".to_string(),
                params: Some(json!({ "scope": scope.to_wire_scope() })),
            },
        )
        .await
        .expect("observability/read");

        assert_eq!(value["usage"]["available"], true);
        assert_eq!(value["usage"]["sessionId"], session_id);
        assert_eq!(value["usage"]["contextInputTokens"], 200);
        assert_eq!(value["usage"]["cacheReadTokens"], 50);
        assert_eq!(value["usage"]["estimatedCostNanodollars"], 0);
        assert_eq!(value["usage"]["cacheReadPercent"], 25.0);
        let categories = value["context"]["categories"]
            .as_array()
            .expect("context categories");
        assert!(!categories.is_empty());
        assert!(
            categories
                .iter()
                .all(|category| category.get("details").is_some())
        );
        assert!(
            categories
                .iter()
                .all(|category| category.get("id").and_then(Value::as_str) != Some("free_space"))
        );
        let serialized_categories = serde_json::to_string(categories).expect("categories json");
        assert!(!serialized_categories.contains("done"));
        assert!(!serialized_categories.contains("content"));
    }

    #[tokio::test]
    async fn observability_read_projects_acp_peer_usage_update() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "peer_agent",
                "opencode",
                "acp:opencode",
                Some(json!({
                    "peer_agent": {
                        "agentName": "opencode",
                        "backendId": "opencode",
                        "backendKind": "acp",
                        "nativeSessionId": "native-1",
                        "usageUpdate": {
                            "sessionUpdate": "usage_update",
                            "used": 1234,
                            "size": 8000,
                            "cost": {"amount": 0.0025, "currency": "USD"}
                        }
                    }
                })),
            )
            .expect("session");
        bind_source_to_thread(&state, &scope, &session_id).expect("bind");
        let (tx, _rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "observability/read".to_string(),
                params: Some(json!({ "scope": scope.to_wire_scope() })),
            },
        )
        .await
        .expect("observability/read");

        assert_eq!(value["context"]["usedTokens"], 1234);
        assert_eq!(value["context"]["contextLimit"], 8000);
        assert_eq!(value["context"]["status"], "reported by ACP peer");
        assert_eq!(value["context"]["categories"], json!([]));
        assert_eq!(value["usage"]["reportedTotalTokens"], 1234);
        assert_eq!(value["usage"]["contextInputTokens"], 1234);
        assert_eq!(value["usage"]["estimatedCostNanodollars"], 2_500_000);
    }

    #[tokio::test]
    async fn observability_read_returns_explicit_thread_usage_without_active_binding() {
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
        append_accounted_assistant(&state, &session_id, 90, 9);
        let (tx, _rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "observability/read".to_string(),
                params: Some(json!({ "scope": scope.to_wire_scope(), "threadId": session_id })),
            },
        )
        .await
        .expect("observability/read");

        assert_eq!(value["usage"]["available"], true);
        assert_eq!(value["usage"]["contextInputTokens"], 90);
        assert_eq!(value["usage"]["cacheReadPercent"], 10.0);
    }

    #[tokio::test]
    async fn observability_read_clears_usage_when_no_active_session() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let (tx, _rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "observability/read".to_string(),
                params: Some(json!({ "scope": scope.to_wire_scope() })),
            },
        )
        .await
        .expect("observability/read");

        assert_eq!(value["usage"]["available"], false);
        assert_eq!(value["usage"]["reportedTotalTokens"], 0);
        assert_eq!(value["context"]["available"], false);
    }

    #[tokio::test]
    async fn browser_observability_read_authorizes_cross_workdir_thread() {
        let (temp, state) = web_state();
        let other_workdir = temp.path().join("other-work");
        std::fs::create_dir_all(&other_workdir).expect("other workdir");
        let other_workdir = canonicalize_workdir(&other_workdir).expect("other canonical");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &other_workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        append_accounted_assistant(&state, &session_id, 300, 150);
        let browser_session_id = "browser-session".to_string();
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .insert(
                browser_session_id.clone(),
                BrowserSession {
                    workdir: state.inner.workdir.clone(),
                    source: state.inner.source.clone(),
                },
            );
        let auth = AuthContext::Browser {
            session_id: browser_session_id,
        };
        let current_scope = default_resolved_scope(&state, &auth).expect("scope");
        let (tx, _rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            auth,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "observability/read".to_string(),
                params: Some(json!({
                    "scope": current_scope.to_wire_scope(),
                    "threadId": session_id
                })),
            },
        )
        .await
        .expect("observability/read");

        assert_eq!(value["usage"]["available"], true);
        assert_eq!(value["usage"]["sessionId"], session_id);
        assert_eq!(value["usage"]["contextInputTokens"], 300);
        assert_eq!(value["usage"]["cacheReadPercent"], 50.0);
    }

    #[test]
    fn start_empty_source_returns_null_thread_and_creates_no_session() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

        let snapshot = start_empty_source(&state, &scope).expect("snapshot");

        assert!(snapshot.get("thread").is_some_and(Value::is_null));
        assert_eq!(
            state
                .inner
                .state
                .store()
                .list_sessions_for_workdir_with_sources(&state.inner.workdir, &[])
                .expect("sessions")
                .len(),
            0
        );
        assert_eq!(
            state
                .inner
                .gateway
                .resolve_source_thread(&state.inner.source)
                .expect("source lookup")
                .as_deref(),
            None
        );
    }

    #[test]
    fn start_empty_source_clears_binding_without_archiving_previous_history() {
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

        let snapshot = start_empty_source(&state, &scope).expect("snapshot");

        assert!(snapshot.get("thread").is_some_and(Value::is_null));
        assert!(
            state
                .inner
                .gateway
                .resolve_source_thread(&state.inner.source)
                .expect("source lookup")
                .is_none()
        );
        let active_ids = state
            .inner
            .state
            .store()
            .list_sessions_for_workdir_with_sources(&state.inner.workdir, &[])
            .expect("active sessions")
            .into_iter()
            .map(|session| session.id)
            .collect::<Vec<_>>();

        assert_eq!(active_ids, vec![session_id]);
    }

    #[tokio::test]
    async fn thread_list_returns_global_top_level_sessions_without_source_partition() {
        let (temp, state) = web_state();
        let other_workdir = temp.path().join("other-work");
        std::fs::create_dir_all(&other_workdir).expect("other workdir");
        let other_workdir = canonicalize_workdir(&other_workdir).expect("other canonical");
        let store = state.inner.state.store();
        let top_level = store
            .create_session_with_metadata(
                &other_workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("top level");
        let internal = store
            .create_session_with_metadata(
                &state.inner.workdir,
                "tui-side",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("internal");
        let child = store
            .create_child_session_with_metadata(
                &top_level,
                &other_workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("child");
        let (out_tx, _out_rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            out_tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "thread/list".to_string(),
                params: None,
            },
        )
        .await
        .expect("thread list");
        let sessions = value["sessions"].as_array().expect("sessions");
        let ids = sessions
            .iter()
            .filter_map(|session| session["id"].as_str())
            .collect::<Vec<_>>();

        assert!(ids.contains(&top_level.as_str()));
        assert!(!ids.contains(&internal.as_str()));
        assert!(!ids.contains(&child.as_str()));
        let listed = sessions
            .iter()
            .find(|session| session["id"].as_str() == Some(top_level.as_str()))
            .expect("top level listed");
        assert_eq!(
            listed["project"]["workdir"],
            other_workdir.display().to_string()
        );
        assert_eq!(listed["project"]["label"], "other-work");
        assert_eq!(listed["visibleEntryCount"], 0);
        assert!(listed.get("source").is_none());
    }

    #[tokio::test]
    async fn browser_cross_project_resume_authorizes_followup_rpcs_on_same_connection() {
        let (temp, state) = web_state();
        let other_workdir = temp.path().join("other-work");
        std::fs::create_dir_all(&other_workdir).expect("other workdir");
        let other_workdir = canonicalize_workdir(&other_workdir).expect("other canonical");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &other_workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        let browser_session_id = "browser-session".to_string();
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .insert(
                browser_session_id.clone(),
                BrowserSession {
                    workdir: state.inner.workdir.clone(),
                    source: state.inner.source.clone(),
                },
            );
        let auth = AuthContext::Browser {
            session_id: browser_session_id,
        };
        let (tx, _rx) = mpsc::unbounded_channel();

        handle_rpc(
            state.clone(),
            auth.clone(),
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "thread/resume".to_string(),
                params: Some(json!({ "threadId": session_id })),
            },
        )
        .await
        .expect("thread/resume");
        let settings = handle_rpc(
            state,
            auth,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(2)),
                method: "settings/read".to_string(),
                params: Some(json!({ "workdir": other_workdir })),
            },
        )
        .await
        .expect("settings/read after cross-project resume");

        assert_eq!(
            settings["project"]["path"],
            other_workdir.display().to_string()
        );
    }

    #[tokio::test]
    async fn browser_project_group_start_adopts_known_session_project_scope() {
        let (temp, state) = web_state();
        let other_workdir = temp.path().join("other-work");
        std::fs::create_dir_all(&other_workdir).expect("other workdir");
        let other_workdir = canonicalize_workdir(&other_workdir).expect("other canonical");
        state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &other_workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("existing project session");
        let browser_session_id = "browser-session".to_string();
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .insert(
                browser_session_id.clone(),
                BrowserSession {
                    workdir: state.inner.workdir.clone(),
                    source: state.inner.source.clone(),
                },
            );
        let auth = AuthContext::Browser {
            session_id: browser_session_id,
        };
        let scope = ResolvedScope {
            workdir: other_workdir.clone(),
            source: workdir_source(&other_workdir),
        }
        .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let snapshot = handle_rpc(
            state.clone(),
            auth.clone(),
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "thread/start".to_string(),
                params: Some(json!({ "scope": scope })),
            },
        )
        .await
        .expect("thread/start in known project");
        assert!(snapshot.get("thread").is_some_and(Value::is_null));
        assert_eq!(
            snapshot["scope"]["workdir"],
            other_workdir.display().to_string()
        );

        let settings = handle_rpc(
            state,
            auth,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(2)),
                method: "settings/read".to_string(),
                params: Some(json!({ "workdir": other_workdir })),
            },
        )
        .await
        .expect("settings/read after project start");

        assert_eq!(
            settings["project"]["path"],
            other_workdir.display().to_string()
        );
    }

    #[tokio::test]
    async fn browser_workspace_create_uses_configured_root_and_authorizes_workdir() {
        let (temp, state) = web_state();
        std::fs::create_dir_all(&state.inner.home).expect("home");
        std::fs::write(
            state.inner.home.join("config.toml"),
            r#"
[workspaces]
root = "~/workspaces"
"#,
        )
        .expect("config");
        let browser_session_id = "browser-session".to_string();
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .insert(
                browser_session_id.clone(),
                BrowserSession {
                    workdir: state.inner.workdir.clone(),
                    source: state.inner.source.clone(),
                },
            );
        let auth = AuthContext::Browser {
            session_id: browser_session_id,
        };
        let (tx, _rx) = mpsc::unbounded_channel();

        let created = handle_rpc(
            state.clone(),
            auth.clone(),
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "workspace/create".to_string(),
                params: Some(json!({ "name": "Notes" })),
            },
        )
        .await
        .expect("workspace/create");
        let workdir = temp
            .path()
            .join("workspaces")
            .join("Notes")
            .canonicalize()
            .expect("created workdir");
        let workdir_string = workdir.display().to_string();

        assert_eq!(created["workdir"], workdir_string);
        assert_eq!(created["scope"]["workdir"], workdir_string);

        let settings = handle_rpc(
            state,
            auth,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(2)),
                method: "settings/read".to_string(),
                params: Some(json!({ "workdir": workdir_string.clone() })),
            },
        )
        .await
        .expect("settings/read after workspace/create");

        assert_eq!(settings["workdir"], workdir_string);
        assert_eq!(settings["project"]["path"], workdir_string);
    }

    #[test]
    fn workspace_dir_name_rejects_path_components() {
        assert_eq!(workspace_dir_name(" notes ").expect("trimmed"), "notes");
        let err = workspace_dir_name("../notes").expect_err("parent path rejected");
        assert!(
            err.to_string()
                .contains("workspace name must be a single directory name")
        );
    }

    #[test]
    fn reset_source_to_empty_archives_previous_binding_without_replacement() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let first_id = state
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
        bind_source_to_thread(&state, &scope, &first_id).expect("bind");

        let snapshot = reset_source_to_empty(&state, &scope).expect("reset");

        assert!(snapshot.get("thread").is_some_and(Value::is_null));
        assert!(
            state
                .inner
                .gateway
                .resolve_source_thread(&state.inner.source)
                .expect("source lookup")
                .is_none()
        );
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
        assert_eq!(
            state
                .inner
                .state
                .store()
                .list_sessions_for_workdir_with_sources(&state.inner.workdir, &[])
                .expect("active sessions")
                .len(),
            0
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
        assert_eq!(backends["backends"][0]["sourceTargets"], json!(["profile"]));

        let write = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "backend/write".to_string(),
                params: Some(json!({
                    "id": "opencode",
                    "target": "project",
                    "enabled": true,
                    "label": "OpenCode",
                    "description": "OpenCode ACP coding agent.",
                    "command": "opencode",
                    "args": ["acp"],
                    "entrypoints": ["peer", "subagent"],
                    "clientCapabilities": ["fs.read", "fs.write", "terminal"]
                })),
            },
        )
        .await
        .expect("backend/write");
        assert_eq!(write["backend"]["id"], "opencode");
        assert_eq!(write["backend"]["sourceTargets"], json!(["project"]));

        let backends = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("3")),
                method: "backend/list".to_string(),
                params: None,
            },
        )
        .await
        .expect("backend/list after write");
        let opencode_backend = backends["backends"]
            .as_array()
            .expect("backends")
            .iter()
            .find(|backend| backend["id"] == "opencode")
            .expect("opencode backend");
        assert_eq!(opencode_backend["sourceTargets"], json!(["project"]));
        assert_eq!(opencode_backend["args"], json!(["acp"]));

        let minimal = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("4")),
                method: "backend/write".to_string(),
                params: Some(json!({
                    "id": "minimal-acp",
                    "target": "profile",
                    "enabled": true,
                    "command": "minimal-agent",
                    "args": ["acp"],
                    "entrypoints": ["peer", "subagent"],
                    "clientCapabilities": ["fs.read", "fs.write", "terminal"]
                })),
            },
        )
        .await
        .expect("backend/write minimal");
        assert_eq!(minimal["backend"]["label"], "minimal-acp");
        assert_eq!(minimal["backend"]["description"], Value::Null);
        assert_eq!(minimal["backend"]["diagnostics"], json!([]));

        let agents = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("5")),
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
        assert!(agents.get("shadowedAgents").is_some());
        let opencode = agents["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .find(|agent| agent["name"] == "opencode")
            .expect("opencode agent");
        assert_eq!(opencode["backend"]["ref"], "opencode");
        let minimal_agent = agents["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .find(|agent| agent["name"] == "minimal-acp")
            .expect("minimal agent");
        assert_eq!(minimal_agent["description"], "minimal-acp");
        assert_eq!(minimal_agent["backend"]["ref"], "minimal-acp");

        let status = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("6")),
                method: "agent/status".to_string(),
                params: None,
            },
        )
        .await
        .expect("agent/status");
        assert!(status.get("control").is_some());
        assert!(status.get("agents").is_some());

        let delete = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("7")),
                method: "backend/delete".to_string(),
                params: Some(json!({
                    "id": "opencode",
                    "target": "project"
                })),
            },
        )
        .await
        .expect("backend/delete");
        assert_eq!(delete["deleted"], true);
    }

    #[tokio::test]
    async fn backend_profile_write_uses_explicit_config_when_set() {
        let temp = tempfile::tempdir().expect("tempdir");
        let explicit_config = temp.path().join("explicit").join("config.toml");
        let (_state_temp, state) = web_state_with_env(BTreeMap::from([(
            "PSYCHEVO_CONFIG".to_string(),
            explicit_config.to_string_lossy().to_string(),
        )]));
        let (tx, _rx) = mpsc::unbounded_channel();

        let write = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "backend/write".to_string(),
                params: Some(json!({
                    "id": "minimal-acp",
                    "target": "profile",
                    "command": "minimal-agent",
                    "entrypoints": ["peer", "subagent"],
                    "clientCapabilities": ["fs.read", "fs.write", "terminal"]
                })),
            },
        )
        .await
        .expect("backend/write");
        assert_eq!(write["backend"]["id"], "minimal-acp");
        assert_eq!(write["backend"]["label"], "minimal-acp");
        assert_eq!(write["backend"]["description"], Value::Null);

        let backends = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "backend/list".to_string(),
                params: None,
            },
        )
        .await
        .expect("backend/list");
        assert!(backends["backends"].as_array().is_some_and(|backends| {
            backends
                .iter()
                .any(|backend| backend["id"] == "minimal-acp")
        }));
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
    async fn settings_read_returns_workbench_project_and_controls() {
        let (_temp, state) = web_state();
        let (tx, _rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "settings/read".to_string(),
                params: None,
            },
        )
        .await
        .expect("settings/read");

        let workdir = state.inner.workdir.display().to_string();
        assert_eq!(result["project"]["path"].as_str(), Some(workdir.as_str()));
        assert!(
            result["project"]["displayPath"]
                .as_str()
                .is_some_and(|path| path.ends_with("/work") || path == "work"),
            "{result:#}"
        );
        assert_eq!(result["controls"]["permissionMode"], "default");
        assert_eq!(result["controls"]["mode"], "default");
        assert_eq!(result["controls"]["agent"], Value::Null);
        assert!(
            result["controls"]["variantOptions"]
                .as_array()
                .expect("variant options")
                .iter()
                .any(|value| value.as_str() == Some("medium"))
        );
    }

    #[tokio::test]
    async fn settings_read_exposes_session_agent() {
        let (_temp, state) = web_state();
        let session = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "model",
                "provider",
                Some(json!({
                    "main_agent": main_agent_metadata(
                        "translate",
                        "translate",
                        psychevo_runtime::AgentSource::Project,
                        None,
                    )
                })),
            )
            .expect("session");
        let (tx, _rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "settings/read".to_string(),
                params: Some(json!({ "threadId": session })),
            },
        )
        .await
        .expect("settings/read");

        assert_eq!(result["controls"]["agent"].as_str(), Some("translate"));
    }

    #[tokio::test]
    async fn settings_update_persists_session_agent_and_default() {
        let (_temp, state) = web_state();
        write_agent_definition(
            &state.inner.workdir.join(".psychevo/agents"),
            "translate",
            "Translate user messages",
        );
        let session = state
            .inner
            .state
            .store()
            .create_session_with_metadata(&state.inner.workdir, "web", "model", "provider", None)
            .expect("session");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "settings/update".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "threadId": session,
                    "agent": "translate"
                })),
            },
        )
        .await
        .expect("settings/update");

        assert_eq!(result["controls"]["agent"].as_str(), Some("translate"));
        let metadata = state
            .inner
            .state
            .store()
            .session_metadata(&session)
            .expect("metadata")
            .expect("metadata value");
        assert_eq!(metadata["main_agent"]["mode"], "agent");
        assert_eq!(metadata["main_agent"]["name"], "translate");
        assert!(!state.inner.workdir.join(".psychevo/config.toml").exists());

        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();
        let result = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "settings/update".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "threadId": session,
                    "agent": null
                })),
            },
        )
        .await
        .expect("settings/update");

        assert_eq!(result["controls"]["agent"], Value::Null);
        let metadata = state
            .inner
            .state
            .store()
            .session_metadata(&session)
            .expect("metadata")
            .expect("metadata value");
        assert_eq!(metadata["main_agent"]["mode"], "default");
    }

    #[tokio::test]
    async fn settings_update_rejects_unknown_or_shadowed_session_agent() {
        let (_temp, state) = web_state();
        let project_agents = state.inner.workdir.join(".psychevo/agents");
        let home_agents = state.inner.home.join("agents");
        write_agent_definition(&project_agents, "review", "Project review");
        let shadowed = write_agent_definition(&home_agents, "review", "Global review");
        let session = state
            .inner
            .state
            .store()
            .create_session_with_metadata(&state.inner.workdir, "web", "model", "provider", None)
            .expect("session");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();

        let (tx, _rx) = mpsc::unbounded_channel();
        let active = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "settings/update".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "threadId": session,
                    "agent": "review"
                })),
            },
        )
        .await
        .expect("active review is valid");
        assert_eq!(active["controls"]["agent"].as_str(), Some("review"));

        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();
        let err = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "settings/update".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "threadId": session,
                    "agent": shadowed.display().to_string()
                })),
            },
        )
        .await
        .expect_err("shadowed path");
        assert!(
            err.to_string().contains("shadowed agent definitions"),
            "{err:#}"
        );

        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();
        let err = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("3")),
                method: "settings/update".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "threadId": session,
                    "agent": "missing"
                })),
            },
        )
        .await
        .expect_err("unknown agent");
        assert!(
            err.to_string().contains("unknown agent: missing"),
            "{err:#}"
        );
    }

    #[tokio::test]
    async fn workspace_file_rpcs_are_scoped_to_current_project_tree() {
        let (_temp, state) = web_state();
        let src = state.inner.workdir.join("src");
        std::fs::create_dir_all(&src).expect("src");
        std::fs::write(src.join("main.rs"), "fn main() {}\n").expect("main");
        for skipped in [".git", ".local", "target", "node_modules"] {
            let dir = state.inner.workdir.join(skipped);
            std::fs::create_dir_all(&dir).expect("skipped dir");
            std::fs::write(dir.join("hidden.txt"), skipped).expect("hidden");
        }
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "workspace/files".to_string(),
                params: Some(json!({ "scope": scope.clone() })),
            },
        )
        .await
        .expect("workspace/files");

        let paths = result["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .filter_map(|entry| entry["path"].as_str())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"src"));
        assert!(paths.contains(&"src/main.rs"));
        assert!(
            paths.iter().all(|path| !path.starts_with(".git")
                && !path.starts_with(".local")
                && !path.starts_with("target")
                && !path.starts_with("node_modules")),
            "{paths:?}"
        );

        let read = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "workspace/file/read".to_string(),
                params: Some(json!({
                    "scope": scope.clone(),
                    "path": "src/main.rs"
                })),
            },
        )
        .await
        .expect("workspace/file/read");
        assert_eq!(read["path"].as_str(), Some("src/main.rs"));
        assert_eq!(read["content"].as_str(), Some("fn main() {}\n"));

        let err = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("3")),
                method: "workspace/file/read".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "path": "/etc/passwd"
                })),
            },
        )
        .await
        .expect_err("absolute path should be rejected");
        assert_eq!(err.to_string(), "workspace path must be relative");
    }

    #[tokio::test]
    async fn workspace_diff_rpc_returns_selected_file_diff_preview() {
        let (_temp, state) = web_state();
        git(&state.inner.workdir, ["init"]);
        git(
            &state.inner.workdir,
            ["config", "user.email", "test@example.com"],
        );
        git(&state.inner.workdir, ["config", "user.name", "Test User"]);
        let src = state.inner.workdir.join("src");
        std::fs::create_dir_all(&src).expect("src");
        std::fs::write(src.join("main.rs"), "fn main() {}\n").expect("main");
        git(&state.inner.workdir, ["add", "."]);
        git(&state.inner.workdir, ["commit", "-m", "initial"]);
        std::fs::write(src.join("main.rs"), "fn main() {}\nfn changed() {}\n").expect("main");
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
                method: "workspace/diff".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "path": "src/main.rs"
                })),
            },
        )
        .await
        .expect("workspace/diff");

        assert_eq!(result["selectedPath"].as_str(), Some("src/main.rs"));
        assert_eq!(result["files"].as_array().expect("files").len(), 1);
        assert_eq!(result["files"][0]["path"].as_str(), Some("src/main.rs"));
        assert_eq!(result["files"][0]["status"].as_str(), Some("modified"));
        assert!(
            result["unifiedDiff"].as_str().is_some_and(|diff| diff
                .contains("diff --git a/src/main.rs b/src/main.rs")
                && diff.contains("+fn changed() {}")),
            "{result:#}"
        );
    }

    #[tokio::test]
    async fn workspace_file_write_rejects_revision_conflicts_and_allows_force() {
        let (_temp, state) = web_state();
        let src = state.inner.workdir.join("src");
        std::fs::create_dir_all(&src).expect("src");
        std::fs::write(src.join("main.rs"), "fn main() {}\n").expect("main");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let read = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "workspace/file/read".to_string(),
                params: Some(json!({
                    "scope": scope.clone(),
                    "path": "src/main.rs"
                })),
            },
        )
        .await
        .expect("workspace/file/read");
        assert_eq!(read["editable"], true, "{read:#}");
        let revision = read["revision"].as_str().expect("revision").to_string();

        std::fs::write(src.join("main.rs"), "fn external() {}\n").expect("external");
        let err = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "workspace/file/write".to_string(),
                params: Some(json!({
                    "scope": scope.clone(),
                    "path": "src/main.rs",
                    "content": "fn gui() {}\n",
                    "expectedRevision": revision,
                    "force": false
                })),
            },
        )
        .await
        .expect_err("revision conflict");
        assert_eq!(err.to_string(), "workspace file changed on disk");

        let written = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("3")),
                method: "workspace/file/write".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "path": "src/main.rs",
                    "content": "fn gui() {}\n",
                    "expectedRevision": "stale",
                    "force": true
                })),
            },
        )
        .await
        .expect("force write");
        assert_eq!(written["path"].as_str(), Some("src/main.rs"));
        assert_eq!(
            std::fs::read_to_string(src.join("main.rs")).expect("main"),
            "fn gui() {}\n"
        );
    }

    #[tokio::test]
    async fn workspace_change_reject_restores_pre_turn_dirty_content() {
        let (_temp, state) = web_state();
        git(&state.inner.workdir, ["init"]);
        git(
            &state.inner.workdir,
            ["config", "user.email", "test@example.com"],
        );
        git(&state.inner.workdir, ["config", "user.name", "Test User"]);
        let path = state.inner.workdir.join("notes.txt");
        std::fs::write(&path, "base\n").expect("base");
        git(&state.inner.workdir, ["add", "."]);
        git(&state.inner.workdir, ["commit", "-m", "initial"]);
        std::fs::write(&path, "user dirty\n").expect("dirty");

        state
            .inner
            .review
            .begin_turn("turn-1", Some("thread-1".to_string()), &state.inner.workdir);
        std::fs::write(&path, "agent changed\n").expect("agent");
        state.inner.review.complete_turn("turn-1");

        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();
        let rejected = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "workspace/change/reject".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "turnId": "turn-1",
                    "path": "notes.txt"
                })),
            },
        )
        .await
        .expect("reject");

        assert_eq!(rejected["accepted"], true, "{rejected:#}");
        assert_eq!(
            std::fs::read_to_string(&path).expect("notes"),
            "user dirty\n"
        );
        assert_eq!(
            rejected["changes"]["groups"][0]["files"][0]["reviewStatus"].as_str(),
            Some("rejected"),
            "{rejected:#}"
        );
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
            ("/usage", "status"),
            ("/context", "status"),
            ("/help", "commands"),
            ("/commands", "commands"),
            ("/sessions", "history"),
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
            assert_eq!(result["known"], true, "{command}: {result:?}");
            assert_eq!(result["action"]["type"], "showPanel");
            assert_eq!(result["action"]["panel"], panel);
            assert!(result["presentationKind"].as_str().is_some());
            assert!(result["feedbackAnchor"].as_str().is_some());
        }

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
                    "command": "/agents",
                    "threadId": null
                })),
            },
        )
        .await
        .expect("command/execute");

        assert_eq!(result["accepted"], false, "{result:?}");
        assert_eq!(result["known"], true, "{result:?}");
        assert!(result["action"].is_null(), "{result:?}");
        assert_eq!(
            result["message"],
            "/agents is managed by the Workbench agent selector and Settings Agents."
        );
    }

    #[tokio::test]
    async fn command_execute_queue_preserves_original_slash_display_text() {
        let (_temp, state) = web_state();
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
                method: "command/execute".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "command": "/queue hello",
                    "threadId": null
                })),
            },
        )
        .await
        .expect("command/execute");

        assert_eq!(result["accepted"], true);
        assert_eq!(result["known"], true);
        assert_eq!(result["presentationKind"], "control");
        assert_eq!(result["feedbackAnchor"], "composer");
        assert_eq!(result["action"]["type"], "queuePrompt");
        assert_eq!(result["action"]["text"], "hello");
        assert_eq!(result["action"]["displayText"], "/queue hello");
    }

    #[tokio::test]
    async fn command_execute_undo_redo_restores_session_snapshot() {
        let (_temp, state) = web_state();
        git(&state.inner.workdir, ["init"]);
        let file = state.inner.workdir.join("tracked.txt");
        std::fs::write(&file, "base\n").expect("base");
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
        let snapshot_root = state.inner.home.join("snapshots");
        let before_first = track_snapshot(&snapshot_root, &session_id, &state.inner.workdir);
        state
            .inner
            .state
            .store()
            .append_message_with_undo_snapshot(
                &session_id,
                &runtime_user_message("first prompt", 1),
                Some(before_first),
            )
            .expect("first user");
        std::fs::write(&file, "after first\n").expect("after first");
        state
            .inner
            .state
            .store()
            .append_message(&session_id, &runtime_assistant_message("first answer", 2))
            .expect("first assistant");
        let before_second = track_snapshot(&snapshot_root, &session_id, &state.inner.workdir);
        state
            .inner
            .state
            .store()
            .append_message_with_undo_snapshot(
                &session_id,
                &runtime_user_message("second prompt", 3),
                Some(before_second),
            )
            .expect("second user");
        std::fs::write(&file, "after second\n").expect("after second");
        state
            .inner
            .state
            .store()
            .append_message(&session_id, &runtime_assistant_message("second answer", 4))
            .expect("second assistant");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let undo = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "command/execute".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "command": "/undo",
                    "threadId": session_id
                })),
            },
        )
        .await
        .expect("command/execute undo");

        assert_eq!(undo["accepted"], true, "{undo:#}");
        assert_eq!(undo["known"], true, "{undo:#}");
        assert_eq!(undo["action"]["type"], "sessionUndo");
        assert_eq!(undo["action"]["threadId"], session_id);
        assert_eq!(undo["action"]["prompt"], "second prompt");
        assert_eq!(undo["action"]["revertedMessages"], 2);
        assert_eq!(
            std::fs::read_to_string(&file).expect("file"),
            "after first\n"
        );
        assert_eq!(
            state
                .inner
                .state
                .store()
                .load_tui_message_summaries(&session_id)
                .expect("visible")
                .len(),
            2
        );

        let redo = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "command/execute".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "command": "/redo",
                    "threadId": session_id
                })),
            },
        )
        .await
        .expect("command/execute redo");

        assert_eq!(redo["accepted"], true, "{redo:#}");
        assert_eq!(redo["known"], true, "{redo:#}");
        assert_eq!(redo["action"]["type"], "sessionRedo");
        assert_eq!(redo["action"]["threadId"], session_id);
        assert_eq!(redo["action"]["restoredMessages"], 2);
        assert_eq!(redo["action"]["complete"], true);
        assert_eq!(
            std::fs::read_to_string(&file).expect("file"),
            "after second\n"
        );
        assert_eq!(
            state
                .inner
                .state
                .store()
                .load_tui_message_summaries(&session_id)
                .expect("visible")
                .len(),
            4
        );
    }

    #[tokio::test]
    async fn command_execute_undo_redo_bounded_without_matching_session() {
        let (temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let no_thread = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "command/execute".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "command": "/undo",
                    "threadId": null
                })),
            },
        )
        .await
        .expect("command/execute no thread");
        assert_eq!(no_thread["accepted"], false, "{no_thread:#}");
        assert_eq!(no_thread["known"], true, "{no_thread:#}");
        assert!(no_thread["action"].is_null(), "{no_thread:#}");
        assert_eq!(no_thread["message"], "no current session to undo");

        let other_workdir = temp.path().join("other");
        std::fs::create_dir_all(&other_workdir).expect("other workdir");
        let other_session = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &other_workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("other session");
        let cross_workdir = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "command/execute".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "command": "/redo",
                    "threadId": other_session
                })),
            },
        )
        .await
        .expect("command/execute cross workdir");
        assert_eq!(cross_workdir["accepted"], false, "{cross_workdir:#}");
        assert_eq!(cross_workdir["known"], true, "{cross_workdir:#}");
        assert!(cross_workdir["action"].is_null(), "{cross_workdir:#}");
        assert!(
            cross_workdir["message"]
                .as_str()
                .is_some_and(|message| message.contains("does not belong")),
            "{cross_workdir:#}"
        );
    }

    #[tokio::test]
    async fn terminal_start_rejects_cwd_outside_workspace() {
        let (temp, state) = web_state();
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).expect("outside dir");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let err = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "terminal/start".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "cwd": outside,
                    "cols": 80,
                    "rows": 24
                })),
            },
        )
        .await
        .expect_err("outside cwd should be rejected");

        assert!(err.to_string().contains("outside the workspace"), "{err:?}");
    }

    #[tokio::test]
    async fn terminal_rpc_streams_output_and_exit_notifications() {
        let shell = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" };
        let (_temp, state) =
            web_state_with_env(BTreeMap::from([("SHELL".to_string(), shell.to_string())]));
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, mut rx) = mpsc::unbounded_channel();

        let started = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "terminal/start".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "cwd": null,
                    "cols": 80,
                    "rows": 24
                })),
            },
        )
        .await
        .expect("terminal/start");
        let terminal_id = started["terminalId"]
            .as_str()
            .expect("terminal id")
            .to_string();

        let resize = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "terminal/resize".to_string(),
                params: Some(json!({
                    "terminalId": terminal_id.clone(),
                    "cols": 100,
                    "rows": 30
                })),
            },
        )
        .await
        .expect("terminal/resize");
        assert_eq!(resize["accepted"], true);

        let command = if cfg!(windows) {
            "echo pevo-terminal-ok\r\nexit\r\n"
        } else {
            "printf pevo-terminal-ok\\n\nexit\n"
        };
        let write = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("3")),
                method: "terminal/write".to_string(),
                params: Some(json!({
                    "terminalId": terminal_id.clone(),
                    "dataBase64": BASE64_STANDARD.encode(command.as_bytes())
                })),
            },
        )
        .await
        .expect("terminal/write");
        assert_eq!(write["accepted"], true);

        let mut output = String::new();
        let mut saw_exit = false;
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(message) = rx.recv().await {
                let notification: Value = serde_json::from_str(&message).expect("notification");
                match notification["method"].as_str() {
                    Some("terminal/output") => {
                        let encoded = notification["params"]["dataBase64"]
                            .as_str()
                            .expect("dataBase64");
                        let bytes = BASE64_STANDARD.decode(encoded).expect("base64");
                        output.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Some("terminal/exited") => {
                        saw_exit = true;
                    }
                    _ => {}
                }
                if output.contains("pevo-terminal-ok") && saw_exit {
                    break;
                }
            }
        })
        .await
        .expect("terminal notifications");

        assert!(output.contains("pevo-terminal-ok"), "{output:?}");
        assert!(saw_exit);
    }

    #[tokio::test]
    async fn command_list_and_completion_use_web_desktop_presentation_catalog() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let list = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "command/list".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "threadId": null
                })),
            },
        )
        .await
        .expect("command/list");
        let commands = list["commands"].as_array().expect("commands");
        let names = commands
            .iter()
            .filter_map(|command| command["name"].as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"diff"), "{names:?}");
        assert!(names.contains(&"sessions"), "{names:?}");
        assert!(names.contains(&"undo"), "{names:?}");
        assert!(names.contains(&"redo"), "{names:?}");
        assert!(!names.contains(&"agents"), "{names:?}");
        assert!(!names.contains(&"model"), "{names:?}");
        assert!(!names.contains(&"tools"), "{names:?}");

        let diff = commands
            .iter()
            .find(|command| command["name"] == "diff")
            .expect("diff command");
        assert_eq!(diff["presentationKind"], "inspect");
        assert_eq!(diff["destination"], "preview");
        assert_eq!(diff["feedbackAnchor"], "trigger");

        let completion = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "completion/list".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "text": "/",
                    "cursor": 1,
                    "threadId": null
                })),
            },
        )
        .await
        .expect("completion/list");
        let items = completion["items"].as_array().expect("items");
        assert!(items.iter().any(|item| item["label"] == "/diff"));
        assert!(items.iter().any(|item| item["label"] == "/undo"));
        assert!(items.iter().any(|item| item["label"] == "/redo"));
        assert!(!items.iter().any(|item| item["label"] == "/agents"));
        assert!(!items.iter().any(|item| item["label"] == "/model"));
        let diff_completion = items
            .iter()
            .find(|item| item["label"] == "/diff")
            .expect("diff completion");
        assert_eq!(
            diff_completion["detail"].as_str(),
            Some("Preview - show workspace diff")
        );
    }

    #[tokio::test]
    async fn command_list_and_execute_include_dynamic_skill_commands() {
        let (_temp, state) = web_state();
        write_project_skill(&state, "x-daily", "Fetch X daily posts.");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let list = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "command/list".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "threadId": null
                })),
            },
        )
        .await
        .expect("command/list");
        let dynamic = list["commands"]
            .as_array()
            .expect("commands")
            .iter()
            .find(|command| command["name"] == "x-daily")
            .expect("dynamic command");
        assert_eq!(dynamic["source"], "dynamic");
        assert_eq!(dynamic["slash"], "/x-daily");
        assert_eq!(dynamic["presentationKind"], "extension");
        assert_eq!(dynamic["destination"], "composer");

        let completion = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "completion/list".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "text": "/x",
                    "cursor": 2,
                    "threadId": null
                })),
            },
        )
        .await
        .expect("completion/list");
        let dynamic_completion = completion["items"]
            .as_array()
            .expect("items")
            .iter()
            .find(|item| item["label"] == "/x-daily")
            .expect("dynamic completion");
        assert_eq!(
            dynamic_completion["detail"].as_str(),
            Some("Prompt - Fetch X daily posts.")
        );

        let result = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("3")),
                method: "command/execute".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "command": "/x-daily latest",
                    "threadId": null
                })),
            },
        )
        .await
        .expect("command/execute");
        assert_eq!(result["accepted"], true);
        assert_eq!(result["known"], true);
        assert_eq!(result["presentationKind"], "extension");
        assert_eq!(result["feedbackAnchor"], "composer");
        assert_eq!(result["action"]["type"], "submitPrompt");
        assert_eq!(result["action"]["text"], "$x-daily latest");
        assert_eq!(result["action"]["displayText"], "/x-daily latest");
    }

    #[tokio::test]
    async fn command_execute_known_unsupported_returns_guidance_without_passthrough() {
        let (_temp, state) = web_state();
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
                method: "command/execute".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "command": "/model",
                    "threadId": null
                })),
            },
        )
        .await
        .expect("command/execute");

        assert_eq!(result["accepted"], false);
        assert_eq!(result["known"], true);
        assert!(result["action"].is_null(), "{result:#}");
        assert_eq!(result["presentationKind"], "control");
        assert_eq!(result["feedbackAnchor"], "composer");
        assert_eq!(result["alternateAction"]["type"], "openComposerControl");
        assert_eq!(result["alternateAction"]["target"], "model");
        assert!(
            result["message"]
                .as_str()
                .is_some_and(|message| message.contains("Workbench model controls")),
            "{result:#}"
        );
    }

    #[tokio::test]
    async fn command_execute_unknown_slash_returns_prompt_passthrough() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        for command in ["/made-up hello", "/tmp/output.txt"] {
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

            assert_eq!(result["accepted"], false);
            assert_eq!(result["known"], false);
            assert_eq!(result["action"]["type"], "passThroughPrompt");
            assert_eq!(result["action"]["text"], command);
            assert!(result["message"].is_null());
            assert!(result["presentationKind"].is_null());
        }
    }

    #[tokio::test]
    async fn shell_start_empty_command_returns_bounded_help_without_spawning() {
        let (_temp, state) = web_state();
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
                method: "shell/start".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "command": "  ",
                    "threadId": null
                })),
            },
        )
        .await
        .expect("shell/start");

        assert_eq!(result["accepted"], false);
        assert_eq!(
            result["message"],
            "shell mode: type !<command> to run a local shell command"
        );
    }

    #[tokio::test]
    async fn turn_start_empty_input_rejects_before_creating_session() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let err = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "turn/start".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "input": [],
                    "threadId": null
                })),
            },
        )
        .await
        .expect_err("empty turn should reject");

        assert_eq!(err.to_string(), "turn/start requires input");
        assert_eq!(
            state
                .inner
                .state
                .store()
                .list_sessions_for_workdir_with_sources(&state.inner.workdir, &[])
                .expect("sessions")
                .len(),
            0
        );
        assert!(
            state
                .inner
                .gateway
                .resolve_source_thread(&state.inner.source)
                .expect("source lookup")
                .is_none()
        );
    }

    #[tokio::test]
    async fn shell_start_first_request_can_be_accepted_without_thread_id() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "shell/start".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "command": "printf shell-ok",
                    "threadId": null
                })),
            },
        )
        .await
        .expect("shell/start");

        assert_eq!(result["accepted"], true);
        assert!(result.get("threadId").is_some_and(Value::is_null));
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

    fn runtime_user_message(text: &str, timestamp_ms: i64) -> RuntimeMessage {
        RuntimeMessage::User {
            content: vec![UserContentBlock::text(text)],
            timestamp_ms,
        }
    }

    fn runtime_assistant_message(text: &str, timestamp_ms: i64) -> RuntimeMessage {
        RuntimeMessage::Assistant {
            content: vec![psychevo_runtime::AssistantBlock::Text {
                text: text.to_string(),
            }],
            timestamp_ms,
            finish_reason: Some("stop".to_string()),
            outcome: psychevo_runtime::Outcome::Normal,
            model: Some("fake-model".to_string()),
            provider: Some("fake-provider".to_string()),
        }
    }

    fn track_snapshot(root: &Path, session_id: &str, workdir: &Path) -> String {
        let git_dir = root.join("sessions").join(session_id);
        std::fs::create_dir_all(&git_dir).expect("snapshot git dir");
        if !git_dir.join("HEAD").exists() {
            let init = std::process::Command::new("git")
                .env("GIT_DIR", &git_dir)
                .env("GIT_WORK_TREE", workdir)
                .arg("init")
                .output()
                .expect("snapshot init");
            assert!(
                init.status.success(),
                "snapshot init failed: {}",
                String::from_utf8_lossy(&init.stderr)
            );
        }
        let add = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&git_dir)
            .arg("--work-tree")
            .arg(workdir)
            .args(["add", "--all", "--", "."])
            .output()
            .expect("snapshot add");
        assert!(
            add.status.success(),
            "snapshot add failed: {}",
            String::from_utf8_lossy(&add.stderr)
        );
        let tree = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&git_dir)
            .arg("--work-tree")
            .arg(workdir)
            .arg("write-tree")
            .output()
            .expect("snapshot write-tree");
        assert!(
            tree.status.success(),
            "snapshot write-tree failed: {}",
            String::from_utf8_lossy(&tree.stderr)
        );
        let hash = String::from_utf8_lossy(&tree.stdout).trim().to_string();
        assert!(!hash.is_empty(), "snapshot hash should not be empty");
        hash
    }

    fn git<I, S>(workdir: &Path, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(workdir)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
