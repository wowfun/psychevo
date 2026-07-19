const RUNTIME_GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(6);
const RUNTIME_FORCE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(4);
const SERVER_CONNECTION_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
#[derive(Debug, Clone)]
pub struct GatewayWebServerConfig {
    pub gateway: Gateway,
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
}

impl GatewayWebServerConfig {
    pub fn new(
        gateway: Gateway,
        home: PathBuf,
        cwd: PathBuf,
        config_path: Option<PathBuf>,
        inherited_env: BTreeMap<String, String>,
        static_dir: PathBuf,
    ) -> Self {
        Self {
            gateway,
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
        }
    }

    pub fn headless(
        gateway: Gateway,
        home: PathBuf,
        cwd: PathBuf,
        config_path: Option<PathBuf>,
        inherited_env: BTreeMap<String, String>,
        token: String,
    ) -> Self {
        Self {
            gateway,
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
        }
    }
}

pub struct BoundGatewayWebServer {
    listener: TcpListener,
    app: Router,
    gateway: Gateway,
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

    pub async fn run(self) -> psychevo_runtime::Result<()> {
        self.run_with_shutdown_signal(std::future::pending()).await
    }

    pub async fn run_with_shutdown_signal<F>(
        self,
        shutdown_signal: F,
    ) -> psychevo_runtime::Result<()>
    where
        F: std::future::Future<Output = ()> + Send,
    {
        let Self {
            listener,
            app,
            gateway,
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
                let shutdown = shutdown_runtimes_with_deadlines(
                    &gateway,
                    RUNTIME_GRACEFUL_SHUTDOWN_TIMEOUT,
                    RUNTIME_FORCE_SHUTDOWN_TIMEOUT,
                ).await;
                result?;
                shutdown
            }
            _ = &mut shutdown_signal => {
                let _ = server_shutdown_tx.send(());
                let shutdown = shutdown_runtimes_with_deadlines(
                    &gateway,
                    RUNTIME_GRACEFUL_SHUTDOWN_TIMEOUT,
                    RUNTIME_FORCE_SHUTDOWN_TIMEOUT,
                ).await;
                let drain = tokio::time::timeout(SERVER_CONNECTION_DRAIN_TIMEOUT, &mut server).await;
                if let Ok(result) = drain {
                    result?;
                }
                shutdown
            }
        }
    }
}

