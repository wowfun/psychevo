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
    Queued {
        receiver: oneshot::Receiver<psychevo::Result<GatewayShellResult>>,
        active_activity_id: Option<String>,
        queue_position: usize,
    },
}

#[derive(Debug)]
enum PendingQueuedActivity {
    #[cfg(test)]
    Turn(Box<PendingQueuedTurn>),
    Shell(Box<PendingQueuedShell>),
    Compact(Box<PendingQueuedCompact>),
}

#[cfg(test)]
#[derive(Debug)]
struct PendingQueuedTurn {
    turn_id: String,
    request: SendTurnRequest,
    responder: oneshot::Sender<psychevo::Result<GatewayTurnResult>>,
}

#[derive(Debug)]
struct PendingQueuedShell {
    shell_id: String,
    request: SendShellRequest,
    responder: oneshot::Sender<psychevo::Result<GatewayShellResult>>,
}

#[derive(Debug)]
struct PendingQueuedCompact {
    _admission: supervisor::GatewayActivityPermit,
    compact_id: String,
    request: SendCompactRequest,
    responder: oneshot::Sender<psychevo::Result<psychevo::compaction::CompactionResult>>,
}

type PendingPermissionMap = Arc<Mutex<HashMap<String, PendingPermission>>>;

struct PendingPermission {
    selector_key: Option<String>,
    responder: oneshot::Sender<PermissionApprovalDecision>,
}

#[cfg(test)]
#[derive(Clone, Default)]
struct GatewayPendingActionContext {
    thread_id: Option<String>,
    turn_id: Option<String>,
    activity_id: Option<String>,
    source_key: Option<String>,
    owner_id: Option<String>,
}

#[cfg(test)]
#[derive(Clone)]
struct GatewayApprovalHandler {
    selector_key: Option<String>,
    pending_permissions: PendingPermissionMap,
    event_sink: GatewayEventEmitter,
    action_context: GatewayPendingActionContext,
    timeout_secs: u64,
    session_authorization_lifetime: Option<&'static str>,
}

#[cfg(test)]
impl GatewayApprovalHandler {
    fn new(
        selector_key: Option<String>,
        pending_permissions: PendingPermissionMap,
        event_sink: GatewayEventEmitter,
        action_context: GatewayPendingActionContext,
        session_authorization_lifetime: Option<&'static str>,
    ) -> Self {
        Self {
            selector_key,
            pending_permissions,
            event_sink,
            action_context,
            timeout_secs: 300,
            session_authorization_lifetime,
        }
    }
}

