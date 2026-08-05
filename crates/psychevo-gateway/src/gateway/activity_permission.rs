use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use psychevo::application::WorkspaceMutationSink;
use psychevo::{ApprovalHandler, Error, ImageInput, PermissionMode, RunMode, ShellCommandControl};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::Gateway;
use super::results::GatewayShellResult;
use super::stream_input::framework_input_parts;
use super::supervisor::GatewayActivityPermit;
use super::turn_shell::gateway_turn_error;
use crate::journey_profile::{self, GatewayProfileFields, gateway_profile_mark};
use crate::projection::GatewayLiveProjector;
use crate::{GatewayEventEmitter, gateway_now_ms};
use psychevo_gateway_protocol::events_transcript::{
    GatewayActionKind, GatewayActionOutcome, GatewayActivityView, GatewayEvent, PendingActionView,
    ThreadActivityView,
};
use psychevo_gateway_protocol::source::{
    GatewayImageInput, GatewayInputPart, GatewaySource, GatewayTurn, GatewayTurnStatus,
};

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
pub(super) struct ActiveThreadState {
    pub(super) running: bool,
    pub(super) active_turn_id: Option<String>,
    pub(super) active_kind: Option<ActiveActivityKind>,
    pub(super) control: Option<ActiveActivityControl>,
    pub(super) queued: VecDeque<PendingQueuedActivity>,
    pub(super) history_mutation_reserved: bool,
}

#[derive(Debug, Default)]
pub(super) struct ActiveQueueState {
    pub(super) activities: HashMap<String, ActiveThreadState>,
    pub(super) aliases: HashMap<String, String>,
}

#[derive(Debug)]
pub(crate) struct HistoryMutationReservation {
    queue: Arc<Mutex<ActiveQueueState>>,
    queue_key: String,
}

impl HistoryMutationReservation {
    pub(super) fn new(queue: Arc<Mutex<ActiveQueueState>>, queue_key: String) -> Self {
        Self { queue, queue_key }
    }
}

