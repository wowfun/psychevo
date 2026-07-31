#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayActivity {
    pub activities: Vec<ThreadActivityView>,
    pub framework_revision: Option<String>,
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
    Shell,
}

enum ShellStartState {
    Standalone,
    Queued {
        receiver: oneshot::Receiver<psychevo::Result<GatewayShellResult>>,
        active_activity_id: Option<String>,
        queue_position: usize,
    },
}

#[derive(Debug)]
enum PendingQueuedActivity {
    Shell(Box<PendingQueuedShell>),
}

#[derive(Debug)]
struct PendingQueuedShell {
    shell_id: String,
    request: SendShellRequest,
    responder: oneshot::Sender<psychevo::Result<GatewayShellResult>>,
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
    pub prompt_display: Option<psychevo::__product::runtime::PromptDisplayMetadata>,
    pub max_context_messages: Option<usize>,
    pub config_path: Option<PathBuf>,
    pub project_context_override: Option<psychevo::__product::runtime::ProjectContextInstructionMode>,
    pub sandbox_override: Option<psychevo::__product::runtime::RunSandboxOverride>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub runtime_profile_ref: Option<String>,
    pub control_values: BTreeMap<String, String>,
    pub initial_thread_preferences: BTreeMap<String, String>,
    pub include_reasoning: bool,
    pub mode: RunMode,
    pub permission_mode: Option<PermissionMode>,
    pub approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub clarify_enabled: bool,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub agent_ref: Option<String>,
    pub no_agents: bool,
    pub no_skills: bool,
    pub selected_capability_roots: Vec<psychevo::__product::capabilities::SelectedCapabilityRoot>,
    pub skill_inputs: Vec<String>,
    pub mcp_servers: Vec<psychevo::__product::runtime::McpServerInput>,
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
    runtime_tools: Vec<psychevo::__product::runtime::RuntimeTool>,
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
        tools: Vec<psychevo::__product::runtime::RuntimeTool>,
    ) {
        self.runtime_tools = tools;
    }

    pub(crate) fn extend_runtime_tools(
        &mut self,
        tools: impl IntoIterator<Item = psychevo::__product::runtime::RuntimeTool>,
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
        let mut request = psychevo::TurnRequest::new(prompt)
            .with_prompt_images(image_inputs, policy.extract_prompt_image_sources)
            .with_prompt_display(policy.prompt_display.or(Some(prompt_display)))
            .with_identity(caller.runtime_source, self.client_turn_id)
            .with_model(policy.model, policy.reasoning_effort)
            .with_runtime(policy.runtime_profile_ref, policy.control_values)
            .with_reasoning_output(policy.include_reasoning)
            .with_execution_policy(policy.mode, policy.permission_mode, policy.config_path)
            .with_approval(policy.approval_handler, policy.clarify_enabled)
            .with_environment(
                policy.inherited_env,
                policy.project_context_override,
                policy.sandbox_override,
            )
            .with_agent(policy.agent_ref, policy.no_agents, policy.no_skills)
            .with_skills(policy.skill_inputs)
            .with_mcp_servers(policy.mcp_servers);
        request.__set_runtime_tools(std::mem::take(&mut caller.runtime_tools));
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
            agent_entrypoint: None,
            mcp_runtime: None,
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
            psychevo::__product::runtime::SessionEventPayload::BlockingActionRequested { .. }
                | psychevo::__product::runtime::SessionEventPayload::BlockingActionUpdated { .. }
                | psychevo::__product::runtime::SessionEventPayload::BlockingActionResolved { .. }
                | psychevo::__product::runtime::SessionEventPayload::BlockingActionCancelled { .. }
        ),
        RunStreamEvent::Scoped { event, .. } => {
            framework_lifecycle_owns_run_stream_interaction(event)
        }
        RunStreamEvent::AssistantTextDelta { .. }
        | RunStreamEvent::ReasoningDelta { .. }
        | RunStreamEvent::ReasoningEnd => false,
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
        psychevo::TurnEvent::ActivityChanged {
            thread_id,
            activity,
        } => Some(GatewayEvent::ActivityChanged {
            thread_id: Some(thread_id),
            activity: GatewayActivityView {
                framework_revision: Some(activity.revision.to_string()),
                running: activity.running,
                active_turn_id: activity.active_turn_id,
                queued_turns: activity.queued_turns,
                ..GatewayActivityView::default()
            },
        }),
        psychevo::TurnEvent::Accepted {
            receipt,
            queue_position: Some(queue_position),
        } => Some(GatewayEvent::TurnQueued {
            thread_id: Some(receipt.thread_id),
            turn_id: receipt.turn_id,
            queue_position,
        }),
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
    fn framework_queued_acceptance_projects_public_turn_queued() {
        let event = gateway_event_from_framework_turn(
            psychevo::TurnEvent::Accepted {
                receipt: psychevo::TurnReceipt {
                    accepted: true,
                    thread_id: "thread-1".to_string(),
                    turn_id: "turn-2".to_string(),
                    client_turn_id: Some("client-turn-2".to_string()),
                },
                queue_position: Some(1),
            },
            "thread-1",
            Some("turn-2"),
        )
        .expect("queued acceptance projection");

        assert!(matches!(
            event,
            GatewayEvent::TurnQueued {
                thread_id: Some(thread_id),
                turn_id,
                queue_position: 1,
            } if thread_id == "thread-1" && turn_id == "turn-2"
        ));
    }

    #[test]
    fn framework_activity_projects_complete_revisioned_state() {
        let event = gateway_event_from_framework_turn(
            psychevo::TurnEvent::ActivityChanged {
                thread_id: "thread-1".to_string(),
                activity: psychevo::ThreadActivitySnapshot {
                    revision: 42,
                    running: true,
                    active_turn_id: Some("turn-1".to_string()),
                    queued_turns: 2,
                },
            },
            "thread-1",
            Some("turn-1"),
        )
        .expect("activity projection");

        assert!(matches!(
            event,
            GatewayEvent::ActivityChanged {
                thread_id: Some(thread_id),
                activity: GatewayActivityView {
                    framework_revision: Some(revision),
                    running: true,
                    active_turn_id: Some(turn_id),
                    queued_turns: 2,
                    ..
                },
            } if thread_id == "thread-1" && turn_id == "turn-1" && revision == "42"
        ));
    }

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
