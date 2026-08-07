use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Error as IoError, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use axum::Router;
use axum::http::HeaderMap;
use axum::http::header::COOKIE;
use axum::routing::{get, post};
use psychevo::Client as FrameworkClient;
use psychevo::PermissionMode;
use psychevo::application::GatewayDurability;
use psychevo::config::McpOAuthCredentialStore;
#[cfg(not(test))]
use psychevo::config::SystemMcpOAuthCredentialStore;
use psychevo::host_paths::normalized_native_path;
use psychevo_gateway_protocol as wire;
use serde_json::json;
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::composition::GatewayApplication;
use crate::gateway::Gateway;
use crate::gateway::activity::GatewayActivity;
use crate::gateway::activity::{ThreadCallerContext, ThreadSurface, ThreadTurnIntent};
use crate::gateway_now_ms;
use psychevo_gateway_protocol::events_transcript::{GatewayEvent, PendingActionView};
use psychevo_gateway_protocol::source::{GatewayInputPart, GatewaySource, GatewayThreadSelector};

use super::auth_input::{bearer_token, now_ms, session_cookie_value};
use super::browser_session_store::BrowserSessionStore;
use super::download_static::{download_session, gateway_fallback, read_media_artifact};
use super::event_delivery::{ConnectionSender, GatewayEventHub};
use super::extension_management::ExtensionAppLeaseStore;
use super::mcp_oauth_store::McpOAuthSessionStore;
use super::rpc_dispatch::{
    consume_launch, create_launch, managed_identity, managed_shutdown, readyz,
    spawn_gateway_live_event_tailer, ws_handler,
};
use super::rpc_json::{cwd_source, rpc_notification};
use super::session_import_application::reconcile_acknowledged_session_deletes;
use super::terminal::TerminalManager;
use super::voice::RealtimeSessionState;
use super::workspace::WorkspaceReviewState;
use super::workspace_external::WorkspaceExternalState;
use super::workspace_preview::{
    WorkspacePreviewLeaseStore, configured_workspace_preview_origins, workspace_preview_resource,
};
use super::{automations, channel_runtime, channels, codex_capability_broker, runtime_profiles};

const RUNTIME_GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(6);
const SERVER_CONNECTION_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
#[derive(Debug)]
pub struct GatewayWebServerConfig {
    runtime: GatewayApplication,
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub config_path: Option<PathBuf>,
    pub inherited_env: BTreeMap<String, String>,
    pub static_dir: Option<PathBuf>,
    pub bind_addr: SocketAddr,
    pub bind_port_fallbacks: u16,
    pub token: String,
    pub managed_state_path: Option<PathBuf>,
    pub managed_instance_id: Option<String>,
    pub workspace_preview_origins: BTreeSet<String>,
}

impl GatewayWebServerConfig {
    pub fn with_static(runtime: GatewayApplication, cwd: PathBuf, static_dir: PathBuf) -> Self {
        let home = runtime.home().clone();
        let config_path = runtime.config_path().cloned();
        let inherited_env = runtime.inherited_env().clone();
        let workspace_preview_origins = configured_workspace_preview_origins(&inherited_env);
        Self {
            runtime,
            home,
            cwd,
            config_path,
            inherited_env,
            static_dir: Some(static_dir),
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            bind_port_fallbacks: 0,
            token: Uuid::now_v7().to_string(),
            managed_state_path: None,
            managed_instance_id: None,
            workspace_preview_origins,
        }
    }

    pub fn headless(runtime: GatewayApplication, cwd: PathBuf, token: String) -> Self {
        let home = runtime.home().clone();
        let config_path = runtime.config_path().cloned();
        let inherited_env = runtime.inherited_env().clone();
        let workspace_preview_origins = configured_workspace_preview_origins(&inherited_env);
        Self {
            runtime,
            home,
            cwd,
            config_path,
            inherited_env,
            static_dir: None,
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            bind_port_fallbacks: 0,
            token,
            managed_state_path: None,
            managed_instance_id: None,
            workspace_preview_origins,
        }
    }
}

