//! High-level in-process Framework interface.
//!
//! This module owns the public Thread/Turn vocabulary. The lower run assembly
//! and state Modules remain implementation details of an Application.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::sync::{Arc, Mutex, Weak};

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex as AsyncMutex, Notify};
use uuid::Uuid;

mod administration;
mod agent_session;
mod agent_tasks;
mod configuration;
mod event_log;
mod gateway_durability;
mod history_editing;
mod interaction_broker;
mod lifecycle;
mod presentation;
mod replay;
mod runtime;
mod shell;
mod structural_history;
mod thread;
mod turn;
mod turn_completion;
mod turn_events;
mod turn_request;
mod voice;

use event_log::EventLog;
use interaction_broker::InteractionBroker;
use runtime::ApplicationRuntime;

pub use administration::{
    AgentCoordinationStatus, AgentMailboxWaitOutcome, AgentMissionRegistration,
    AgentMissionRunStatus, AgentRelationship, AgentRelationshipAgent, AgentRelationshipStatus,
    AgentSource, AgentTeamRegistration, AgentTeamRunStatus, AgentUsageObservation,
    AutoCompactionRequest, RefreshThreadContextRequest, RefreshThreadContextResult,
    SetThreadMainAgentSelection, SideConversationAgentBindingSnapshot, SideConversationSurface,
    StartSideConversationRequest, ThreadAgentBinding, ThreadMainAgentSelection,
    ThreadModelSelection, ThreadRedoResult, ThreadUndoResult, ThreadUsageSummary,
    UpdateThreadAgentControlState, UsageOverview, UsageQuery, suggested_thread_title,
};
pub use agent_tasks::{AgentTaskReceipt, StartAgentTaskRequest};
pub use configuration::{
    Configuration, ConfigurationQuery, ConfigureProviderRequest, CreateCustomProviderRequest,
    CustomProviderResult, ModelMetadataCacheTarget,
};
pub use gateway_durability::{
    AutomationRunFinishInput, AutomationRunRecord, AutomationRunRecoveryCandidate,
    AutomationRunStatus, AutomationRunTerminalStatus, AutomationTaskInput, AutomationTaskKind,
    AutomationTaskRecord, GatewayActivityClaimInput, GatewayActivityKind, GatewayActivityRecord,
    GatewayActivityState, GatewayActivityTerminalStatus, GatewayChannelOutboxInput,
    GatewayChannelOutboxRecord, GatewayChannelOutboxStatus, GatewayControlCommandInput,
    GatewayControlCommandKind, GatewayControlCommandRecord, GatewayControlCommandStatus,
    GatewayDurability, GatewayLiveEventCommit, GatewayLiveEventRecord, GatewayLiveSnapshotInput,
    GatewayLiveSnapshotRecord, GatewaySourceBindingRecord, GatewaySourceLaneInput,
    GatewaySourceLaneRecord,
};
pub use history_editing::{
    ThreadConversationEditConflict, ThreadConversationEditRestoreOutcome,
    ThreadConversationEditStageOutcome, ThreadConversationEditUnavailable, ThreadEditableDraft,
    ThreadEditableDraftFidelity, ThreadEditableDraftPart, ThreadEditableDraftRead,
    ThreadEditableDraftReadOutcome, ThreadEditableDraftUnavailable,
    ThreadHistoryEditingEligibility, ThreadHistoryEditingStaged, ThreadHistoryEditingState,
    ThreadHistoryEditingUnavailable,
};
pub use presentation::{
    HumanThreadBrowserQuery, HumanThreadBrowserWorkspace, HumanThreadListPage,
    HumanThreadListQuery, HumanThreadSummary, ThreadLifecycleActionPresentation,
    ThreadLifecyclePresentation, ThreadPresentationBackend, ThreadTurnStartReceipt,
    UserShellDisplay,
};
pub use psychevo_agent_core::{
    AssistantBlock, ControlInputError, MAX_CONTROL_INPUT_BYTES as MAX_QUEUED_STEER_BYTES,
    MAX_CONTROL_INPUT_ITEMS as MAX_QUEUED_STEERS, Message, ProviderToolBlock, TerminalReason,
    ToolBinding, ToolCallBlock, ToolDisplayBodyPolicy, ToolDisplayCategory, ToolDisplaySpec,
    ToolExecutionMode, ToolOutput, UserContentBlock, user_text_message,
};
pub use psychevo_ai::{AbortSignal, AssistantSource, Outcome, UrlCitationSource};
pub use replay::{
    HistoryReplayItem, HistoryReplayPage, HistoryReplayWarning, HistoryReplayWarningKind,
};
pub use shell::{
    ShellCommand, ShellCommandControl, ShellCommandEvent, ShellCommandOutcome, ShellCommandRequest,
    ShellCommandResult,
};
pub use structural_history::{
    ThreadCompaction, ThreadStructuralHistory, ThreadTurnTerminal, ThreadTurnTerminalStatus,
};
pub use voice::{
    VoiceAudioFormat, VoiceAudioInput, VoiceAudioOutput, VoiceRealtimeCloseReason,
    VoiceRealtimeConnection, VoiceRealtimeControl, VoiceRealtimeEvent, VoiceRealtimeEvents,
    VoiceRealtimeRequest, VoiceRealtimeTransport, VoiceRealtimeVoice, VoiceSpeech,
    VoiceSpeechRequest, VoiceTranscription, VoiceTranscriptionRequest,
};

/// Validate a message retained by a caller before a `TurnHandle` exists.
///
/// This is the same byte and shape contract enforced by queued Turn steering.
pub fn validate_queued_steer(message: &Message) -> std::result::Result<(), ControlInputError> {
    psychevo_agent_core::validate_steer_message(message)
}

use crate::compaction::CompactionReason;
use crate::config::McpOAuthCredentialStore;
#[cfg(test)]
use crate::state::{FrameworkInteractionStatus, GatewayTurnDeliveryInput};
use crate::state::{SessionListCursor, StateRuntime};
pub use crate::types::{
    ApprovalHandler, BlockingActionKind, ClarifyAnswer, ClarifyInteractionOutcome, ClarifyQuestion,
    ClarifyQuestionOption, ClarifyRequestEvent, ClarifyResolvedEvent, ClarifyResolvedReason,
    ClarifyResponse, ClarifyResult, EDITABLE_INPUT_METADATA_KEY, FilesystemApprovalLifetime,
    FilesystemApprovalRequest, FilesystemApprovalScope, FilesystemApprovalTarget, ImageInput,
    McpServerInput, McpStartupApprovalRequest, McpStartupApprovalTarget, McpTransportInput,
    PermissionApprovalDecision, PermissionApprovalOutcome, PermissionApprovalRequest,
    PermissionMode, ProjectContextInstructionMode, PromptDisplayMetadata, ResolvedMcpServerInput,
    RunMode, RunSandboxMode, RunSandboxOverride, RunStreamEvent, RunStreamSink, RunTerminalError,
    RunWarning, RuntimeTool, SelectedAgent, SessionEvent, SessionEventPayload,
    StoredEditableInputEnvelope, StoredEditableInputPart, TUI_DISPLAY_METADATA_KEY,
    USER_SHELL_METADATA_KEY, WorkspaceMutation, WorkspaceMutationSink,
};
use crate::{Error, Result};

