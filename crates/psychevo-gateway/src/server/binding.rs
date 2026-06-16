const INTERNAL_SESSION_SOURCES: &[&str] = SIDE_CONVERSATION_SESSION_SOURCES;
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
            runtime_ref: None,
            runtime_session_id: None,
            runtime_options: BTreeMap::new(),
            include_reasoning: false,
            mode: RunMode::Default,
            permission_mode: Some(PermissionMode::Default),
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: true,
            inherited_env: Some(self.inner.inherited_env.clone()),
            agent: None,
            external_agent_delegate: None,
            no_agents: false,
            no_skills: false,
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
        }
    }

    #[cfg(test)]
    fn record_event(&self, event: &GatewayEvent) {
        self.record_event_with_context(event, PendingInteractionContext::default());
    }

    fn record_event_with_context(&self, event: &GatewayEvent, context: PendingInteractionContext) {
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
                            thread_id: context.thread_id,
                            turn_id: context.turn_id,
                            activity_id: context.activity_id,
                            owner_id: context.owner_id,
                            lease_expires_at_ms: context.lease_expires_at_ms,
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
            GatewayEvent::TurnCompleted {
                thread_id, turn_id, ..
            } => {
                self.remove_pending_permissions_for_completed_turn(thread_id.as_deref(), turn_id);
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

    fn pending_context_for_live_event(
        &self,
        record: &psychevo_runtime::GatewayLiveEventRecord,
    ) -> PendingInteractionContext {
        let mut context = PendingInteractionContext {
            thread_id: record.thread_id.clone(),
            turn_id: record.turn_id.clone(),
            activity_id: record.activity_id.clone(),
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
        PendingInteractionContext {
            thread_id: thread_id.map(str::to_string),
            turn_id: activity.active_turn_id.clone(),
            activity_id: activity.active_turn_id,
            owner_id: activity
                .owner_id
                .or_else(|| Some(self.inner.gateway.owner_id().to_string())),
            lease_expires_at_ms: activity.lease_expires_at_ms,
        }
    }

    fn remove_pending_permission(&self, request_id: &str) {
        self.inner
            .pending_permissions
            .lock()
            .expect("web pending permissions poisoned")
            .remove(request_id);
    }

    fn remove_pending_permissions_for_completed_turn(
        &self,
        thread_id: Option<&str>,
        turn_id: &str,
    ) {
        self.inner
            .pending_permissions
            .lock()
            .expect("web pending permissions poisoned")
            .retain(|_, permission| {
                if permission.turn_id.as_deref() == Some(turn_id) {
                    return false;
                }
                if let Some(thread_id) = thread_id
                    && permission.thread_id.as_deref() == Some(thread_id)
                {
                    return false;
                }
                true
            });
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
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    activity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lease_expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Default)]
struct PendingInteractionContext {
    thread_id: Option<String>,
    turn_id: Option<String>,
    activity_id: Option<String>,
    owner_id: Option<String>,
    lease_expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingClarifyView {
    request_id: String,
    raw: Value,
}
