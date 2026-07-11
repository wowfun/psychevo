#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayActivity {
    pub running: bool,
    pub active_turn_id: Option<String>,
    pub queued_turns: usize,
    pub started_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub owner_id: Option<String>,
    pub owner_surface: Option<String>,
    pub lease_expires_at_ms: Option<i64>,
    pub takeover_state: Option<String>,
}

#[derive(Debug, Default)]
struct ActiveThreadState {
    running: bool,
    active_turn_id: Option<String>,
    active_kind: Option<ActiveActivityKind>,
    control: Option<RunControlHandle>,
    queued: VecDeque<PendingQueuedActivity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveActivityKind {
    Turn,
    Shell,
    Compact,
}

enum ShellStartState {
    Standalone,
    Auxiliary(RunControlHandle),
    Queued(oneshot::Receiver<psychevo_runtime::Result<GatewayShellResult>>),
}

#[derive(Debug)]
enum PendingQueuedActivity {
    Turn(Box<PendingQueuedTurn>),
    Shell(Box<PendingQueuedShell>),
    Compact(Box<PendingQueuedCompact>),
}

#[derive(Debug)]
struct PendingQueuedTurn {
    turn_id: String,
    request: SendTurnRequest,
    responder: oneshot::Sender<psychevo_runtime::Result<GatewayTurnResult>>,
}

#[derive(Debug)]
struct PendingQueuedShell {
    shell_id: String,
    request: SendShellRequest,
    responder: oneshot::Sender<psychevo_runtime::Result<GatewayShellResult>>,
}

#[derive(Debug)]
struct PendingQueuedCompact {
    compact_id: String,
    request: SendCompactRequest,
    responder: oneshot::Sender<psychevo_runtime::Result<psychevo_runtime::CompactionResult>>,
}

type PendingPermissionMap = Arc<Mutex<HashMap<String, PendingPermission>>>;

type PendingRuntimeInteractionMap = Arc<Mutex<HashMap<String, PendingRuntimeInteraction>>>;

#[derive(Clone)]
struct PendingRuntimeInteraction {
    interaction: psychevo_runtime_host::RuntimeInteraction,
    profile: RuntimeProfile,
    event_sink: Option<GatewayEventSink>,
}

struct PendingPermission {
    selector_key: Option<String>,
    responder: oneshot::Sender<PermissionApprovalDecision>,
}

#[derive(Clone)]
struct GatewayApprovalHandler {
    selector_key: Option<String>,
    pending_permissions: PendingPermissionMap,
    event_sink: GatewayEventSink,
    timeout_secs: u64,
    session_authorization_lifetime: Option<&'static str>,
}

impl GatewayApprovalHandler {
    fn new(
        selector_key: Option<String>,
        pending_permissions: PendingPermissionMap,
        event_sink: GatewayEventSink,
        session_authorization_lifetime: Option<&'static str>,
    ) -> Self {
        Self {
            selector_key,
            pending_permissions,
            event_sink,
            timeout_secs: 300,
            session_authorization_lifetime,
        }
    }
}

impl fmt::Debug for GatewayApprovalHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayApprovalHandler")
            .field("selector_key", &self.selector_key)
            .field("timeout_secs", &self.timeout_secs)
            .finish_non_exhaustive()
    }
}

