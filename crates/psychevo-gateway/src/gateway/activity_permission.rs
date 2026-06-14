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

type PendingPermissionMap = Arc<Mutex<HashMap<String, PendingPermission>>>;

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
}

impl GatewayApprovalHandler {
    fn new(
        selector_key: Option<String>,
        pending_permissions: PendingPermissionMap,
        event_sink: GatewayEventSink,
    ) -> Self {
        Self {
            selector_key,
            pending_permissions,
            event_sink,
            timeout_secs: 300,
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
            event_sink(GatewayEvent::PermissionRequested {
                request_id: request_id.clone(),
                tool_name: request.tool_name.clone(),
                summary: request.summary.clone(),
                reason: request.reason.clone(),
                matched_rule: request.matched_rule.clone(),
                suggested_rule: request.suggested_rule.clone(),
                allow_always: request.allow_always,
                timeout_secs: request.timeout_secs,
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
            event_sink(GatewayEvent::PermissionResolved {
                request_id,
                decision: permission_decision_from_runtime(&decision),
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
    pub workdir: PathBuf,
    pub command: String,
    pub context: UserShellContextOptions,
    pub stream: Option<RunStreamSink>,
    pub event_sink: Option<GatewayEventSink>,
    pub lineage: Option<Value>,
}

impl fmt::Debug for SendShellRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SendShellRequest")
            .field("thread_id", &self.thread_id)
            .field("source", &self.source)
            .field("bind_source", &self.bind_source)
            .field("workdir", &self.workdir)
            .field("command", &self.command)
            .field("context", &self.context)
            .field("has_stream", &self.stream.is_some())
            .field("has_event_sink", &self.event_sink.is_some())
            .field("lineage", &self.lineage)
            .finish()
    }
}
