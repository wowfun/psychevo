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
    responder: oneshot::Sender<psychevo_runtime::Result<psychevo_runtime::compaction::CompactionResult>>,
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
            let filesystem = request.filesystem.clone();
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
                        "filesystem": filesystem,
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
                    "filesystemScope": decision.filesystem_scope,
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

/// Caller intent for one Thread Application turn.
///
/// Runtime state, public-thread session lowering, Adapter delegates, resident
/// session ids, and the active-queue envelope deliberately do not cross this
/// Interface.
#[derive(Clone)]
pub struct ThreadTurnPolicy {
    pub cwd: PathBuf,
    pub snapshot_root: Option<PathBuf>,
    pub continue_latest: bool,
    pub extract_prompt_image_sources: bool,
    pub prompt_display: Option<psychevo_runtime::types::PromptDisplayMetadata>,
    pub max_context_messages: Option<usize>,
    pub config_path: Option<PathBuf>,
    pub project_context_override: Option<psychevo_runtime::types::ProjectContextInstructionMode>,
    pub sandbox_override: Option<psychevo_runtime::types::RunSandboxOverride>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub runtime_profile_ref: Option<String>,
    pub control_values: BTreeMap<String, String>,
    pub initial_thread_preferences: BTreeMap<String, String>,
    pub include_reasoning: bool,
    pub mode: RunMode,
    pub permission_mode: Option<PermissionMode>,
    pub approval_mode: Option<psychevo_runtime::types::ApprovalMode>,
    pub approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub clarify_enabled: bool,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub agent_ref: Option<String>,
    pub no_agents: bool,
    pub no_skills: bool,
    pub selected_capability_roots: Vec<psychevo_runtime::extensions::SelectedCapabilityRoot>,
    pub skill_inputs: Vec<String>,
    pub mcp_servers: Vec<psychevo_runtime::types::McpServerInput>,
}

impl fmt::Debug for ThreadTurnPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThreadTurnPolicy")
            .field("cwd", &self.cwd)
            .field("snapshot_root", &self.snapshot_root)
            .field("continue_latest", &self.continue_latest)
            .field("config_path", &self.config_path)
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("runtime_profile_ref", &self.runtime_profile_ref)
            .field(
                "control_ids",
                &self.control_values.keys().collect::<Vec<_>>(),
            )
            .field(
                "initial_thread_preference_ids",
                &self.initial_thread_preferences.keys().collect::<Vec<_>>(),
            )
            .field("mode", &self.mode)
            .field("permission_mode", &self.permission_mode)
            .field("agent_ref", &self.agent_ref)
            .field(
                "inherited_env_count",
                &self.inherited_env.as_ref().map(BTreeMap::len),
            )
            .field("skill_input_count", &self.skill_inputs.len())
            .field("mcp_server_count", &self.mcp_servers.len())
            .finish_non_exhaustive()
    }
}

impl ThreadTurnPolicy {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            snapshot_root: None,
            continue_latest: false,
            extract_prompt_image_sources: false,
            prompt_display: None,
            max_context_messages: None,
            config_path: None,
            project_context_override: None,
            sandbox_override: None,
            model: None,
            reasoning_effort: None,
            runtime_profile_ref: None,
            control_values: BTreeMap::new(),
            initial_thread_preferences: BTreeMap::new(),
            include_reasoning: false,
            mode: RunMode::Default,
            permission_mode: None,
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            inherited_env: None,
            agent_ref: None,
            no_agents: false,
            no_skills: false,
            selected_capability_roots: Vec::new(),
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
        }
    }
}

pub struct ThreadTurnRequest {
    pub thread_id: Option<String>,
    pub source: Option<GatewaySource>,
    pub bind_source: Option<GatewaySource>,
    pub reset_source_binding: bool,
    pub input: Vec<GatewayInputPart>,
    pub policy: ThreadTurnPolicy,
    pub runtime_source: Option<String>,
    pub continue_sources: Vec<String>,
    pub stream: Option<RunStreamSink>,
    pub event_sink: Option<GatewayEventSink>,
    pub(crate) workspace_mutations: Option<WorkspaceMutationSink>,
    pub control_handle: Option<RunControlHandle>,
    pub control: Option<RunControl>,
    pub lineage: Option<Value>,
    /// A transport may reserve the public turn id before dispatch so early
    /// events and the response correlate to one identity.
    pub turn_id: Option<String>,
    runtime_tools: Vec<psychevo_runtime::types::RuntimeTool>,
}