pub struct BoundGatewayWebServer {
    listener: TcpListener,
    app: Router,
    runtime: Arc<GatewayApplication>,
    local_addr: SocketAddr,
    token: String,
    managed_shutdown_rx: Option<tokio::sync::watch::Receiver<bool>>,
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

    pub async fn run(self) -> psychevo::Result<()> {
        self.run_with_shutdown_signal(std::future::pending()).await
    }

    pub async fn run_with_shutdown_signal<F>(self, shutdown_signal: F) -> psychevo::Result<()>
    where
        F: std::future::Future<Output = ()> + Send,
    {
        let Self {
            listener,
            app,
            runtime,
            managed_shutdown_rx,
            ..
        } = self;
        let (server_shutdown_tx, server_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server = std::future::IntoFuture::into_future(
            axum::serve(listener, app.into_make_service()).with_graceful_shutdown(async move {
                let _ = server_shutdown_rx.await;
            }),
        );
        tokio::pin!(server);
        let managed_shutdown_signal = async move {
            let Some(mut receiver) = managed_shutdown_rx else {
                std::future::pending::<()>().await;
                return;
            };
            if *receiver.borrow() {
                return;
            }
            while receiver.changed().await.is_ok() {
                if *receiver.borrow() {
                    return;
                }
            }
        };
        let shutdown_signal = async move {
            tokio::select! {
                _ = shutdown_signal => {}
                _ = managed_shutdown_signal => {}
            }
        };
        tokio::pin!(shutdown_signal);

        tokio::select! {
            result = &mut server => {
                let shutdown = runtime
                    .shutdown_with_deadline(RUNTIME_GRACEFUL_SHUTDOWN_TIMEOUT)
                    .await;
                result?;
                shutdown
            }
            _ = &mut shutdown_signal => {
                let _ = server_shutdown_tx.send(());
                let shutdown = runtime
                    .shutdown_with_deadline(RUNTIME_GRACEFUL_SHUTDOWN_TIMEOUT)
                    .await;
                let drain = tokio::time::timeout(SERVER_CONNECTION_DRAIN_TIMEOUT, &mut server).await;
                if let Ok(result) = drain {
                    result?;
                }
                shutdown
            }
        }
    }
}

pub async fn bind_gateway_web_server(
    config: GatewayWebServerConfig,
) -> psychevo::Result<BoundGatewayWebServer> {
    let listener = bind_tcp_listener(config.bind_addr, config.bind_port_fallbacks).await?;
    let local_addr = listener.local_addr()?;
    let token = config.token.clone();
    if let Some(path) = &config.managed_state_path {
        write_managed_state(path, local_addr, &config)?;
    }
    let managed = config.managed_instance_id.is_some();
    let (managed_shutdown_tx, managed_shutdown_rx) = tokio::sync::watch::channel(false);
    let state = WebState::new_with_managed_shutdown(config, managed.then_some(managed_shutdown_tx));
    let runtime = state.inner.runtime.clone();
    spawn_gateway_live_event_tailer(state.clone());
    let mut app = Router::new()
        .route("/readyz", get(readyz))
        .route("/health", get(readyz))
        .route("/_gateway/launch", post(create_launch))
        .route("/_gateway/launch/{launch_id}", get(consume_launch))
        .route("/ws", get(ws_handler))
        .route(
            "/download/session/{session_id}/{kind}",
            get(download_session),
        )
        .route("/_gateway/media/{artifact_id}", get(read_media_artifact));
    app = app.route(
        "/_gateway/workspace-preview/{resource_id}",
        get(workspace_preview_resource)
            .head(workspace_preview_resource)
            .options(workspace_preview_resource),
    );
    if managed {
        app = app
            .route("/_gateway/managed/identity", get(managed_identity))
            .route("/_gateway/managed/shutdown", post(managed_shutdown));
    }
    let app = app.fallback(get(gateway_fallback)).with_state(state);
    Ok(BoundGatewayWebServer {
        listener,
        app,
        runtime,
        local_addr,
        token,
        managed_shutdown_rx: managed.then_some(managed_shutdown_rx),
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
) -> psychevo::Result<()> {
    let executable = executable_fingerprint(&std::env::current_exe()?)?;
    let state = wire::agents_backend_rpc::ManagedServerState {
        instance_id: config.managed_instance_id.clone(),
        pid: std::process::id(),
        base_url: format!("http://{local_addr}"),
        readyz_url: format!("http://{local_addr}/readyz"),
        started_at_ms: now_ms(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        executable_path: Some(executable.path),
        executable_modified_ms: Some(executable.modified_ms),
        executable_size: Some(executable.size),
        executable_inode: executable.inode.map(|value| value.to_string()),
        static_dir: config
            .static_dir
            .as_deref()
            .map(canonical_path_string)
            .transpose()?,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    psychevo::host_process::atomic_replace(path, &serde_json::to_vec_pretty(&state)?)?;
    Ok(())
}

struct ExecutableFingerprint {
    path: String,
    modified_ms: i64,
    size: u64,
    inode: Option<u64>,
}

fn executable_fingerprint(path: &Path) -> psychevo::Result<ExecutableFingerprint> {
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

fn canonical_path_string(path: &Path) -> psychevo::Result<String> {
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
pub(super) struct WebState {
    pub(super) inner: Arc<WebStateInner>,
}

pub(super) struct WebStateInner {
    pub(super) runtime: Arc<GatewayApplication>,
    pub(super) gateway: Gateway,
    pub(super) framework: FrameworkClient,
    pub(super) durability: GatewayDurability,
    pub(super) event_hub: GatewayEventHub,
    pub(super) home: PathBuf,
    pub(super) cwd: PathBuf,
    pub(super) config_path: Option<PathBuf>,
    pub(super) inherited_env: BTreeMap<String, String>,
    pub(super) static_dir: Option<PathBuf>,
    pub(super) token: String,
    pub(super) managed_instance_id: Option<String>,
    pub(super) managed_shutdown_tx: Option<tokio::sync::watch::Sender<bool>>,
    pub(super) source: GatewaySource,
    pub(super) launches: Mutex<HashMap<String, LaunchEntry>>,
    pub(super) browser_sessions: Mutex<BrowserSessionStore>,
    pub(super) terminals: TerminalManager,
    pub(super) review: WorkspaceReviewState,
    pub(super) workspace_external: WorkspaceExternalState,
    pub(super) workspace_preview: WorkspacePreviewLeaseStore,
    pub(super) workspace_preview_origins: BTreeSet<String>,
    pub(super) pending_actions: Mutex<HashMap<String, PendingActionView>>,
    pub(super) wechat_qr_sessions: Mutex<HashMap<String, channels::WechatQrSetupSession>>,
    pub(super) mcp_oauth_sessions: Arc<Mutex<McpOAuthSessionStore>>,
    pub(super) mcp_oauth_credentials: Arc<dyn McpOAuthCredentialStore>,
    pub(super) voice_policies: Mutex<HashMap<String, wire::voice::VoicePolicyMode>>,
    pub(super) realtime_sessions: Mutex<HashMap<String, RealtimeSessionState>>,
    pub(super) runnable_target_catalog_generation: std::sync::atomic::AtomicU64,
    pub(super) runnable_target_catalogs:
        Mutex<BTreeMap<PathBuf, (u64, Arc<runtime_profiles::RunnableTargetCatalog>)>>,
    pub(super) agent_session_imports: Mutex<AgentSessionImportRegistry>,
    pub(super) channel_runtime: channel_runtime::ChannelRuntimeState,
    pub(super) codex_capability_broker: codex_capability_broker::CodexCapabilityBroker,
    pub(super) codex_elicitations:
        Mutex<HashMap<String, codex_capability_broker::PendingCodexElicitation>>,
    pub(super) extension_app_leases: ExtensionAppLeaseStore,
}

const AGENT_SESSION_IMPORT_TTL_MS: i64 = 10 * 60 * 1_000;
const MAX_AGENT_SESSION_IMPORT_HANDLES: usize = 2_048;

#[derive(Debug, Clone)]
pub(super) struct AgentSessionImportCandidate {
    pub(super) native_session_id: String,
    pub(super) runtime_profile_ref: String,
    pub(super) cwd: PathBuf,
    pub(super) title: Option<String>,
    pub(super) expires_at_ms: i64,
}

#[derive(Debug, Clone)]
pub(super) struct AgentSessionImportCursor {
    pub(super) cursor: String,
    pub(super) runtime_profile_ref: String,
    pub(super) expires_at_ms: i64,
}

#[derive(Debug, Default)]
pub(super) struct AgentSessionImportRegistry {
    pub(super) candidates: HashMap<String, AgentSessionImportCandidate>,
    pub(super) cursors: HashMap<String, AgentSessionImportCursor>,
}

impl AgentSessionImportRegistry {
    pub(super) fn retain_live(&mut self, now_ms: i64) {
        self.candidates
            .retain(|_, candidate| candidate.expires_at_ms > now_ms);
        self.cursors
            .retain(|_, cursor| cursor.expires_at_ms > now_ms);
        while self.candidates.len() + self.cursors.len() >= MAX_AGENT_SESSION_IMPORT_HANDLES {
            if let Some(key) = self.candidates.keys().next().cloned() {
                self.candidates.remove(&key);
            } else if let Some(key) = self.cursors.keys().next().cloned() {
                self.cursors.remove(&key);
            } else {
                break;
            }
        }
    }

    pub(super) fn insert_candidate(
        &mut self,
        runtime_profile_ref: String,
        cwd: PathBuf,
        native_session_id: String,
        title: Option<String>,
    ) -> String {
        let now_ms = gateway_now_ms();
        self.retain_live(now_ms);
        let id = format!("candidate:{}", Uuid::now_v7());
        self.candidates.insert(
            id.clone(),
            AgentSessionImportCandidate {
                native_session_id,
                runtime_profile_ref,
                cwd,
                title,
                expires_at_ms: now_ms + AGENT_SESSION_IMPORT_TTL_MS,
            },
        );
        id
    }

    pub(super) fn insert_cursor(&mut self, runtime_profile_ref: String, cursor: String) -> String {
        let now_ms = gateway_now_ms();
        self.retain_live(now_ms);
        let id = format!("cursor:{}", Uuid::now_v7());
        self.cursors.insert(
            id.clone(),
            AgentSessionImportCursor {
                cursor,
                runtime_profile_ref,
                expires_at_ms: now_ms + AGENT_SESSION_IMPORT_TTL_MS,
            },
        );
        id
    }
}

#[derive(Debug, Clone)]
pub(super) struct BrowserSession {
    pub(super) cwd: PathBuf,
    pub(super) source: GatewaySource,
    pub(super) external_action_grants: BTreeSet<PathBuf>,
}

impl BrowserSession {
    pub(super) fn with_external_action_grant(cwd: PathBuf, source: GatewaySource) -> Self {
        Self {
            external_action_grants: BTreeSet::from([normalized_native_path(&cwd)]),
            cwd,
            source,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct LaunchEntry {
    pub(super) open_token: String,
    pub(super) expires_at_ms: i64,
    pub(super) cwd: PathBuf,
    pub(super) source: GatewaySource,
}

#[derive(Debug, Clone)]
pub(super) enum AuthContext {
    Bearer,
    Browser { session_id: String },
}

impl AuthContext {
    pub(super) fn is_bearer(&self) -> bool {
        matches!(self, Self::Bearer)
    }
}

pub(super) fn merge_framework_activity(
    activity: &mut GatewayActivity,
    running: bool,
    active_turn_id: Option<String>,
    queued_turns: usize,
    kind: wire::events_transcript::FrameworkTurnKind,
) {
    activity.running |= running;
    if let Some(turn_id) = active_turn_id {
        activity.active_turn_id = Some(turn_id.clone());
        if !activity.activities.iter().any(|candidate| {
            matches!(
                candidate,
                wire::events_transcript::ThreadActivityView::FrameworkTurn {
                    turn_id: candidate_turn_id,
                    ..
                } if candidate_turn_id == &turn_id
            )
        }) {
            activity.activities.insert(
                0,
                wire::events_transcript::ThreadActivityView::FrameworkTurn {
                    activity_id: turn_id.clone(),
                    turn_id,
                    kind,
                    queued_turns,
                },
            );
        }
    }
    activity.queued_turns = queued_turns;
}

impl WebState {
    #[cfg(test)]
    pub(super) fn new(config: GatewayWebServerConfig) -> Self {
        Self::new_with_managed_shutdown(config, None)
    }

    pub(super) fn new_with_managed_shutdown(
        config: GatewayWebServerConfig,
        managed_shutdown_tx: Option<tokio::sync::watch::Sender<bool>>,
    ) -> Self {
        let runtime = Arc::new(config.runtime);
        let durability = runtime.application().gateway_durability();
        let framework = runtime.client().clone();
        let gateway = runtime.gateway().clone();
        let source = cwd_source(&config.cwd);
        let channel_runtime = channel_runtime::ChannelRuntimeState::new(&config.home);
        let codex_capability_broker =
            codex_capability_broker::CodexCapabilityBroker::new(&config.inherited_env);
        let workspace_external =
            WorkspaceExternalState::production(&config.inherited_env, &config.cwd);
        let web_state = Self {
            inner: Arc::new(WebStateInner {
                runtime,
                gateway,
                framework,
                durability,
                event_hub: GatewayEventHub::default(),
                home: config.home,
                cwd: config.cwd,
                config_path: config.config_path,
                inherited_env: config.inherited_env,
                static_dir: config.static_dir,
                token: config.token,
                managed_instance_id: config.managed_instance_id,
                managed_shutdown_tx,
                source,
                launches: Mutex::new(HashMap::new()),
                browser_sessions: Mutex::new(BrowserSessionStore::default()),
                terminals: TerminalManager::default(),
                review: WorkspaceReviewState::default(),
                workspace_external,
                workspace_preview: WorkspacePreviewLeaseStore::production(),
                workspace_preview_origins: config.workspace_preview_origins,
                pending_actions: Mutex::new(HashMap::new()),
                wechat_qr_sessions: Mutex::new(HashMap::new()),
                mcp_oauth_sessions: Arc::new(Mutex::new(McpOAuthSessionStore::default())),
                mcp_oauth_credentials: mcp_oauth_credential_store(),
                voice_policies: Mutex::new(HashMap::new()),
                realtime_sessions: Mutex::new(HashMap::new()),
                runnable_target_catalog_generation: std::sync::atomic::AtomicU64::new(1),
                runnable_target_catalogs: Mutex::new(BTreeMap::new()),
                agent_session_imports: Mutex::new(AgentSessionImportRegistry::default()),
                channel_runtime,
                codex_capability_broker,
                codex_elicitations: Mutex::new(HashMap::new()),
                extension_app_leases: ExtensionAppLeaseStore::default(),
            }),
        };
        channel_runtime::reconcile(web_state.clone());
        automations::reconcile(web_state.clone());
        let delete_reconcile_state = web_state.clone();
        let delete_reconcile_gateway = web_state.inner.gateway.clone();
        delete_reconcile_gateway.spawn_background("session-delete-reconcile", async move {
            reconcile_acknowledged_session_deletes(&delete_reconcile_state).await;
        });
        web_state
    }

    pub(super) fn auth_from_headers(&self, headers: &HeaderMap) -> Option<AuthContext> {
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
            .authenticate(cookie)
            .map(|_| AuthContext::Browser {
                session_id: cookie.to_string(),
            })
    }

    pub(super) fn selector(&self, source: &GatewaySource) -> GatewayThreadSelector {
        GatewayThreadSelector::source(source.source_key())
    }

    pub(super) async fn activity(
        &self,
        source: &GatewaySource,
        thread_id: Option<&str>,
    ) -> GatewayActivity {
        let mut activity = match thread_id {
            Some(thread_id) => {
                self.inner
                    .gateway
                    .activity_for_selector(GatewayThreadSelector::thread_id(thread_id))
                    .await
            }
            None => {
                self.inner
                    .gateway
                    .activity_for_selector(self.selector(source))
                    .await
            }
        };
        let framework_thread_id = match thread_id {
            Some(thread_id) => Some(thread_id.to_string()),
            None => self
                .inner
                .gateway
                .resolve_source_thread(source)
                .await
                .ok()
                .flatten(),
        };
        if let Some(thread_id) = framework_thread_id
            && let Ok(thread) = self.inner.framework.resume_thread(&thread_id).await
        {
            let framework_activity = thread.activity();
            activity.framework_revision = Some(framework_activity.revision.to_string());
            let kind = thread
                .summary()
                .await
                .ok()
                .filter(|summary| summary.parent_thread_id.is_some())
                .map_or(wire::events_transcript::FrameworkTurnKind::Root, |_| {
                    wire::events_transcript::FrameworkTurnKind::DelegatedChild
                });
            merge_framework_activity(
                &mut activity,
                framework_activity.running,
                framework_activity.active_turn_id,
                framework_activity.queued_turns,
                kind,
            );
        } else {
            activity.framework_revision = Some(
                self.inner
                    .framework
                    .activity_snapshot()
                    .revision
                    .to_string(),
            );
        }
        activity
    }

    pub(super) async fn session_activity_snapshot(
        &self,
    ) -> psychevo::Result<(String, BTreeMap<String, GatewayActivity>)> {
        let mut snapshot = self.inner.gateway.session_activity_snapshot().await?;
        let framework_snapshot = self.inner.framework.activity_snapshot();
        let revision = framework_snapshot.revision.to_string();
        let framework_thread_ids = framework_snapshot
            .threads
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let summaries = self
            .inner
            .framework
            .thread_summaries(&framework_thread_ids)
            .await?;
        for activity in snapshot.values_mut() {
            activity.framework_revision = Some(revision.clone());
        }
        for (thread_id, framework_activity) in framework_snapshot.threads {
            let kind = summaries
                .get(&thread_id)
                .filter(|summary| summary.parent_thread_id.is_some())
                .map_or(wire::events_transcript::FrameworkTurnKind::Root, |_| {
                    wire::events_transcript::FrameworkTurnKind::DelegatedChild
                });
            let activity = snapshot.entry(thread_id).or_default();
            activity.framework_revision = Some(revision.clone());
            merge_framework_activity(
                activity,
                framework_activity.running,
                framework_activity.active_turn_id,
                framework_activity.queued_turns,
                kind,
            );
        }
        Ok((revision, snapshot))
    }

    pub(super) fn thread_turn_request(
        &self,
        cwd: PathBuf,
        thread_id: Option<String>,
        input: Vec<GatewayInputPart>,
    ) -> (ThreadCallerContext, ThreadTurnIntent) {
        let mut inherited_env = self.inner.inherited_env.clone();
        inherited_env
            .entry("PSYCHEVO_HOME".to_string())
            .or_insert_with(|| self.inner.home.to_string_lossy().into_owned());
        let mut caller = ThreadCallerContext::new(ThreadSurface::Web, cwd.clone());
        caller.set_runtime_tools(automations::automation_runtime_tools(
            self.clone(),
            cwd,
            thread_id.clone(),
        ));
        let mut intent = ThreadTurnIntent::new(input);
        intent.thread_id = thread_id;
        intent.policy.snapshot_root = Some(self.inner.home.join("snapshots"));
        intent.policy.extract_prompt_image_sources = true;
        intent.policy.config_path = self.inner.config_path.clone();
        intent.policy.permission_mode = Some(PermissionMode::Default);
        intent.policy.clarify_enabled = true;
        intent.policy.inherited_env = Some(inherited_env);
        (caller, intent)
    }

    #[cfg(test)]
    pub(super) fn record_event(&self, event: &GatewayEvent) {
        self.record_event_with_context(event, PendingInteractionContext::default());
    }

    pub(super) fn record_event_with_context(
        &self,
        event: &GatewayEvent,
        context: PendingInteractionContext,
    ) {
        match event {
            GatewayEvent::ActionRequested { action } | GatewayEvent::ActionUpdated { action } => {
                self.inner
                    .pending_actions
                    .lock()
                    .expect("web pending actions poisoned")
                    .insert(
                        action.action_id.clone(),
                        pending_action_with_context(action.clone(), context),
                    );
            }
            GatewayEvent::ActionResolved { action_id, .. }
            | GatewayEvent::ActionCancelled { action_id, .. } => {
                self.inner
                    .pending_actions
                    .lock()
                    .expect("web pending actions poisoned")
                    .remove(action_id);
            }
            GatewayEvent::TurnCompleted {
                thread_id, turn_id, ..
            } => {
                self.remove_pending_actions_for_completed_turn(thread_id.as_deref(), turn_id);
            }
            _ => {}
        }
    }

    pub(super) fn publish_gateway_event_with_context(
        &self,
        event: GatewayEvent,
        context: PendingInteractionContext,
        review_cwd: Option<&Path>,
    ) {
        self.publish_gateway_event_for_connection(event, context, review_cwd, None);
    }

    pub(super) fn publish_gateway_event_for_connection(
        &self,
        event: GatewayEvent,
        context: PendingInteractionContext,
        review_cwd: Option<&Path>,
        connection: Option<&ConnectionSender>,
    ) {
        self.record_event_with_context(&event, context.clone());
        if let Some(cwd) = review_cwd {
            self.record_review_event(&event, cwd);
        }
        let display_event = self.event_with_pending_context(event, &context);
        if let Some(connection) = connection.filter(|sender| sender.is_internal_adapter()) {
            let _ = connection.send(rpc_notification("gateway/event", json!(display_event)));
        }
        self.inner.event_hub.publish(&display_event);
    }

    pub(super) fn pending_context_for_selector(
        &self,
        selector: &GatewayThreadSelector,
        thread_id: Option<&str>,
    ) -> PendingInteractionContext {
        let activity = self.inner.gateway.local_activity_for_selector(selector);
        let source_key = match selector {
            GatewayThreadSelector::Source { source_key } => Some(source_key.0.clone()),
            GatewayThreadSelector::ThreadId { .. } => None,
        };
        PendingInteractionContext {
            thread_id: thread_id.map(str::to_string),
            turn_id: activity.active_turn_id.clone(),
            activity_id: activity.active_turn_id,
            source_key,
            owner_id: activity
                .owner_id
                .or_else(|| Some(self.inner.gateway.owner_id().to_string())),
            lease_expires_at_ms: activity.lease_expires_at_ms,
        }
    }

    pub(super) fn event_with_pending_context(
        &self,
        event: GatewayEvent,
        context: &PendingInteractionContext,
    ) -> GatewayEvent {
        match event {
            GatewayEvent::ActionRequested { action } => GatewayEvent::ActionRequested {
                action: pending_action_with_context(action, context.clone()),
            },
            GatewayEvent::ActionUpdated { action } => GatewayEvent::ActionUpdated {
                action: pending_action_with_context(action, context.clone()),
            },
            event => event,
        }
    }

    pub(super) fn remove_pending_permission(&self, request_id: &str) {
        self.inner
            .pending_actions
            .lock()
            .expect("web pending actions poisoned")
            .remove(request_id);
    }

    pub(super) fn remove_pending_actions_for_completed_turn(
        &self,
        thread_id: Option<&str>,
        turn_id: &str,
    ) {
        self.inner
            .pending_actions
            .lock()
            .expect("web pending actions poisoned")
            .retain(|_, action| {
                if action.turn_id.as_deref() == Some(turn_id) {
                    return false;
                }
                if let Some(thread_id) = thread_id
                    && action.thread_id.as_deref() == Some(thread_id)
                {
                    return false;
                }
                true
            });
    }

    pub(super) fn record_review_event(&self, event: &GatewayEvent, cwd: &Path) {
        self.inner.review.observe_event(event, cwd);
    }
}

#[cfg(not(test))]
fn mcp_oauth_credential_store() -> Arc<dyn McpOAuthCredentialStore> {
    Arc::new(SystemMcpOAuthCredentialStore)
}

#[cfg(test)]
fn mcp_oauth_credential_store() -> Arc<dyn McpOAuthCredentialStore> {
    Arc::new(TestMcpOAuthCredentialStore::default())
}

#[cfg(test)]
#[derive(Default)]
struct TestMcpOAuthCredentialStore {
    tokens: Mutex<HashMap<String, String>>,
}

#[cfg(test)]
impl McpOAuthCredentialStore for TestMcpOAuthCredentialStore {
    fn load_access_token(&self, account: &str) -> psychevo::Result<Option<String>> {
        Ok(self
            .tokens
            .lock()
            .expect("test MCP OAuth credentials poisoned")
            .get(account)
            .cloned())
    }

    fn save_access_token(&self, account: &str, access_token: &str) -> psychevo::Result<()> {
        self.tokens
            .lock()
            .expect("test MCP OAuth credentials poisoned")
            .insert(account.to_string(), access_token.to_string());
        Ok(())
    }

    fn clear_access_token(&self, account: &str) -> psychevo::Result<bool> {
        Ok(self
            .tokens
            .lock()
            .expect("test MCP OAuth credentials poisoned")
            .remove(account)
            .is_some())
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct PendingInteractionContext {
    pub(super) thread_id: Option<String>,
    pub(super) turn_id: Option<String>,
    pub(super) activity_id: Option<String>,
    pub(super) source_key: Option<String>,
    pub(super) owner_id: Option<String>,
    pub(super) lease_expires_at_ms: Option<i64>,
}

impl From<crate::gateway::live_projection::GatewayLiveProjectionContext>
    for PendingInteractionContext
{
    fn from(context: crate::gateway::live_projection::GatewayLiveProjectionContext) -> Self {
        Self {
            thread_id: context.thread_id,
            turn_id: context.turn_id,
            activity_id: context.activity_id,
            source_key: context.source_key,
            owner_id: context.owner_id,
            lease_expires_at_ms: context.lease_expires_at_ms,
        }
    }
}

fn pending_action_with_context(
    mut action: PendingActionView,
    context: PendingInteractionContext,
) -> PendingActionView {
    action.thread_id = action.thread_id.or(context.thread_id);
    action.turn_id = action.turn_id.or(context.turn_id);
    action.activity_id = action.activity_id.or(context.activity_id);
    action.source_key = action.source_key.or(context.source_key);
    action.owner_id = action.owner_id.or(context.owner_id);
    action.lease_expires_at_ms = action.lease_expires_at_ms.or(context.lease_expires_at_ms);
    action
}
