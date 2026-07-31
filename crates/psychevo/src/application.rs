//! High-level in-process Framework interface.
//!
//! This module owns the public Thread/Turn vocabulary. The lower run assembly
//! and state Modules remain implementation details of an Application.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures::FutureExt;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, Notify, oneshot};
use uuid::Uuid;

mod agent_session;
mod event_log;
mod interaction_broker;
mod lifecycle;
mod runtime;
mod thread;
mod turn;
mod turn_completion;
mod turn_events;
mod turn_request;

use event_log::EventLog;
use interaction_broker::{
    FrameworkApprovalHandler, FrameworkInteractionControl, InteractionBroker,
};
use runtime::{ApplicationRuntime, TurnPhase};

use crate::compaction::{CompactSessionOptions, CompactionReason, CompactionResult};
use crate::paths::canonicalize_cwd;
use crate::run::{run_live_streaming_controlled, run_live_streaming_controlled_with_provider};
use crate::state::{
    GatewayTurnDeliveryInput, GatewayTurnTerminalInput, NativeSessionForkInput, SessionListCursor,
    StateRuntime,
};
use crate::types::{
    ApprovalHandler, ImageInput, McpServerInput, PermissionApprovalDecision, PermissionMode,
    ProjectContextInstructionMode, RunMode, RunOptions, RunSandboxOverride, RunStreamEvent,
    RunStreamSink, RuntimeTool, SessionEventPayload, SessionSummary, run_control,
};
use crate::{Error, Result};

#[cfg(test)]
use crate::types::{
    ClarifyAnswer, ClarifyResponse, PermissionApprovalOutcome, PermissionApprovalRequest,
};