async fn shutdown_runtimes_with_deadlines(
    gateway: &Gateway,
    graceful_timeout: Duration,
    force_timeout: Duration,
) -> psychevo_runtime::Result<()> {
    let graceful = tokio::time::timeout(graceful_timeout, gateway.shutdown_runtimes(false)).await;
    let graceful_failure = match graceful {
        Ok(Ok(())) => return Ok(()),
        Ok(Err(error)) => format!("graceful runtime shutdown failed: {error}"),
        Err(_) => format!(
            "graceful runtime shutdown exceeded {} ms",
            graceful_timeout.as_millis()
        ),
    };
    match tokio::time::timeout(force_timeout, gateway.shutdown_runtimes(true)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(Error::Message(format!(
            "{graceful_failure}; forced runtime shutdown failed: {error}"
        ))),
        Err(_) => Err(Error::Message(format!(
            "{graceful_failure}; forced runtime shutdown exceeded {} ms",
            force_timeout.as_millis()
        ))),
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
    let gateway = config.gateway.clone();
    let managed = config.managed_instance_id.is_some();
    let (managed_shutdown_tx, managed_shutdown_rx) = tokio::sync::watch::channel(false);
    let state = WebState::new_with_managed_shutdown(
        config,
        managed.then_some(managed_shutdown_tx),
    );
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
    if managed {
        app = app
            .route("/_gateway/managed/identity", get(managed_identity))
            .route("/_gateway/managed/shutdown", post(managed_shutdown));
    }
    let app = app.fallback(get(gateway_fallback)).with_state(state);
    Ok(BoundGatewayWebServer {
        listener,
        app,
        gateway,
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
) -> psychevo_runtime::Result<()> {
    let executable = executable_fingerprint(&std::env::current_exe()?)?;
    let state = wire::ManagedServerState {
        instance_id: config.managed_instance_id.clone(),
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
    psychevo_runtime::host_process::atomic_replace(path, &serde_json::to_vec_pretty(&state)?)?;
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
    cwd: PathBuf,
    config_path: Option<PathBuf>,
    inherited_env: BTreeMap<String, String>,
    static_dir: Option<PathBuf>,
    token: String,
    managed_instance_id: Option<String>,
    managed_shutdown_tx: Option<tokio::sync::watch::Sender<bool>>,
    source: GatewaySource,
    launches: Mutex<HashMap<String, LaunchEntry>>,
    browser_sessions: Mutex<HashMap<String, BrowserSession>>,
    terminals: TerminalManager,
    review: WorkspaceReviewState,
    workspace_external: WorkspaceExternalState,
    pending_actions: Mutex<HashMap<String, PendingActionView>>,
    wechat_qr_sessions: Mutex<HashMap<String, channels::WechatQrSetupSession>>,
    mcp_oauth_sessions: Mutex<HashMap<String, McpOAuthSession>>,
    voice_policies: Mutex<HashMap<String, wire::VoicePolicyMode>>,
    realtime_sessions: Mutex<HashMap<String, RealtimeSessionState>>,
    runnable_target_catalog_generation: std::sync::atomic::AtomicU64,
    runnable_target_catalogs:
        Mutex<HashMap<PathBuf, (u64, Arc<runtime_profiles::RunnableTargetCatalog>)>>,
    agent_session_imports: Mutex<AgentSessionImportRegistry>,
    channel_runtime: channel_runtime::ChannelRuntimeState,
    codex_capability_broker: codex_capability_broker::CodexCapabilityBroker,
    codex_elicitations:
        Mutex<HashMap<String, codex_capability_broker::PendingCodexElicitation>>,
}

const AGENT_SESSION_IMPORT_TTL_MS: i64 = 10 * 60 * 1_000;
const MAX_AGENT_SESSION_IMPORT_HANDLES: usize = 2_048;

#[derive(Debug, Clone)]
struct AgentSessionImportCandidate {
    native_session_id: String,
    runtime_profile_ref: String,
    cwd: PathBuf,
    title: Option<String>,
    expires_at_ms: i64,
}

#[derive(Debug, Clone)]
struct AgentSessionImportCursor {
    cursor: String,
    runtime_profile_ref: String,
    expires_at_ms: i64,
}

#[derive(Debug, Default)]
struct AgentSessionImportRegistry {
    candidates: HashMap<String, AgentSessionImportCandidate>,
    cursors: HashMap<String, AgentSessionImportCursor>,
}

impl AgentSessionImportRegistry {
    fn retain_live(&mut self, now_ms: i64) {
        self.candidates
            .retain(|_, candidate| candidate.expires_at_ms > now_ms);
        self.cursors.retain(|_, cursor| cursor.expires_at_ms > now_ms);
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

    fn insert_candidate(
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

    fn insert_cursor(&mut self, runtime_profile_ref: String, cursor: String) -> String {
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
struct McpOAuthSession {
    status: Arc<Mutex<McpOAuthSessionStatus>>,
}

#[derive(Debug, Clone)]
enum McpOAuthSessionStatus {
    Pending,
    Succeeded,
    Failed(String),
}

#[derive(Debug, Clone)]
struct BrowserSession {
    cwd: PathBuf,
    source: GatewaySource,
    external_action_grants: BTreeSet<PathBuf>,
}

impl BrowserSession {
    fn with_external_action_grant(cwd: PathBuf, source: GatewaySource) -> Self {
        Self {
            external_action_grants: BTreeSet::from([normalized_native_path(&cwd)]),
            cwd,
            source,
        }
    }
}

#[derive(Debug, Clone)]
struct LaunchEntry {
    open_token: String,
    expires_at_ms: i64,
    cwd: PathBuf,
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
    #[cfg(test)]
    fn new(config: GatewayWebServerConfig) -> Self {
        Self::new_with_managed_shutdown(config, None)
    }

    fn new_with_managed_shutdown(
        config: GatewayWebServerConfig,
        managed_shutdown_tx: Option<tokio::sync::watch::Sender<bool>>,
    ) -> Self {
        let state = config.gateway.state().clone();
        let source = cwd_source(&config.cwd);
        let channel_runtime = channel_runtime::ChannelRuntimeState::new(&config.home);
        let codex_capability_broker =
            codex_capability_broker::CodexCapabilityBroker::new(&config.inherited_env);
        let workspace_external =
            WorkspaceExternalState::production(&config.inherited_env, &config.cwd);
        let web_state = Self {
            inner: Arc::new(WebStateInner {
                gateway: config.gateway,
                state,
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
                browser_sessions: Mutex::new(HashMap::new()),
                terminals: TerminalManager::default(),
                review: WorkspaceReviewState::default(),
                workspace_external,
                pending_actions: Mutex::new(HashMap::new()),
                wechat_qr_sessions: Mutex::new(HashMap::new()),
                mcp_oauth_sessions: Mutex::new(HashMap::new()),
                voice_policies: Mutex::new(HashMap::new()),
                realtime_sessions: Mutex::new(HashMap::new()),
                runnable_target_catalog_generation: std::sync::atomic::AtomicU64::new(1),
                runnable_target_catalogs: Mutex::new(HashMap::new()),
                agent_session_imports: Mutex::new(AgentSessionImportRegistry::default()),
                channel_runtime,
                codex_capability_broker,
                codex_elicitations: Mutex::new(HashMap::new()),
            }),
        };
        channel_runtime::reconcile(web_state.clone());
        automations::reconcile(web_state.clone());
        reconcile_acknowledged_session_deletes(&web_state);
        web_state
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

    fn run_options(&self, cwd: PathBuf, thread_id: Option<String>) -> RunOptions {
        let mut inherited_env = self.inner.inherited_env.clone();
        inherited_env
            .entry("PSYCHEVO_HOME".to_string())
            .or_insert_with(|| self.inner.home.to_string_lossy().into_owned());
        RunOptions {
            state: self.inner.state.clone(),
            cwd: cwd.clone(),
            snapshot_root: Some(self.inner.home.join("snapshots")),
            session: thread_id.clone(),
            continue_latest: false,
            prompt: String::new(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: true,
            prompt_display: None,
            max_context_messages: None,
            config_path: self.inner.config_path.clone(),
            project_context_override: None,
            sandbox_override: None,
            model: None,
            reasoning_effort: None,
            runtime_ref: None,
            runtime_session_id: None,
            runtime_options: BTreeMap::new(),
            include_reasoning: false,
            mode: RunMode::Default,
            permission_mode: Some(PermissionMode::Default),
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: true,
            inherited_env: Some(inherited_env),
            agent: None,
            external_agent_delegate: None,
            no_agents: false,
            no_skills: false,
            selected_capability_roots: Vec::new(),
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
            workspace_mutations: None,
            runtime_tools: automations::automation_runtime_tools(
                self.clone(),
                cwd.clone(),
                thread_id.clone(),
            ),
        }
    }

    fn thread_turn_request(
        &self,
        cwd: PathBuf,
        thread_id: Option<String>,
        input: Vec<GatewayInputPart>,
    ) -> crate::ThreadTurnRequest {
        let mut inherited_env = self.inner.inherited_env.clone();
        inherited_env
            .entry("PSYCHEVO_HOME".to_string())
            .or_insert_with(|| self.inner.home.to_string_lossy().into_owned());
        let mut request = crate::ThreadTurnRequest::new(cwd.clone(), input);
        request.thread_id = thread_id.clone();
        request.policy.snapshot_root = Some(self.inner.home.join("snapshots"));
        request.policy.extract_prompt_image_sources = true;
        request.policy.config_path = self.inner.config_path.clone();
        request.policy.permission_mode = Some(PermissionMode::Default);
        request.policy.clarify_enabled = true;
        request.policy.inherited_env = Some(inherited_env);
        request.set_runtime_tools(automations::automation_runtime_tools(
            self.clone(),
            cwd,
            thread_id,
        ));
        request
    }

    #[cfg(test)]
    fn record_event(&self, event: &GatewayEvent) {
        self.record_event_with_context(event, PendingInteractionContext::default());
    }

    fn record_event_with_context(&self, event: &GatewayEvent, context: PendingInteractionContext) {
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

    fn pending_context_for_live_event(
        &self,
        record: &psychevo_runtime::GatewayLiveEventRecord,
    ) -> PendingInteractionContext {
        let mut context = PendingInteractionContext {
            thread_id: record.thread_id.clone(),
            turn_id: record.turn_id.clone(),
            activity_id: record.activity_id.clone(),
            source_key: None,
            owner_id: record.owner_id.clone(),
            lease_expires_at_ms: None,
        };
        if let Some(activity_id) = &record.activity_id
            && let Ok(Some(activity)) = self.inner.state.store().gateway_activity(activity_id)
        {
            if context.thread_id.is_none() {
                context.thread_id = activity.thread_id;
            }
            if context.turn_id.is_none() {
                context.turn_id = activity.turn_id;
            }
            if context.owner_id.is_none() {
                context.owner_id = Some(activity.owner_id);
            }
            if context.source_key.is_none() {
                context.source_key = activity.source_key;
            }
            context.lease_expires_at_ms = Some(activity.lease_expires_at_ms);
        }
        context
    }

    fn pending_context_for_selector(
        &self,
        selector: &GatewayThreadSelector,
        thread_id: Option<&str>,
    ) -> PendingInteractionContext {
        let activity = self.inner.gateway.activity_for_selector(selector.clone());
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

    fn event_with_pending_context(
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

    fn remove_pending_permission(&self, request_id: &str) {
        self.inner
            .pending_actions
            .lock()
            .expect("web pending actions poisoned")
            .remove(request_id);
    }

    fn remove_pending_actions_for_completed_turn(&self, thread_id: Option<&str>, turn_id: &str) {
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

    fn record_review_event(&self, event: &GatewayEvent, cwd: &Path) {
        self.inner.review.observe_event(event, cwd);
    }
}

#[derive(Debug, Clone, Default)]
struct PendingInteractionContext {
    thread_id: Option<String>,
    turn_id: Option<String>,
    activity_id: Option<String>,
    source_key: Option<String>,
    owner_id: Option<String>,
    lease_expires_at_ms: Option<i64>,
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