#[cfg(test)]
impl fmt::Debug for GatewayApprovalHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayApprovalHandler")
            .field("selector_key", &self.selector_key)
            .field("timeout_secs", &self.timeout_secs)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
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
        let action_context = self.action_context.clone();
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
            let _ = event_sink.emit(GatewayEvent::ActionRequested {
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
                    thread_id: action_context.thread_id,
                    turn_id: action_context.turn_id,
                    activity_id: action_context.activity_id,
                    source_key: action_context.source_key,
                    owner_id: action_context.owner_id,
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
            let _ = event_sink.emit(GatewayEvent::ActionResolved {
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

/// Caller-visible execution preferences. Runtime state, Adapter delegates, and
/// queue envelopes never cross the Thread Application Interface.
#[derive(Clone)]
pub struct ThreadTurnPolicy {
    pub snapshot_root: Option<PathBuf>,
    pub continue_latest: bool,
    pub extract_prompt_image_sources: bool,
    pub prompt_display: Option<psychevo::types::PromptDisplayMetadata>,
    pub max_context_messages: Option<usize>,
    pub config_path: Option<PathBuf>,
    pub project_context_override: Option<psychevo::types::ProjectContextInstructionMode>,
    pub sandbox_override: Option<psychevo::types::RunSandboxOverride>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub runtime_profile_ref: Option<String>,
    pub control_values: BTreeMap<String, String>,
    pub initial_thread_preferences: BTreeMap<String, String>,
    pub include_reasoning: bool,
    pub mode: RunMode,
    pub permission_mode: Option<PermissionMode>,
    pub approval_mode: Option<psychevo::types::ApprovalMode>,
    pub approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub clarify_enabled: bool,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub agent_ref: Option<String>,
    pub no_agents: bool,
    pub no_skills: bool,
    pub selected_capability_roots: Vec<psychevo::extensions::SelectedCapabilityRoot>,
    pub skill_inputs: Vec<String>,
    pub mcp_servers: Vec<psychevo::types::McpServerInput>,
}

impl fmt::Debug for ThreadTurnPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThreadTurnPolicy")
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

impl Default for ThreadTurnPolicy {
    fn default() -> Self {
        Self {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreadSurface {
    Cli,
    Tui,
    InboundAcp,
    Web,
    Channel,
    Automation,
    Other(String),
}

/// Immutable host facts and observation/control ports supplied by one caller.
/// Callback construction is owned here so Adapters do not assemble Gateway
/// event emitters or lower queue requests.
pub struct ThreadCallerContext {
    pub surface: ThreadSurface,
    pub cwd: PathBuf,
    pub runtime_source: String,
    pub continue_sources: Vec<String>,
    stream_observer: Option<RunStreamSink>,
    event_observer: Option<GatewayEventEmitter>,
    workspace_mutations: Option<WorkspaceMutationSink>,
    control_handle: Option<RunControlHandle>,
    control: Option<RunControl>,
    runtime_tools: Vec<psychevo::types::RuntimeTool>,
}

impl ThreadCallerContext {
    pub fn new(surface: ThreadSurface, cwd: PathBuf) -> Self {
        Self {
            surface,
            cwd,
            runtime_source: "gateway".to_string(),
            continue_sources: Vec::new(),
            stream_observer: None,
            event_observer: None,
            workspace_mutations: None,
            control_handle: None,
            control: None,
            runtime_tools: Vec::new(),
        }
    }

    pub fn observe_runtime_events(
        &mut self,
        observer: impl Fn(RunStreamEvent) + Send + Sync + 'static,
    ) {
        self.stream_observer = Some(Arc::new(observer));
    }

    pub fn observe_gateway_events(
        &mut self,
        observer: impl Fn(GatewayEvent) + Send + Sync + 'static,
    ) {
        self.event_observer = Some(GatewayEventEmitter::new(observer));
    }

    pub fn set_control(&mut self, handle: RunControlHandle, control: RunControl) {
        self.control_handle = Some(handle);
        self.control = Some(control);
    }

    pub(crate) fn set_workspace_mutations(&mut self, sink: WorkspaceMutationSink) {
        self.workspace_mutations = Some(sink);
    }

    pub(crate) fn set_event_observer(&mut self, observer: GatewayEventEmitter) {
        self.event_observer = Some(observer);
    }

    pub(crate) fn set_runtime_tools(
        &mut self,
        tools: Vec<psychevo::types::RuntimeTool>,
    ) {
        self.runtime_tools = tools;
    }

    pub(crate) fn extend_runtime_tools(
        &mut self,
        tools: impl IntoIterator<Item = psychevo::types::RuntimeTool>,
    ) {
        self.runtime_tools.extend(tools);
    }
}

impl fmt::Debug for ThreadCallerContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThreadCallerContext")
            .field("surface", &self.surface)
            .field("cwd", &self.cwd)
            .field("runtime_source", &self.runtime_source)
            .field("continue_sources", &self.continue_sources)
            .field("has_runtime_observer", &self.stream_observer.is_some())
            .field("has_gateway_observer", &self.event_observer.is_some())
            .field("runtime_tool_count", &self.runtime_tools.len())
            .finish_non_exhaustive()
    }
}

/// Caller intent for one accepted turn.
pub struct ThreadTurnIntent {
    pub thread_id: Option<String>,
    pub source: Option<GatewaySource>,
    pub bind_source: Option<GatewaySource>,
    pub reset_source_binding: bool,
    pub input: Vec<GatewayInputPart>,
    pub policy: ThreadTurnPolicy,
    pub lineage: Option<Value>,
    pub client_turn_id: Option<String>,
    pub turn_id: Option<String>,
}

impl ThreadTurnIntent {
    pub fn new(input: Vec<GatewayInputPart>) -> Self {
        Self {
            thread_id: None,
            source: None,
            bind_source: None,
            reset_source_binding: false,
            input,
            policy: ThreadTurnPolicy::default(),
            lineage: None,
            client_turn_id: None,
            turn_id: None,
        }
    }

    #[cfg(test)]
    fn into_queue_request(
        self,
        caller: ThreadCallerContext,
        state: StateRuntime,
        explicit_thread: bool,
    ) -> SendTurnRequest {
        let policy = self.policy;
        SendTurnRequest {
            thread_id: self.thread_id.clone(),
            explicit_thread,
            source: self.source,
            bind_source: self.bind_source,
            reset_source_binding: self.reset_source_binding,
            input: self.input,
            initial_thread_preferences: policy.initial_thread_preferences,
            options: RunOptions {
                state,
                cwd: caller.cwd,
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
                workspace_mutations: caller.workspace_mutations,
                runtime_tools: caller.runtime_tools,
            },
            runtime_source: Some(caller.runtime_source),
            continue_sources: caller.continue_sources,
            stream: caller.stream_observer,
            event_sink: caller.event_observer,
            control_handle: caller.control_handle,
            control: caller.control,
            lineage: self.lineage,
        }
    }

    pub(crate) fn into_framework_request(
        self,
        mut caller: ThreadCallerContext,
    ) -> psychevo::Result<FrameworkTurnSubmission> {
        let prepared_source_key = self.source.as_ref().map(|source| source.source_key().0);
        let thread_id = self.thread_id.ok_or_else(|| {
            Error::Message("Framework Turn submission requires a materialized Thread".to_string())
        })?;
        let (prompt, image_inputs, prompt_display, _) = framework_input_parts(&self.input)?;
        let input_parts = self
            .input
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()?;
        let turn_id = self.turn_id.clone();
        let event_sink = caller.event_observer.clone();
        let turn_event_observer = event_sink.clone().map(|event_sink| {
            let thread_id = thread_id.clone();
            let turn_id = turn_id.clone();
            Arc::new(move |event: psychevo::TurnEvent| {
                if let Some(event) =
                    gateway_event_from_framework_turn(event, &thread_id, turn_id.as_deref())
                {
                    emit_framework_gateway_event(
                        &event_sink,
                        &thread_id,
                        turn_id.as_deref(),
                        event,
                    );
                }
            }) as Arc<dyn Fn(psychevo::TurnEvent) + Send + Sync>
        });
        let run_stream_observer = match (caller.stream_observer.take(), event_sink.clone()) {
            (observer, Some(event_sink)) => {
                let turn_id = turn_id.clone().ok_or_else(|| {
                    Error::Message(
                        "Gateway Framework projection requires an accepted Turn identity"
                            .to_string(),
                    )
                })?;
                Some(framework_run_stream_observer(
                    turn_id,
                    thread_id.clone(),
                    observer,
                    event_sink,
                ))
            }
            (observer, None) => observer,
        };
        let policy = self.policy;
        let mut request = psychevo::TurnRequest::new(prompt);
        request.image_inputs = image_inputs;
        request.extract_prompt_image_sources = policy.extract_prompt_image_sources;
        request.prompt_display = policy.prompt_display.or(Some(prompt_display));
        request.client_turn_id = self.client_turn_id;
        request.source = caller.runtime_source;
        request.config_path = policy.config_path;
        request.model = policy.model;
        request.reasoning_effort = policy.reasoning_effort;
        request.runtime_ref = policy.runtime_profile_ref;
        request.runtime_options = policy.control_values;
        request.include_reasoning = policy.include_reasoning;
        request.mode = policy.mode;
        request.permission_mode = policy.permission_mode;
        request.approval_mode = policy.approval_mode;
        request.approval_handler = policy.approval_handler;
        request.clarify_enabled = policy.clarify_enabled;
        request.inherited_env = policy.inherited_env;
        request.project_context = policy.project_context_override;
        request.sandbox = policy.sandbox_override;
        request.agent = policy.agent_ref;
        request.no_agents = policy.no_agents;
        request.no_skills = policy.no_skills;
        request.skill_inputs = policy.skill_inputs;
        request.mcp_servers = policy.mcp_servers;
        request.tools = std::mem::take(&mut caller.runtime_tools);
        request.__set_adapter_options(psychevo::AdapterTurnOptions {
            snapshot_root: policy.snapshot_root,
            max_context_messages: policy.max_context_messages,
            selected_capability_roots: policy.selected_capability_roots,
            workspace_mutations: caller.workspace_mutations.take(),
            input_parts,
            run_stream_observer,
            initial_thread_preferences: policy.initial_thread_preferences,
            prepared_source_key,
            turn_event_observer,
        });
        if let Some(turn_id) = self.turn_id {
            request.__set_turn_id(turn_id);
        }
        match (caller.control_handle.take(), caller.control.take()) {
            (Some(handle), Some(control)) => request.__set_control(handle, control),
            (None, None) => {}
            _ => {
                return Err(Error::Message(
                    "Framework Turn control must provide both handle and receiver".to_string(),
                ));
            }
        }
        Ok(FrameworkTurnSubmission {
            thread_id,
            request,
        })
    }
}

fn framework_run_stream_observer(
    turn_id: String,
    thread_id: String,
    observer: Option<RunStreamSink>,
    event_sink: GatewayEventEmitter,
) -> RunStreamSink {
    let projector = Arc::new(Mutex::new(GatewayLiveProjector::new(Some(
        thread_id.clone(),
    ))));
    Arc::new(move |event: RunStreamEvent| {
        if let Some(observer) = observer.as_ref() {
            observer(event.clone());
        }
        if framework_lifecycle_owns_run_stream_interaction(&event) {
            return;
        }
        if let Some(projected) = projector
            .lock()
            .expect("Gateway Framework live projector poisoned")
            .project(&turn_id, &event)
            && !matches!(
                projected,
                GatewayEvent::TurnStarted { .. } | GatewayEvent::TurnCompleted { .. }
            )
        {
            emit_framework_gateway_event(&event_sink, &thread_id, Some(&turn_id), projected);
        }
    })
}

fn framework_lifecycle_owns_run_stream_interaction(event: &RunStreamEvent) -> bool {
    match event {
        RunStreamEvent::ClarifyRequest(_) | RunStreamEvent::ClarifyResolved(_) => true,
        RunStreamEvent::Event(event) => matches!(
            &event.payload,
            psychevo::types::SessionEventPayload::BlockingActionRequested { .. }
                | psychevo::types::SessionEventPayload::BlockingActionUpdated { .. }
                | psychevo::types::SessionEventPayload::BlockingActionResolved { .. }
                | psychevo::types::SessionEventPayload::BlockingActionCancelled { .. }
        ),
        RunStreamEvent::Scoped { event, .. } => {
            framework_lifecycle_owns_run_stream_interaction(event)
        }
        RunStreamEvent::ReasoningDelta { .. } | RunStreamEvent::ReasoningEnd => false,
    }
}

fn emit_framework_gateway_event(
    event_sink: &GatewayEventEmitter,
    thread_id: &str,
    turn_id: Option<&str>,
    event: GatewayEvent,
) {
    let fields = journey_profile::gateway_profile_event_fields(&event);
    gateway_profile_mark(
        "gateway_event_emitted",
        turn_id,
        Some(thread_id),
        fields,
    );
    if matches!(&event, GatewayEvent::TurnCompleted { .. }) {
        gateway_profile_mark(
            "gateway_turn_completed",
            turn_id,
            Some(thread_id),
            GatewayProfileFields::default(),
        );
    }
    let _ = event_sink.emit(event);
}

fn gateway_event_from_framework_turn(
    event: psychevo::TurnEvent,
    fallback_thread_id: &str,
    fallback_turn_id: Option<&str>,
) -> Option<GatewayEvent> {
    match event {
        psychevo::TurnEvent::Started { thread_id, turn_id } => {
            Some(GatewayEvent::TurnStarted {
                thread_id: Some(thread_id),
                turn_id,
                selected_skills: Vec::new(),
            })
        }
        psychevo::TurnEvent::Completed {
            thread_id,
            turn_id,
            outcome,
        } => {
            let (status, outcome) = match outcome {
                psychevo::TurnOutcome::Completed => (GatewayTurnStatus::Completed, "normal"),
                psychevo::TurnOutcome::Stopped => (GatewayTurnStatus::Interrupted, "stopped"),
                psychevo::TurnOutcome::Failed => (GatewayTurnStatus::Failed, "failed"),
                psychevo::TurnOutcome::Interrupted => {
                    (GatewayTurnStatus::Interrupted, "aborted")
                }
            };
            let turn = GatewayTurn {
                id: turn_id.clone(),
                thread_id: Some(thread_id.clone()),
                status,
                outcome: Some(outcome.to_string()),
                error: None,
                started_at_ms: None,
                completed_at_ms: Some(gateway_now_ms()),
            };
            Some(GatewayEvent::TurnCompleted {
                thread_id: Some(thread_id),
                turn_id,
                turn,
                committed_entries: Vec::new(),
            })
        }
        psychevo::TurnEvent::Failed {
            thread_id,
            turn_id,
            message,
        } => {
            let turn = GatewayTurn {
                id: turn_id.clone(),
                thread_id: Some(thread_id.clone()),
                status: GatewayTurnStatus::Failed,
                outcome: Some("failed".to_string()),
                error: Some(gateway_turn_error(&message, None)),
                started_at_ms: None,
                completed_at_ms: Some(gateway_now_ms()),
            };
            Some(GatewayEvent::TurnCompleted {
                thread_id: Some(thread_id),
                turn_id,
                turn,
                committed_entries: Vec::new(),
            })
        }
        psychevo::TurnEvent::InteractionRequested {
            interaction_id,
            kind,
            payload,
        } => {
            let kind = match kind.as_str() {
                "permission" => GatewayActionKind::Permission,
                "clarify" | "user_input" => GatewayActionKind::Clarify,
                _ => return None,
            };
            let title = payload
                .get("toolName")
                .or_else(|| payload.get("title"))
                .or_else(|| payload.get("message"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let summary = payload
                .get("summary")
                .or_else(|| payload.get("reason"))
                .or_else(|| payload.get("message"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            Some(GatewayEvent::ActionRequested {
                action: PendingActionView {
                    action_id: interaction_id,
                    kind,
                    title,
                    summary,
                    payload,
                    thread_id: Some(fallback_thread_id.to_string()),
                    turn_id: fallback_turn_id.map(ToString::to_string),
                    activity_id: None,
                    source_key: None,
                    owner_id: None,
                    lease_expires_at_ms: None,
                },
            })
        }
        psychevo::TurnEvent::InteractionResolved {
            interaction_id,
            kind,
            reason,
        } => {
            let kind = match kind.as_str() {
                "permission" => GatewayActionKind::Permission,
                "clarify" | "user_input" => GatewayActionKind::Clarify,
                _ => return None,
            };
            Some(GatewayEvent::ActionResolved {
                action_id: interaction_id,
                kind,
                outcome: match reason.as_str() {
                    "deny" | "rejected" => GatewayActionOutcome::Rejected,
                    "cancelled" | "timed_out" | "turn_finished" => {
                        GatewayActionOutcome::Cancelled
                    }
                    _ => GatewayActionOutcome::Accepted,
                },
                payload: json!({ "reason": reason }),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod framework_projection_tests {
    use super::*;

    #[test]
    fn framework_stream_keeps_stateful_acp_plan_projection() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        let stream = framework_run_stream_observer(
            "turn-1".to_string(),
            "thread-1".to_string(),
            None,
            GatewayEventEmitter::new(move |event| {
                observed
                    .lock()
                    .expect("observed Gateway events poisoned")
                    .push(event);
            }),
        );

        stream(RunStreamEvent::value(json!({
            "type": "acp_peer_plan",
            "body": "- [~] Project through the common application path",
            "plan": {
                "sessionUpdate": "plan",
                "entries": [{
                    "content": "Project through the common application path",
                    "priority": "high",
                    "status": "in_progress"
                }]
            }
        })));

        let events = events.lock().expect("observed Gateway events poisoned");
        let entry = events
            .iter()
            .find_map(|event| match event {
                GatewayEvent::EntryStarted { entry, .. }
                | GatewayEvent::EntryUpdated { entry, .. }
                | GatewayEvent::EntryCompleted { entry, .. } => Some(entry),
                _ => None,
            })
            .expect("ACP plan must be live-projected");
        assert_eq!(entry.thread_id, "thread-1");
        assert!(entry.blocks.iter().any(|block| {
            block.title.as_deref() == Some("Plan")
                && block
                    .body
                    .as_deref()
                    .is_some_and(|body| body.contains("common application path"))
        }));
    }

    #[test]
    fn framework_stream_does_not_duplicate_application_owned_interactions() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        let stream = framework_run_stream_observer(
            "turn-1".to_string(),
            "thread-1".to_string(),
            None,
            GatewayEventEmitter::new(move |event| {
                observed
                    .lock()
                    .expect("observed Gateway events poisoned")
                    .push(event);
            }),
        );

        stream(RunStreamEvent::value(json!({
            "type": "action_requested",
            "action_id": "clarify-1",
            "kind": "clarify",
            "payload": {
                "questions": [{
                    "question": "Which workspace?"
                }]
            }
        })));

        assert!(
            events
                .lock()
                .expect("observed Gateway events poisoned")
                .is_empty(),
            "Application TurnEvent projection owns the public interaction lifecycle"
        );
    }
}

pub(crate) struct FrameworkTurnSubmission {
    pub(crate) thread_id: String,
    pub(crate) request: psychevo::TurnRequest,
}

impl fmt::Debug for ThreadTurnIntent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThreadTurnIntent")
            .field("thread_id", &self.thread_id)
            .field("source", &self.source)
            .field("bind_source", &self.bind_source)
            .field("reset_source_binding", &self.reset_source_binding)
            .field("input", &self.input)
            .field("policy", &self.policy)
            .field("client_turn_id", &self.client_turn_id)
            .field("turn_id", &self.turn_id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedTurnReceipt {
    pub accepted: bool,
    pub thread_id: String,
    pub turn_id: String,
    pub client_turn_id: Option<String>,
}

pub struct AcceptedTurn {
    pub receipt: AcceptedTurnReceipt,
    completion: AcceptedTurnCompletion,
}

impl AcceptedTurn {
    pub fn into_completion(self) -> AcceptedTurnCompletion {
        self.completion
    }

    pub async fn wait(self) -> psychevo::Result<GatewayTurnResult> {
        self.completion.wait().await
    }
}

impl fmt::Debug for AcceptedTurn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcceptedTurn")
            .field("receipt", &self.receipt)
            .finish_non_exhaustive()
    }
}

pub struct AcceptedTurnCompletion {
    receiver: oneshot::Receiver<psychevo::Result<GatewayTurnResult>>,
}

impl AcceptedTurnCompletion {
    pub async fn wait(self) -> psychevo::Result<GatewayTurnResult> {
        self.receiver.await.map_err(|_| {
            Error::Message(
                "accepted turn ended without publishing a completion result".to_string(),
            )
        })?
    }
}

impl fmt::Debug for AcceptedTurnCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcceptedTurnCompletion")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
pub(crate) struct SendTurnRequest {
    pub thread_id: Option<String>,
    pub explicit_thread: bool,
    pub source: Option<GatewaySource>,
    pub bind_source: Option<GatewaySource>,
    pub reset_source_binding: bool,
    pub input: Vec<GatewayInputPart>,
    pub initial_thread_preferences: BTreeMap<String, String>,
    pub options: RunOptions,
    pub runtime_source: Option<String>,
    pub continue_sources: Vec<String>,
    pub stream: Option<RunStreamSink>,
    pub event_sink: Option<GatewayEventEmitter>,
    pub control_handle: Option<RunControlHandle>,
    pub control: Option<RunControl>,
    pub lineage: Option<Value>,
}

#[cfg(test)]
impl fmt::Debug for SendTurnRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SendTurnRequest")
            .field("thread_id", &self.thread_id)
            .field("explicit_thread", &self.explicit_thread)
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
    pub event_sink: Option<GatewayEventEmitter>,
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
    pub reason: psychevo::compaction::CompactionReason,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub event_sink: Option<GatewayEventEmitter>,
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