const DEFAULT_EVENT_CAPACITY: usize = 256;
const DEFAULT_THREAD_LIST_LIMIT: usize = 50;
const MAX_THREAD_LIST_LIMIT: usize = 200;
#[cfg(not(test))]
const FORCE_SHUTDOWN_TOTAL: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(test)]
const FORCE_SHUTDOWN_TOTAL: std::time::Duration = std::time::Duration::from_secs(1);
#[cfg(not(test))]
const FORCE_ADAPTER_BUDGET: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(test)]
const FORCE_ADAPTER_BUDGET: std::time::Duration = std::time::Duration::from_millis(25);
#[cfg(not(test))]
const FORCE_COOPERATIVE_JOIN_BUDGET: std::time::Duration = std::time::Duration::from_secs(6);
#[cfg(test)]
const FORCE_COOPERATIVE_JOIN_BUDGET: std::time::Duration = std::time::Duration::from_millis(100);
#[cfg(not(test))]
const FORCE_STATE_CLOSE_BUDGET: std::time::Duration = std::time::Duration::from_secs(1);
#[cfg(test)]
const FORCE_STATE_CLOSE_BUDGET: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownReport {
    pub forced: bool,
    pub adapter: ShutdownAdapterStatus,
    pub state_close: ShutdownStateCloseStatus,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ShutdownStateCloseStatus {
    Closed,
    TimedOut,
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
    native_backend: NativeTurnBackend,
    mcp_oauth_credentials: Arc<dyn McpOAuthCredentialStore>,
    home: PathBuf,
    config_path: Option<PathBuf>,
    inherited_env: BTreeMap<String, String>,
    event_capacity: usize,
    force_shutdown_requested: AtomicBool,
    force_shutdown_notify: Notify,
    shutdown_complete: Mutex<Option<ShutdownReport>>,
    shutdown_finalizer: AsyncMutex<()>,
    #[cfg(test)]
    graceful_shutdown_owner_entered: Notify,
    runtime: Arc<ApplicationRuntime>,
}

#[derive(Default)]
pub struct ApplicationBuilder {
    home: Option<PathBuf>,
    database_path: Option<PathBuf>,
    database_connection_limit: Option<u32>,
    config_path: Option<PathBuf>,
    inherited_env: Option<BTreeMap<String, String>>,
    event_capacity: Option<usize>,
    limits: Option<ApplicationLimits>,
    agent_sessions: Option<Arc<dyn AgentSessionAdapter>>,
    provider: Option<psychevo_ai::Provider>,
    mcp_oauth_credentials: Option<Arc<dyn McpOAuthCredentialStore>>,
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<ApplicationInner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationLimits {
    pub max_operations: usize,
    pub max_thread_operations: usize,
}

impl Default for ApplicationLimits {
    fn default() -> Self {
        Self {
            max_operations: 64,
            max_thread_operations: 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationPanicDiagnostic {
    pub diagnostic_id: String,
    pub actor: String,
    pub task_id: u64,
    pub payload: String,
    pub backtrace: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationOperationalSnapshot {
    pub open: bool,
    pub limits: ApplicationLimits,
    pub accepted_operations: usize,
    pub tracked_threads: usize,
    pub tracked_tasks: usize,
    pub oldest_queued: Option<ApplicationQueuedOperationSnapshot>,
    pub storage: ApplicationStorageSnapshot,
    pub task_panics: u64,
    pub panic_diagnostics: Vec<ApplicationPanicDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationQueuedOperationSnapshot {
    pub kind: String,
    pub id: String,
    pub thread_id: Option<String>,
    pub age_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationStorageSnapshot {
    pub connection_limit: u32,
    pub pool_size: u32,
    pub pool_idle: usize,
    pub in_flight_operations: u64,
    pub completed_operations: u64,
    pub failed_operations: u64,
    pub busy_operations: u64,
    pub acquire_latency_micros: u64,
    pub execute_latency_micros: u64,
}

#[derive(Debug, Clone)]
pub struct StartThreadRequest {
    pub cwd: PathBuf,
    pub source: String,
    pub metadata: Option<Value>,
    requested_id: Option<String>,
    initial_source: Option<InitialThreadSourceAssociation>,
    execution_source_key: Option<String>,
    initial_binding: Option<InitialAgentBinding>,
    initial_thread_preferences: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct InitialThreadSourceAssociation {
    pub source_key: String,
    pub source_kind: String,
    pub raw_identity: Value,
    pub visible_name: Option<String>,
    pub lineage: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialAgentBinding {
    pub agent_ref: Option<String>,
    pub agent_fingerprint: String,
    pub agent_definition_json: String,
    pub runtime_ref: String,
    pub backend_kind: String,
    pub native_kind: String,
    pub native_session_id: Option<String>,
    pub profile_fingerprint: String,
    pub profile_revision: String,
    pub profile_config_json: String,
    pub adapter_kind: String,
    pub adapter_revision: String,
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

pub trait AgentSessionAdapter: Send + Sync + fmt::Debug + 'static {
    fn prepare_turn(
        self: Arc<Self>,
        request: AgentTurnPreparation,
    ) -> BoxFuture<'static, Result<Box<dyn PreparedAgentTurn>>>;

    fn shutdown(&self, _force: bool) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn apply_thread_lifecycle(
        &self,
        _request: AgentThreadLifecycleRequest,
    ) -> BoxFuture<'static, Result<AgentThreadLifecycleOutcome>> {
        Box::pin(async { Ok(AgentThreadLifecycleOutcome::Unchanged) })
    }

    fn import_thread(
        self: Arc<Self>,
        _request: AgentThreadImportRequest,
    ) -> BoxFuture<'static, Result<AgentThreadPublication>> {
        Box::pin(async {
            Err(Error::Message(
                "this Agent Session Adapter does not support Thread import".to_string(),
            ))
        })
    }

    fn fork_thread(
        self: Arc<Self>,
        _request: AgentThreadForkRequest,
    ) -> BoxFuture<'static, Result<AgentThreadPublication>> {
        Box::pin(async {
            Err(Error::Message(
                "this Agent Session Adapter does not support Thread fork".to_string(),
            ))
        })
    }

    fn abort_thread_publication(
        &self,
        _request: AgentThreadPublicationAbortRequest,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

pub trait PreparedAgentTurn: Send + fmt::Debug {
    fn admission(&self) -> AgentAdmissionFacts {
        AgentAdmissionFacts::default()
    }

    fn invoke(
        self: Box<Self>,
        invocation: AgentTurnInvocation,
    ) -> BoxFuture<'static, Result<TurnResult>>;
}

#[derive(Debug, Clone, Default)]
pub struct AgentAdmissionFacts {
    pub initial_binding: Option<InitialAgentBinding>,
}

#[derive(Debug, Clone)]
pub struct AgentThreadLifecycleRequest {
    pub thread: ThreadExecutionContext,
    pub binding: Option<AgentBindingSnapshot>,
    pub action: AgentThreadLifecycleAction,
    pub current: AgentThreadLifecycleSnapshot,
    mcp_resolver: agent_session::AgentMcpServerResolver,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentThreadLifecycleAction {
    Archive { reason: Option<String> },
    Restore,
    Delete,
}

/// Framework-owned lifecycle facts captured before an Adapter operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentThreadLifecycleSnapshot {
    pub projection: Option<AgentImportedLifecycle>,
    pub remote_delete: AgentRemoteDeleteState,
}

impl Default for AgentThreadLifecycleSnapshot {
    fn default() -> Self {
        Self {
            projection: None,
            remote_delete: AgentRemoteDeleteState::NotRequested,
        }
    }
}

/// Durable progress of an external Agent's remote-delete protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRemoteDeleteState {
    NotRequested,
    Prepared { at_ms: i64 },
    Acknowledged { at_ms: i64 },
}

/// The only lifecycle facts an Adapter may ask the Framework to commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentThreadLifecycleOutcome {
    Unchanged,
    Projection(AgentImportedLifecycle),
    RemoteDeletePrepared { at_ms: i64 },
    RemoteDeleteAcknowledged { at_ms: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentSessionImportToken(String);

impl AgentSessionImportToken {
    pub fn unique() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct ImportAgentThreadRequest {
    pub cwd: PathBuf,
    pub source: String,
    pub preparation: AgentSessionImportToken,
}

impl ImportAgentThreadRequest {
    pub fn new(cwd: impl Into<PathBuf>, preparation: AgentSessionImportToken) -> Self {
        Self {
            cwd: cwd.into(),
            source: "sdk".to_string(),
            preparation,
        }
    }
}

#[derive(Clone)]
pub struct ImportAgentThreadResult {
    pub thread: Thread,
    pub existing: bool,
}

impl fmt::Debug for ImportAgentThreadResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImportAgentThreadResult")
            .field("thread", &self.thread)
            .field("existing", &self.existing)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct AgentThreadImportRequest {
    pub thread: ThreadExecutionContext,
    pub preparation: AgentSessionImportToken,
    mcp_resolver: agent_session::AgentMcpServerResolver,
}

#[derive(Debug, Clone)]
pub struct AgentThreadForkRequest {
    pub source: ThreadExecutionContext,
    pub destination: ThreadExecutionContext,
    pub binding: AgentBindingSnapshot,
    mcp_resolver: agent_session::AgentMcpServerResolver,
}

#[derive(Debug, Clone)]
pub struct ForkAgentThreadRequest {
    pub source: String,
}

impl Default for ForkAgentThreadRequest {
    fn default() -> Self {
        Self {
            source: "sdk".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentThreadPublication {
    pub binding: InitialAgentBinding,
    pub messages: Vec<AgentImportedMessage>,
    pub metadata: BTreeMap<String, Value>,
    pub title: Option<String>,
    pub lifecycle: AgentImportedLifecycle,
    pub history: AgentImportedHistory,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentImportedMessage {
    pub message: Message,
    pub usage: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentImportedLifecycle {
    pub target_label: String,
    pub fork: bool,
    pub delete: bool,
    pub close: bool,
    pub resume: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHistoryOwner {
    Agent,
    Process,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHistoryFidelity {
    Full,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentImportedHistory {
    pub owner: AgentHistoryOwner,
    pub fidelity: AgentHistoryFidelity,
    pub resumable: bool,
    pub hint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentThreadPublicationAbortRequest {
    pub thread: ThreadExecutionContext,
    pub binding: InitialAgentBinding,
}

pub struct AgentTurnInvocation {
    pub thread: ThreadExecutionContext,
    pub history: HistoryReader,
    pub receipt: TurnReceipt,
    pub binding: Option<AgentBindingSnapshot>,
    pub target: AgentTargetSelection,
    pub input: AgentTurnInput,
    pub model: AgentModelSelection,
    pub execution: AgentExecutionPolicy,
    pub capabilities: AgentCapabilitySelection,
    pub environment: AgentEnvironmentOverlay,
    pub persistence: Arc<dyn AgentTurnPersistence>,
    pub events: TurnEventSender,
    pub control: TurnControl,
    child_turns: AgentChildTurnDispatcher,
    mcp_resolver: agent_session::AgentMcpServerResolver,
}

#[derive(Debug, Clone)]
pub struct AgentTurnPreparation {
    pub thread: ThreadExecutionContext,
    pub binding: Option<AgentBindingSnapshot>,
    pub target: AgentTargetSelection,
    pub inherited_env: BTreeMap<String, String>,
    pub purpose: AgentTurnPurpose,
    pub native_backend: NativeTurnBackend,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentTurnPurpose {
    #[default]
    Peer,
    Child,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentPreparationToken(String);

impl AgentPreparationToken {
    pub fn unique() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct AgentTargetSelection {
    pub agent_ref: Option<String>,
    pub runtime_profile_ref: Option<String>,
    pub runtime_options: BTreeMap<String, String>,
    pub preparation: Option<AgentPreparationToken>,
    pub expected_profile_revision: Option<u64>,
    pub expected_backend_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentBindingSnapshot {
    pub thread_id: String,
    pub agent_ref: Option<String>,
    pub agent_fingerprint: String,
    pub agent_definition_json: String,
    pub runtime_ref: String,
    pub backend_kind: String,
    pub native_kind: String,
    pub native_session_id: Option<String>,
    pub cwd: String,
    pub profile_fingerprint: String,
    pub profile_revision: String,
    pub profile_config_json: String,
    pub adapter_kind: String,
    pub adapter_revision: String,
    pub binding_revision: i64,
    pub control_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentInputPart {
    Text {
        text: String,
    },
    Image {
        input: ImageInput,
    },
    Context {
        label: String,
        text: String,
        visible_to_model: bool,
    },
    Resource {
        uri: String,
        mime_type: Option<String>,
        text: Option<String>,
        blob: Option<String>,
    },
    ResourceLink {
        name: String,
        uri: String,
        description: Option<String>,
        mime_type: Option<String>,
        size: Option<i64>,
    },
}

#[derive(Debug)]
pub struct AgentTurnInput {
    pub prompt: String,
    pub image_inputs: Vec<ImageInput>,
    pub parts: Vec<AgentInputPart>,
    pub extract_prompt_image_sources: bool,
    pub prompt_display: Option<crate::types::PromptDisplayMetadata>,
}

#[derive(Debug, Clone)]
pub struct AgentModelSelection {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub include_reasoning: bool,
}

#[derive(Clone)]
pub struct AgentExecutionPolicy {
    pub source: String,
    pub config_path: Option<PathBuf>,
    pub mode: RunMode,
    pub permission_mode: Option<PermissionMode>,
    pub approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub clarify_enabled: bool,
    pub project_context: Option<ProjectContextInstructionMode>,
    pub sandbox: Option<RunSandboxOverride>,
    pub snapshot_root: Option<PathBuf>,
    pub max_context_messages: Option<usize>,
    pub workspace_mutations: Option<crate::types::WorkspaceMutationSink>,
}

pub struct AgentCapabilitySelection {
    pub no_agents: bool,
    pub no_skills: bool,
    pub selected_capability_roots: Vec<crate::extensions::SelectedCapabilityRoot>,
    pub skill_inputs: Vec<String>,
    pub mcp_servers: Vec<McpServerInput>,
    pub tools: Vec<RuntimeTool>,
    pub mcp_runtime: crate::mcp::McpRuntime,
}

#[derive(Debug, Clone)]
pub struct AgentEnvironmentOverlay {
    pub inherited_env: BTreeMap<String, String>,
}

pub trait AgentTurnPersistence: Send + Sync + fmt::Debug {
    fn confirm_delivery(&self) -> BoxFuture<'static, Result<()>>;

    fn mark_delivery_unknown(&self) -> BoxFuture<'static, Result<()>>;

    fn attach_native_session(
        &self,
        binding_revision: i64,
        native_session_id: String,
    ) -> BoxFuture<'static, Result<AgentBindingSnapshot>>;

    fn clear_agent_usage_observation(&self) -> BoxFuture<'static, Result<()>>;

    fn has_prior_terminal(&self) -> BoxFuture<'static, Result<bool>>;

    fn append_message(
        &self,
        message: psychevo_agent_core::Message,
    ) -> BoxFuture<'static, Result<()>>;

    fn append_message_with_metrics(
        &self,
        message: psychevo_agent_core::Message,
        usage: Option<Value>,
        metadata: Option<Value>,
    ) -> BoxFuture<'static, Result<()>>;

    fn set_metadata_field(
        &self,
        key: String,
        value: Option<Value>,
    ) -> BoxFuture<'static, Result<()>>;

    fn set_visible_title_if_empty(&self, title: String) -> BoxFuture<'static, Result<()>>;

    fn prior_unknown_delivery(&self) -> BoxFuture<'static, Result<Option<AgentUnknownDelivery>>>;

    fn reconcile_unknown_delivery(
        &self,
        turn_id: String,
        metadata: Value,
    ) -> BoxFuture<'static, Result<bool>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentUnknownDelivery {
    pub turn_id: String,
}

#[derive(Clone)]
struct FrameworkAgentTurnPersistence {
    state: StateRuntime,
    thread_id: String,
    turn_id: String,
    boundary_session_seq: Arc<AtomicI64>,
}

#[derive(Clone)]
pub struct NativeTurnBackend {
    state: StateRuntime,
    provider: Option<psychevo_ai::Provider>,
}

impl fmt::Debug for NativeTurnBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeTurnBackend(..)")
    }
}

#[derive(Clone)]
struct AgentChildTurnDispatcher {
    inner: Weak<ApplicationInner>,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
}

#[derive(Clone)]
struct AgentChildTurnTemplate {
    extract_prompt_image_sources: bool,
    model: AgentModelSelection,
    execution: AgentExecutionPolicy,
    capabilities: ResolvedCapabilityPlan,
    environment: AgentEnvironmentOverlay,
}

impl fmt::Debug for AgentChildTurnDispatcher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentChildTurnDispatcher")
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct TurnEventSender {
    log: Arc<EventLog>,
    interactions: InteractionBroker,
}

#[derive(Clone)]
pub struct TurnControl {
    handle: crate::types::RunControlHandle,
    abort: psychevo_ai::AbortSignal,
    interactions: InteractionBroker,
    events: TurnEventSender,
    runtime: Arc<Mutex<Option<crate::types::RunControl>>>,
}

#[derive(Clone)]
struct NativeAgentSessionAdapter;

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
    include_reasoning: bool,
    mode: RunMode,
    permission_mode: Option<PermissionMode>,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
    clarify_enabled: bool,
    inherited_env: Option<BTreeMap<String, String>>,
    project_context: Option<ProjectContextInstructionMode>,
    sandbox: Option<RunSandboxOverride>,
    no_agents: bool,
    no_skills: bool,
    skill_inputs: Vec<String>,
    mcp_servers: Vec<McpServerInput>,
    tools: Vec<RuntimeTool>,
    input_parts: Vec<AgentInputPart>,
    snapshot_root: Option<PathBuf>,
    max_context_messages: Option<usize>,
    selected_capability_roots: Vec<crate::extensions::SelectedCapabilityRoot>,
    workspace_mutations: Option<crate::types::WorkspaceMutationSink>,
    initial_thread_preferences: BTreeMap<String, String>,
    admission_mission: Option<AgentMissionRegistration>,
    target: AgentTargetSelection,
    requested_turn_id: Option<String>,
    admission_cancellation: Option<TurnAdmissionCancellation>,
}

struct ResolvedTurnPlan {
    client_turn_id: Option<String>,
    requested_turn_id: Option<String>,
    initial_thread_preferences: BTreeMap<String, String>,
    admission_mission: Option<AgentMissionRegistration>,
    target: AgentTargetSelection,
    input: AgentTurnInput,
    model: AgentModelSelection,
    execution: AgentExecutionPolicy,
    capabilities: ResolvedCapabilityPlan,
    environment: AgentEnvironmentOverlay,
    admission_cancellation: Option<TurnAdmissionCancellation>,
}

#[derive(Debug, Clone, Default)]
pub struct TurnAdmissionCancellation {
    token: tokio_util::sync::CancellationToken,
}

impl TurnAdmissionCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    pub(crate) async fn cancelled(&self) {
        self.token.cancelled().await;
    }
}

#[derive(Clone)]
struct ResolvedCapabilityPlan {
    no_agents: bool,
    no_skills: bool,
    selected_capability_roots: Vec<crate::extensions::SelectedCapabilityRoot>,
    skill_inputs: Vec<String>,
    mcp_servers: Vec<McpServerInput>,
    tools: Vec<RuntimeTool>,
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
    boundary_session_seq: Option<i64>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QueuedSteerId(psychevo_agent_core::PendingInputId);

impl QueuedSteerId {
    pub fn matches_observed_id(self, observed: u64) -> bool {
        self.0.as_u64() == observed
    }
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

/// Framework-owned durable status for a completed Turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameworkTurnTerminalStatus {
    Completed,
    Failed,
    Interrupted,
}

impl FrameworkTurnTerminalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    pub(crate) fn parse_persisted(value: &str) -> Option<Self> {
        match value {
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

/// Framework-owned durable outcome for a completed Turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameworkTurnTerminalOutcome {
    Normal,
    Stopped,
    Failed,
    Aborted,
}

impl FrameworkTurnTerminalOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
        }
    }

    pub(crate) fn parse_persisted(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "stopped" => Some(Self::Stopped),
            "failed" => Some(Self::Failed),
            "aborted" => Some(Self::Aborted),
            _ => None,
        }
    }
}

/// Narrow durable evidence used to fence a retained-live Framework terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameworkTurnTerminalEvidence {
    pub turn_id: String,
    pub thread_id: String,
    pub status: FrameworkTurnTerminalStatus,
    pub outcome: FrameworkTurnTerminalOutcome,
    pub completed_at_ms: i64,
    pub boundary_session_seq: i64,
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
    pub terminal_reason: Option<psychevo_agent_core::TerminalReason>,
    pub terminal_error: Option<crate::types::RunTerminalError>,
    pub selected_agent: Option<crate::types::SelectedAgent>,
    pub selected_skills: Vec<crate::skills::SelectedSkill>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
    pub source: String,
    pub parent_thread_id: Option<String>,
    pub cwd: String,
    pub model: String,
    pub provider: String,
    pub title: Option<String>,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub end_reason: Option<String>,
    pub archived_at_ms: Option<i64>,
    pub forked_from_thread_id: Option<String>,
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
    pub source_key: Option<String>,
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

fn gateway_terminal_facts(
    outcome: TurnOutcome,
) -> (FrameworkTurnTerminalStatus, FrameworkTurnTerminalOutcome) {
    match outcome {
        TurnOutcome::Completed => (
            FrameworkTurnTerminalStatus::Completed,
            FrameworkTurnTerminalOutcome::Normal,
        ),
        TurnOutcome::Stopped => (
            FrameworkTurnTerminalStatus::Interrupted,
            FrameworkTurnTerminalOutcome::Stopped,
        ),
        TurnOutcome::Failed => (
            FrameworkTurnTerminalStatus::Failed,
            FrameworkTurnTerminalOutcome::Failed,
        ),
        TurnOutcome::Interrupted => (
            FrameworkTurnTerminalStatus::Interrupted,
            FrameworkTurnTerminalOutcome::Aborted,
        ),
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
    Runtime {
        data: Value,
    },
    Scoped {
        thread_id: String,
        turn_id: String,
        event: Box<TurnEvent>,
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

    trait TestAgentSession: Send + Sync + fmt::Debug + 'static {
        fn run_turn(
            &self,
            invocation: AgentTurnInvocation,
        ) -> BoxFuture<'static, Result<TurnResult>>;

        fn admission_facts(&self, _request: &AgentTurnPreparation) -> AgentAdmissionFacts {
            AgentAdmissionFacts::default()
        }

        fn observe_preparation(&self) {}

        fn apply_thread_lifecycle(
            &self,
            _request: AgentThreadLifecycleRequest,
        ) -> BoxFuture<'static, Result<AgentThreadLifecycleOutcome>> {
            Box::pin(async { Ok(AgentThreadLifecycleOutcome::Unchanged) })
        }

        fn shutdown(&self, _force: bool) -> BoxFuture<'static, Result<()>> {
            Box::pin(async { Ok(()) })
        }

        fn import_thread(
            &self,
            _request: AgentThreadImportRequest,
        ) -> BoxFuture<'static, Result<AgentThreadPublication>> {
            Box::pin(async {
                Err(Error::Message(
                    "test Adapter does not support Thread import".to_string(),
                ))
            })
        }

        fn fork_thread(
            &self,
            _request: AgentThreadForkRequest,
        ) -> BoxFuture<'static, Result<AgentThreadPublication>> {
            Box::pin(async {
                Err(Error::Message(
                    "test Adapter does not support Thread fork".to_string(),
                ))
            })
        }

        fn abort_thread_publication(
            &self,
            _request: AgentThreadPublicationAbortRequest,
        ) -> BoxFuture<'static, Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    struct PreparedTestAgentTurn<T: TestAgentSession>(Arc<T>, AgentAdmissionFacts);

    impl<T: TestAgentSession> fmt::Debug for PreparedTestAgentTurn<T> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("PreparedTestAgentTurn(..)")
        }
    }

    impl<T: TestAgentSession> PreparedAgentTurn for PreparedTestAgentTurn<T> {
        fn admission(&self) -> AgentAdmissionFacts {
            self.1.clone()
        }

        fn invoke(
            self: Box<Self>,
            invocation: AgentTurnInvocation,
        ) -> BoxFuture<'static, Result<TurnResult>> {
            self.0.run_turn(invocation)
        }
    }

    impl<T: TestAgentSession> AgentSessionAdapter for T {
        fn prepare_turn(
            self: Arc<Self>,
            request: AgentTurnPreparation,
        ) -> BoxFuture<'static, Result<Box<dyn PreparedAgentTurn>>> {
            self.observe_preparation();
            let admission = self.admission_facts(&request);
            Box::pin(async move {
                Ok(Box::new(PreparedTestAgentTurn(self, admission)) as Box<dyn PreparedAgentTurn>)
            })
        }

        fn shutdown(&self, force: bool) -> BoxFuture<'static, Result<()>> {
            TestAgentSession::shutdown(self, force)
        }

        fn apply_thread_lifecycle(
            &self,
            request: AgentThreadLifecycleRequest,
        ) -> BoxFuture<'static, Result<AgentThreadLifecycleOutcome>> {
            TestAgentSession::apply_thread_lifecycle(self, request)
        }

        fn import_thread(
            self: Arc<Self>,
            request: AgentThreadImportRequest,
        ) -> BoxFuture<'static, Result<AgentThreadPublication>> {
            TestAgentSession::import_thread(self.as_ref(), request)
        }

        fn fork_thread(
            self: Arc<Self>,
            request: AgentThreadForkRequest,
        ) -> BoxFuture<'static, Result<AgentThreadPublication>> {
            TestAgentSession::fork_thread(self.as_ref(), request)
        }

        fn abort_thread_publication(
            &self,
            request: AgentThreadPublicationAbortRequest,
        ) -> BoxFuture<'static, Result<()>> {
            TestAgentSession::abort_thread_publication(self, request)
        }
    }

    #[derive(Debug)]
    struct FakeAgentSessionAdapter {
        started: Arc<Notify>,
        release: Arc<Notify>,
        completed: Arc<AtomicUsize>,
    }

    #[derive(Debug, Default)]
    struct BoundaryPersistenceAgentSessionAdapter {
        calls: AtomicUsize,
    }

    #[derive(Debug)]
    struct LifecycleAgentSessionAdapter {
        turn: Arc<FakeAgentSessionAdapter>,
        lifecycle_started: Arc<AtomicUsize>,
        lifecycle_entered: Arc<Notify>,
        release_lifecycle: Option<Arc<Notify>>,
        lifecycle_requests: Arc<Mutex<Vec<AgentThreadLifecycleRequest>>>,
        lifecycle_outcomes: Arc<Mutex<std::collections::VecDeque<AgentThreadLifecycleOutcome>>>,
        lifecycle_error: Option<String>,
    }

    #[derive(Debug)]
    struct InteractionAgentSessionAdapter {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[derive(Debug)]
    struct PreparationCountingInteractionAdapter {
        inner: InteractionAgentSessionAdapter,
        preparations: Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct PermissionInteractionAgentSessionAdapter {
        started: Arc<Notify>,
        decision: Arc<Mutex<Option<PermissionApprovalDecision>>>,
    }

    #[derive(Debug)]
    struct CountingApprovalHandler {
        calls: Arc<AtomicUsize>,
        decision: PermissionApprovalDecision,
    }

    #[derive(Debug)]
    struct CancellableApprovalHandler {
        cancellations: Arc<Mutex<Vec<(String, String)>>>,
    }

    impl ApprovalHandler for CountingApprovalHandler {
        fn request_permission(
            &self,
            _request: PermissionApprovalRequest,
        ) -> BoxFuture<'static, PermissionApprovalDecision> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let decision = self.decision.clone();
            Box::pin(async move { decision })
        }
    }

    impl ApprovalHandler for CancellableApprovalHandler {
        fn request_permission(
            &self,
            _request: PermissionApprovalRequest,
        ) -> BoxFuture<'static, PermissionApprovalDecision> {
            Box::pin(std::future::pending())
        }

        fn cancel_permission_with_reason(
            &self,
            tool_call_id: &str,
            reason: &str,
        ) -> BoxFuture<'static, ()> {
            self.cancellations
                .lock()
                .expect("raw approval cancellations poisoned")
                .push((tool_call_id.to_string(), reason.to_string()));
            Box::pin(async {})
        }
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
    struct OutcomeSequenceAgentSessionAdapter {
        outcomes: Mutex<std::collections::VecDeque<TurnOutcome>>,
    }

    #[derive(Debug)]
    struct SnapshotOrderingAgentSessionAdapter {
        calls: Arc<AtomicUsize>,
        first_started: Arc<Notify>,
        release_first: Arc<Notify>,
        second_snapshot_items: Arc<Mutex<Option<usize>>>,
    }

    #[derive(Debug)]
    struct ImportAgentSessionAdapter {
        imported: AgentThreadPublication,
        fail_import: bool,
        import_count: Arc<AtomicUsize>,
        internal_release_count: Arc<AtomicUsize>,
        abort_requests: Arc<Mutex<Vec<AgentThreadPublicationAbortRequest>>>,
        lifecycle_requests: Arc<Mutex<Vec<AgentThreadLifecycleRequest>>>,
    }

    #[derive(Debug)]
    struct ForkAgentSessionAdapter {
        source_publication: AgentThreadPublication,
        fork_publication: AgentThreadPublication,
        fork_requests: Arc<Mutex<Vec<AgentThreadForkRequest>>>,
        fork_started: Arc<Notify>,
        release_fork: Option<Arc<Notify>>,
        fork_error: Option<String>,
        abort_requests: Arc<Mutex<Vec<AgentThreadPublicationAbortRequest>>>,
    }

    impl TestAgentSession for ImportAgentSessionAdapter {
        fn run_turn(
            &self,
            _invocation: AgentTurnInvocation,
        ) -> BoxFuture<'static, Result<TurnResult>> {
            Box::pin(async {
                Err(Error::Message(
                    "import test Adapter does not run turns".to_string(),
                ))
            })
        }

        fn import_thread(
            &self,
            _request: AgentThreadImportRequest,
        ) -> BoxFuture<'static, Result<AgentThreadPublication>> {
            self.import_count.fetch_add(1, Ordering::SeqCst);
            if self.fail_import {
                self.internal_release_count.fetch_add(1, Ordering::SeqCst);
                return Box::pin(async {
                    Err(Error::Message("injected Agent load failure".to_string()))
                });
            }
            let imported = self.imported.clone();
            Box::pin(async move { Ok(imported) })
        }

        fn abort_thread_publication(
            &self,
            request: AgentThreadPublicationAbortRequest,
        ) -> BoxFuture<'static, Result<()>> {
            self.abort_requests
                .lock()
                .expect("abort requests poisoned")
                .push(request);
            Box::pin(async { Ok(()) })
        }

        fn apply_thread_lifecycle(
            &self,
            request: AgentThreadLifecycleRequest,
        ) -> BoxFuture<'static, Result<AgentThreadLifecycleOutcome>> {
            self.lifecycle_requests
                .lock()
                .expect("lifecycle requests poisoned")
                .push(request);
            Box::pin(async { Ok(AgentThreadLifecycleOutcome::Unchanged) })
        }
    }

    impl TestAgentSession for ForkAgentSessionAdapter {
        fn run_turn(
            &self,
            _invocation: AgentTurnInvocation,
        ) -> BoxFuture<'static, Result<TurnResult>> {
            Box::pin(async {
                Err(Error::Message(
                    "fork test Adapter does not run turns".to_string(),
                ))
            })
        }

        fn import_thread(
            &self,
            _request: AgentThreadImportRequest,
        ) -> BoxFuture<'static, Result<AgentThreadPublication>> {
            let publication = self.source_publication.clone();
            Box::pin(async move { Ok(publication) })
        }

        fn fork_thread(
            &self,
            request: AgentThreadForkRequest,
        ) -> BoxFuture<'static, Result<AgentThreadPublication>> {
            self.fork_requests
                .lock()
                .expect("fork requests poisoned")
                .push(request);
            self.fork_started.notify_one();
            let release_fork = self.release_fork.clone();
            let fork_error = self.fork_error.clone();
            let publication = self.fork_publication.clone();
            Box::pin(async move {
                if let Some(release_fork) = release_fork {
                    release_fork.notified().await;
                }
                if let Some(message) = fork_error {
                    return Err(Error::Message(message));
                }
                Ok(publication)
            })
        }

        fn abort_thread_publication(
            &self,
            request: AgentThreadPublicationAbortRequest,
        ) -> BoxFuture<'static, Result<()>> {
            self.abort_requests
                .lock()
                .expect("abort requests poisoned")
                .push(request);
            Box::pin(async { Ok(()) })
        }
    }

    fn imported_agent_thread() -> AgentThreadPublication {
        AgentThreadPublication {
            binding: InitialAgentBinding {
                agent_ref: Some("agent:test".to_string()),
                agent_fingerprint: "agent-fingerprint".to_string(),
                agent_definition_json: "{\"name\":\"Test Agent\"}".to_string(),
                runtime_ref: "runtime:test".to_string(),
                backend_kind: "acp".to_string(),
                native_kind: "agent_session".to_string(),
                native_session_id: Some("native-session-1".to_string()),
                profile_fingerprint: "profile-fingerprint".to_string(),
                profile_revision: "1".to_string(),
                profile_config_json: "{\"id\":\"runtime:test\"}".to_string(),
                adapter_kind: "acp".to_string(),
                adapter_revision: "1".to_string(),
            },
            messages: vec![
                AgentImportedMessage {
                    message: Message::User {
                        content: vec![UserContentBlock::text("first")],
                        timestamp_ms: 10,
                    },
                    usage: None,
                    metadata: Some(serde_json::json!({"order": 1})),
                },
                AgentImportedMessage {
                    message: Message::Assistant {
                        content: vec![AssistantBlock::Text {
                            text: "second".to_string(),
                        }],
                        timestamp_ms: 20,
                        finish_reason: Some("end_turn".to_string()),
                        outcome: Outcome::Normal,
                        model: Some("test-agent".to_string()),
                        provider: Some("acp:test".to_string()),
                    },
                    usage: Some(serde_json::json!({"inputTokens": 3})),
                    metadata: Some(serde_json::json!({"order": 2})),
                },
            ],
            metadata: BTreeMap::from([(
                "adapterFact".to_string(),
                serde_json::json!({"stable": true}),
            )]),
            title: Some("  Imported   Thread  ".to_string()),
            lifecycle: AgentImportedLifecycle {
                target_label: "Test Agent · Test Profile".to_string(),
                fork: true,
                delete: false,
                close: true,
                resume: true,
            },
            history: AgentImportedHistory {
                owner: AgentHistoryOwner::Agent,
                fidelity: AgentHistoryFidelity::Partial,
                resumable: true,
                hint: Some("one replay fact was unavailable".to_string()),
            },
        }
    }

    fn imported_agent_thread_with_native_session(
        native_session_id: &str,
    ) -> AgentThreadPublication {
        let mut publication = imported_agent_thread();
        publication.binding.native_session_id = Some(native_session_id.to_string());
        publication
    }

    struct ForkAdapterFixture {
        adapter: Arc<ForkAgentSessionAdapter>,
        fork_started: Arc<Notify>,
        fork_requests: Arc<Mutex<Vec<AgentThreadForkRequest>>>,
        abort_requests: Arc<Mutex<Vec<AgentThreadPublicationAbortRequest>>>,
    }

    fn fork_adapter(
        release_fork: Option<Arc<Notify>>,
        fork_error: Option<&str>,
    ) -> ForkAdapterFixture {
        let fork_started = Arc::new(Notify::new());
        let fork_requests = Arc::new(Mutex::new(Vec::new()));
        let abort_requests = Arc::new(Mutex::new(Vec::new()));
        ForkAdapterFixture {
            adapter: Arc::new(ForkAgentSessionAdapter {
                source_publication: imported_agent_thread_with_native_session("native-source"),
                fork_publication: imported_agent_thread_with_native_session("native-fork"),
                fork_requests: Arc::clone(&fork_requests),
                fork_started: Arc::clone(&fork_started),
                release_fork,
                fork_error: fork_error.map(str::to_string),
                abort_requests: Arc::clone(&abort_requests),
            }),
            fork_started,
            fork_requests,
            abort_requests,
        }
    }

    struct ImportAdapterFixture {
        adapter: Arc<ImportAgentSessionAdapter>,
        import_count: Arc<AtomicUsize>,
        internal_release_count: Arc<AtomicUsize>,
        abort_requests: Arc<Mutex<Vec<AgentThreadPublicationAbortRequest>>>,
        lifecycle_requests: Arc<Mutex<Vec<AgentThreadLifecycleRequest>>>,
    }

    fn import_adapter(fail_import: bool) -> ImportAdapterFixture {
        let import_count = Arc::new(AtomicUsize::new(0));
        let internal_release_count = Arc::new(AtomicUsize::new(0));
        let abort_requests = Arc::new(Mutex::new(Vec::new()));
        let lifecycle_requests = Arc::new(Mutex::new(Vec::new()));
        ImportAdapterFixture {
            adapter: Arc::new(ImportAgentSessionAdapter {
                imported: imported_agent_thread(),
                fail_import,
                import_count: import_count.clone(),
                internal_release_count: internal_release_count.clone(),
                abort_requests: abort_requests.clone(),
                lifecycle_requests: lifecycle_requests.clone(),
            }),
            import_count,
            internal_release_count,
            abort_requests,
            lifecycle_requests,
        }
    }

    #[tokio::test]
    async fn agent_import_atomically_publishes_identity_order_and_fidelity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = import_adapter(false);
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(fixture.adapter)
            .build()
            .await
            .expect("application");
        let mut request =
            ImportAgentThreadRequest::new(temp.path(), AgentSessionImportToken::unique());
        request.source = "web".to_string();
        let imported = application
            .client()
            .import_agent_thread(request)
            .await
            .expect("import");

        assert!(!imported.existing);
        assert_eq!(fixture.import_count.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.internal_release_count.load(Ordering::SeqCst), 0);
        assert!(
            fixture
                .abort_requests
                .lock()
                .expect("aborts poisoned")
                .is_empty()
        );
        let snapshot = imported.thread.snapshot().await.expect("snapshot");
        assert_eq!(snapshot.source, "web");
        assert_eq!(snapshot.title.as_deref(), Some("Imported Thread"));
        assert_eq!(snapshot.message_count, 2);
        assert_eq!(
            snapshot
                .items
                .iter()
                .map(|item| item.metadata.as_ref().unwrap()["order"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(snapshot.items[1].usage.as_ref().unwrap()["inputTokens"], 3);
        let metadata = application
            .inner
            .state
            .session_metadata(imported.thread.id())
            .await
            .expect("metadata read")
            .expect("metadata");
        assert_eq!(metadata["adapterFact"]["stable"], true);
        assert_eq!(
            metadata["agentSessionLifecycle"]["targetLabel"],
            "Test Agent · Test Profile"
        );
        assert_eq!(metadata["agentSessionLifecycle"]["fork"], true);
        assert_eq!(metadata["agentSessionHistory"]["owner"], "agent");
        assert_eq!(metadata["agentSessionHistory"]["fidelity"], "partial");
        assert_eq!(metadata["agentSessionHistory"]["resumable"], true);

        imported.thread.archive().await.expect("archive");
        {
            let lifecycle = fixture
                .lifecycle_requests
                .lock()
                .expect("lifecycle poisoned");
            let binding = lifecycle[0].binding.as_ref().expect("binding snapshot");
            assert_eq!(binding.thread_id, imported.thread.id());
            assert_eq!(binding.runtime_ref, "runtime:test");
            assert_eq!(
                binding.native_session_id.as_deref(),
                Some("native-session-1")
            );
            assert_eq!(
                lifecycle[0].current.projection.as_ref(),
                Some(&AgentImportedLifecycle {
                    target_label: "Test Agent · Test Profile".to_string(),
                    fork: true,
                    delete: false,
                    close: true,
                    resume: true,
                })
            );
            assert_eq!(
                lifecycle[0].current.remote_delete,
                AgentRemoteDeleteState::NotRequested
            );
        }
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn agent_import_load_failure_self_releases_and_publishes_no_thread() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = import_adapter(true);
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(fixture.adapter)
            .build()
            .await
            .expect("application");

        let error = application
            .client()
            .import_agent_thread(ImportAgentThreadRequest::new(
                temp.path(),
                AgentSessionImportToken::unique(),
            ))
            .await
            .expect_err("load failure");
        assert!(error.to_string().contains("injected Agent load failure"));
        assert_eq!(fixture.import_count.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.internal_release_count.load(Ordering::SeqCst), 1);
        assert!(
            fixture
                .abort_requests
                .lock()
                .expect("aborts poisoned")
                .is_empty()
        );
        assert!(
            application
                .client()
                .list_threads(ThreadListQuery::default())
                .await
                .expect("list")
                .threads
                .is_empty()
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn agent_import_commit_failure_aborts_resident_without_remote_delete() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = import_adapter(false);
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(fixture.adapter)
            .build()
            .await
            .expect("application");
        application
            .inner
            .state
            .fail_next_agent_thread_import_commit();

        let error = application
            .client()
            .import_agent_thread(ImportAgentThreadRequest::new(
                temp.path(),
                AgentSessionImportToken::unique(),
            ))
            .await
            .expect_err("commit failure");
        assert!(
            error
                .to_string()
                .contains("injected Agent Thread import commit failure")
        );
        {
            let aborts = fixture.abort_requests.lock().expect("aborts poisoned");
            assert_eq!(aborts.len(), 1);
            assert_eq!(aborts[0].binding.runtime_ref, "runtime:test");
            assert_eq!(
                aborts[0].binding.native_session_id.as_deref(),
                Some("native-session-1")
            );
        }
        let skipped_remote_delete = fixture
            .lifecycle_requests
            .lock()
            .expect("lifecycle poisoned")
            .iter()
            .all(|request| !matches!(request.action, AgentThreadLifecycleAction::Delete));
        assert!(
            skipped_remote_delete,
            "import rollback must never become remote delete"
        );
        assert!(
            application
                .client()
                .list_threads(ThreadListQuery::default())
                .await
                .expect("list")
                .threads
                .is_empty()
        );
        assert!(
            application
                .inner
                .state
                .gateway_runtime_binding_by_native_session("runtime:test", "native-session-1")
                .await
                .expect("binding read")
                .is_none()
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn concurrent_duplicate_agent_import_returns_the_single_binding_winner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = import_adapter(false);
        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("state.db"))
            .agent_session_adapter(fixture.adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let first = client.import_agent_thread(ImportAgentThreadRequest::new(
            temp.path(),
            AgentSessionImportToken::unique(),
        ));
        let second = client.import_agent_thread(ImportAgentThreadRequest::new(
            temp.path(),
            AgentSessionImportToken::unique(),
        ));
        let (first, second) = tokio::join!(first, second);
        let first = first.expect("first import");
        let second = second.expect("second import");

        assert_eq!(first.thread.id(), second.thread.id());
        assert_ne!(first.existing, second.existing);
        assert_eq!(fixture.import_count.load(Ordering::SeqCst), 2);
        assert_eq!(
            fixture
                .abort_requests
                .lock()
                .expect("aborts poisoned")
                .len(),
            1
        );
        let listed = client
            .list_threads(ThreadListQuery::default())
            .await
            .expect("list");
        assert_eq!(listed.threads.len(), 1);
        assert_eq!(listed.threads[0].id, first.thread.id());
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn agent_import_uses_one_thread_mutation_lane_without_self_deadlock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = import_adapter(false);
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(fixture.adapter)
            .build()
            .await
            .expect("application");
        let imported = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            application
                .client()
                .import_agent_thread(ImportAgentThreadRequest::new(
                    temp.path(),
                    AgentSessionImportToken::unique(),
                )),
        )
        .await
        .expect("import mutation lane timeout")
        .expect("import");
        assert_eq!(
            imported
                .thread
                .snapshot()
                .await
                .expect("snapshot")
                .message_count,
            2
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn agent_fork_publishes_only_after_adapter_readiness_and_preserves_parent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let release_fork = Arc::new(Notify::new());
        let fixture = fork_adapter(Some(Arc::clone(&release_fork)), None);
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(fixture.adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let source = client
            .import_agent_thread(ImportAgentThreadRequest::new(
                temp.path(),
                AgentSessionImportToken::unique(),
            ))
            .await
            .expect("source import")
            .thread;

        let source_for_fork = source.clone();
        let fork = tokio::spawn(async move {
            source_for_fork
                .fork_agent(ForkAgentThreadRequest {
                    source: "web".to_string(),
                })
                .await
        });
        fixture.fork_started.notified().await;
        let request = fixture
            .fork_requests
            .lock()
            .expect("fork requests poisoned")[0]
            .clone();
        assert_eq!(request.source.id, source.id());
        assert_eq!(request.destination.source, "web");
        assert!(
            client
                .thread_summary(&request.destination.id)
                .await
                .expect("destination summary before readiness")
                .is_none(),
            "the Framework must not publish the destination before the Adapter returns readiness"
        );
        assert!(
            client
                .agent_thread_by_native_session("runtime:test", "native-fork")
                .await
                .expect("destination binding before readiness")
                .is_none()
        );

        release_fork.notify_one();
        let forked = fork.await.expect("fork task").expect("Agent fork");
        assert_eq!(forked.id(), request.destination.id);
        let snapshot = forked.snapshot().await.expect("fork snapshot");
        assert_eq!(snapshot.parent_thread_id.as_deref(), Some(source.id()));
        assert_eq!(snapshot.forked_from_thread_id.as_deref(), Some(source.id()));
        assert_eq!(snapshot.source, "web");
        assert_eq!(snapshot.message_count, 2);
        assert_eq!(
            client
                .agent_thread_by_native_session("runtime:test", "native-fork")
                .await
                .expect("published native binding")
                .expect("fork binding")
                .id(),
            forked.id()
        );
        assert!(
            fixture
                .abort_requests
                .lock()
                .expect("aborts poisoned")
                .is_empty()
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn agent_fork_adapter_failure_publishes_no_destination() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = fork_adapter(None, Some("injected Agent fork failure"));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(fixture.adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let source = client
            .import_agent_thread(ImportAgentThreadRequest::new(
                temp.path(),
                AgentSessionImportToken::unique(),
            ))
            .await
            .expect("source import")
            .thread;

        let error = source
            .fork_agent(ForkAgentThreadRequest::default())
            .await
            .expect_err("Adapter fork failure");
        assert!(error.to_string().contains("injected Agent fork failure"));
        let destination_id = fixture
            .fork_requests
            .lock()
            .expect("fork requests poisoned")[0]
            .destination
            .id
            .clone();
        assert!(
            client
                .thread_summary(&destination_id)
                .await
                .expect("destination summary")
                .is_none()
        );
        assert!(
            client
                .agent_thread_by_native_session("runtime:test", "native-fork")
                .await
                .expect("destination binding")
                .is_none()
        );
        assert!(
            fixture
                .abort_requests
                .lock()
                .expect("aborts poisoned")
                .is_empty()
        );
        assert_eq!(
            client
                .list_threads(ThreadListQuery::default())
                .await
                .expect("Thread list")
                .threads
                .len(),
            1
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn agent_fork_commit_failure_aborts_the_prepared_publication() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = fork_adapter(None, None);
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(fixture.adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let source = client
            .import_agent_thread(ImportAgentThreadRequest::new(
                temp.path(),
                AgentSessionImportToken::unique(),
            ))
            .await
            .expect("source import")
            .thread;
        application
            .inner
            .state
            .fail_next_agent_thread_import_commit();

        let error = source
            .fork_agent(ForkAgentThreadRequest::default())
            .await
            .expect_err("commit failure");
        assert!(
            error
                .to_string()
                .contains("injected Agent Thread import commit failure")
        );
        let destination_id = fixture
            .fork_requests
            .lock()
            .expect("fork requests poisoned")[0]
            .destination
            .id
            .clone();
        {
            let aborts = fixture.abort_requests.lock().expect("aborts poisoned");
            assert_eq!(aborts.len(), 1);
            assert_eq!(aborts[0].thread.id, destination_id);
            assert_eq!(
                aborts[0].binding.native_session_id.as_deref(),
                Some("native-fork")
            );
        }
        assert!(
            client
                .thread_summary(&destination_id)
                .await
                .expect("destination summary")
                .is_none()
        );
        assert!(
            client
                .agent_thread_by_native_session("runtime:test", "native-fork")
                .await
                .expect("destination binding")
                .is_none()
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn native_agent_session_lookup_returns_the_bound_thread_or_none() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = fork_adapter(None, None);
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(fixture.adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let source = client
            .import_agent_thread(ImportAgentThreadRequest::new(
                temp.path(),
                AgentSessionImportToken::unique(),
            ))
            .await
            .expect("source import")
            .thread;

        assert_eq!(
            client
                .agent_thread_by_native_session(" runtime:test ", " native-source ")
                .await
                .expect("native lookup")
                .expect("bound Thread")
                .id(),
            source.id()
        );
        assert!(
            client
                .agent_thread_by_native_session("runtime:test", "missing-native-session")
                .await
                .expect("missing native lookup")
                .is_none()
        );
        application.shutdown().await.expect("shutdown");
    }

    #[test]
    fn public_turn_outcomes_map_to_distinct_gateway_terminal_facts() {
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Completed),
            (
                FrameworkTurnTerminalStatus::Completed,
                FrameworkTurnTerminalOutcome::Normal,
            )
        );
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Stopped),
            (
                FrameworkTurnTerminalStatus::Interrupted,
                FrameworkTurnTerminalOutcome::Stopped,
            )
        );
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Failed),
            (
                FrameworkTurnTerminalStatus::Failed,
                FrameworkTurnTerminalOutcome::Failed,
            )
        );
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Interrupted),
            (
                FrameworkTurnTerminalStatus::Interrupted,
                FrameworkTurnTerminalOutcome::Aborted,
            )
        );
    }

    #[test]
    fn non_clean_shutdown_report_is_a_teardown_error() {
        let report = ShutdownReport {
            forced: true,
            adapter: ShutdownAdapterStatus::TimedOut,
            state_close: ShutdownStateCloseStatus::TimedOut,
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
        assert!(message.contains("stateClose"));
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

    impl TestAgentSession for ForceAwareAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
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

    impl TestAgentSession for ShutdownReleasesAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
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

    impl TestAgentSession for PendingAgentSessionAdapter {
        fn run_turn(
            &self,
            _request: AgentTurnInvocation,
        ) -> BoxFuture<'static, Result<TurnResult>> {
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

    impl TestAgentSession for FailingAgentSessionAdapter {
        fn run_turn(
            &self,
            _request: AgentTurnInvocation,
        ) -> BoxFuture<'static, Result<TurnResult>> {
            Box::pin(async { Err(Error::Message("adapter fixture failed".to_string())) })
        }
    }

    impl TestAgentSession for PanickingAgentSessionAdapter {
        fn run_turn(
            &self,
            _request: AgentTurnInvocation,
        ) -> BoxFuture<'static, Result<TurnResult>> {
            Box::pin(async { panic!("adapter fixture panic") })
        }
    }

    impl TestAgentSession for OutcomeSequenceAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
            let outcome = self
                .outcomes
                .lock()
                .expect("outcome sequence poisoned")
                .pop_front()
                .expect("outcome sequence exhausted");
            Box::pin(async move {
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome,
                    final_answer: format!("{outcome:?}"),
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

    impl TestAgentSession for SnapshotOrderingAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
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

    impl TestAgentSession for InteractionAgentSessionAdapter {
        fn admission_facts(&self, request: &AgentTurnPreparation) -> AgentAdmissionFacts {
            AgentAdmissionFacts {
                initial_binding: Some(InitialAgentBinding {
                    agent_ref: None,
                    agent_fingerprint: "agent-fingerprint".to_string(),
                    agent_definition_json: "null".to_string(),
                    runtime_ref: "native".to_string(),
                    backend_kind: "native".to_string(),
                    native_kind: "native".to_string(),
                    native_session_id: Some(request.thread.id.clone()),
                    profile_fingerprint: "profile-fingerprint".to_string(),
                    profile_revision: "profile-revision".to_string(),
                    profile_config_json: "{}".to_string(),
                    adapter_kind: "native".to_string(),
                    adapter_revision: "test".to_string(),
                }),
            }
        }

        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
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

    impl TestAgentSession for PreparationCountingInteractionAdapter {
        fn observe_preparation(&self) {
            self.preparations.fetch_add(1, Ordering::SeqCst);
        }

        fn admission_facts(&self, request: &AgentTurnPreparation) -> AgentAdmissionFacts {
            self.inner.admission_facts(request)
        }

        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
            self.inner.run_turn(request)
        }
    }

    impl TestAgentSession for PermissionInteractionAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let observed_decision = self.decision.clone();
            Box::pin(async move {
                let handler = request
                    .execution
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

    impl TestAgentSession for CancelledPermissionAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let cancel = self.cancel.clone();
            Box::pin(async move {
                let handler = request
                    .execution
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

    impl TestAgentSession for ClarifyInteractionAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let observed_outcome = self.outcome.clone();
            Box::pin(async move {
                started.notify_one();
                let outcome = request
                    .control
                    .request_clarification(crate::types::ClarifyRequestEvent {
                        call_id: "clarify-application-1".to_string(),
                        questions: vec![crate::types::ClarifyQuestion {
                            header: "Target".to_string(),
                            question: "Which directories?".to_string(),
                            options: Vec::new(),
                            multiple: true,
                            custom: true,
                            secret: false,
                        }],
                    })
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

    impl TestAgentSession for FakeAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
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

    impl TestAgentSession for BoundaryPersistenceAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                request
                    .persistence
                    .append_message(user_text_message(format!("persisted turn {call}")))
                    .await?;
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

    impl TestAgentSession for LifecycleAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnInvocation) -> BoxFuture<'static, Result<TurnResult>> {
            self.turn.run_turn(request)
        }

        fn apply_thread_lifecycle(
            &self,
            request: AgentThreadLifecycleRequest,
        ) -> BoxFuture<'static, Result<AgentThreadLifecycleOutcome>> {
            let lifecycle_started = Arc::clone(&self.lifecycle_started);
            let lifecycle_entered = Arc::clone(&self.lifecycle_entered);
            let release_lifecycle = self.release_lifecycle.clone();
            let lifecycle_requests = Arc::clone(&self.lifecycle_requests);
            let lifecycle_outcomes = Arc::clone(&self.lifecycle_outcomes);
            let lifecycle_error = self.lifecycle_error.clone();
            Box::pin(async move {
                lifecycle_requests
                    .lock()
                    .expect("lifecycle requests poisoned")
                    .push(request);
                lifecycle_started.fetch_add(1, Ordering::SeqCst);
                lifecycle_entered.notify_one();
                if let Some(release_lifecycle) = release_lifecycle {
                    release_lifecycle.notified().await;
                }
                if let Some(message) = lifecycle_error {
                    return Err(Error::Message(message));
                }
                Ok(lifecycle_outcomes
                    .lock()
                    .expect("lifecycle outcomes poisoned")
                    .pop_front()
                    .unwrap_or(AgentThreadLifecycleOutcome::Unchanged))
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
    async fn application_requires_explicit_home_and_valid_positive_limits() {
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

        let invalid = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .limits(ApplicationLimits {
                max_operations: 0,
                max_thread_operations: 0,
            })
            .build()
            .await;
        assert!(
            matches!(invalid, Err(Error::Message(message)) if message.contains("operation limits"))
        );

        let invalid = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .database_connection_limit(0)
            .build()
            .await;
        assert!(
            matches!(invalid, Err(Error::Message(message)) if message.contains("database connection limit"))
        );

        let invalid = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .limits(ApplicationLimits {
                max_operations: 2,
                max_thread_operations: 3,
            })
            .build()
            .await;
        assert!(
            matches!(invalid, Err(Error::Message(message)) if message.contains("cannot exceed"))
        );

        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("limited-state.db"))
            .database_connection_limit(2)
            .build()
            .await
            .expect("application with limited storage pool");
        let storage = application.operational_snapshot().storage;
        assert_eq!(storage.connection_limit, 2);
        assert!(storage.pool_size <= storage.connection_limit);
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn application_owns_one_build_time_base_environment() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .inherited_environment(BTreeMap::from([
                ("BASE_ONLY".to_string(), "captured".to_string()),
                (
                    "PSYCHEVO_HOME".to_string(),
                    "must-be-overridden".to_string(),
                ),
            ]))
            .build()
            .await
            .expect("application");
        let client = application.client();
        let expected_home = temp.path().to_string_lossy().into_owned();

        let inherited = client.application_environment(None);
        assert_eq!(
            inherited.get("BASE_ONLY").map(String::as_str),
            Some("captured")
        );
        assert_eq!(
            inherited.get("PSYCHEVO_HOME").map(String::as_str),
            Some(expected_home.as_str())
        );

        let explicit = client.application_environment(Some(BTreeMap::from([(
            "EXPLICIT_ONLY".to_string(),
            "request".to_string(),
        )])));
        assert!(!explicit.contains_key("BASE_ONLY"));
        assert_eq!(
            explicit.get("EXPLICIT_ONLY").map(String::as_str),
            Some("request")
        );
        assert_eq!(
            explicit.get("PSYCHEVO_HOME").map(String::as_str),
            Some(expected_home.as_str())
        );

        application.shutdown().await.expect("shutdown");
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
        let persisted = application
            .inner
            .state
            .session_summary(thread.id())
            .await
            .expect("persisted summary")
            .expect("thread summary");
        let snapshot = thread.snapshot().await.expect("snapshot");
        assert_eq!(snapshot.source, "sdk");
        assert_eq!(snapshot.parent_thread_id, persisted.parent_session_id);
        assert_eq!(snapshot.model, persisted.model);
        assert_eq!(snapshot.provider, persisted.provider);
        assert_eq!(snapshot.ended_at_ms, persisted.ended_at_ms);
        assert_eq!(snapshot.end_reason, persisted.end_reason);
        assert_eq!(snapshot.archived_at_ms, persisted.archived_at_ms);
        assert_eq!(
            snapshot.forked_from_thread_id,
            persisted.forked_from_thread_id
        );
        assert_eq!(snapshot.tool_call_count, persisted.tool_call_count);
        assert_eq!(
            thread
                .usage_summary()
                .await
                .expect("Thread usage summary")
                .session_id,
            thread.id()
        );
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
        let archived = client
            .list_threads(ThreadListQuery {
                archived: true,
                ..ThreadListQuery::default()
            })
            .await
            .expect("archived")
            .threads;
        assert_eq!(archived.len(), 1);
        assert!(archived[0].archived);
        assert!(archived[0].archived_at_ms.is_some());
        thread.restore().await.expect("restore");
        assert_eq!(
            client
                .list_threads(ThreadListQuery::default())
                .await
                .expect("restored")
                .threads
                .len(),
            1
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn thread_administration_owns_metadata_backed_surface_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let client = application.client();
        let mut parent_request = StartThreadRequest::new(temp.path());
        parent_request.metadata = Some(serde_json::json!({
            "context_limit": 131_072,
            "agent": { "name": "base-agent" },
        }));
        let parent = client
            .start_thread(parent_request)
            .await
            .expect("parent Thread");
        let child_id = application
            .inner
            .state
            .create_child_session_with_metadata(
                parent.id(),
                temp.path(),
                "agent",
                "child-model",
                "child-provider",
                None,
            )
            .await
            .expect("child Thread");
        application
            .inner
            .state
            .upsert_agent_edge(
                parent.id(),
                &child_id,
                crate::state::store_agents::AgentEdgeStatus::Open,
                None,
            )
            .await
            .expect("Agent edge");
        let child = client.resume_thread(child_id).await.expect("child handle");

        assert_eq!(
            child
                .context_limit_with_parent_fallback()
                .await
                .expect("context limit"),
            Some(131_072)
        );
        assert_eq!(
            parent
                .main_agent_selection()
                .await
                .expect("initial main Agent"),
            ThreadMainAgentSelection::Missing {
                base_agent: Some("base-agent".to_string()),
            }
        );
        assert_eq!(
            parent
                .set_main_agent_selection(SetThreadMainAgentSelection::Default)
                .await
                .expect("default main Agent"),
            ThreadMainAgentSelection::Default {
                base_agent: Some("base-agent".to_string()),
            }
        );
        assert_eq!(
            parent
                .set_main_agent_selection(SetThreadMainAgentSelection::Agent {
                    input: " reviewer ".to_string(),
                    name: "reviewer".to_string(),
                    source: AgentSource::BuiltIn,
                    path: None,
                })
                .await
                .expect("named main Agent"),
            ThreadMainAgentSelection::Agent {
                input: "reviewer".to_string(),
            }
        );
        assert_eq!(
            parent
                .main_agent_selection()
                .await
                .expect("persisted main Agent"),
            ThreadMainAgentSelection::Agent {
                input: "reviewer".to_string(),
            }
        );

        let selected = parent
            .set_model_selection(ThreadModelSelection {
                provider: " fake ".to_string(),
                model: " model-v2 ".to_string(),
                reasoning_effort: Some("high".to_string()),
            })
            .await
            .expect("model selection");
        assert_eq!(selected.provider, "fake");
        assert_eq!(selected.model, "model-v2");
        assert_eq!(selected.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(
            parent
                .model_selection()
                .await
                .expect("Thread model selection"),
            Some(selected.clone())
        );
        assert_eq!(
            client
                .thread_model_selection(parent.id())
                .await
                .expect("Client model selection"),
            Some(selected.clone())
        );
        assert!(
            client
                .thread_model_selection("missing-thread")
                .await
                .expect("missing model selection")
                .is_none()
        );
        let summary = parent.summary().await.expect("updated summary");
        assert_eq!(summary.provider, "fake");
        assert_eq!(summary.model, "model-v2");
        let summaries = client
            .thread_summaries(&[
                child.id().to_string(),
                parent.id().to_string(),
                "missing-thread".to_string(),
                parent.id().to_string(),
            ])
            .await
            .expect("batched Thread summaries");
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[parent.id()].provider, "fake");
        assert_eq!(summaries[parent.id()].model, "model-v2");
        assert_eq!(summaries[child.id()].provider, "child-provider");
        assert_eq!(summaries[child.id()].model, "child-model");
        assert!(
            client
                .thread_summaries(&[])
                .await
                .expect("empty Thread summary batch")
                .is_empty()
        );
        let oversized = (0..=200)
            .map(|index| format!("thread-{index}"))
            .collect::<Vec<_>>();
        assert!(
            client
                .thread_summaries(&oversized)
                .await
                .expect_err("oversized Thread summary batch")
                .to_string()
                .contains("200-Thread limit")
        );
        let metadata = application
            .inner
            .state
            .session_metadata(parent.id())
            .await
            .expect("metadata")
            .expect("Thread metadata");
        assert_eq!(metadata["composerModel"]["model"], "fake/model-v2");
        assert_eq!(metadata["composerModel"]["reasoningEffort"], "high");
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn adapter_lifecycle_cleanup_runs_in_the_thread_mutation_lane() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (turn_adapter, turn_started, release_turn, _) = fake_adapter();
        let lifecycle_started = Arc::new(AtomicUsize::new(0));
        let lifecycle_entered = Arc::new(Notify::new());
        let release_lifecycle = Arc::new(Notify::new());
        let lifecycle_requests = Arc::new(Mutex::new(Vec::new()));
        let adapter = Arc::new(LifecycleAgentSessionAdapter {
            turn: turn_adapter,
            lifecycle_started: Arc::clone(&lifecycle_started),
            lifecycle_entered: Arc::clone(&lifecycle_entered),
            release_lifecycle: Some(Arc::clone(&release_lifecycle)),
            lifecycle_requests: Arc::clone(&lifecycle_requests),
            lifecycle_outcomes: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            lifecycle_error: None,
        });
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
        let turn = thread
            .start_turn(TurnRequest::new("hold the Thread lane"))
            .await
            .expect("turn");
        turn_started.notified().await;

        let archived = {
            let thread = thread.clone();
            tokio::spawn(async move { thread.archive_with_reason("adapter_cleanup").await })
        };
        loop {
            let queued = application
                .inner
                .runtime
                .state
                .lock()
                .expect("runtime state poisoned")
                .threads
                .get(thread.id())
                .map_or(0, |cell| cell.operations.len());
            if queued == 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            lifecycle_started.load(Ordering::SeqCst),
            0,
            "adapter cleanup cannot overlap the active Turn"
        );

        release_turn.notify_one();
        turn.wait().await.expect("turn result");
        lifecycle_entered.notified().await;
        {
            let requests = lifecycle_requests
                .lock()
                .expect("lifecycle requests poisoned");
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].thread.id, thread.id());
            assert!(matches!(
                &requests[0].action,
                AgentThreadLifecycleAction::Archive { reason: Some(reason) }
                    if reason == "adapter_cleanup"
            ));
        }
        assert!(
            application
                .inner
                .state
                .session_summary(thread.id())
                .await
                .expect("summary")
                .expect("thread")
                .archived_at_ms
                .is_none(),
            "Framework state changes only after adapter cleanup succeeds"
        );
        release_lifecycle.notify_one();
        archived.await.expect("archive task").expect("archive");
        let summary = application
            .inner
            .state
            .session_summary(thread.id())
            .await
            .expect("summary")
            .expect("thread");
        assert!(summary.archived_at_ms.is_some());
        assert_eq!(summary.end_reason.as_deref(), Some("adapter_cleanup"));
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn adapter_lifecycle_failure_does_not_publish_framework_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (turn_adapter, _, _, _) = fake_adapter();
        let lifecycle_requests = Arc::new(Mutex::new(Vec::new()));
        let adapter = Arc::new(LifecycleAgentSessionAdapter {
            turn: turn_adapter,
            lifecycle_started: Arc::new(AtomicUsize::new(0)),
            lifecycle_entered: Arc::new(Notify::new()),
            release_lifecycle: None,
            lifecycle_requests: Arc::clone(&lifecycle_requests),
            lifecycle_outcomes: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            lifecycle_error: Some("adapter close failed".to_string()),
        });
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

        let error = thread
            .archive_with_reason("must_not_commit")
            .await
            .expect_err("adapter failure");
        assert!(error.to_string().contains("adapter close failed"));
        assert!(matches!(
            &lifecycle_requests
                .lock()
                .expect("lifecycle requests poisoned")[0]
                .action,
            AgentThreadLifecycleAction::Archive { reason: Some(reason) }
                if reason == "must_not_commit"
        ));
        let summary = application
            .inner
            .state
            .session_summary(thread.id())
            .await
            .expect("summary")
            .expect("thread");
        assert!(summary.archived_at_ms.is_none());
        assert!(summary.end_reason.is_none());
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn restore_persists_typed_projection_before_the_local_transition() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (turn_adapter, _, _, _) = fake_adapter();
        let lifecycle_requests = Arc::new(Mutex::new(Vec::new()));
        let lifecycle_outcomes = Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let adapter = Arc::new(LifecycleAgentSessionAdapter {
            turn: turn_adapter,
            lifecycle_started: Arc::new(AtomicUsize::new(0)),
            lifecycle_entered: Arc::new(Notify::new()),
            release_lifecycle: None,
            lifecycle_requests,
            lifecycle_outcomes: Arc::clone(&lifecycle_outcomes),
            lifecycle_error: None,
        });
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
        thread.archive().await.expect("archive");
        lifecycle_outcomes
            .lock()
            .expect("lifecycle outcomes poisoned")
            .push_back(AgentThreadLifecycleOutcome::Projection(
                AgentImportedLifecycle {
                    target_label: "Restored Agent".to_string(),
                    fork: true,
                    delete: true,
                    close: true,
                    resume: true,
                },
            ));
        let mut transaction = application
            .inner
            .state
            .begin_sqlx_write()
            .await
            .expect("restore trigger transaction");
        sqlx::query(
            "CREATE TRIGGER reject_restore BEFORE UPDATE OF archived_at_ms ON sessions \
             WHEN NEW.archived_at_ms IS NULL BEGIN SELECT RAISE(FAIL, 'injected restore failure'); END",
        )
        .execute(&mut *transaction)
        .await
        .expect("restore trigger");
        transaction.commit().await.expect("commit restore trigger");

        let error = thread.restore().await.expect_err("local restore failure");
        assert!(error.to_string().contains("injected restore failure"));
        let metadata = application
            .inner
            .state
            .session_metadata(thread.id())
            .await
            .expect("metadata")
            .expect("Thread metadata");
        assert_eq!(
            metadata["agentSessionLifecycle"]["targetLabel"],
            "Restored Agent"
        );
        assert!(
            application
                .inner
                .state
                .session_summary(thread.id())
                .await
                .expect("summary")
                .expect("Thread")
                .archived_at_ms
                .is_some(),
            "the typed Adapter projection commits before the failed local restore"
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn reconciler_finishes_acknowledged_local_delete_without_remote_redelivery() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (turn_adapter, _, _, _) = fake_adapter();
        let lifecycle_requests = Arc::new(Mutex::new(Vec::new()));
        let lifecycle_outcomes = Arc::new(Mutex::new(std::collections::VecDeque::from([
            AgentThreadLifecycleOutcome::RemoteDeletePrepared { at_ms: 10 },
            AgentThreadLifecycleOutcome::RemoteDeleteAcknowledged { at_ms: 20 },
        ])));
        let adapter = Arc::new(LifecycleAgentSessionAdapter {
            turn: turn_adapter,
            lifecycle_started: Arc::new(AtomicUsize::new(0)),
            lifecycle_entered: Arc::new(Notify::new()),
            release_lifecycle: None,
            lifecycle_requests: Arc::clone(&lifecycle_requests),
            lifecycle_outcomes,
            lifecycle_error: None,
        });
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
        let mut transaction = application
            .inner
            .state
            .begin_sqlx_write()
            .await
            .expect("delete trigger transaction");
        sqlx::query(
            "CREATE TRIGGER reject_thread_delete BEFORE DELETE ON sessions \
             BEGIN SELECT RAISE(FAIL, 'injected local delete failure'); END",
        )
        .execute(&mut *transaction)
        .await
        .expect("delete trigger");
        transaction.commit().await.expect("commit delete trigger");

        let error = thread.delete().await.expect_err("local delete failure");
        assert!(error.to_string().contains("injected local delete failure"));
        let metadata = application
            .inner
            .state
            .session_metadata(thread.id())
            .await
            .expect("metadata")
            .expect("Thread metadata");
        assert_eq!(
            metadata["agentSessionDeleteIntent"]["state"],
            "remoteAcknowledged"
        );
        assert_eq!(
            lifecycle_requests
                .lock()
                .expect("lifecycle requests poisoned")
                .iter()
                .filter(|request| matches!(
                    request.current.remote_delete,
                    AgentRemoteDeleteState::Prepared { .. }
                ))
                .count(),
            1,
            "the Adapter receives the remotely destructive stage exactly once"
        );

        let mut transaction = application
            .inner
            .state
            .begin_sqlx_write()
            .await
            .expect("drop delete trigger transaction");
        sqlx::query("DROP TRIGGER reject_thread_delete")
            .execute(&mut *transaction)
            .await
            .expect("drop delete trigger");
        transaction
            .commit()
            .await
            .expect("commit dropped delete trigger");
        let lifecycle_request_count = lifecycle_requests
            .lock()
            .expect("lifecycle requests poisoned")
            .len();
        assert_eq!(
            application
                .client()
                .reconcile_acknowledged_agent_deletes()
                .await
                .expect("reconcile acknowledged delete"),
            1
        );
        assert_eq!(
            lifecycle_requests
                .lock()
                .expect("lifecycle requests poisoned")
                .iter()
                .filter(|request| matches!(
                    request.current.remote_delete,
                    AgentRemoteDeleteState::Prepared { .. }
                ))
                .count(),
            1,
            "reconciliation after the durable acknowledgement must not redeliver remote delete"
        );
        assert_eq!(
            lifecycle_requests
                .lock()
                .expect("lifecycle requests poisoned")
                .len(),
            lifecycle_request_count + 1,
            "reconciliation performs one local-delete lifecycle observation"
        );
        assert!(
            application
                .inner
                .state
                .session_summary(thread.id())
                .await
                .expect("summary")
                .is_none()
        );
        assert_eq!(
            application
                .client()
                .reconcile_acknowledged_agent_deletes()
                .await
                .expect("idempotent reconciliation"),
            0
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn invalid_adapter_lifecycle_outcome_is_rejected_before_state_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (turn_adapter, _, _, _) = fake_adapter();
        let adapter = Arc::new(LifecycleAgentSessionAdapter {
            turn: turn_adapter,
            lifecycle_started: Arc::new(AtomicUsize::new(0)),
            lifecycle_entered: Arc::new(Notify::new()),
            release_lifecycle: None,
            lifecycle_requests: Arc::new(Mutex::new(Vec::new())),
            lifecycle_outcomes: Arc::new(Mutex::new(std::collections::VecDeque::from([
                AgentThreadLifecycleOutcome::Projection(AgentImportedLifecycle {
                    target_label: "Invalid archive projection".to_string(),
                    fork: false,
                    delete: false,
                    close: false,
                    resume: false,
                }),
            ]))),
            lifecycle_error: None,
        });
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

        let error = thread.archive().await.expect_err("invalid outcome");
        assert!(
            error
                .to_string()
                .contains("only restore may update the Agent lifecycle projection")
        );
        let summary = application
            .inner
            .state
            .session_summary(thread.id())
            .await
            .expect("summary")
            .expect("Thread");
        assert!(summary.archived_at_ms.is_none());
        assert!(
            application
                .inner
                .state
                .session_metadata(thread.id())
                .await
                .expect("metadata")
                .unwrap_or_default()
                .get("agentSessionLifecycle")
                .is_none()
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
        let limits = ApplicationLimits {
            max_operations: 3,
            max_thread_operations: 2,
        };
        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("state.db"))
            .limits(limits)
            .build()
            .await
            .expect("application");
        let reservations = ["occupied-0", "occupied-0", "occupied-1"]
            .into_iter()
            .map(|thread_id| {
                application
                    .inner
                    .runtime
                    .reserve_mutation(thread_id)
                    .expect("fill Application capacity")
            })
            .collect::<Vec<_>>();

        let error = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect_err("sixty-fifth operation");

        let data = error.structured_data().expect("structured overload");
        assert_eq!(data["kind"], "application_overloaded");
        assert_eq!(data["scope"], "application");
        assert_eq!(data["limit"], limits.max_operations);
        assert_eq!(data["occupancy"], limits.max_operations);
        assert_eq!(data["retryable"], true);
        assert!(data["oldestQueuedAgeMs"].as_u64().is_some());
        assert_eq!(data["oldestQueuedOperationKind"], "mutation");
        assert!(data["oldestQueuedOperationId"].as_str().is_some());
        assert_eq!(data["oldestQueuedThreadId"], "occupied-0");
        assert!(data.get("threadId").is_none());
        let snapshot = application.operational_snapshot();
        assert!(snapshot.open);
        assert_eq!(snapshot.limits, limits);
        assert_eq!(snapshot.accepted_operations, limits.max_operations);
        assert_eq!(snapshot.tracked_threads, 2);
        let queued = snapshot.oldest_queued.expect("oldest queued operation");
        assert_eq!(queued.kind, "mutation");
        assert_eq!(queued.thread_id.as_deref(), Some("occupied-0"));
        assert!(!queued.id.is_empty());
        assert_eq!(snapshot.panic_diagnostics, Vec::new());
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
        let limits = ApplicationLimits {
            max_operations: 4,
            max_thread_operations: 2,
        };
        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("state.db"))
            .limits(limits)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let reservations = (0..limits.max_thread_operations)
            .map(|_| {
                application
                    .inner
                    .runtime
                    .reserve_mutation(thread.id())
                    .expect("fill Thread capacity")
            })
            .collect::<Vec<_>>();

        let error = thread.archive().await.expect_err("thirty-third operation");

        let data = error.structured_data().expect("structured overload");
        assert_eq!(data["kind"], "application_overloaded");
        assert_eq!(data["scope"], "thread");
        assert_eq!(data["limit"], limits.max_thread_operations);
        assert_eq!(data["occupancy"], limits.max_thread_operations);
        assert_eq!(data["retryable"], true);
        assert_eq!(data["threadId"], thread.id());
        assert!(data["oldestQueuedAgeMs"].as_u64().is_some());
        assert_eq!(data["oldestQueuedOperationKind"], "mutation");
        assert!(data["oldestQueuedOperationId"].as_str().is_some());
        assert_eq!(data["oldestQueuedThreadId"], thread.id());
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
        let clamped = thread
            .history()
            .latest(Some(usize::MAX))
            .await
            .expect("hard-bounded page");
        assert_eq!(clamped.items.len(), MAX_HISTORY_PAGE_SIZE);
        assert_eq!(clamped.next_before, Some(6));
        assert_eq!(
            thread
                .history()
                .display_message_count()
                .await
                .expect("display count"),
            205
        );

        let snapshot = thread.snapshot().await.expect("bounded snapshot");
        assert_eq!(snapshot.items.len(), DEFAULT_HISTORY_PAGE_SIZE);
        assert_eq!(
            snapshot.items.first().map(|item| item.session_seq),
            Some(106)
        );
        assert_eq!(snapshot.history_cursor, Some(106));
        application
            .inner
            .state
            .append_message_with_metrics(
                thread.id(),
                &psychevo_agent_core::Message::Assistant {
                    content: vec![psychevo_agent_core::AssistantBlock::Text {
                        text: "answer".to_string(),
                    }],
                    timestamp_ms: 1,
                    finish_reason: Some("stop".to_string()),
                    outcome: psychevo_ai::Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
                Some(serde_json::json!({"total_tokens": 42})),
                None,
            )
            .await
            .expect("assistant usage");
        assert_eq!(
            thread
                .history()
                .latest_assistant_usage()
                .await
                .expect("latest assistant usage"),
            Some(serde_json::json!({"total_tokens": 42}))
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn history_reader_contains_only_visible_same_thread_cursors() {
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
        let other_thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("other thread");
        application
            .inner
            .state
            .append_message(thread.id(), &user_text_message("visible"))
            .await
            .expect("visible message");
        application
            .inner
            .state
            .append_message_with_metrics(
                thread.id(),
                &user_text_message("inherited context"),
                None,
                Some(serde_json::json!({"side_inherited": {"hidden": true}})),
            )
            .await
            .expect("hidden inherited message");
        application
            .inner
            .state
            .append_message(thread.id(), &user_text_message("reverted"))
            .await
            .expect("reverted message");
        application
            .inner
            .state
            .set_session_revert_state(
                thread.id(),
                crate::state::SessionRevertState::workspace_undo(3, "snapshot".to_string()),
            )
            .await
            .expect("revert boundary");
        for text in ["other-1", "other-2", "other-3", "other-4"] {
            application
                .inner
                .state
                .append_message(other_thread.id(), &user_text_message(text))
                .await
                .expect("other thread message");
        }

        let history = thread.history();
        assert!(history.contains(1).await.expect("visible cursor"));
        assert!(!history.contains(2).await.expect("hidden cursor"));
        assert!(!history.contains(3).await.expect("reverted cursor"));
        assert!(!history.contains(4).await.expect("cross-thread cursor"));
        assert!(!history.contains(0).await.expect("invalid cursor"));

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
        let finalizer_entered = Arc::new(Notify::new());
        let finalizer_entered_in_task = Arc::clone(&finalizer_entered);
        let release_finalizer = Arc::new(Notify::new());
        let release_finalizer_in_task = Arc::clone(&release_finalizer);
        let mut agent_abort = receivers.abort_signal();
        assert!(
            supervisor
                .spawn_background(Box::pin(async move {
                    agent_abort.wait_for_abort().await;
                    finalizer_entered_in_task.notify_one();
                    release_finalizer_in_task.notified().await;
                    finalized_in_task.store(true, std::sync::atomic::Ordering::SeqCst);
                }))
                .is_ok(),
            "spawn agent"
        );

        let shutdown_application = application.clone();
        let shutdown = tokio::spawn(async move { shutdown_application.shutdown().await });
        finalizer_entered.notified().await;
        tokio::task::yield_now().await;
        assert!(
            !shutdown.is_finished(),
            "Application shutdown must await the Agent finalizer"
        );
        release_finalizer.notify_one();
        shutdown.await.expect("shutdown task").expect("shutdown");

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
        let queued = thread
            .start_turn(TurnRequest::new("queued behind active turn"))
            .await
            .expect("queued turn");
        assert_eq!(thread.activity().queued_turns, 1);

        let graceful_application = application.clone();
        let graceful =
            tokio::spawn(async move { graceful_application.shutdown().await.expect("graceful") });
        application
            .inner
            .graceful_shutdown_owner_entered
            .notified()
            .await;
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
        assert_eq!(
            queued
                .wait()
                .await
                .expect("queued interrupted result")
                .outcome,
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
        assert_eq!(report.state_close, ShutdownStateCloseStatus::Closed);
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
        assert_eq!(report.state_close, ShutdownStateCloseStatus::Closed);
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
        assert_eq!(terminal.status, FrameworkTurnTerminalStatus::Interrupted);
        assert_eq!(
            terminal.outcome,
            Some(FrameworkTurnTerminalOutcome::Aborted)
        );
        reopened.close().await;
    }

    #[tokio::test]
    async fn forced_shutdown_reports_adapter_and_state_close_timeouts_independently() {
        let temp = tempfile::tempdir().expect("tempdir");
        let adapter_started = Arc::new(Notify::new());
        let close_entered = Arc::new(Notify::new());
        let close_release = Arc::new(Notify::new());
        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("state.db"))
            .agent_session_adapter(Arc::new(PendingAgentSessionAdapter {
                started: adapter_started.clone(),
            }))
            .build()
            .await
            .expect("application");
        application
            .inner
            .state
            .set_close_barrier_for_test(close_entered, Arc::clone(&close_release));
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("pending through forced shutdown"))
            .await
            .expect("turn");
        let turn_id = handle.receipt().turn_id.clone();
        adapter_started.notified().await;

        let report = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            application.shutdown_force(),
        )
        .await
        .expect("forced shutdown total deadline")
        .expect("forced shutdown");

        assert_eq!(report.adapter, ShutdownAdapterStatus::TimedOut);
        assert_eq!(report.state_close, ShutdownStateCloseStatus::TimedOut);
        assert!(!report.is_clean());
        assert!(report.aborted_tasks > 0);
        assert!(report.pending_terminal_failures.is_empty());
        assert_eq!(
            handle.wait().await.expect("forced settlement").outcome,
            TurnOutcome::Interrupted
        );
        let terminal = application
            .inner
            .state
            .gateway_turn_terminal(&turn_id)
            .await
            .expect("terminal query")
            .expect("durable forced terminal before State close");
        assert_eq!(terminal.status, FrameworkTurnTerminalStatus::Interrupted);
        assert_eq!(
            terminal.outcome,
            Some(FrameworkTurnTerminalOutcome::Aborted)
        );
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
        assert_eq!(terminal.status, FrameworkTurnTerminalStatus::Completed);
        assert_eq!(terminal.outcome, Some(FrameworkTurnTerminalOutcome::Normal));
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn every_accepted_outcome_has_one_authoritative_terminal_across_resume() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cases = [
            (
                TurnOutcome::Completed,
                FrameworkTurnTerminalStatus::Completed,
                FrameworkTurnTerminalOutcome::Normal,
            ),
            (
                TurnOutcome::Stopped,
                FrameworkTurnTerminalStatus::Interrupted,
                FrameworkTurnTerminalOutcome::Stopped,
            ),
            (
                TurnOutcome::Failed,
                FrameworkTurnTerminalStatus::Failed,
                FrameworkTurnTerminalOutcome::Failed,
            ),
            (
                TurnOutcome::Interrupted,
                FrameworkTurnTerminalStatus::Interrupted,
                FrameworkTurnTerminalOutcome::Aborted,
            ),
        ];
        let adapter = Arc::new(OutcomeSequenceAgentSessionAdapter {
            outcomes: Mutex::new(cases.iter().map(|(outcome, _, _)| *outcome).collect()),
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
        let mut expected_terminals = Vec::new();

        for (outcome, status, durable_outcome) in cases {
            let handle = thread
                .start_turn(TurnRequest::new(format!("exercise {outcome:?}")))
                .await
                .expect("accepted turn");
            let turn_id = handle.receipt().turn_id.clone();
            let mut events = handle.events();
            assert_eq!(handle.wait().await.expect("turn result").outcome, outcome);

            let mut terminal_events = 0;
            while let Some(event) = events.next().await {
                match event {
                    TurnEvent::Completed {
                        thread_id,
                        turn_id: observed_turn_id,
                        outcome: observed_outcome,
                    } => {
                        terminal_events += 1;
                        assert_eq!(thread_id, thread.id());
                        assert_eq!(observed_turn_id, turn_id);
                        assert_eq!(observed_outcome, outcome);
                    }
                    TurnEvent::Failed { message, .. } => {
                        panic!("successful Adapter result published a failure: {message}")
                    }
                    _ => {}
                }
            }
            assert_eq!(terminal_events, 1, "{outcome:?} terminal event count");

            for _ in 0..2 {
                let resumed = client.resume_turn(&turn_id).await.expect("durable resume");
                assert_eq!(
                    resumed.wait().await.expect("resumed result").outcome,
                    outcome
                );
            }
            expected_terminals.push((turn_id, status, durable_outcome));
        }

        let terminals = application
            .inner
            .state
            .list_gateway_turn_terminals_for_thread(thread.id())
            .await
            .expect("terminal rows");
        assert_eq!(terminals.len(), expected_terminals.len());
        for (turn_id, status, outcome) in expected_terminals {
            let matching = terminals
                .iter()
                .filter(|terminal| terminal.turn_id == turn_id)
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1, "authoritative row for {turn_id}");
            assert_eq!(matching[0].status, status);
            assert_eq!(matching[0].outcome, Some(outcome));
        }

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
        assert!(
            !application
                .client()
                .activity_snapshot()
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
    async fn client_turn_id_is_non_whitespace_and_preserved_as_opaque_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, _) = fake_adapter();
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

        let rejected_turn_id = Uuid::now_v7().to_string();
        let mut rejected = TurnRequest::new("invalid correlation");
        rejected.requested_turn_id = Some(rejected_turn_id.clone());
        rejected.client_turn_id = Some(" \t ".to_string());
        let error = thread
            .start_turn(rejected)
            .await
            .expect_err("whitespace-only client Turn id");
        assert!(error.to_string().contains("non-whitespace"));
        assert!(
            application
                .inner
                .state
                .gateway_turn_delivery(&rejected_turn_id)
                .await
                .expect("rejected delivery lookup")
                .is_none()
        );

        let opaque_id = " correlation-id ".to_string();
        let accepted_turn_id = Uuid::now_v7().to_string();
        let mut accepted = TurnRequest::new("opaque correlation");
        accepted.requested_turn_id = Some(accepted_turn_id.clone());
        accepted.client_turn_id = Some(opaque_id.clone());
        let handle = thread.start_turn(accepted).await.expect("accepted Turn");
        assert_eq!(
            handle.receipt().client_turn_id.as_deref(),
            Some(opaque_id.as_str())
        );
        let delivery = application
            .inner
            .state
            .gateway_turn_delivery(&accepted_turn_id)
            .await
            .expect("delivery lookup")
            .expect("delivery");
        assert_eq!(
            serde_json::from_str::<Value>(delivery.input_json.as_deref().expect("input JSON"))
                .expect("valid input JSON")["clientTurnId"],
            opaque_id
        );
        let receipts = application
            .inner
            .state
            .gateway_turn_start_receipts(thread.id())
            .await
            .expect("receipt lookup");
        assert!(receipts.iter().any(|receipt| {
            receipt.client_turn_id == opaque_id && receipt.turn_id == accepted_turn_id
        }));

        started.notified().await;
        release.notify_one();
        handle.wait().await.expect("turn result");
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn failed_durable_acceptance_never_enters_public_thread_activity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(InteractionAgentSessionAdapter {
                started,
                release,
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let turn_id = Uuid::now_v7().to_string();
        let empty_preferences = BTreeMap::new();
        application
            .inner
            .state
            .accept_framework_turn(crate::state::ExistingFrameworkThreadTurnInput {
                delivery: GatewayTurnDeliveryInput {
                    turn_id: &turn_id,
                    thread_id: thread.id(),
                    runtime_ref: "native",
                    input_json: "{}",
                    input_hash: "existing",
                },
                client_turn_id: None,
                runtime_binding: None,
                initial_thread_preferences: &empty_preferences,
                mission: None,
            })
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
        request
            .initial_thread_preferences
            .insert("model".to_string(), "must-roll-back".to_string());
        let pending_thread = thread.clone();
        let pending = tokio::spawn(async move { pending_thread.start_turn(request).await });

        entered.notified().await;
        assert!(
            !thread.has_activity(),
            "pending durable acceptance must be invisible to public activity"
        );
        assert!(
            !application
                .client()
                .activity_snapshot()
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
        assert!(
            application
                .inner
                .state
                .gateway_runtime_binding(thread.id())
                .await
                .expect("binding lookup")
                .is_none(),
            "binding and its initial preferences must roll back with delivery acceptance"
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn concurrent_identical_first_turns_share_the_binding_winner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(InteractionAgentSessionAdapter {
                started: started.clone(),
                release: release.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let entered = Arc::new(Notify::new());
        let accept = Arc::new(Notify::new());
        application
            .inner
            .state
            .set_gateway_turn_acceptance_barrier_for_test(entered.clone(), accept.clone());

        let first_turn_id = Uuid::now_v7().to_string();
        let mut first_request = TurnRequest::new("first concurrent binding candidate");
        first_request.requested_turn_id = Some(first_turn_id.clone());
        first_request
            .initial_thread_preferences
            .insert("model".to_string(), "shared-model".to_string());
        let first_thread = thread.clone();
        let first = tokio::spawn(async move { first_thread.start_turn(first_request).await });
        entered.notified().await;

        let second_turn_id = Uuid::now_v7().to_string();
        let mut second_request = TurnRequest::new("second concurrent binding candidate");
        second_request.requested_turn_id = Some(second_turn_id.clone());
        second_request
            .initial_thread_preferences
            .insert("model".to_string(), "shared-model".to_string());
        let second_handle = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            thread.start_turn(second_request),
        )
        .await
        .expect("second acceptance deadline")
        .expect("second Turn accepts the binding winner");

        accept.notify_one();
        let first_handle = tokio::time::timeout(std::time::Duration::from_secs(1), first)
            .await
            .expect("first acceptance deadline")
            .expect("first acceptance task")
            .expect("losing identical binding candidate converges on the winner");
        let binding = application
            .inner
            .state
            .gateway_runtime_binding(thread.id())
            .await
            .expect("binding lookup")
            .expect("single binding winner");
        assert_eq!(
            binding.thread_preferences,
            BTreeMap::from([(
                "model".to_string(),
                Value::String("shared-model".to_string()),
            )])
        );
        for turn_id in [&first_turn_id, &second_turn_id] {
            assert!(
                application
                    .inner
                    .state
                    .gateway_turn_delivery(turn_id)
                    .await
                    .expect("delivery lookup")
                    .is_some(),
                "each accepted Turn has its own durable delivery"
            );
        }

        started.notified().await;
        release.notify_one();
        first_handle.wait().await.expect("first result");
        started.notified().await;
        release.notify_one();
        second_handle.wait().await.expect("second result");
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn bound_thread_delivery_inherits_runtime_and_rejects_mismatched_intent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let preparations = Arc::new(AtomicUsize::new(0));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(PreparationCountingInteractionAdapter {
                inner: InteractionAgentSessionAdapter {
                    started: started.clone(),
                    release: release.clone(),
                },
                preparations: Arc::clone(&preparations),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let cwd = application
            .inner
            .state
            .session_summary(thread.id())
            .await
            .expect("Thread summary query")
            .expect("Thread summary")
            .cwd;
        application
            .inner
            .state
            .create_gateway_runtime_binding(crate::state::GatewayRuntimeBindingInput {
                thread_id: thread.id(),
                agent_ref: Some("remote-agent"),
                agent_fingerprint: "remote-agent-fingerprint",
                agent_definition_json: r#"{"name":"remote-agent"}"#,
                runtime_ref: "acp-production",
                backend_kind: "acp",
                native_kind: "acp",
                native_session_id: Some("remote-session"),
                cwd: &cwd,
                profile_fingerprint: "acp-profile-fingerprint",
                profile_revision: "7",
                profile_config_json: r#"{"command":"remote-agent"}"#,
                adapter_kind: "acp",
                adapter_revision: "7",
                ownership: crate::state::GatewayRuntimeBindingOwnership::ReadWrite,
                parent_thread_id: None,
            })
            .await
            .expect("binding");

        let mismatched_turn_id = Uuid::now_v7().to_string();
        let mut mismatched = TurnRequest::new("wrong runtime intent")
            .with_runtime(Some("native".to_string()), BTreeMap::new());
        mismatched.requested_turn_id = Some(mismatched_turn_id.clone());
        let error = thread
            .start_turn(mismatched)
            .await
            .expect_err("explicit runtime conflict");
        assert!(
            error
                .to_string()
                .contains("conflicts with the immutable binding")
        );
        assert!(
            application
                .inner
                .state
                .gateway_turn_delivery(&mismatched_turn_id)
                .await
                .expect("mismatched delivery lookup")
                .is_none()
        );
        assert_eq!(
            preparations.load(Ordering::SeqCst),
            0,
            "an immutable binding conflict must not execute Adapter preparation"
        );

        let inherited_turn_id = Uuid::now_v7().to_string();
        let mut inherited = TurnRequest::new("inherit bound runtime");
        inherited.requested_turn_id = Some(inherited_turn_id.clone());
        let handle = thread
            .start_turn(inherited)
            .await
            .expect("omitted runtime inherits binding");
        assert_eq!(preparations.load(Ordering::SeqCst), 1);
        let delivery = application
            .inner
            .state
            .gateway_turn_delivery(&inherited_turn_id)
            .await
            .expect("delivery lookup")
            .expect("durable delivery");
        assert_eq!(delivery.runtime_ref, "acp-production");
        assert_eq!(
            serde_json::from_str::<Value>(delivery.input_json.as_deref().expect("input JSON"))
                .expect("valid input JSON")["runtimeRef"],
            "acp-production"
        );

        started.notified().await;
        release.notify_one();
        handle.wait().await.expect("turn result");
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn receipt_write_failure_rolls_back_binding_preferences_and_delivery() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(InteractionAgentSessionAdapter {
                started: Arc::new(Notify::new()),
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
        let cwd = application
            .inner
            .state
            .session_summary(thread.id())
            .await
            .expect("Thread summary query")
            .expect("Thread summary")
            .cwd;
        let mut connection = application
            .inner
            .state
            .acquire_sqlx()
            .await
            .expect("state connection");
        sqlx::query("UPDATE sessions SET metadata_json = 'not-json' WHERE id = ?1")
            .bind(thread.id())
            .execute(&mut *connection)
            .await
            .expect("inject invalid receipt metadata");
        drop(connection);

        let rejected_turn_id = Uuid::now_v7().to_string();
        let initial_preferences = BTreeMap::from([(
            "model".to_string(),
            Value::String("must-roll-back".to_string()),
        )]);
        application
            .inner
            .state
            .accept_framework_turn(crate::state::ExistingFrameworkThreadTurnInput {
                delivery: GatewayTurnDeliveryInput {
                    turn_id: &rejected_turn_id,
                    thread_id: thread.id(),
                    runtime_ref: "native",
                    input_json: "{}",
                    input_hash: "receipt-write-failure",
                },
                client_turn_id: Some("receipt-cannot-be-written"),
                runtime_binding: Some(crate::state::GatewayRuntimeBindingInput {
                    thread_id: thread.id(),
                    agent_ref: None,
                    agent_fingerprint: "test-agent-fingerprint",
                    agent_definition_json: "null",
                    runtime_ref: "native",
                    backend_kind: "native",
                    native_kind: "native",
                    native_session_id: None,
                    cwd: &cwd,
                    profile_fingerprint: "test-profile-fingerprint",
                    profile_revision: "1",
                    profile_config_json: "{}",
                    adapter_kind: "native",
                    adapter_revision: "test",
                    ownership: crate::state::GatewayRuntimeBindingOwnership::ReadWrite,
                    parent_thread_id: None,
                }),
                initial_thread_preferences: &initial_preferences,
                mission: None,
            })
            .await
            .expect_err("invalid receipt metadata must reject the transaction");

        assert!(
            application
                .inner
                .state
                .gateway_runtime_binding(thread.id())
                .await
                .expect("binding lookup")
                .is_none(),
            "binding and preferences must roll back with the receipt write"
        );
        assert!(
            application
                .inner
                .state
                .gateway_turn_delivery(&rejected_turn_id)
                .await
                .expect("delivery lookup")
                .is_none(),
            "the new delivery must roll back with the receipt write"
        );
        let mut connection = application
            .inner
            .state
            .acquire_sqlx()
            .await
            .expect("state connection");
        sqlx::query("UPDATE sessions SET metadata_json = NULL WHERE id = ?1")
            .bind(thread.id())
            .execute(&mut *connection)
            .await
            .expect("restore Thread metadata");
        drop(connection);
        assert!(!thread.has_activity());
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn turn_capacity_rejection_precedes_first_binding_materialization() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .limits(ApplicationLimits {
                max_operations: 1,
                max_thread_operations: 1,
            })
            .agent_session_adapter(Arc::new(InteractionAgentSessionAdapter {
                started: Arc::new(Notify::new()),
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
        let occupied = application
            .inner
            .runtime
            .reserve_mutation("occupied-thread")
            .expect("occupy the sole operation slot");
        let rejected_turn_id = Uuid::now_v7().to_string();
        let mut request = TurnRequest::new("over capacity");
        request.requested_turn_id = Some(rejected_turn_id.clone());
        request
            .initial_thread_preferences
            .insert("model".to_string(), "must-not-persist".to_string());
        let error = thread
            .start_turn(request)
            .await
            .expect_err("full Application capacity must reject the Turn");

        assert_eq!(
            error.structured_data().expect("structured overload")["kind"],
            "application_overloaded"
        );
        assert!(
            application
                .inner
                .state
                .gateway_runtime_binding(thread.id())
                .await
                .expect("binding lookup")
                .is_none()
        );
        assert!(
            application
                .inner
                .state
                .gateway_turn_delivery(&rejected_turn_id)
                .await
                .expect("delivery lookup")
                .is_none()
        );
        drop(occupied);
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn new_thread_first_turn_rolls_back_every_association_when_delivery_conflicts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(FailingAgentSessionAdapter))
            .build()
            .await
            .expect("application");
        let client = application.client();
        let existing = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("existing thread");
        let turn_id = Uuid::now_v7().to_string();
        let empty_preferences = BTreeMap::new();
        application
            .inner
            .state
            .accept_framework_turn(crate::state::ExistingFrameworkThreadTurnInput {
                delivery: GatewayTurnDeliveryInput {
                    turn_id: &turn_id,
                    thread_id: existing.id(),
                    runtime_ref: "native",
                    input_json: "{}",
                    input_hash: "reserved-delivery",
                },
                client_turn_id: Some("existing-client-turn"),
                runtime_binding: None,
                initial_thread_preferences: &empty_preferences,
                mission: None,
            })
            .await
            .expect("reserve conflicting delivery");

        let new_thread_id = Uuid::now_v7().to_string();
        let source_key = format!("web:test:{new_thread_id}");
        let mut start = StartThreadRequest::new(temp.path());
        start.source = "web".to_string();
        start.requested_id = Some(new_thread_id.clone());
        start.initial_source = Some(InitialThreadSourceAssociation {
            source_key: source_key.clone(),
            source_kind: "web".to_string(),
            raw_identity: serde_json::json!({"test": new_thread_id}),
            visible_name: Some("Atomic admission".to_string()),
            lineage: Some(serde_json::json!({"root": true})),
        });
        start.initial_binding = Some(InitialAgentBinding {
            agent_ref: Some("default".to_string()),
            agent_fingerprint: "agent-fingerprint".to_string(),
            agent_definition_json: "{}".to_string(),
            runtime_ref: "native".to_string(),
            backend_kind: "native".to_string(),
            native_kind: "native".to_string(),
            native_session_id: None,
            profile_fingerprint: "profile-fingerprint".to_string(),
            profile_revision: "profile-revision".to_string(),
            profile_config_json: "{}".to_string(),
            adapter_kind: "native".to_string(),
            adapter_revision: "1".to_string(),
        });
        start
            .initial_thread_preferences
            .insert("model".to_string(), serde_json::json!("fixture-model"));
        let mut request = TurnRequest::new("must roll back");
        request.requested_turn_id = Some(turn_id.clone());
        request.client_turn_id = Some("retry-client-turn".to_string());
        request.target.runtime_profile_ref = Some("native".to_string());

        client
            .start_thread_with_turn(start, request)
            .await
            .expect_err("duplicate delivery rejects the whole admission");

        assert!(
            application
                .inner
                .state
                .session_summary(&new_thread_id)
                .await
                .expect("new thread lookup")
                .is_none(),
            "the session insert must roll back"
        );
        assert!(
            application
                .inner
                .state
                .gateway_runtime_binding(&new_thread_id)
                .await
                .expect("binding lookup")
                .is_none(),
            "the runtime binding insert must roll back"
        );
        assert!(
            application
                .inner
                .state
                .gateway_source_binding(&source_key)
                .await
                .expect("source lookup")
                .is_none(),
            "the source association insert must roll back"
        );
        assert!(
            application
                .inner
                .state
                .gateway_turn_start_receipts(&new_thread_id)
                .await
                .expect("clientTurnId receipt lookup")
                .is_empty(),
            "the clientTurnId receipt must roll back"
        );
        assert_eq!(
            application.inner.runtime.mcp_runtime_count(),
            0,
            "failed admission must not materialize a Thread runtime"
        );
        let delivery = application
            .inner
            .state
            .gateway_turn_delivery(&turn_id)
            .await
            .expect("delivery lookup")
            .expect("original delivery");
        assert_eq!(delivery.thread_id, existing.id());
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn mission_registration_and_new_thread_turn_admission_are_atomic() {
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
            .expect("existing Thread");
        let mission_id = Uuid::now_v7().to_string();
        thread
            .register_agent_mission(AgentMissionRegistration {
                id: mission_id.clone(),
                goal: "existing mission".to_string(),
                lead_agent_name: "general".to_string(),
                team: None,
                metadata: None,
            })
            .await
            .expect("initial mission");

        let rolled_back_team_id = Uuid::now_v7().to_string();
        thread
            .register_agent_mission(AgentMissionRegistration {
                id: mission_id.clone(),
                goal: "duplicate mission".to_string(),
                lead_agent_name: "general".to_string(),
                team: Some(AgentTeamRegistration {
                    id: rolled_back_team_id,
                    name: "must-roll-back".to_string(),
                    description: None,
                    source_path: None,
                    leader_agent_name: "general".to_string(),
                    members: serde_json::json!([]),
                    max_parallel_agents: 1,
                }),
                metadata: None,
            })
            .await
            .expect_err("duplicate mission must roll back its team");
        let status = thread
            .agent_coordination_status()
            .await
            .expect("coordination status");
        assert!(status.team.is_none(), "failed registration leaked a team");
        assert_eq!(
            status.mission.expect("original mission").goal,
            "existing mission"
        );

        let admitted_thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("mission admission Thread");
        let admitted_handle = admitted_thread
            .start_turn(TurnRequest::new("mission turn").with_admission_mission(
                AgentMissionRegistration {
                    id: Uuid::now_v7().to_string(),
                    goal: "admitted with turn".to_string(),
                    lead_agent_name: "general".to_string(),
                    team: None,
                    metadata: None,
                },
            ))
            .await
            .expect("mission Turn admission");
        assert_eq!(
            admitted_thread
                .agent_coordination_status()
                .await
                .expect("admitted mission status")
                .mission
                .expect("admitted mission")
                .goal,
            "admitted with turn"
        );
        admitted_handle
            .wait()
            .await
            .expect_err("fixture invocation fails after admission");

        let new_thread_id = Uuid::now_v7().to_string();
        let start = StartThreadRequest::new(temp.path()).with_initial_context(
            new_thread_id.clone(),
            None,
            BTreeMap::new(),
        );
        client
            .start_thread_with_turn(
                start,
                TurnRequest::new("must reject atomically").with_admission_mission(
                    AgentMissionRegistration {
                        id: mission_id,
                        goal: "duplicate admission mission".to_string(),
                        lead_agent_name: "general".to_string(),
                        team: None,
                        metadata: None,
                    },
                ),
            )
            .await
            .expect_err("mission conflict must reject whole first-Turn admission");
        assert!(
            client
                .thread_summary(&new_thread_id)
                .await
                .expect("new Thread lookup")
                .is_none(),
            "failed mission admission left an empty Thread"
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
        let turn_id = handle.receipt().turn_id.clone();

        let error = handle.wait().await.expect_err("panic must fail the turn");
        assert_eq!(error.to_string(), "Framework Turn actor panicked");
        assert!(!thread.has_activity());
        assert_eq!(
            application.inner.runtime.thread_activity(thread.id()),
            (false, None, 0),
            "panic recovery must release both the active lane and queued work"
        );
        let pending = application
            .inner
            .runtime
            .pending_terminal(&turn_id)
            .expect("panic recovery must retain a typed pending terminal");
        assert_eq!(
            pending.completion.expect_err("panic terminal must fail"),
            Arc::<str>::from("Framework Turn actor panicked")
        );

        for _ in 0..100 {
            if application
                .inner
                .runtime
                .task_panics
                .load(Ordering::Relaxed)
                == 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            application
                .inner
                .runtime
                .task_panics
                .load(Ordering::Relaxed),
            1
        );
        let snapshot = application.operational_snapshot();
        assert_eq!(snapshot.task_panics, 1);
        let diagnostic = snapshot
            .panic_diagnostics
            .last()
            .expect("structured panic diagnostic");
        assert_eq!(diagnostic.actor, format!("framework_turn:{turn_id}"));
        assert_eq!(diagnostic.payload, "adapter fixture panic");
        assert!(diagnostic.task_id > 0);
        assert!(!diagnostic.backtrace.is_empty() && diagnostic.backtrace.len() <= 8_192);

        let resumed = application
            .client()
            .resume_turn(&turn_id)
            .await
            .expect("same-process resume persists the pending terminal");
        assert_eq!(
            resumed
                .wait()
                .await
                .expect_err("resumed panic terminal must fail")
                .to_string(),
            "Framework Turn actor panicked"
        );
        assert!(
            application
                .inner
                .runtime
                .pending_terminal(&turn_id)
                .is_none()
        );
        assert!(
            application
                .inner
                .state
                .gateway_turn_terminal(&turn_id)
                .await
                .expect("terminal lookup")
                .is_some(),
            "resume must durably persist the retained panic terminal"
        );
        let report = application.shutdown().await.expect("shutdown");
        assert_eq!(report.task_panics, 1);
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
        assert!(matches!(
            application
                .client()
                .activity_snapshot()
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
        let runtime = Arc::new(ApplicationRuntime::new(ApplicationLimits::default()));
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
        let mut start = StartThreadRequest::new(temp.path());
        start.source = "tui".to_string();
        let thread = client.start_thread(start).await.expect("thread");
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
        assert_eq!(
            fork.summary()
                .await
                .expect("fork summary")
                .forked_from_thread_id
                .as_deref(),
            Some(thread.id())
        );
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
    async fn pending_terminal_retry_keeps_its_pre_later_turn_message_boundary() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(BoundaryPersistenceAgentSessionAdapter::default()))
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        application
            .inner
            .state
            .fail_next_framework_terminal_for_test();
        let first = thread
            .start_turn(TurnRequest::new("first"))
            .await
            .expect("first accepted");
        let first_turn_id = first.receipt().turn_id.clone();
        first.wait().await.expect_err("first terminal commit fails");

        let second = thread
            .start_turn(TurnRequest::new("second"))
            .await
            .expect("second accepted");
        let second_turn_id = second.receipt().turn_id.clone();
        second.wait().await.expect("second completes");
        client
            .resume_turn(&first_turn_id)
            .await
            .expect("first terminal retry")
            .wait()
            .await
            .expect("first terminal result");

        let first_terminal = application
            .inner
            .state
            .gateway_turn_terminal(&first_turn_id)
            .await
            .expect("first terminal query")
            .expect("first terminal");
        let second_terminal = application
            .inner
            .state
            .gateway_turn_terminal(&second_turn_id)
            .await
            .expect("second terminal query")
            .expect("second terminal");
        assert_eq!(first_terminal.boundary_session_seq, 1);
        assert_eq!(second_terminal.boundary_session_seq, 2);
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
        let empty_preferences = BTreeMap::new();
        first
            .inner
            .state
            .accept_framework_turn(crate::state::ExistingFrameworkThreadTurnInput {
                delivery: GatewayTurnDeliveryInput {
                    turn_id: &turn_id,
                    thread_id: thread.id(),
                    runtime_ref: "native",
                    input_json: "{}",
                    input_hash: "fixture",
                },
                client_turn_id: None,
                runtime_binding: None,
                initial_thread_preferences: &empty_preferences,
                mission: None,
            })
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
        assert_ne!(
            delivery.status,
            crate::state::GatewayTurnDeliveryStatus::Terminal
        );
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

    #[test]
    fn clarify_turn_event_preserves_the_decodable_request_payload() {
        let event = TurnEvent::from_run_stream(RunStreamEvent::ClarifyRequest(
            crate::types::ClarifyRequestEvent {
                call_id: "clarify-roundtrip-1".to_string(),
                questions: vec![crate::types::ClarifyQuestion {
                    header: "Target".to_string(),
                    question: "Which workspace?".to_string(),
                    options: Vec::new(),
                    multiple: false,
                    custom: true,
                    secret: false,
                }],
            },
        ))
        .expect("clarify Turn event");
        let TurnEvent::InteractionRequested {
            interaction_id,
            kind,
            payload,
        } = event
        else {
            panic!("expected clarify interaction");
        };

        assert_eq!(interaction_id, "clarify-roundtrip-1");
        assert_eq!(kind, "clarify");
        assert_eq!(payload["call_id"], "clarify-roundtrip-1");
        assert_eq!(payload["questions"][0]["question"], "Which workspace?");
    }

    #[test]
    fn interaction_turn_event_preserves_multiword_action_kind() {
        let event =
            TurnEvent::from_run_stream(RunStreamEvent::session(crate::types::SessionEvent::new(
                crate::types::SessionEventPayload::BlockingActionRequested {
                    action_id: "custom-1".to_string(),
                    kind: crate::types::BlockingActionKind::CustomTool,
                    payload: serde_json::json!({"tool": "calendar"}),
                },
            )))
            .expect("custom-tool Turn event");

        assert!(matches!(
            event,
            TurnEvent::InteractionRequested { kind, .. } if kind == "custom_tool"
        ));
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
        assert_eq!(durable[0].status, FrameworkInteractionStatus::Resolved);
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
    async fn delegated_child_wraps_the_raw_approval_handler_exactly_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let decision = Arc::new(Mutex::new(None));
        let raw_calls = Arc::new(AtomicUsize::new(0));
        let captured_parent_wrapper_calls = Arc::new(AtomicUsize::new(0));
        let raw_handler: Arc<dyn ApprovalHandler> = Arc::new(CountingApprovalHandler {
            calls: raw_calls.clone(),
            decision: PermissionApprovalDecision::allow_once(),
        });
        let captured_parent_wrapper: Arc<dyn ApprovalHandler> = Arc::new(CountingApprovalHandler {
            calls: captured_parent_wrapper_calls.clone(),
            decision: PermissionApprovalDecision::deny(),
        });
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
        let parent = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("parent Thread");
        let child_id = application
            .inner
            .state
            .create_child_session_with_metadata(
                parent.id(),
                temp.path(),
                "agent",
                "fake-model",
                "fake-provider",
                None,
            )
            .await
            .expect("child Thread");
        application
            .inner
            .state
            .upsert_agent_edge(
                parent.id(),
                &child_id,
                crate::state::AgentEdgeStatus::Open,
                None,
            )
            .await
            .expect("child edge");

        let plan = TurnRequest::new("child permission")
            .with_approval(Some(captured_parent_wrapper), false)
            .resolve(BTreeMap::new(), None);
        let dispatcher = AgentChildTurnDispatcher {
            inner: Arc::downgrade(&application.inner),
            approval_handler: Some(raw_handler),
        };
        let handle = dispatcher
            .start_child_turn(parent.id(), &child_id, plan)
            .await
            .expect("child turn");
        started.notified().await;
        let result = handle.wait().await.expect("child result");

        assert_eq!(result.thread_id, child_id);
        assert_eq!(raw_calls.load(Ordering::SeqCst), 1);
        assert_eq!(captured_parent_wrapper_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            decision
                .lock()
                .expect("observed permission decision poisoned")
                .as_ref(),
            Some(&PermissionApprovalDecision::allow_once())
        );
        let child_interactions = application
            .inner
            .state
            .framework_interactions_for_thread(&child_id, false)
            .await
            .expect("child interactions");
        let parent_interactions = application
            .inner
            .state
            .framework_interactions_for_thread(parent.id(), false)
            .await
            .expect("parent interactions");
        assert_eq!(child_interactions.len(), 1);
        assert_eq!(
            child_interactions[0].status,
            FrameworkInteractionStatus::Resolved
        );
        assert!(parent_interactions.is_empty());
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
        assert_eq!(durable[0].status, FrameworkInteractionStatus::Cancelled);
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn handler_cancellation_resolves_a_durable_permission_without_a_live_receiver() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let cancel = Arc::new(Notify::new());
        let raw_cancellations = Arc::new(Mutex::new(Vec::new()));
        let raw_handler: Arc<dyn ApprovalHandler> = Arc::new(CancellableApprovalHandler {
            cancellations: raw_cancellations.clone(),
        });
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
            .start_turn(
                TurnRequest::new("cancel MCP startup permission")
                    .with_approval(Some(raw_handler), false),
            )
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
        assert_eq!(durable[0].status, FrameworkInteractionStatus::Cancelled);
        assert_eq!(
            *raw_cancellations
                .lock()
                .expect("raw approval cancellations poisoned"),
            vec![("mcp_startup:pending".to_string(), "timed_out".to_string(),)]
        );
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

    #[cfg(unix)]
    #[tokio::test]
    async fn application_shell_command_exposes_typed_events_and_control_without_runtime_handles() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let application = Application::builder()
            .home(&home)
            .build()
            .await
            .expect("application");
        let command_text = "  printf 'shell-api\\n'  ";
        let command = application
            .client()
            .shell_command(
                ShellCommandRequest::new(&cwd, command_text)
                    .source("tui")
                    .transient(),
            )
            .expect("shell command");
        let control = command.control();
        assert!(!control.is_interrupted());
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);

        let result = command
            .run(move |event| captured.lock().expect("events").push(event))
            .await
            .expect("shell result");

        assert_eq!(result.outcome, ShellCommandOutcome::Completed);
        assert_eq!(result.command, command_text);
        assert_eq!(result.output["output"], "shell-api\n");
        {
            let events = events.lock().expect("events");
            assert!(matches!(
                events.as_slice(),
                [
                    ShellCommandEvent::Started { command, .. },
                    ShellCommandEvent::Completed { .. }
                ] if command == command_text
            ));
        }
        application.shutdown().await.expect("shutdown");
    }
}