impl Drop for HistoryMutationReservation {
    fn drop(&mut self) {
        let mut queue = self.queue.lock().expect("gateway active queue poisoned");
        let remove = queue
            .activities
            .get_mut(&self.queue_key)
            .is_some_and(|state| {
                state.history_mutation_reserved = false;
                !state.running
                    && state.active_turn_id.is_none()
                    && state.active_kind.is_none()
                    && state.control.is_none()
                    && state.queued.is_empty()
            });
        if remove {
            queue.activities.remove(&self.queue_key);
            queue
                .aliases
                .retain(|_, primary| primary != &self.queue_key);
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum ActiveActivityControl {
    Shell(ShellCommandControl),
}

impl ActiveActivityControl {
    pub(super) fn interrupt(&self) {
        match self {
            Self::Shell(control) => control.interrupt(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActiveActivityKind {
    Shell,
}

pub(super) enum ShellStartState {
    Standalone {
        permit: GatewayActivityPermit,
    },
    Queued {
        receiver: oneshot::Receiver<psychevo::Result<GatewayShellResult>>,
        active_activity_id: Option<String>,
        queue_position: usize,
    },
}

#[derive(Debug)]
pub(super) enum PendingQueuedActivity {
    Shell(Box<PendingQueuedShell>),
}

impl PendingQueuedActivity {
    pub(super) fn queued_at_ms(&self) -> i64 {
        match self {
            Self::Shell(pending) => pending.queued_at_ms,
        }
    }
}

#[derive(Debug)]
pub(super) struct PendingQueuedShell {
    pub(super) shell_id: String,
    pub(super) queued_at_ms: i64,
    pub(super) request: SendShellRequest,
    pub(super) permit: GatewayActivityPermit,
    pub(super) responder: oneshot::Sender<psychevo::Result<GatewayShellResult>>,
}

/// Caller-visible execution preferences. Runtime state, Adapter delegates, and
/// queue envelopes never cross the Thread Application Interface.
#[derive(Clone)]
pub struct ThreadTurnPolicy {
    pub snapshot_root: Option<PathBuf>,
    pub continue_latest: bool,
    pub extract_prompt_image_sources: bool,
    pub prompt_display: Option<psychevo::PromptDisplayMetadata>,
    pub max_context_messages: Option<usize>,
    pub config_path: Option<PathBuf>,
    pub project_context_override: Option<psychevo::ProjectContextInstructionMode>,
    pub sandbox_override: Option<psychevo::RunSandboxOverride>,
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
    pub selected_capability_roots: Vec<psychevo::extensions::SelectedCapabilityRoot>,
    pub skill_inputs: Vec<String>,
    pub mcp_servers: Vec<psychevo::McpServerInput>,
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

pub(crate) type TurnEventObserver = Arc<dyn Fn(psychevo::TurnEvent) + Send + Sync>;

/// Immutable host facts and observation/control ports supplied by one caller.
/// Callback construction is owned here so Adapters do not assemble Gateway
/// event emitters or lower queue requests.
pub struct ThreadCallerContext {
    pub surface: ThreadSurface,
    pub cwd: PathBuf,
    pub runtime_source: String,
    pub continue_sources: Vec<String>,
    turn_event_observer: Option<TurnEventObserver>,
    event_observer: Option<GatewayEventEmitter>,
    workspace_mutations: Option<WorkspaceMutationSink>,
    runtime_tools: Vec<psychevo::application::RuntimeTool>,
}

impl ThreadCallerContext {
    pub fn new(surface: ThreadSurface, cwd: PathBuf) -> Self {
        Self {
            surface,
            cwd,
            runtime_source: "gateway".to_string(),
            continue_sources: Vec::new(),
            turn_event_observer: None,
            event_observer: None,
            workspace_mutations: None,
            runtime_tools: Vec::new(),
        }
    }

    pub fn observe_turn_events(
        &mut self,
        observer: impl Fn(psychevo::TurnEvent) + Send + Sync + 'static,
    ) {
        self.turn_event_observer = Some(Arc::new(observer));
    }

    pub fn observe_gateway_events(
        &mut self,
        observer: impl Fn(GatewayEvent) + Send + Sync + 'static,
    ) {
        self.event_observer = Some(GatewayEventEmitter::new(observer));
    }

    pub(crate) fn set_workspace_mutations(&mut self, sink: WorkspaceMutationSink) {
        self.workspace_mutations = Some(sink);
    }

    #[cfg(test)]
    pub(super) fn set_turn_event_observer(&mut self, observer: TurnEventObserver) {
        self.turn_event_observer = Some(observer);
    }

    pub(crate) fn set_event_observer(&mut self, observer: GatewayEventEmitter) {
        self.event_observer = Some(observer);
    }

    pub(crate) fn set_runtime_tools(&mut self, tools: Vec<psychevo::application::RuntimeTool>) {
        self.runtime_tools = tools;
    }

    pub(crate) fn extend_runtime_tools(
        &mut self,
        tools: impl IntoIterator<Item = psychevo::application::RuntimeTool>,
    ) {
        self.runtime_tools.extend(tools);
    }

    #[cfg(test)]
    pub(crate) fn has_runtime_tool(&self, name: &str) -> bool {
        self.runtime_tools.iter().any(|tool| tool.name() == name)
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
            .field(
                "has_turn_event_observer",
                &self.turn_event_observer.is_some(),
            )
            .field("has_gateway_observer", &self.event_observer.is_some())
            .field("runtime_tool_count", &self.runtime_tools.len())
            .finish_non_exhaustive()
    }
}

/// Caller intent for one accepted turn.
pub struct ThreadTurnIntent {
    pub thread_id: Option<String>,
    pub source: Option<GatewaySource>,
    pub input: Vec<GatewayInputPart>,
    pub policy: ThreadTurnPolicy,
    pub client_turn_id: Option<String>,
    pub turn_id: Option<String>,
    pub(crate) agent_preparation: Option<psychevo::AgentPreparationToken>,
}

impl ThreadTurnIntent {
    pub fn new(input: Vec<GatewayInputPart>) -> Self {
        Self {
            thread_id: None,
            source: None,
            input,
            policy: ThreadTurnPolicy::default(),
            client_turn_id: None,
            turn_id: None,
            agent_preparation: None,
        }
    }

    pub(crate) fn into_framework_request(
        self,
        mut caller: ThreadCallerContext,
    ) -> psychevo::Result<FrameworkTurnSubmission> {
        let thread_id = self.thread_id.ok_or_else(|| {
            Error::Message("Framework Turn submission requires a materialized Thread".to_string())
        })?;
        let (prompt, image_inputs, prompt_display, _) = framework_input_parts(&self.input)?;
        let input_parts = framework_agent_input_parts(self.input.clone());
        let turn_id = self.turn_id.clone();
        let turn_event_observer = match (
            caller.turn_event_observer.take(),
            caller.event_observer.clone(),
        ) {
            (observer, Some(event_sink)) => {
                let turn_id = turn_id.clone().ok_or_else(|| {
                    Error::Message(
                        "Gateway Framework projection requires an accepted Turn identity"
                            .to_string(),
                    )
                })?;
                Some(framework_turn_event_observer(
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
        request = request
            .with_input_parts(input_parts)
            .with_runtime_tools(std::mem::take(&mut caller.runtime_tools))
            .with_framework_context(
                policy.snapshot_root,
                policy.max_context_messages,
                policy.selected_capability_roots,
                caller.workspace_mutations.take(),
            )
            .with_initial_thread_preferences(policy.initial_thread_preferences);
        if let Some(turn_id) = self.turn_id {
            request = request.with_requested_turn_id(turn_id);
        }
        if let Some(preparation) = self.agent_preparation {
            request = request.with_agent_preparation(preparation);
        }
        Ok(FrameworkTurnSubmission {
            thread_id,
            request,
            observers: FrameworkTurnObservers {
                turn_events: turn_event_observer,
            },
        })
    }
}

fn framework_agent_input_parts(input: Vec<GatewayInputPart>) -> Vec<psychevo::AgentInputPart> {
    input
        .into_iter()
        .map(|part| match part {
            GatewayInputPart::Text { text } => psychevo::AgentInputPart::Text { text },
            GatewayInputPart::Image { input } => psychevo::AgentInputPart::Image {
                input: match input {
                    GatewayImageInput::LocalPath { path } => {
                        ImageInput::LocalPath(PathBuf::from(path))
                    }
                    GatewayImageInput::Url { url } => ImageInput::ImageUrl(url),
                },
            },
            GatewayInputPart::Context {
                label,
                text,
                visible_to_model,
            } => psychevo::AgentInputPart::Context {
                label,
                text,
                visible_to_model,
            },
            GatewayInputPart::Resource {
                uri,
                mime_type,
                text,
                blob,
            } => psychevo::AgentInputPart::Resource {
                uri,
                mime_type,
                text,
                blob,
            },
            GatewayInputPart::ResourceLink {
                name,
                uri,
                description,
                mime_type,
                size,
            } => psychevo::AgentInputPart::ResourceLink {
                name,
                uri,
                description,
                mime_type,
                size,
            },
        })
        .collect()
}

fn framework_turn_event_observer(
    turn_id: String,
    thread_id: String,
    observer: Option<TurnEventObserver>,
    event_sink: GatewayEventEmitter,
) -> TurnEventObserver {
    let projector = Arc::new(Mutex::new(GatewayLiveProjector::new(Some(
        thread_id.clone(),
    ))));
    Arc::new(move |event: psychevo::TurnEvent| {
        let lifecycle = gateway_event_from_framework_turn(&event, &thread_id, Some(&turn_id));
        let live = projector
            .lock()
            .expect("Gateway Framework live projector poisoned")
            .project_turn_event(&turn_id, &event);
        if let Some(projected) = lifecycle {
            emit_framework_gateway_event(&event_sink, &thread_id, Some(&turn_id), projected);
        }
        if let Some(observer) = &observer {
            observer(event);
        }
        if let Some(projected) = live {
            emit_framework_gateway_event(&event_sink, &thread_id, Some(&turn_id), projected);
        }
    })
}

fn emit_framework_gateway_event(
    event_sink: &GatewayEventEmitter,
    thread_id: &str,
    turn_id: Option<&str>,
    event: GatewayEvent,
) {
    let fields = journey_profile::gateway_profile_event_fields(&event);
    gateway_profile_mark("gateway_event_emitted", turn_id, Some(thread_id), fields);
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
    event: &psychevo::TurnEvent,
    fallback_thread_id: &str,
    fallback_turn_id: Option<&str>,
) -> Option<GatewayEvent> {
    match event {
        psychevo::TurnEvent::Scoped {
            thread_id,
            turn_id,
            event,
        } => gateway_event_from_framework_turn(event, thread_id, Some(turn_id)),
        psychevo::TurnEvent::ActivityChanged {
            thread_id,
            activity,
        } => Some(GatewayEvent::ActivityChanged {
            thread_id: Some(thread_id.clone()),
            activity: GatewayActivityView {
                framework_revision: Some(activity.revision.to_string()),
                running: activity.running,
                active_turn_id: activity.active_turn_id.clone(),
                queued_turns: activity.queued_turns,
                ..GatewayActivityView::default()
            },
        }),
        psychevo::TurnEvent::Accepted {
            receipt,
            queue_position: Some(queue_position),
        } => Some(GatewayEvent::TurnQueued {
            thread_id: Some(receipt.thread_id.clone()),
            turn_id: receipt.turn_id.clone(),
            queue_position: *queue_position,
        }),
        psychevo::TurnEvent::Started { thread_id, turn_id } => Some(GatewayEvent::TurnStarted {
            thread_id: Some(thread_id.clone()),
            turn_id: turn_id.clone(),
            selected_skills: Vec::new(),
        }),
        psychevo::TurnEvent::Completed {
            thread_id,
            turn_id,
            outcome,
        } => {
            let (status, outcome) = match outcome {
                psychevo::TurnOutcome::Completed => (GatewayTurnStatus::Completed, "normal"),
                psychevo::TurnOutcome::Stopped => (GatewayTurnStatus::Interrupted, "stopped"),
                psychevo::TurnOutcome::Failed => (GatewayTurnStatus::Failed, "failed"),
                psychevo::TurnOutcome::Interrupted => (GatewayTurnStatus::Interrupted, "aborted"),
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
                thread_id: Some(thread_id.clone()),
                turn_id: turn_id.clone(),
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
                error: Some(gateway_turn_error(message, None)),
                started_at_ms: None,
                completed_at_ms: Some(gateway_now_ms()),
            };
            Some(GatewayEvent::TurnCompleted {
                thread_id: Some(thread_id.clone()),
                turn_id: turn_id.clone(),
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
                    action_id: interaction_id.clone(),
                    kind,
                    title,
                    summary,
                    payload: payload.clone(),
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
                action_id: interaction_id.clone(),
                kind,
                outcome: match reason.as_str() {
                    "deny" | "rejected" => GatewayActionOutcome::Rejected,
                    "cancelled" | "timed_out" | "turn_finished" => GatewayActionOutcome::Cancelled,
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
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::{
        GatewayLiveProjector, framework_turn_event_observer, gateway_event_from_framework_turn,
    };
    use crate::{GatewayEventEmitter, gateway_event_from_turn_event};
    use psychevo_gateway_protocol::events_transcript::{
        GatewayActivityView, GatewayEvent, TranscriptBlockStatus,
    };

    #[test]
    fn framework_queued_acceptance_projects_public_turn_queued() {
        let event = gateway_event_from_framework_turn(
            &psychevo::TurnEvent::Accepted {
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
            &psychevo::TurnEvent::ActivityChanged {
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
    fn framework_turn_event_keeps_stateful_acp_plan_projection() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        let observer = framework_turn_event_observer(
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

        observer(psychevo::TurnEvent::Runtime {
            data: json!({
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
            }),
        });

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
    fn framework_runtime_extension_does_not_duplicate_application_owned_interactions() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        let observer = framework_turn_event_observer(
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

        observer(psychevo::TurnEvent::Runtime {
            data: json!({
                "type": "action_requested",
                "action_id": "clarify-1",
                "kind": "clarify",
                "payload": {
                    "questions": [{
                        "question": "Which workspace?"
                    }]
                }
            }),
        });

        assert!(
            events
                .lock()
                .expect("observed Gateway events poisoned")
                .is_empty(),
            "Application TurnEvent projection owns the public interaction lifecycle"
        );
    }

    #[test]
    fn framework_typed_tool_and_warning_project_without_a_runtime_stream_bridge() {
        let mut projector = GatewayLiveProjector::new(Some("thread-1".to_string()));
        let tool = projector
            .project_turn_event(
                "turn-1",
                &psychevo::TurnEvent::Tool {
                    stage: psychevo::ItemStage::Started,
                    data: json!({
                        "type": "tool_call_pending",
                        "tool_name": "exec_command",
                        "tool_call_id": "call-1",
                    }),
                },
            )
            .expect("tool projection");
        let GatewayEvent::EntryStarted { entry, .. } = tool else {
            panic!("typed Tool start must project an entry start");
        };
        assert_eq!(entry.thread_id, "thread-1");
        assert_eq!(entry.status, TranscriptBlockStatus::Running);
        assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Pending);
        assert_eq!(entry.blocks[0].title.as_deref(), Some("exec_command"));

        let warning = projector
            .project_turn_event(
                "turn-1",
                &psychevo::TurnEvent::Warning {
                    data: json!({
                        "type": "warning",
                        "kind": "bounded_warning",
                        "message": "bounded warning",
                        "suggestion": "retry once",
                    }),
                },
            )
            .expect("warning projection");
        assert!(matches!(
            warning,
            GatewayEvent::Warning {
                kind,
                message,
                suggestion: Some(suggestion),
                ..
            } if kind == "bounded_warning"
                && message == "bounded warning"
                && suggestion == "retry once"
        ));
    }

    #[test]
    fn framework_typed_message_start_has_a_stable_started_entry() {
        let message = json!({
            "role": "assistant",
            "content": "hello",
        });
        let started = gateway_event_from_turn_event(
            "turn-1",
            &psychevo::TurnEvent::Message {
                stage: psychevo::ItemStage::Started,
                message: message.clone(),
                usage: None,
                metadata: None,
                accounting: None,
            },
        )
        .expect("typed message start projection");
        let completed = gateway_event_from_turn_event(
            "turn-1",
            &psychevo::TurnEvent::Message {
                stage: psychevo::ItemStage::Completed,
                message,
                usage: None,
                metadata: None,
                accounting: None,
            },
        )
        .expect("typed message completion projection");

        let GatewayEvent::EntryStarted { entry: started, .. } = started else {
            panic!("typed Message Started must remain an entry start");
        };
        let GatewayEvent::EntryCompleted {
            entry: completed, ..
        } = completed
        else {
            panic!("typed Message Completed must remain an entry completion");
        };
        assert_eq!(started.id, "live:turn-1:assistant");
        assert_eq!(started.id, completed.id);
        assert_eq!(started.blocks[0].id, completed.blocks[0].id);
        assert_eq!(started.status, TranscriptBlockStatus::Running);
        assert_eq!(completed.status, TranscriptBlockStatus::Completed);
    }

    #[test]
    fn framework_clarify_projection_preserves_the_decodable_request() {
        let event = gateway_event_from_framework_turn(
            &psychevo::TurnEvent::InteractionRequested {
                interaction_id: "clarify-1".to_string(),
                kind: "clarify".to_string(),
                payload: json!({
                    "call_id": "clarify-1",
                    "questions": [{
                        "header": "Target",
                        "question": "Which workspace?",
                        "options": [],
                        "multiple": false,
                        "custom": true,
                        "secret": false,
                    }],
                }),
            },
            "thread-1",
            Some("turn-1"),
        )
        .expect("clarify projection");
        let GatewayEvent::ActionRequested { action } = event else {
            panic!("clarify projection must remain an action request");
        };

        assert_eq!(action.action_id, "clarify-1");
        assert_eq!(action.thread_id.as_deref(), Some("thread-1"));
        assert_eq!(action.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(action.payload["call_id"], "clarify-1");
        assert_eq!(
            action.payload["questions"][0]["question"],
            "Which workspace?"
        );
    }
}

pub(crate) struct FrameworkTurnSubmission {
    pub(crate) thread_id: String,
    pub(crate) request: psychevo::TurnRequest,
    pub(crate) observers: FrameworkTurnObservers,
}

pub(crate) struct FrameworkTurnObservers {
    turn_events: Option<TurnEventObserver>,
}

impl FrameworkTurnObservers {
    pub(crate) fn attach(self, gateway: &Gateway, handle: psychevo::TurnHandle) {
        let Some(observer) = self.turn_events else {
            return;
        };
        let turn_id = handle.receipt().turn_id.clone();
        let mut events = handle.events();
        gateway.spawn_background(format!("framework-turn-events:{turn_id}"), async move {
            while let Some(event) = events.next().await {
                let terminal = matches!(
                    event,
                    psychevo::TurnEvent::Completed { .. } | psychevo::TurnEvent::Failed { .. }
                );
                observer(event);
                if terminal {
                    break;
                }
            }
        });
    }
}

impl fmt::Debug for ThreadTurnIntent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThreadTurnIntent")
            .field("thread_id", &self.thread_id)
            .field("source", &self.source)
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
    pub source: Option<GatewaySource>,
    pub bind_source: Option<GatewaySource>,
    pub reset_source_binding: bool,
    pub input: Vec<GatewayInputPart>,
    pub cwd: PathBuf,
    pub policy: ThreadTurnPolicy,
    pub workspace_mutations: Option<WorkspaceMutationSink>,
    pub runtime_tools: Vec<psychevo::application::RuntimeTool>,
    pub runtime_source: Option<String>,
    pub continue_sources: Vec<String>,
    pub turn_events: Option<TurnEventObserver>,
    pub event_sink: Option<GatewayEventEmitter>,
    pub lineage: Option<Value>,
}

#[cfg(test)]
impl fmt::Debug for SendTurnRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SendTurnRequest")
            .field("thread_id", &self.thread_id)
            .field("source", &self.source)
            .field("bind_source", &self.bind_source)
            .field("reset_source_binding", &self.reset_source_binding)
            .field("input", &self.input)
            .field("cwd", &self.cwd)
            .field("policy", &self.policy)
            .field(
                "has_workspace_mutations",
                &self.workspace_mutations.is_some(),
            )
            .field("runtime_tool_count", &self.runtime_tools.len())
            .field("runtime_source", &self.runtime_source)
            .field("continue_sources", &self.continue_sources)
            .field("has_turn_events", &self.turn_events.is_some())
            .field("has_event_sink", &self.event_sink.is_some())
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
    pub execution: ShellExecutionIntent,
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
            .field("execution", &self.execution)
            .field("has_event_sink", &self.event_sink.is_some())
            .field("lineage", &self.lineage)
            .finish()
    }
}

#[derive(Clone)]
pub struct ShellExecutionIntent {
    pub(super) continue_latest: bool,
    pub(super) continue_sources: Vec<String>,
    pub(super) runtime_source: String,
    pub(super) model: Option<String>,
    pub(super) reasoning_effort: Option<String>,
    pub(super) mode: RunMode,
    pub(super) inherited_env: Option<BTreeMap<String, String>>,
}

impl ShellExecutionIntent {
    pub fn new(runtime_source: impl Into<String>) -> Self {
        Self {
            continue_latest: false,
            continue_sources: Vec::new(),
            runtime_source: runtime_source.into(),
            model: None,
            reasoning_effort: None,
            mode: RunMode::Default,
            inherited_env: None,
        }
    }

    pub fn continue_latest(mut self, sources: impl IntoIterator<Item = String>) -> Self {
        self.continue_latest = true;
        self.continue_sources = sources.into_iter().collect();
        self
    }

    pub fn model(mut self, model: Option<String>, reasoning_effort: Option<String>) -> Self {
        self.model = model;
        self.reasoning_effort = reasoning_effort;
        self
    }

    pub fn mode(mut self, mode: RunMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn inherited_environment(mut self, environment: BTreeMap<String, String>) -> Self {
        self.inherited_env = Some(environment);
        self
    }
}

impl fmt::Debug for ShellExecutionIntent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ShellExecutionIntent")
            .field("continue_latest", &self.continue_latest)
            .field("continue_sources", &self.continue_sources)
            .field("runtime_source", &self.runtime_source)
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("mode", &self.mode)
            .field("has_inherited_env", &self.inherited_env.is_some())
            .finish()
    }
}