impl ThreadTurnRequest {
    pub fn new(cwd: PathBuf, input: Vec<GatewayInputPart>) -> Self {
        Self {
            thread_id: None,
            source: None,
            bind_source: None,
            reset_source_binding: false,
            input,
            policy: ThreadTurnPolicy::new(cwd),
            runtime_source: None,
            continue_sources: Vec::new(),
            stream: None,
            event_sink: None,
            workspace_mutations: None,
            control_handle: None,
            control: None,
            lineage: None,
            turn_id: None,
            runtime_tools: Vec::new(),
        }
    }

    pub(crate) fn set_runtime_tools(&mut self, tools: Vec<psychevo_runtime::types::RuntimeTool>) {
        self.runtime_tools = tools;
    }

    pub(crate) fn extend_runtime_tools(
        &mut self,
        tools: impl IntoIterator<Item = psychevo_runtime::types::RuntimeTool>,
    ) {
        self.runtime_tools.extend(tools);
    }

    fn into_queue_request(self, state: StateRuntime) -> SendTurnRequest {
        let policy = self.policy;
        SendTurnRequest {
            thread_id: self.thread_id.clone(),
            source: self.source,
            bind_source: self.bind_source,
            reset_source_binding: self.reset_source_binding,
            input: self.input,
            initial_thread_preferences: policy.initial_thread_preferences,
            options: RunOptions {
                state,
                cwd: policy.cwd,
                snapshot_root: policy.snapshot_root,
                session: self.thread_id,
                continue_latest: policy.continue_latest,
                prompt: String::new(),
                image_inputs: Vec::new(),
                extract_prompt_image_sources: policy.extract_prompt_image_sources,
                prompt_display: policy.prompt_display,
                max_context_messages: policy.max_context_messages,
                config_path: policy.config_path,
                project_context_override: policy.project_context_override,
                sandbox_override: policy.sandbox_override,
                model: policy.model,
                reasoning_effort: policy.reasoning_effort,
                runtime_ref: policy.runtime_profile_ref,
                runtime_session_id: None,
                runtime_options: policy.control_values,
                include_reasoning: policy.include_reasoning,
                mode: policy.mode,
                permission_mode: policy.permission_mode,
                approval_mode: policy.approval_mode,
                approval_handler: policy.approval_handler,
                clarify_enabled: policy.clarify_enabled,
                inherited_env: policy.inherited_env,
                agent: policy.agent_ref,
                external_agent_delegate: None,
                no_agents: policy.no_agents,
                no_skills: policy.no_skills,
                selected_capability_roots: policy.selected_capability_roots,
                skill_inputs: policy.skill_inputs,
                mcp_servers: policy.mcp_servers,
                workspace_mutations: self.workspace_mutations,
                runtime_tools: self.runtime_tools,
            },
            runtime_source: self.runtime_source,
            continue_sources: self.continue_sources,
            stream: self.stream,
            event_sink: self.event_sink,
            control_handle: self.control_handle,
            control: self.control,
            lineage: self.lineage,
        }
    }
}

impl fmt::Debug for ThreadTurnRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThreadTurnRequest")
            .field("thread_id", &self.thread_id)
            .field("source", &self.source)
            .field("bind_source", &self.bind_source)
            .field("reset_source_binding", &self.reset_source_binding)
            .field("input", &self.input)
            .field("policy", &self.policy)
            .field("runtime_source", &self.runtime_source)
            .field("continue_sources", &self.continue_sources)
            .field("turn_id", &self.turn_id)
            .field("runtime_tool_count", &self.runtime_tools.len())
            .finish_non_exhaustive()
    }
}

pub(crate) struct SendTurnRequest {
    pub thread_id: Option<String>,
    pub source: Option<GatewaySource>,
    pub bind_source: Option<GatewaySource>,
    pub reset_source_binding: bool,
    pub input: Vec<GatewayInputPart>,
    pub initial_thread_preferences: BTreeMap<String, String>,
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
            .field(
                "initial_thread_preference_ids",
                &self.initial_thread_preferences.keys().collect::<Vec<_>>(),
            )
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
    pub reason: psychevo_runtime::compaction::CompactionReason,
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