impl ApprovalHandler for GatewayApprovalHandler {
    fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> BoxFuture<'static, PermissionApprovalDecision> {
        let request_id = if request.tool_call_id.trim().is_empty() {
            Uuid::now_v7().to_string()
        } else {
            request.tool_call_id.clone()
        };
        let selector_key = self.selector_key.clone();
        let pending_permissions = self.pending_permissions.clone();
        let event_sink = self.event_sink.clone();
        let timeout_secs = self.timeout_secs;
        let session_authorization_lifetime = self.session_authorization_lifetime;
        Box::pin(async move {
            let (responder, receiver) = oneshot::channel();
            {
                let mut pending = pending_permissions
                    .lock()
                    .expect("gateway pending permission map poisoned");
                pending.insert(
                    request_id.clone(),
                    PendingPermission {
                        selector_key,
                        responder,
                    },
                );
            }
            let allow_always = request.allow_always;
            event_sink(GatewayEvent::ActionRequested {
                action: PendingActionView {
                    action_id: request_id.clone(),
                    kind: GatewayActionKind::Permission,
                    title: Some(request.tool_name.clone()),
                    summary: Some(if request.summary.trim().is_empty() {
                        request.reason.clone()
                    } else {
                        request.summary.clone()
                    }),
                    payload: json!({
                        "toolName": request.tool_name,
                        "summary": request.summary,
                        "reason": request.reason,
                        "matchedRule": request.matched_rule,
                        "suggestedRule": request.suggested_rule,
                        "allowSession": session_authorization_lifetime.is_some(),
                        "allowAlways": allow_always,
                        "authorizationLifetime": session_authorization_lifetime,
                        "alwaysAuthorizationLifetime": allow_always.then_some("permanent"),
                        "timeoutSecs": request.timeout_secs,
                    }),
                    thread_id: None,
                    turn_id: None,
                    activity_id: None,
                    source_key: None,
                    owner_id: None,
                    lease_expires_at_ms: None,
                },
            });
            let decision = timeout(Duration::from_secs(timeout_secs), receiver)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_else(PermissionApprovalDecision::deny);
            {
                let mut pending = pending_permissions
                    .lock()
                    .expect("gateway pending permission map poisoned");
                pending.remove(&request_id);
            }
            event_sink(GatewayEvent::ActionResolved {
                action_id: request_id,
                kind: GatewayActionKind::Permission,
                outcome: permission_action_outcome(&decision),
                payload: json!({
                    "decision": permission_decision_from_runtime(&decision),
                }),
            });
            decision
        })
    }
}

#[derive(Debug, Clone)]
pub struct QueuedGatewayInput {
    pub input: Vec<GatewayInputPart>,
}

pub struct SendTurnRequest {
    pub thread_id: Option<String>,
    pub source: Option<GatewaySource>,
    pub bind_source: Option<GatewaySource>,
    pub reset_source_binding: bool,
    pub input: Vec<GatewayInputPart>,
    pub options: RunOptions,
    pub runtime_source: Option<String>,
    pub continue_sources: Vec<String>,
    pub stream: Option<RunStreamSink>,
    pub event_sink: Option<GatewayEventSink>,
    pub control_handle: Option<RunControlHandle>,
    pub control: Option<RunControl>,
    pub lineage: Option<Value>,
}

impl fmt::Debug for SendTurnRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SendTurnRequest")
            .field("thread_id", &self.thread_id)
            .field("source", &self.source)
            .field("bind_source", &self.bind_source)
            .field("reset_source_binding", &self.reset_source_binding)
            .field("input", &self.input)
            .field("options", &self.options)
            .field("runtime_source", &self.runtime_source)
            .field("continue_sources", &self.continue_sources)
            .field("has_stream", &self.stream.is_some())
            .field("has_event_sink", &self.event_sink.is_some())
            .field("has_control_handle", &self.control_handle.is_some())
            .field("has_control", &self.control.is_some())
            .field("lineage", &self.lineage)
            .finish()
    }
}

pub struct SendShellRequest {
    pub thread_id: Option<String>,
    pub source: Option<GatewaySource>,
    pub bind_source: Option<GatewaySource>,
    pub cwd: PathBuf,
    pub command: String,
    pub context: UserShellContextOptions,
    pub stream: Option<RunStreamSink>,
    pub event_sink: Option<GatewayEventSink>,
    pub lineage: Option<Value>,
}

pub struct SendCompactRequest {
    pub thread_id: Option<String>,
    pub source: Option<GatewaySource>,
    pub runtime_ref: Option<String>,
    pub cwd: PathBuf,
    pub config_path: Option<PathBuf>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub instructions: Option<String>,
    pub force: bool,
    pub reason: psychevo_runtime::CompactionReason,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub event_sink: Option<GatewayEventSink>,
}

impl fmt::Debug for SendCompactRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SendCompactRequest")
            .field("thread_id", &self.thread_id)
            .field("source", &self.source)
            .field("runtime_ref", &self.runtime_ref)
            .field("cwd", &self.cwd)
            .field("config_path", &self.config_path)
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("has_instructions", &self.instructions.is_some())
            .field("force", &self.force)
            .field("reason", &self.reason)
            .field("has_inherited_env", &self.inherited_env.is_some())
            .field("has_event_sink", &self.event_sink.is_some())
            .finish()
    }
}

impl fmt::Debug for SendShellRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SendShellRequest")
            .field("thread_id", &self.thread_id)
            .field("source", &self.source)
            .field("bind_source", &self.bind_source)
            .field("cwd", &self.cwd)
            .field("command", &self.command)
            .field("context", &self.context)
            .field("has_stream", &self.stream.is_some())
            .field("has_event_sink", &self.event_sink.is_some())
            .field("lineage", &self.lineage)
            .finish()
    }
}