const DEFAULT_EVENT_CAPACITY: usize = 256;
const DEFAULT_THREAD_LIST_LIMIT: usize = 50;
const MAX_THREAD_LIST_LIMIT: usize = 200;
#[cfg(not(test))]
const FORCE_SHUTDOWN_TOTAL: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(test)]
const FORCE_SHUTDOWN_TOTAL: std::time::Duration = std::time::Duration::from_millis(250);
#[cfg(not(test))]
const FORCE_ADAPTER_BUDGET: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(test)]
const FORCE_ADAPTER_BUDGET: std::time::Duration = std::time::Duration::from_millis(25);
#[cfg(not(test))]
const FORCE_COOPERATIVE_JOIN_BUDGET: std::time::Duration = std::time::Duration::from_secs(6);
#[cfg(test)]
const FORCE_COOPERATIVE_JOIN_BUDGET: std::time::Duration = std::time::Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownReport {
    pub forced: bool,
    pub adapter: ShutdownAdapterStatus,
    pub task_panics: u64,
    pub aborted_tasks: usize,
    pub pending_terminal_failures: Vec<PendingTerminalFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ShutdownAdapterStatus {
    Completed,
    Failed { message: String },
    TimedOut,
    ContractViolation { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingTerminalFailure {
    pub turn_id: String,
    pub message: String,
}

#[derive(Clone)]
pub struct Application {
    inner: Arc<ApplicationInner>,
}

struct ApplicationInner {
    state: StateRuntime,
    agent_sessions: Arc<dyn AgentSessionAdapter>,
    home: PathBuf,
    config_path: Option<PathBuf>,
    event_capacity: usize,
    force_shutdown_requested: AtomicBool,
    force_shutdown_notify: Notify,
    shutdown_complete: Mutex<Option<ShutdownReport>>,
    shutdown_finalizer: AsyncMutex<()>,
    runtime: Arc<ApplicationRuntime>,
}

#[derive(Default)]
pub struct ApplicationBuilder {
    home: Option<PathBuf>,
    database_path: Option<PathBuf>,
    state: Option<StateRuntime>,
    config_path: Option<PathBuf>,
    event_capacity: Option<usize>,
    agent_sessions: Option<Arc<dyn AgentSessionAdapter>>,
    provider: Option<psychevo_ai::Provider>,
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<ApplicationInner>,
}

#[derive(Debug, Clone)]
pub struct StartThreadRequest {
    pub cwd: PathBuf,
    pub source: String,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ThreadListQuery {
    pub cwd: Option<PathBuf>,
    pub archived: bool,
    pub sources: Vec<String>,
    pub cursor: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThreadListPage {
    pub threads: Vec<ThreadSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadListCursor {
    cwd: Option<String>,
    archived: bool,
    sources: Vec<String>,
    position: SessionListCursor,
}

#[derive(Clone)]
pub struct Thread {
    client: Client,
    id: String,
}

#[derive(Debug, Clone)]
pub struct CompactThreadRequest {
    pub config_path: Option<PathBuf>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub instructions: Option<String>,
    pub force: bool,
    pub reason: CompactionReason,
}

impl Default for CompactThreadRequest {
    fn default() -> Self {
        Self {
            config_path: None,
            model: None,
            reasoning_effort: None,
            inherited_env: None,
            instructions: None,
            force: false,
            reason: CompactionReason::Manual,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ForkThreadRequest {
    pub before_session_seq: Option<i64>,
}

pub trait AgentSessionAdapter: Send + Sync + fmt::Debug {
    fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>>;

    fn shutdown(&self, _force: bool) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

pub struct AgentTurnRequest {
    pub thread: ThreadExecutionContext,
    pub history: HistoryReader,
    pub receipt: TurnReceipt,
    pub input: TurnRequest,
    pub events: TurnEventSender,
    pub control: TurnControl,
    native_control: Option<crate::types::RunControl>,
}

#[derive(Clone)]
pub struct TurnEventSender {
    log: Arc<EventLog>,
    interactions: InteractionBroker,
}

#[derive(Clone)]
pub struct TurnControl {
    handle: crate::types::RunControlHandle,
    interactions: InteractionBroker,
}

#[derive(Clone)]
struct NativeAgentSessionAdapter {
    state: StateRuntime,
    config_path: Option<PathBuf>,
    provider: Option<psychevo_ai::Provider>,
}

#[derive(Debug)]
pub struct TurnRequest {
    prompt: String,
    image_inputs: Vec<ImageInput>,
    extract_prompt_image_sources: bool,
    prompt_display: Option<crate::types::PromptDisplayMetadata>,
    client_turn_id: Option<String>,
    source: String,
    config_path: Option<PathBuf>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    runtime_ref: Option<String>,
    runtime_options: BTreeMap<String, String>,
    include_reasoning: bool,
    mode: RunMode,
    permission_mode: Option<PermissionMode>,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
    clarify_enabled: bool,
    inherited_env: Option<BTreeMap<String, String>>,
    project_context: Option<ProjectContextInstructionMode>,
    sandbox: Option<RunSandboxOverride>,
    agent: Option<String>,
    no_agents: bool,
    no_skills: bool,
    skill_inputs: Vec<String>,
    mcp_servers: Vec<McpServerInput>,
    tools: Vec<RuntimeTool>,
    adapter_options: AdapterTurnOptions,
    requested_turn_id: Option<String>,
    prepared_control: Option<PreparedTurnControl>,
}

#[doc(hidden)]
#[derive(Default)]
pub struct AdapterTurnOptions {
    pub snapshot_root: Option<PathBuf>,
    pub max_context_messages: Option<usize>,
    pub selected_capability_roots: Vec<crate::extensions::SelectedCapabilityRoot>,
    pub workspace_mutations: Option<crate::types::WorkspaceMutationSink>,
    pub input_parts: Vec<Value>,
    pub run_stream_observer: Option<RunStreamSink>,
    pub initial_thread_preferences: BTreeMap<String, String>,
    pub prepared_source_key: Option<String>,
    pub turn_event_observer: Option<Arc<dyn Fn(TurnEvent) + Send + Sync>>,
    pub agent_entrypoint: Option<crate::agents::AgentEntrypoint>,
    #[doc(hidden)]
    pub mcp_runtime: Option<crate::mcp::McpRuntime>,
}

struct PreparedTurnControl {
    handle: crate::types::RunControlHandle,
    control: crate::types::RunControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnReceipt {
    pub accepted: bool,
    pub thread_id: String,
    pub turn_id: String,
    pub client_turn_id: Option<String>,
}

type SharedTurnCompletion = std::result::Result<Arc<TurnResult>, Arc<str>>;

#[derive(Clone)]
struct PendingTerminal {
    receipt: TurnReceipt,
    completion: SharedTurnCompletion,
    terminal_event: TurnEvent,
    completed_at_ms: i64,
    last_error: String,
}

struct TurnCompletion {
    value: Mutex<Option<SharedTurnCompletion>>,
    notify: Notify,
}

#[derive(Clone)]
pub struct TurnHandle {
    receipt: TurnReceipt,
    events: Arc<EventLog>,
    completion: Arc<TurnCompletion>,
    control: crate::types::RunControlHandle,
    interaction_broker: Option<InteractionBroker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum InteractionResponse {
    Permission(PermissionApprovalDecision),
    Clarify(Vec<Vec<String>>),
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionResponseReceipt {
    pub accepted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    Completed,
    Stopped,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnResult {
    pub thread_id: String,
    pub outcome: TurnOutcome,
    pub final_answer: String,
    pub provider: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub tool_failures: usize,
    pub context_limit: Option<u64>,
    pub context_snapshot: Option<crate::context_usage::ContextSnapshot>,
    pub warnings: Vec<crate::types::RunWarning>,
    pub terminal_reason: Option<crate::__agent_core::TerminalReason>,
    pub terminal_error: Option<crate::types::RunTerminalError>,
    pub selected_agent: Option<crate::types::SelectedAgent>,
    pub selected_skills: Vec<crate::skills::SelectedSkill>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
    pub source: String,
    pub cwd: String,
    pub title: Option<String>,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
    pub archived: bool,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub active_turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadActivitySnapshot {
    pub revision: u64,
    pub running: bool,
    pub active_turn_id: Option<String>,
    pub queued_turns: usize,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationActivitySnapshot {
    pub revision: u64,
    pub threads: BTreeMap<String, ThreadActivitySnapshot>,
}

const DEFAULT_HISTORY_PAGE_SIZE: usize = 100;
const MAX_HISTORY_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadExecutionContext {
    pub id: String,
    pub cwd: String,
    pub source: String,
}

#[derive(Clone)]
pub struct HistoryReader {
    state: StateRuntime,
    thread_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub thread_id: String,
    pub items: Vec<ThreadItem>,
    pub next_before: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    #[serde(flatten)]
    pub summary: ThreadSummary,
    pub pending_interactions: Vec<PendingInteraction>,
    pub items: Vec<ThreadItem>,
    pub history_cursor: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItem {
    pub session_seq: i64,
    pub message: psychevo_agent_core::Message,
    pub usage: Option<Value>,
    pub metadata: Option<Value>,
    pub accounting: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInteraction {
    pub interaction_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub kind: String,
    pub status: String,
    pub payload: Value,
    pub resolution: Option<Value>,
    pub requested_at_ms: i64,
    pub resolved_at_ms: Option<i64>,
}

fn gateway_terminal_facts(outcome: TurnOutcome) -> (&'static str, &'static str) {
    match outcome {
        TurnOutcome::Completed => ("completed", "normal"),
        TurnOutcome::Stopped => ("interrupted", "stopped"),
        TurnOutcome::Failed => ("failed", "failed"),
        TurnOutcome::Interrupted => ("interrupted", "aborted"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStage {
    Started,
    Updated,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TurnEvent {
    ActivityChanged {
        thread_id: String,
        activity: ThreadActivitySnapshot,
    },
    Accepted {
        receipt: TurnReceipt,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        queue_position: Option<usize>,
    },
    Started {
        thread_id: String,
        turn_id: String,
    },
    Message {
        stage: ItemStage,
        message: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        accounting: Option<Value>,
    },
    MessageDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ReasoningCompleted {
        text: Option<String>,
    },
    Tool {
        stage: ItemStage,
        data: Value,
    },
    InteractionRequested {
        interaction_id: String,
        kind: String,
        payload: Value,
    },
    InteractionResolved {
        interaction_id: String,
        kind: String,
        reason: String,
    },
    Warning {
        data: Value,
    },
    Completed {
        thread_id: String,
        turn_id: String,
        outcome: TurnOutcome,
    },
    Failed {
        thread_id: String,
        turn_id: String,
        message: String,
    },
    ResyncRequired {
        missed: u64,
    },
}

pub struct TurnEventStream {
    log: Arc<EventLog>,
    cursor: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct FakeAgentSessionAdapter {
        started: Arc<Notify>,
        release: Arc<Notify>,
        completed: Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct InteractionAgentSessionAdapter {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[derive(Debug)]
    struct PermissionInteractionAgentSessionAdapter {
        started: Arc<Notify>,
        decision: Arc<Mutex<Option<PermissionApprovalDecision>>>,
    }

    #[derive(Debug)]
    struct CancelledPermissionAgentSessionAdapter {
        started: Arc<Notify>,
        cancel: Arc<Notify>,
    }

    #[derive(Debug)]
    struct ClarifyInteractionAgentSessionAdapter {
        started: Arc<Notify>,
        outcome: Arc<Mutex<Option<crate::types::ClarifyInteractionOutcome>>>,
    }

    #[derive(Debug)]
    struct ForceAwareAgentSessionAdapter {
        started: Arc<Notify>,
        shutdown_modes: Arc<Mutex<Vec<bool>>>,
    }

    #[derive(Debug)]
    struct ShutdownReleasesAgentSessionAdapter {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[derive(Debug)]
    struct PendingAgentSessionAdapter {
        started: Arc<Notify>,
    }

    #[derive(Debug)]
    struct FailingAgentSessionAdapter;

    #[derive(Debug)]
    struct PanickingAgentSessionAdapter;

    #[derive(Debug)]
    struct SnapshotOrderingAgentSessionAdapter {
        calls: Arc<AtomicUsize>,
        first_started: Arc<Notify>,
        release_first: Arc<Notify>,
        second_snapshot_items: Arc<Mutex<Option<usize>>>,
    }

    #[test]
    fn public_turn_outcomes_map_to_distinct_gateway_terminal_facts() {
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Completed),
            ("completed", "normal")
        );
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Stopped),
            ("interrupted", "stopped")
        );
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Failed),
            ("failed", "failed")
        );
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Interrupted),
            ("interrupted", "aborted")
        );
    }

    #[test]
    fn non_clean_shutdown_report_is_a_teardown_error() {
        let report = ShutdownReport {
            forced: true,
            adapter: ShutdownAdapterStatus::TimedOut,
            task_panics: 1,
            aborted_tasks: 2,
            pending_terminal_failures: vec![PendingTerminalFailure {
                turn_id: "turn-1".to_string(),
                message: "write failed".to_string(),
            }],
        };
        let error = report.require_clean().expect_err("non-clean report");
        let message = error.to_string();
        assert!(message.contains("timed_out"));
        assert!(message.contains("turn-1"));
        assert!(message.contains("write failed"));
    }

    #[tokio::test]
    async fn shutdown_report_includes_agent_task_panics() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("state.db"))
            .build()
            .await
            .expect("application");
        application
            .inner
            .runtime
            .agent_supervisor
            .record_task_panic();

        let report = application.shutdown().await.expect("shutdown");

        assert_eq!(report.task_panics, 1);
        assert!(!report.is_clean());
    }

    impl AgentSessionAdapter for ForceAwareAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            Box::pin(async move {
                started.notify_one();
                while !request.control.is_interrupted() {
                    tokio::task::yield_now().await;
                }
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Interrupted,
                    final_answer: String::new(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }

        fn shutdown(&self, force: bool) -> BoxFuture<'static, Result<()>> {
            let shutdown_modes = self.shutdown_modes.clone();
            Box::pin(async move {
                shutdown_modes
                    .lock()
                    .expect("shutdown modes poisoned")
                    .push(force);
                Ok(())
            })
        }
    }

    impl AgentSessionAdapter for ShutdownReleasesAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let release = self.release.clone();
            Box::pin(async move {
                started.notify_one();
                release.notified().await;
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Interrupted,
                    final_answer: String::new(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }

        fn shutdown(&self, force: bool) -> BoxFuture<'static, Result<()>> {
            let release = self.release.clone();
            Box::pin(async move {
                assert!(force);
                release.notify_waiters();
                Ok(())
            })
        }
    }

    impl AgentSessionAdapter for PendingAgentSessionAdapter {
        fn run_turn(&self, _request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            Box::pin(async move {
                started.notify_one();
                std::future::pending::<Result<TurnResult>>().await
            })
        }

        fn shutdown(&self, force: bool) -> BoxFuture<'static, Result<()>> {
            assert!(force);
            Box::pin(std::future::pending())
        }
    }

    impl AgentSessionAdapter for FailingAgentSessionAdapter {
        fn run_turn(&self, _request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            Box::pin(async { Err(Error::Message("adapter fixture failed".to_string())) })
        }
    }

    impl AgentSessionAdapter for PanickingAgentSessionAdapter {
        fn run_turn(&self, _request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            Box::pin(async { panic!("adapter fixture panic") })
        }
    }

    impl AgentSessionAdapter for SnapshotOrderingAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let first_started = self.first_started.clone();
            let release_first = self.release_first.clone();
            let second_snapshot_items = self.second_snapshot_items.clone();
            Box::pin(async move {
                if call == 0 {
                    first_started.notify_one();
                    release_first.notified().await;
                } else {
                    *second_snapshot_items
                        .lock()
                        .expect("snapshot observation poisoned") = Some(
                        request
                            .history
                            .latest(Some(MAX_HISTORY_PAGE_SIZE))
                            .await?
                            .items
                            .len(),
                    );
                }
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: format!("turn {call}"),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl AgentSessionAdapter for InteractionAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let release = self.release.clone();
            Box::pin(async move {
                request.events.emit(TurnEvent::InteractionRequested {
                    interaction_id: "clarify-1".to_string(),
                    kind: "clarify".to_string(),
                    payload: serde_json::json!([{"question": "Proceed?"}]),
                });
                started.notify_one();
                release.notified().await;
                request.events.emit(TurnEvent::InteractionResolved {
                    interaction_id: "clarify-1".to_string(),
                    kind: "clarify".to_string(),
                    reason: "answered".to_string(),
                });
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: "fake answer".to_string(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl AgentSessionAdapter for PermissionInteractionAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let observed_decision = self.decision.clone();
            Box::pin(async move {
                let handler = request
                    .input
                    .approval_handler
                    .as_ref()
                    .expect("Application must install the Framework approval handler");
                let approval = handler.request_permission(PermissionApprovalRequest {
                    tool_call_id: "permission-1".to_string(),
                    tool_name: "exec_command".to_string(),
                    summary: "Run the checked command".to_string(),
                    reason: "The fixture requires an explicit decision".to_string(),
                    matched_rule: None,
                    suggested_rule: None,
                    allow_always: false,
                    filesystem: None,
                    mcp_startup: None,
                    timeout_secs: 30,
                });
                started.notify_one();
                *observed_decision
                    .lock()
                    .expect("observed permission decision poisoned") = Some(approval.await);
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: "permission accepted".to_string(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl AgentSessionAdapter for CancelledPermissionAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let cancel = self.cancel.clone();
            Box::pin(async move {
                let handler = request
                    .input
                    .approval_handler
                    .clone()
                    .expect("Application must install the Framework approval handler");
                let mut approval = handler.request_permission(PermissionApprovalRequest {
                    tool_call_id: "mcp_startup:pending".to_string(),
                    tool_name: "mcp_startup".to_string(),
                    summary: "Start pending MCP server".to_string(),
                    reason: "The fixture waits for startup cancellation".to_string(),
                    matched_rule: None,
                    suggested_rule: None,
                    allow_always: false,
                    filesystem: None,
                    mcp_startup: None,
                    timeout_secs: 30,
                });
                started.notify_one();
                tokio::select! {
                    _ = &mut approval => {}
                    _ = cancel.notified() => {
                        handler.cancel_permission("mcp_startup:pending").await;
                        let _ = approval.await;
                    }
                }
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: "permission cancelled".to_string(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl AgentSessionAdapter for ClarifyInteractionAgentSessionAdapter {
        fn run_turn(
            &self,
            mut request: AgentTurnRequest,
        ) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let observed_outcome = self.outcome.clone();
            Box::pin(async move {
                let native_control = request
                    .native_control
                    .take()
                    .expect("Application must install native Turn control");
                let control = native_control.handle();
                let events = request.events.clone();
                let stream: RunStreamSink = Arc::new(move |event| {
                    if let Some(event) = TurnEvent::from_run_stream(event) {
                        events.emit(event);
                    }
                    started.notify_one();
                });
                let outcome = control
                    .request_clarification(
                        crate::types::ClarifyRequestEvent {
                            call_id: "clarify-application-1".to_string(),
                            questions: vec![crate::types::ClarifyQuestion {
                                header: "Target".to_string(),
                                question: "Which directories?".to_string(),
                                options: Vec::new(),
                                multiple: true,
                                custom: true,
                                secret: false,
                            }],
                        },
                        stream,
                        Some(native_control.abort_signal()),
                    )
                    .await;
                *observed_outcome
                    .lock()
                    .expect("observed clarify outcome poisoned") = Some(outcome);
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: "clarification accepted".to_string(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl AgentSessionAdapter for FakeAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = Arc::clone(&self.started);
            let release = Arc::clone(&self.release);
            let completed = Arc::clone(&self.completed);
            Box::pin(async move {
                request.events.emit(TurnEvent::Message {
                    stage: ItemStage::Completed,
                    message: serde_json::json!({
                        "role": "assistant",
                        "text": "fake answer",
                    }),
                    usage: None,
                    metadata: None,
                    accounting: None,
                });
                started.notify_one();
                release.notified().await;
                completed.fetch_add(1, Ordering::SeqCst);
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: "fake answer".to_string(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    fn fake_adapter() -> (
        Arc<FakeAgentSessionAdapter>,
        Arc<Notify>,
        Arc<Notify>,
        Arc<AtomicUsize>,
    ) {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let completed = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(FakeAgentSessionAdapter {
                started: Arc::clone(&started),
                release: Arc::clone(&release),
                completed: Arc::clone(&completed),
            }),
            started,
            release,
            completed,
        )
    }

    #[tokio::test]
    async fn bounded_event_log_reports_resync_and_replays_retained_events() {
        let log = Arc::new(EventLog::new(2));
        for index in 0..3 {
            log.push(TurnEvent::Warning {
                data: serde_json::json!({ "index": index }),
            });
        }
        log.close();
        let mut stream = TurnEventStream { log, cursor: 0 };
        assert_eq!(
            stream.next().await,
            Some(TurnEvent::ResyncRequired { missed: 1 })
        );
        assert!(matches!(
            stream.next().await,
            Some(TurnEvent::Warning { data }) if data["index"] == 1
        ));
        assert!(matches!(
            stream.next().await,
            Some(TurnEvent::Warning { data }) if data["index"] == 2
        ));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn application_requires_explicit_home_and_nonzero_event_capacity() {
        let missing = Application::builder().build().await;
        assert!(matches!(missing, Err(Error::Message(message)) if message.contains("home")));

        let temp = tempfile::tempdir().expect("tempdir");
        let invalid = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .event_capacity(0)
            .build()
            .await;
        assert!(
            matches!(invalid, Err(Error::Message(message)) if message.contains("greater than zero"))
        );
    }

    #[tokio::test]
    async fn client_materializes_lists_resumes_and_archives_threads() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        assert_eq!(thread.snapshot().await.expect("snapshot").source, "sdk");
        assert_eq!(
            client
                .resume_thread(thread.id())
                .await
                .expect("resume")
                .id(),
            thread.id()
        );
        assert_eq!(
            client
                .list_threads(ThreadListQuery::default())
                .await
                .expect("list")
                .threads
                .len(),
            1
        );
        thread.archive().await.expect("archive");
        assert!(
            client
                .list_threads(ThreadListQuery::default())
                .await
                .expect("active")
                .threads
                .is_empty()
        );
        assert_eq!(
            client
                .list_threads(ThreadListQuery {
                    archived: true,
                    ..ThreadListQuery::default()
                })
                .await
                .expect("archived")
                .threads
                .len(),
            1
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn accepted_thread_mutation_survives_a_dropped_caller() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("state.db"))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let thread_id = thread.id().to_string();
        let blocker = application
            .inner
            .state
            .begin_sqlx_write()
            .await
            .expect("write blocker");
        let caller = tokio::spawn(async move { thread.archive().await });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while application.inner.state.diagnostics().in_flight_operations == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("archive reached the state boundary");
        caller.abort();
        blocker.rollback().await.expect("release write blocker");

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let summary = application
                    .inner
                    .state
                    .session_summary(&thread_id)
                    .await
                    .expect("summary")
                    .expect("thread");
                if summary.archived_at_ms.is_some() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("runtime-owned archive completed after caller drop");
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn application_overload_rejects_start_thread_before_creating_a_session() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("state.db"))
            .build()
            .await
            .expect("application");
        let reservations = (0..runtime::MAX_APPLICATION_OPERATIONS)
            .map(|index| {
                application
                    .inner
                    .runtime
                    .reserve_mutation(&format!("occupied-{index}"))
                    .expect("fill Application capacity")
            })
            .collect::<Vec<_>>();

        let error = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect_err("sixty-fifth operation");

        assert_eq!(
            error.structured_data(),
            Some(&serde_json::json!({
                "kind": "application_overloaded",
                "scope": "application",
                "limit": runtime::MAX_APPLICATION_OPERATIONS,
            }))
        );
        assert!(
            application
                .inner
                .state
                .list_sessions_for_cwd_with_sources(temp.path(), &["sdk"])
                .await
                .expect("sessions")
                .is_empty()
        );
        drop(reservations);
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn thread_overload_rejects_archive_before_mutating_the_session() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("state.db"))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let reservations = (0..runtime::MAX_THREAD_OPERATIONS)
            .map(|_| {
                application
                    .inner
                    .runtime
                    .reserve_mutation(thread.id())
                    .expect("fill Thread capacity")
            })
            .collect::<Vec<_>>();

        let error = thread.archive().await.expect_err("thirty-third operation");

        assert_eq!(
            error.structured_data(),
            Some(&serde_json::json!({
                "kind": "application_overloaded",
                "scope": "thread",
                "limit": runtime::MAX_THREAD_OPERATIONS,
            }))
        );
        assert_eq!(
            application
                .inner
                .state
                .session_summary(thread.id())
                .await
                .expect("summary")
                .expect("thread")
                .archived_at_ms,
            None
        );
        drop(reservations);
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn client_lists_threads_in_stable_bounded_pages() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let client = application.client();
        for _ in 0..3 {
            client
                .start_thread(StartThreadRequest::new(temp.path()))
                .await
                .expect("thread");
        }

        let first = client
            .list_threads(ThreadListQuery {
                cwd: Some(temp.path().to_path_buf()),
                limit: 2,
                ..ThreadListQuery::default()
            })
            .await
            .expect("first page");
        assert_eq!(first.threads.len(), 2);
        let cursor = first.next_cursor.expect("next cursor");
        let second = client
            .list_threads(ThreadListQuery {
                cwd: Some(temp.path().to_path_buf()),
                cursor: Some(cursor.clone()),
                limit: 2,
                ..ThreadListQuery::default()
            })
            .await
            .expect("second page");
        assert_eq!(second.threads.len(), 1);
        assert!(second.next_cursor.is_none());
        assert!(
            first
                .threads
                .iter()
                .all(|thread| second.threads.iter().all(|next| next.id != thread.id))
        );

        let mismatch = client
            .list_threads(ThreadListQuery {
                archived: true,
                cursor: Some(cursor),
                ..ThreadListQuery::default()
            })
            .await
            .expect_err("cursor filter mismatch");
        assert!(
            mismatch
                .to_string()
                .contains("does not match the current filters")
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn history_reader_returns_the_latest_bounded_tail_then_older_pages() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(FailingAgentSessionAdapter))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        for index in 0..205 {
            application
                .inner
                .state
                .append_message(
                    thread.id(),
                    &psychevo_agent_core::user_text_message(format!("message-{index}")),
                )
                .await
                .expect("append history");
        }

        let latest = thread.history().latest(Some(5)).await.expect("latest page");
        assert_eq!(latest.items.len(), 5);
        assert_eq!(
            latest
                .items
                .iter()
                .map(|item| item.session_seq)
                .collect::<Vec<_>>(),
            vec![201, 202, 203, 204, 205]
        );
        assert_eq!(latest.next_before, Some(201));
        let older = thread
            .history()
            .before(latest.next_before, Some(5))
            .await
            .expect("older page");
        assert_eq!(
            older
                .items
                .iter()
                .map(|item| item.session_seq)
                .collect::<Vec<_>>(),
            vec![196, 197, 198, 199, 200]
        );
        assert!(latest.items.iter().all(|item| {
            older
                .items
                .iter()
                .all(|older| older.session_seq != item.session_seq)
        }));

        let snapshot = thread.snapshot().await.expect("bounded snapshot");
        assert_eq!(snapshot.items.len(), DEFAULT_HISTORY_PAGE_SIZE);
        assert_eq!(
            snapshot.items.first().map(|item| item.session_seq),
            Some(106)
        );
        assert_eq!(snapshot.history_cursor, Some(106));
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn shutdown_closes_new_work_admission() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let client = application.client();
        application.shutdown().await.expect("shutdown");
        let error = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect_err("closed admission");
        assert!(error.to_string().contains("shutting down"));
    }

    #[tokio::test]
    async fn graceful_shutdown_cancels_and_awaits_application_owned_agents() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let supervisor = application.inner.runtime.agent_supervisor.clone();
        let (control, receivers) = psychevo_agent_core::ControlHandle::new();
        supervisor
            .register(
                crate::agents::AgentRunRecord {
                    id: "shutdown-child".to_string(),
                    task_name: Some("shutdown-child".to_string()),
                    agent_name: "worker".to_string(),
                    task: "wait for shutdown".to_string(),
                    parent_session_id: "parent".to_string(),
                    child_session_id: None,
                    role: crate::agents::AgentInvocationRole::Subagent,
                    background: true,
                    status: crate::agents::AgentRunStatus::Running,
                    edge_status: None,
                    started_at_ms: 0,
                    ended_at_ms: None,
                    outcome: None,
                    final_answer: None,
                    error: None,
                    effective_max_spawn_depth: Some(0),
                    team_run_id: None,
                    mission_run_id: None,
                    team_name: None,
                    team_member_id: None,
                    agent_path: None,
                },
                Some(control),
                4,
            )
            .expect("register agent");
        let finalized = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finalized_in_task = finalized.clone();
        let mut agent_abort = receivers.abort_signal();
        assert!(
            supervisor
                .spawn_background(Box::pin(async move {
                    agent_abort.wait_for_abort().await;
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    finalized_in_task.store(true, std::sync::atomic::Ordering::SeqCst);
                }))
                .is_ok(),
            "spawn agent"
        );

        application.shutdown().await.expect("shutdown");

        assert!(
            finalized.load(std::sync::atomic::Ordering::SeqCst),
            "Application shutdown returned before the Agent finalizer"
        );
    }

    #[tokio::test]
    async fn concurrent_forced_shutdown_upgrades_graceful_owner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let shutdown_modes = Arc::new(Mutex::new(Vec::new()));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(ForceAwareAgentSessionAdapter {
                started: started.clone(),
                shutdown_modes: shutdown_modes.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("wait for force"))
            .await
            .expect("turn");
        started.notified().await;

        let graceful_application = application.clone();
        let graceful =
            tokio::spawn(async move { graceful_application.shutdown().await.expect("graceful") });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(
            !graceful.is_finished(),
            "graceful shutdown must be draining"
        );
        let forced = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            application.shutdown_force(),
        )
        .await;
        if forced.is_err() {
            panic!(
                "forced shutdown deadline; adapter shutdown modes: {:?}; turn interrupted: {}",
                *shutdown_modes.lock().expect("shutdown modes poisoned"),
                handle.control.inner.is_aborted()
            );
        }
        let forced_report = forced.expect("checked deadline").expect("forced shutdown");
        let graceful_report = tokio::time::timeout(std::time::Duration::from_secs(1), graceful)
            .await
            .expect("graceful owner deadline")
            .expect("graceful task");
        assert_eq!(graceful_report, forced_report);
        assert!(forced_report.forced);
        assert_eq!(
            *shutdown_modes.lock().expect("shutdown modes poisoned"),
            vec![true]
        );
        assert_eq!(
            handle.wait().await.expect("interrupted result").outcome,
            TurnOutcome::Interrupted
        );
    }

    #[tokio::test]
    async fn forced_shutdown_invokes_adapter_shutdown_before_joining_turn_tasks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(ShutdownReleasesAgentSessionAdapter {
                started: started.clone(),
                release: Arc::new(Notify::new()),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("adapter-owned release"))
            .await
            .expect("turn");
        started.notified().await;

        let report = application.shutdown_force().await.expect("forced shutdown");
        assert_eq!(report.adapter, ShutdownAdapterStatus::Completed);
        assert_eq!(report.aborted_tasks, 0);
        assert_eq!(
            handle.wait().await.expect("interrupted result").outcome,
            TurnOutcome::Interrupted
        );
    }

    #[tokio::test]
    async fn forced_shutdown_aborts_a_cancellation_safe_pending_adapter() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let database_path = temp.path().join("state.db");
        let application = Application::builder()
            .home(temp.path())
            .database_path(&database_path)
            .agent_session_adapter(Arc::new(PendingAgentSessionAdapter {
                started: started.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("pending forever"))
            .await
            .expect("turn");
        let turn_id = handle.receipt().turn_id.clone();
        started.notified().await;

        let report = application.shutdown_force().await.expect("forced shutdown");
        assert_eq!(report.adapter, ShutdownAdapterStatus::TimedOut);
        assert!(report.aborted_tasks > 0);
        assert!(report.pending_terminal_failures.is_empty());
        assert_eq!(
            handle.wait().await.expect("forced settlement").outcome,
            TurnOutcome::Interrupted
        );
        let reopened = StateRuntime::open(&database_path)
            .await
            .expect("reopen settled state");
        let terminal = reopened
            .gateway_turn_terminal(&turn_id)
            .await
            .expect("terminal query")
            .expect("durable forced terminal");
        assert_eq!(terminal.status, "interrupted");
        assert_eq!(terminal.outcome.as_deref(), Some("aborted"));
        reopened.close().await;
    }

    #[tokio::test]
    async fn fake_agent_adapter_uses_the_public_accepted_turn_and_event_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, completed) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("test prompt"))
            .await
            .expect("accepted turn");
        let mut events = handle.events();
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Accepted { receipt, .. }) if receipt.turn_id == handle.receipt().turn_id
        ));
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::ActivityChanged {
                thread_id,
                activity: ThreadActivitySnapshot {
                    running: true,
                    active_turn_id: Some(turn_id),
                    queued_turns: 0,
                    ..
                },
            }) if thread_id == thread.id() && turn_id == handle.receipt().turn_id
        ));
        started.notified().await;
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Started { thread_id, .. }) if thread_id == thread.id()
        ));
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Message {
                stage: ItemStage::Completed,
                message,
                ..
            }) if message["text"] == "fake answer"
        ));
        release.notify_one();
        let result = handle.wait().await.expect("turn result");
        assert_eq!(result.final_answer, "fake answer");
        assert_eq!(completed.load(Ordering::SeqCst), 1);
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::ActivityChanged {
                thread_id,
                activity: ThreadActivitySnapshot {
                    running: false,
                    active_turn_id: None,
                    queued_turns: 0,
                    ..
                },
            }) if thread_id == thread.id()
        ));
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Completed {
                outcome: TurnOutcome::Completed,
                ..
            })
        ));
        assert_eq!(events.next().await, None);
        let terminal = application
            .inner
            .state
            .gateway_turn_terminal(&handle.receipt().turn_id)
            .await
            .expect("terminal read")
            .expect("terminal");
        assert_eq!(terminal.status, "completed");
        assert_eq!(terminal.outcome.as_deref(), Some("normal"));
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn panicking_turn_observer_cannot_orphan_accepted_work() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, completed) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let mut request = TurnRequest::new("observer panic");
        request.adapter_options.turn_event_observer =
            Some(Arc::new(|_| panic!("observer fixture panic")));

        let handle = thread
            .start_turn(request)
            .await
            .expect("observer must not escape admission");
        started.notified().await;
        release.notify_one();
        assert_eq!(
            handle.wait().await.expect("turn result").outcome,
            TurnOutcome::Completed
        );
        assert_eq!(completed.load(Ordering::SeqCst), 1);
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn dropping_turn_handle_does_not_cancel_accepted_work() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, completed) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("keep running"))
            .await
            .expect("accepted turn");
        started.notified().await;
        drop(handle);
        release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while completed.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("accepted work completed");
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn duplicate_active_turn_id_is_rejected_without_replacing_the_original_handle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, _) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let turn_id = Uuid::now_v7().to_string();
        let mut first_request = TurnRequest::new("first");
        first_request.requested_turn_id = Some(turn_id.clone());
        let first = thread
            .start_turn(first_request)
            .await
            .expect("first accepted turn");
        started.notified().await;

        let mut duplicate_request = TurnRequest::new("duplicate");
        duplicate_request.requested_turn_id = Some(turn_id.clone());
        let duplicate = thread
            .start_turn(duplicate_request)
            .await
            .expect_err("active Turn id must be reserved atomically");
        assert!(
            duplicate.to_string().contains("already registered"),
            "{duplicate:#}"
        );

        let resumed = client
            .resume_turn(&turn_id)
            .await
            .expect("original active Turn remains registered");
        release.notify_one();
        assert_eq!(
            resumed.wait().await.expect("original turn result").outcome,
            TurnOutcome::Completed
        );
        first.wait().await.expect("first handle result");
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn thread_delete_removes_state_and_materialized_mcp_runtime() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        application
            .inner
            .runtime
            .mcp_runtime(thread.id())
            .snapshot(
                &[McpServerInput::new(
                    "repo",
                    crate::types::McpTransportInput::Unsupported {
                        kind: "test".to_string(),
                    },
                )],
                temp.path(),
                None,
                false,
            )
            .await;
        assert_eq!(application.inner.runtime.mcp_runtime_count(), 1);

        thread.delete().await.expect("delete");

        assert_eq!(application.inner.runtime.mcp_runtime_count(), 0);
        assert!(
            application
                .inner
                .state
                .session_summary(thread.id())
                .await
                .expect("deleted lookup")
                .is_none()
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn caller_cancellation_drops_only_the_turn_acceptance_receipt() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, _) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let entered = Arc::new(Notify::new());
        let accept = Arc::new(Notify::new());
        application
            .inner
            .state
            .set_gateway_turn_acceptance_barrier_for_test(entered.clone(), accept.clone());
        let turn_id = Uuid::now_v7().to_string();
        let client_turn_id = "cancelled-caller-1".to_string();
        let mut request = TurnRequest::new("caller goes away");
        request.requested_turn_id = Some(turn_id.clone());
        request.client_turn_id = Some(client_turn_id.clone());
        let caller_thread = thread.clone();
        let caller = tokio::spawn(async move { caller_thread.start_turn(request).await });
        entered.notified().await;
        #[cfg(feature = "product")]
        assert!(
            !application
                .client()
                .__activity_snapshot()
                .threads
                .contains_key(thread.id()),
            "a reservation awaiting durable acceptance is not public activity"
        );
        caller.abort();
        assert!(
            caller
                .await
                .expect_err("caller task cancelled")
                .is_cancelled()
        );

        accept.notify_one();
        started.notified().await;
        let delivery = application
            .inner
            .state
            .gateway_turn_delivery(&turn_id)
            .await
            .expect("delivery query")
            .expect("accepted delivery");
        assert_eq!(delivery.thread_id, thread.id());
        let receipts = application
            .inner
            .state
            .gateway_turn_start_receipts(thread.id())
            .await
            .expect("start receipts");
        assert!(receipts.iter().any(|receipt| {
            receipt.client_turn_id == client_turn_id && receipt.turn_id == turn_id
        }));
        let resumed = client
            .resume_turn(&turn_id)
            .await
            .expect("Application retained active Turn");
        release.notify_one();
        assert_eq!(
            resumed.wait().await.expect("turn result").final_answer,
            "fake answer"
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn failed_durable_acceptance_never_enters_public_thread_activity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(FailingAgentSessionAdapter))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let turn_id = Uuid::now_v7().to_string();
        application
            .inner
            .state
            .accept_gateway_turn(
                GatewayTurnDeliveryInput {
                    turn_id: &turn_id,
                    thread_id: thread.id(),
                    runtime_ref: "native",
                    input_json: "{}",
                    input_hash: "existing",
                },
                None,
            )
            .await
            .expect("existing durable delivery");

        let entered = Arc::new(Notify::new());
        let accept = Arc::new(Notify::new());
        application
            .inner
            .state
            .set_gateway_turn_acceptance_barrier_for_test(entered.clone(), accept.clone());
        let mut request = TurnRequest::new("duplicate durable delivery");
        request.requested_turn_id = Some(turn_id);
        let pending_thread = thread.clone();
        let pending = tokio::spawn(async move { pending_thread.start_turn(request).await });

        entered.notified().await;
        assert!(
            !thread.has_activity(),
            "pending durable acceptance must be invisible to public activity"
        );
        #[cfg(feature = "product")]
        assert!(
            !application
                .client()
                .__activity_snapshot()
                .threads
                .contains_key(thread.id())
        );

        accept.notify_one();
        pending
            .await
            .expect("acceptance task")
            .expect_err("duplicate durable delivery must fail");
        assert!(
            !thread.has_activity(),
            "failed durable acceptance must not leave public activity"
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn adapter_panic_settles_and_releases_thread_activity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(PanickingAgentSessionAdapter))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("panic"))
            .await
            .expect("accepted turn");

        let error = handle.wait().await.expect_err("panic must fail the turn");
        assert!(error.to_string().contains("panicked"));
        assert!(!thread.has_activity());
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn queued_turn_builds_snapshot_after_acquiring_thread_lane() {
        let temp = tempfile::tempdir().expect("tempdir");
        let calls = Arc::new(AtomicUsize::new(0));
        let first_started = Arc::new(Notify::new());
        let release_first = Arc::new(Notify::new());
        let second_snapshot_items = Arc::new(Mutex::new(None));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(SnapshotOrderingAgentSessionAdapter {
                calls,
                first_started: first_started.clone(),
                release_first: release_first.clone(),
                second_snapshot_items: second_snapshot_items.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let first = thread
            .start_turn(TurnRequest::new("first"))
            .await
            .expect("first turn");
        let first_turn_id = first.receipt().turn_id.clone();
        let mut first_events = first.events();
        assert!(matches!(
            first_events.next().await,
            Some(TurnEvent::Accepted { .. })
        ));
        let first_activity_revision = match first_events.next().await {
            Some(TurnEvent::ActivityChanged { activity, .. }) => activity.revision,
            event => panic!("expected first activity snapshot, got {event:?}"),
        };
        first_started.notified().await;
        let second = thread
            .start_turn(TurnRequest::new("second"))
            .await
            .expect("second turn");
        let mut second_events = second.events();
        let accepted = second_events.next().await.expect("second acceptance");
        assert!(matches!(
            accepted,
            TurnEvent::Accepted {
                queue_position: Some(1),
                ..
            }
        ));
        let queued_activity = second_events.next().await.expect("queued activity");
        let queued_activity_revision = match queued_activity {
            TurnEvent::ActivityChanged {
                activity:
                    ThreadActivitySnapshot {
                        revision,
                        running: true,
                        active_turn_id: Some(ref active_turn_id),
                        queued_turns: 1,
                    },
                ..
            } if active_turn_id == &first_turn_id => revision,
            event => panic!("expected queued activity snapshot, got {event:?}"),
        };
        assert!(queued_activity_revision > first_activity_revision);
        #[cfg(feature = "product")]
        assert!(matches!(
            application
                .client()
                .__activity_snapshot()
                .threads
                .get(thread.id()),
            Some(ThreadActivitySnapshot {
                running: true,
                active_turn_id: Some(active_turn_id),
                queued_turns: 1,
                ..
            }) if active_turn_id == &first_turn_id
        ));

        application
            .inner
            .state
            .append_message(
                thread.id(),
                &psychevo_agent_core::user_text_message("committed before first releases"),
            )
            .await
            .expect("append message");
        release_first.notify_one();
        first.wait().await.expect("first result");
        second.wait().await.expect("second result");
        assert_eq!(
            *second_snapshot_items
                .lock()
                .expect("snapshot observation poisoned"),
            Some(1)
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn turn_and_thread_mutation_reservations_share_one_fifo_and_evict_when_idle() {
        let runtime = Arc::new(ApplicationRuntime::new());
        let mutation_only_thread = "mutation-only";
        let initial_revision = runtime
            .versioned_thread_activity(mutation_only_thread)
            .revision;
        let mutation_only = runtime
            .reserve_mutation(mutation_only_thread)
            .expect("mutation-only reservation");
        assert!(
            !runtime
                .thread_activity_snapshot()
                .contains_key(mutation_only_thread),
            "Thread mutations are not public Framework Turn activity"
        );
        assert_eq!(
            runtime
                .versioned_thread_activity(mutation_only_thread)
                .revision,
            initial_revision
        );
        drop(mutation_only);
        assert_eq!(
            runtime
                .versioned_thread_activity(mutation_only_thread)
                .revision,
            initial_revision
        );

        let thread_id = "thread-operation-fifo";
        let first = runtime
            .reserve_turn_for_test(thread_id, "turn-1")
            .expect("first reservation");
        first.await.expect("first Turn is ready");
        let mut mutation = runtime
            .reserve_mutation(thread_id)
            .expect("mutation reservation");
        let mut second = runtime
            .reserve_turn_for_test(thread_id, "turn-2")
            .expect("second reservation");

        assert_eq!(
            runtime.thread_activity_snapshot().get(thread_id),
            Some(&(true, Some("turn-1".to_string()), 1))
        );
        assert!(matches!(
            mutation
                .ready
                .as_mut()
                .expect("mutation reservation")
                .try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert!(matches!(
            second.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        runtime.settle_turn(thread_id, "turn-1", None);
        assert_eq!(
            runtime.thread_activity_snapshot().get(thread_id),
            Some(&(true, Some("turn-2".to_string()), 0))
        );
        mutation
            .ready
            .take()
            .expect("mutation reservation")
            .await
            .expect("mutation is next");
        assert!(matches!(
            second.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        let revision_before_mutation_release =
            runtime.versioned_thread_activity(thread_id).revision;
        drop(mutation);
        second.await.expect("second Turn follows mutation");
        assert_eq!(
            runtime.versioned_thread_activity(thread_id).revision,
            revision_before_mutation_release,
            "an internal mutation release does not create a public activity transition"
        );
        assert_eq!(
            runtime.thread_activity_snapshot().get(thread_id),
            Some(&(true, Some("turn-2".to_string()), 0))
        );
        runtime.settle_turn(thread_id, "turn-2", None);
        assert!(!runtime.thread_activity_snapshot().contains_key(thread_id));
        assert!(
            !runtime
                .state
                .lock()
                .expect("runtime state")
                .threads
                .contains_key(thread_id),
            "idle Thread cells must be evicted"
        );
    }

    #[tokio::test]
    async fn active_and_completed_turns_reattach_and_interactions_are_durable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let adapter = Arc::new(InteractionAgentSessionAdapter {
            started: started.clone(),
            release: release.clone(),
        });
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("reattach"))
            .await
            .expect("accepted turn");
        let turn_id = handle.receipt().turn_id.clone();
        started.notified().await;

        let resumed = client.resume_turn(&turn_id).await.expect("active turn");
        assert_eq!(resumed.receipt(), handle.receipt());
        assert_eq!(
            thread
                .snapshot()
                .await
                .expect("active snapshot")
                .active_turn_id
                .as_deref(),
            Some(turn_id.as_str())
        );

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if !thread
                    .pending_interactions()
                    .await
                    .expect("pending interactions")
                    .is_empty()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("interaction persisted");

        release.notify_one();
        let result = handle.wait().await.expect("completion");
        assert_eq!(result.final_answer, "fake answer");

        let completed = client.resume_turn(&turn_id).await.expect("durable turn");
        assert_eq!(
            completed.wait().await.expect("durable result").final_answer,
            "fake answer"
        );
        assert!(
            thread
                .pending_interactions()
                .await
                .expect("resolved interactions")
                .is_empty()
        );
        let fork = thread
            .fork(ForkThreadRequest::default())
            .await
            .expect("fork");
        assert_ne!(fork.id(), thread.id());
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn failed_turn_reattaches_from_its_durable_terminal_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(FailingAgentSessionAdapter))
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("fail"))
            .await
            .expect("accepted turn");
        let turn_id = handle.receipt().turn_id.clone();
        let error = handle.wait().await.expect_err("adapter failure");
        assert!(error.to_string().contains("adapter fixture failed"));

        let resumed = client.resume_turn(&turn_id).await.expect("failed turn");
        let resumed_error = resumed.wait().await.expect_err("durable adapter failure");
        assert!(resumed_error.to_string().contains("adapter fixture failed"));
        let mut events = resumed.events();
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Accepted { .. })
        ));
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Failed { message, .. }) if message.contains("adapter fixture failed")
        ));
        assert_eq!(events.next().await, None);
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn terminal_persistence_failure_is_not_published_as_turn_completion() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, _) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("persist terminal"))
            .await
            .expect("accepted turn");
        let turn_id = handle.receipt().turn_id.clone();
        started.notified().await;
        application.inner.state.close().await;
        release.notify_one();

        let error = handle.wait().await.expect_err("persistence failure");
        assert!(
            error
                .to_string()
                .contains("failed to persist Framework Turn terminal")
        );
        assert!(matches!(
            client
                .resume_turn(&turn_id)
                .await
                .expect_err("pending terminal retry must preserve its typed failure"),
            Error::TerminalPersistence { .. }
        ));
        let mut events = handle.events();
        let mut saw_durability_warning = false;
        while let Some(event) = events.next().await {
            assert!(
                !matches!(
                    event,
                    TurnEvent::Completed { .. } | TurnEvent::Failed { .. }
                ),
                "a non-durable terminal must not be published"
            );
            if matches!(
                event,
                TurnEvent::Warning { ref data }
                    if data["kind"] == "framework_terminal_persistence"
            ) {
                saw_durability_warning = true;
            }
        }
        assert!(saw_durability_warning);
        assert!(
            !thread.has_activity(),
            "terminal persistence failure must release running/control projection"
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn resume_turn_retries_the_same_in_memory_pending_terminal_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, _) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("retry terminal"))
            .await
            .expect("accepted turn");
        let turn_id = handle.receipt().turn_id.clone();
        started.notified().await;
        application
            .inner
            .state
            .fail_next_framework_terminal_for_test();
        release.notify_one();
        assert!(matches!(
            handle.wait().await.expect_err("first terminal commit fails"),
            Error::Message(message)
                if message.contains("failed to persist Framework Turn terminal")
        ));

        let resumed = client
            .resume_turn(&turn_id)
            .await
            .expect("same-process retry succeeds");
        assert_eq!(
            resumed
                .wait()
                .await
                .expect("retried durable result")
                .final_answer,
            "fake answer"
        );
        assert!(
            application
                .inner
                .state
                .gateway_turn_terminal(&turn_id)
                .await
                .expect("terminal query")
                .is_some()
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn restart_reports_a_durable_nonterminal_delivery_as_outcome_indeterminate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let database_path = temp.path().join("state.db");
        let first = Application::builder()
            .home(temp.path())
            .database_path(&database_path)
            .agent_session_adapter(Arc::new(FailingAgentSessionAdapter))
            .build()
            .await
            .expect("first application");
        let thread = first
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let turn_id = Uuid::now_v7().to_string();
        first
            .inner
            .state
            .accept_gateway_turn(
                GatewayTurnDeliveryInput {
                    turn_id: &turn_id,
                    thread_id: thread.id(),
                    runtime_ref: "native",
                    input_json: "{}",
                    input_hash: "fixture",
                },
                None,
            )
            .await
            .expect("durable nonterminal delivery");
        first.shutdown().await.expect("first shutdown");

        let restarted = Application::builder()
            .home(temp.path())
            .database_path(&database_path)
            .agent_session_adapter(Arc::new(FailingAgentSessionAdapter))
            .build()
            .await
            .expect("restarted application");
        assert!(matches!(
            restarted
                .client()
                .resume_turn(&turn_id)
                .await
                .expect_err("unknown outcome must never be replayed or fabricated"),
            Error::OutcomeIndeterminate { turn_id: ref actual } if actual == &turn_id
        ));
        let delivery = restarted
            .inner
            .state
            .gateway_turn_delivery(&turn_id)
            .await
            .expect("delivery query")
            .expect("delivery retained");
        assert_ne!(delivery.status, "terminal");
        restarted.shutdown().await.expect("restarted shutdown");
    }

    #[tokio::test]
    async fn shutdown_waits_for_turn_admission_before_closing_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, _) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("state.db"))
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let blocker = application
            .inner
            .state
            .begin_sqlx_write()
            .await
            .expect("write blocker");
        let start =
            tokio::spawn(
                async move { thread.start_turn(TurnRequest::new("admission race")).await },
            );
        while application.inner.state.diagnostics().in_flight_operations == 0 {
            tokio::task::yield_now().await;
        }
        let shutdown_application = application.clone();
        let shutdown = tokio::spawn(async move { shutdown_application.shutdown().await });
        for _ in 0..32 {
            tokio::task::yield_now().await;
        }
        assert!(
            application.client().ensure_open().is_ok(),
            "shutdown must wait for the admitted start section"
        );
        blocker.rollback().await.expect("release write blocker");
        let handle = start
            .await
            .expect("start task")
            .expect("admitted Turn must not become a ghost rejection");
        started.notified().await;
        release.notify_one();
        handle.wait().await.expect("admitted turn result");
        shutdown.await.expect("shutdown task").expect("shutdown");
    }

    #[test]
    fn completed_message_turn_event_preserves_usage_metadata_and_accounting() {
        let event = TurnEvent::from_run_stream(RunStreamEvent::value(serde_json::json!({
            "type": "message_end",
            "message": {"role": "assistant", "content": []},
            "usage": {"input_tokens": 3},
            "metadata": {"provider": "fake"},
            "accounting": {"reported_total_tokens": 5},
        })))
        .expect("message event");
        let TurnEvent::Message {
            stage: ItemStage::Completed,
            usage,
            metadata,
            accounting,
            ..
        } = event
        else {
            panic!("expected completed message");
        };
        assert_eq!(usage, Some(serde_json::json!({"input_tokens": 3})));
        assert_eq!(metadata, Some(serde_json::json!({"provider": "fake"})));
        assert_eq!(
            accounting,
            Some(serde_json::json!({"reported_total_tokens": 5}))
        );
    }

    #[tokio::test]
    async fn application_owns_durable_permission_rendezvous() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let decision = Arc::new(Mutex::new(None));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(PermissionInteractionAgentSessionAdapter {
                started: started.clone(),
                decision: decision.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("request permission"))
            .await
            .expect("turn");
        started.notified().await;

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let pending = thread
                    .pending_interactions()
                    .await
                    .expect("pending interactions");
                if pending
                    .iter()
                    .any(|interaction| interaction.interaction_id == "permission-1")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("permission interaction persisted");

        let expected_decision =
            PermissionApprovalDecision::allow_filesystem_turn(temp.path().display().to_string());
        assert!(
            thread
                .respond(
                    "permission-1",
                    InteractionResponse::Permission(expected_decision.clone()),
                )
                .await
                .expect("permission response")
                .accepted
        );
        assert!(
            !thread
                .respond(
                    "permission-1",
                    InteractionResponse::Permission(PermissionApprovalDecision::deny()),
                )
                .await
                .expect("duplicate permission response")
                .accepted,
            "Framework permission response must be accepted exactly once"
        );
        assert_eq!(
            handle.wait().await.expect("turn result").final_answer,
            "permission accepted"
        );
        assert_eq!(
            decision
                .lock()
                .expect("observed permission decision poisoned")
                .as_ref()
                .cloned(),
            Some(expected_decision.clone())
        );
        let durable = application
            .inner
            .state
            .framework_interactions_for_thread(thread.id(), false)
            .await
            .expect("durable interactions");
        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].status, "resolved");
        assert_eq!(
            durable[0].resolution,
            Some(
                serde_json::to_value(InteractionResponse::Permission(expected_decision))
                    .expect("serialize response")
            )
        );
        assert!(
            thread
                .pending_interactions()
                .await
                .expect("resolved interactions")
                .is_empty()
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn cancelling_a_permission_interaction_wakes_its_waiter() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let decision = Arc::new(Mutex::new(None));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(PermissionInteractionAgentSessionAdapter {
                started: started.clone(),
                decision: decision.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("cancel permission"))
            .await
            .expect("turn");
        started.notified().await;
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if thread
                    .pending_interactions()
                    .await
                    .expect("pending interactions")
                    .iter()
                    .any(|interaction| interaction.interaction_id == "permission-1")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("permission interaction persisted");

        assert!(
            !thread
                .respond(
                    "permission-1",
                    InteractionResponse::Clarify(vec![vec!["wrong kind".to_string()]]),
                )
                .await
                .expect("mismatched response")
                .accepted,
            "a mismatched typed response must not terminate the durable interaction"
        );
        assert!(
            thread
                .respond("permission-1", InteractionResponse::Cancel)
                .await
                .expect("cancel response")
                .accepted
        );
        tokio::time::timeout(std::time::Duration::from_secs(1), handle.wait())
            .await
            .expect("permission waiter must wake")
            .expect("turn result");
        assert_eq!(
            decision
                .lock()
                .expect("observed permission decision poisoned")
                .as_ref()
                .map(|decision| decision.outcome),
            Some(PermissionApprovalOutcome::Deny)
        );
        let durable = application
            .inner
            .state
            .framework_interactions_for_thread(thread.id(), false)
            .await
            .expect("durable interactions");
        assert_eq!(durable[0].status, "cancelled");
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn handler_cancellation_resolves_a_durable_permission_without_a_live_receiver() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let cancel = Arc::new(Notify::new());
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(CancelledPermissionAgentSessionAdapter {
                started: started.clone(),
                cancel: cancel.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("cancel MCP startup permission"))
            .await
            .expect("turn");
        started.notified().await;
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if thread
                    .pending_interactions()
                    .await
                    .expect("pending interactions")
                    .iter()
                    .any(|interaction| interaction.interaction_id == "mcp_startup:pending")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("permission interaction persisted");

        cancel.notify_one();
        handle.wait().await.expect("turn result");

        assert!(
            thread
                .pending_interactions()
                .await
                .expect("pending interactions")
                .is_empty()
        );
        let durable = application
            .inner
            .state
            .framework_interactions_for_thread(thread.id(), false)
            .await
            .expect("durable interactions");
        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].status, "cancelled");
        assert_eq!(
            durable[0]
                .resolution
                .as_ref()
                .and_then(|value| value.get("reason").and_then(serde_json::Value::as_str)),
            Some("timed_out")
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn application_persists_and_delivers_the_same_clarify_response() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let outcome = Arc::new(Mutex::new(None));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(ClarifyInteractionAgentSessionAdapter {
                started: started.clone(),
                outcome: outcome.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("request clarification"))
            .await
            .expect("turn");
        started.notified().await;
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if thread
                    .pending_interactions()
                    .await
                    .expect("pending interactions")
                    .iter()
                    .any(|interaction| interaction.interaction_id == "clarify-application-1")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("clarify interaction persisted");

        let answers = vec![vec!["/workspace/a".to_string(), "/workspace/b".to_string()]];
        assert!(
            thread
                .respond(
                    "clarify-application-1",
                    InteractionResponse::Clarify(answers.clone()),
                )
                .await
                .expect("clarify response")
                .accepted
        );
        handle.wait().await.expect("turn result");
        assert_eq!(
            *outcome.lock().expect("observed clarify outcome poisoned"),
            Some(crate::types::ClarifyInteractionOutcome::Answered(
                ClarifyResponse {
                    answers: vec![ClarifyAnswer {
                        answers: answers[0].clone(),
                    }],
                }
            ))
        );
        let durable = application
            .inner
            .state
            .framework_interactions_for_thread(thread.id(), false)
            .await
            .expect("durable interactions");
        assert_eq!(durable.len(), 1);
        assert_eq!(
            durable[0].resolution,
            Some(
                serde_json::to_value(InteractionResponse::Clarify(answers))
                    .expect("serialize response")
            )
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn injected_provider_runs_through_the_native_framework_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::write(
            home.join("config.toml"),
            r#"
model = "fake/fake-model"

[provider.fake]
api = "http://127.0.0.1:9/v1"
no_auth = true

[provider.fake.models.fake-model]
"#,
        )
        .expect("config");
        crate::tests::seed_managed_rg(&home);
        let provider = psychevo_ai::Fake::with_language(psychevo_ai::FakeLanguageAdapter::text(
            "native fake answer",
        ))
        .expect("fake provider")
        .provider();
        let application = Application::builder()
            .home(&home)
            .provider(provider)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .expect("thread");
        let mut request = TurnRequest::new("answer through the injected provider");
        request.no_agents = true;
        request.no_skills = true;
        request.inherited_env = Some(BTreeMap::from([(
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        )]));
        let handle = thread.start_turn(request).await.expect("accepted turn");
        let result = handle.wait().await.expect("turn result");
        assert_eq!(result.final_answer, "native fake answer");
        assert_eq!(result.provider, "fake");
        assert_eq!(result.model, "fake-model");
        let snapshot = thread.snapshot().await.expect("durable snapshot");
        assert!(
            snapshot.items.iter().any(|item| {
                matches!(
                    &item.message,
                    psychevo_agent_core::Message::Assistant { content, .. }
                        if content.iter().any(|block| {
                            matches!(
                                block,
                                psychevo_agent_core::AssistantBlock::Text { text }
                                    if text == "native fake answer"
                            )
                        })
                )
            }),
            "authoritative Thread snapshot must contain the durable assistant item"
        );
        application.shutdown().await.expect("shutdown");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_framework_run_awaits_plugin_worker_shutdown() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        let source = temp.path().join("plugin");
        let shutdown_marker = temp.path().join("worker-shutdown");
        std::fs::create_dir_all(source.join(".codex-plugin")).expect("plugin manifest dir");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::write(
            source.join(".codex-plugin/plugin.json"),
            r#"{"name":"shutdown-owner","version":"1.0.0","description":"shutdown owner"}"#,
        )
        .expect("plugin manifest");
        std::fs::write(
            source.join("psychevo.plugin.json"),
            r#"{"runtime":{"worker":{"command":"./worker.py"}}}"#,
        )
        .expect("plugin overlay");
        let worker = source.join("worker.py");
        std::fs::write(
            &worker,
            format!(
                r#"#!/usr/bin/env python3
import json, pathlib, sys
for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    result = {{"tools": []}} if method == "contributions/list" else {{"ok": True}}
    print(json.dumps({{"jsonrpc": "2.0", "id": request.get("id"), "result": result}}), flush=True)
    if method == "shutdown":
        pathlib.Path({marker}).write_text("shutdown")
        break
"#,
                marker = serde_json::to_string(&shutdown_marker).expect("marker json"),
            ),
        )
        .expect("worker");
        let mut permissions = std::fs::metadata(&worker)
            .expect("worker metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&worker, permissions).expect("worker chmod");
        crate::plugins::install_plugin(
            &home,
            &cwd,
            crate::plugins::PluginInstallOptions {
                source: source.display().to_string(),
                source_kind: None,
                scope: crate::plugins::PluginScope::Global,
                git_ref: None,
                npm_version: None,
                npm_registry: None,
                force: false,
            },
        )
        .expect("plugin install");
        std::fs::write(
            home.join("config.toml"),
            r#"
model = "fake/fake-model"

[provider.fake]
api = "http://127.0.0.1:9/v1"
no_auth = true

[provider.fake.models.fake-model]

[plugins."shutdown-owner"]
enabled = true
"#,
        )
        .expect("config");
        crate::tests::seed_managed_rg(&home);
        let provider =
            psychevo_ai::Fake::with_language(psychevo_ai::FakeLanguageAdapter::text("done"))
                .expect("fake provider")
                .provider();
        let application = Application::builder()
            .home(&home)
            .provider(provider)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .expect("thread");
        let mut request = TurnRequest::new("finish normally");
        request.no_agents = true;
        request.no_skills = true;

        thread
            .start_turn(request)
            .await
            .expect("accepted turn")
            .wait()
            .await
            .expect("turn result");

        assert_eq!(
            std::fs::read_to_string(shutdown_marker).expect("shutdown marker"),
            "shutdown"
        );
        application.shutdown().await.expect("shutdown");
    }
}
