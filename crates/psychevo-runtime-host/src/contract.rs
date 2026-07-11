use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;

pub type RuntimeFuture<T> = Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Native,
    Acp,
    Codex,
    OpenCode,
}

#[derive(Clone, PartialEq)]
pub struct RuntimeProfile {
    pub id: String,
    pub label: String,
    pub kind: RuntimeKind,
    pub enabled: bool,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub backend_ref: Option<String>,
    pub default_model: Option<String>,
    pub default_mode: Option<String>,
    pub default_agent: Option<String>,
    pub approval_mode: Option<String>,
    pub sandbox: Option<String>,
    pub workspace_roots: Vec<PathBuf>,
    pub options: Value,
    pub revision: u64,
    pub fingerprint: String,
}

impl fmt::Debug for RuntimeProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeProfile")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("kind", &self.kind)
            .field("enabled", &self.enabled)
            .field("command", &self.command)
            .field("args", &self.args)
            .field("env_keys", &self.env.keys().collect::<Vec<_>>())
            .field("backend_ref", &self.backend_ref)
            .field("revision", &self.revision)
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotScope {
    Profile,
    Workspace {
        cwd: PathBuf,
    },
    Session {
        cwd: PathBuf,
        thread_id: String,
        native_session_id: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct SnapshotQuery {
    pub profile: RuntimeProfile,
    pub scope: SnapshotScope,
    pub mode: SnapshotMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SnapshotMode {
    #[default]
    Cached,
    BoundedProbe,
    CatalogRefresh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStability {
    Stable,
    Experimental,
    Unavailable,
}

/// The interaction family understood by product surfaces. Adapter-native
/// labels remain available on [`RuntimeInteraction::kind`] for display only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeInteractionKind {
    Permission,
    Question,
    UserInput,
}

/// The highest interaction exposure a turn permits, and the minimum exposure
/// an interaction requires. `GuiAdvancedOnly` includes `Standard` interactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeInteractionExposure {
    Standard,
    GuiAdvancedOnly,
}

impl RuntimeInteractionExposure {
    pub fn allows(self, required: Self) -> bool {
        matches!(required, Self::Standard) || self == Self::GuiAdvancedOnly
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInteractionPolicy {
    pub kind: RuntimeInteractionKind,
    pub stability: RuntimeStability,
    pub exposure: RuntimeInteractionExposure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessStatus {
    Unchecked,
    Ready,
    Missing,
    NeedsAuth,
    Unsupported,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessStage {
    pub id: String,
    pub status: ReadinessStatus,
    pub summary: String,
    pub observed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlState {
    RuntimeDefault,
    ReadOnlyCurrent,
    Selectable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeControlDependency {
    pub control_id: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeControlDescriptor {
    pub id: String,
    pub label: String,
    pub state: ControlState,
    pub current_value: Option<Value>,
    pub choices: Vec<RuntimeControlChoice>,
    #[serde(default)]
    pub depends_on: Option<RuntimeControlDependency>,
    pub channel_safe: bool,
    pub capability_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeControlChoice {
    pub value: Value,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapability {
    pub id: String,
    pub enabled: bool,
    pub stability: RuntimeStability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePlanStepStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePlanStep {
    pub step: String,
    pub status: RuntimePlanStepStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePlanUpdate {
    pub runtime_ref: String,
    pub thread_id: String,
    pub turn_id: String,
    pub explanation: Option<String>,
    pub steps: Vec<RuntimePlanStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiffUpdate {
    pub runtime_ref: String,
    pub thread_id: String,
    pub turn_id: String,
    pub diff: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTokenUsageBreakdown {
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTokenUsage {
    pub total: RuntimeTokenUsageBreakdown,
    pub last: RuntimeTokenUsageBreakdown,
    pub model_context_window: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUsageUpdate {
    pub runtime_ref: String,
    pub thread_id: String,
    pub turn_id: String,
    pub usage: RuntimeTokenUsage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeGoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeGoal {
    pub objective: String,
    pub status: RuntimeGoalStatus,
    pub token_budget: Option<i64>,
    pub tokens_used: i64,
    pub time_used_seconds: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeGoalChange {
    pub runtime_ref: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    /// `None` represents the typed native goal-cleared notification.
    pub goal: Option<RuntimeGoal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCompactionStatus {
    Started,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCompactionChange {
    pub runtime_ref: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub item_id: Option<String>,
    pub status: RuntimeCompactionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRateLimitReachedType {
    RateLimitReached,
    WorkspaceOwnerCreditsDepleted,
    WorkspaceMemberCreditsDepleted,
    WorkspaceOwnerUsageLimitReached,
    WorkspaceMemberUsageLimitReached,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRateLimitWindow {
    pub used_percent: i32,
    pub window_duration_mins: Option<i64>,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCreditsSnapshot {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSpendControlLimitSnapshot {
    pub limit: String,
    pub used: String,
    pub remaining_percent: i32,
    pub resets_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRateLimitSnapshot {
    pub limit_id: Option<String>,
    pub limit_name: Option<String>,
    pub primary: Option<RuntimeRateLimitWindow>,
    pub secondary: Option<RuntimeRateLimitWindow>,
    pub credits: Option<RuntimeCreditsSnapshot>,
    pub individual_limit: Option<RuntimeSpendControlLimitSnapshot>,
    pub plan_type: Option<String>,
    pub rate_limit_reached_type: Option<RuntimeRateLimitReachedType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAccountRateLimits {
    pub rate_limits: RuntimeRateLimitSnapshot,
    pub rate_limits_by_limit_id: BTreeMap<String, RuntimeRateLimitSnapshot>,
    pub reset_credits_available: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAccountRateLimitsUpdate {
    pub runtime_ref: String,
    pub rate_limits: RuntimeAccountRateLimits,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub runtime_ref: String,
    pub kind: RuntimeKind,
    pub profile_revision: u64,
    pub capability_revision: u64,
    pub adapter_version: String,
    pub runtime_version: Option<String>,
    pub stability: RuntimeStability,
    pub provenance: String,
    pub readiness: Vec<ReadinessStage>,
    pub controls: Vec<RuntimeControlDescriptor>,
    pub capabilities: Vec<RuntimeCapability>,
    pub process_epoch: Option<u64>,
    pub instance_epoch: Option<u64>,
    pub binding_epoch: Option<u64>,
    pub extension: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ExecuteRequest {
    pub profile: RuntimeProfile,
    pub expected_profile_revision: u64,
    pub expected_capability_revision: Option<u64>,
    pub expected_binding_revision: Option<u64>,
    pub intent: RuntimeIntent,
}

#[derive(Debug, Clone)]
pub enum RuntimeIntent {
    Turn(RuntimeTurnRequest),
    Session(RuntimeSessionRequest),
    Compaction(RuntimeCompactionRequest),
    Interaction(RuntimeInteractionResponse),
    Mcp(RuntimeMcpRequest),
    Control(RuntimeControlSetRequest),
    Auth(RuntimeAuthRequest),
    Extension(RuntimeExtensionRequest),
}

#[derive(Debug, Clone)]
pub struct RuntimeTurnRequest {
    pub turn_id: String,
    pub thread_id: String,
    pub native_session_id: Option<String>,
    pub cwd: PathBuf,
    pub prompt: String,
    /// Agent Definition instructions injected through the runtime's native
    /// system/developer-instruction field. `None` means runtime default.
    pub instructions: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub agent: Option<String>,
    pub features: BTreeMap<String, Value>,
    /// Highest interaction exposure explicitly authorized by the caller.
    pub interaction_exposure: RuntimeInteractionExposure,
    pub binding_epoch: u64,
}

#[derive(Debug, Clone)]
pub struct RuntimeSessionRequest {
    pub operation: RuntimeSessionOperation,
    pub thread_id: Option<String>,
    pub native_session_id: Option<String>,
    pub cwd: PathBuf,
    pub cursor: Option<String>,
    pub argument: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCompactionRequest {
    pub thread_id: String,
    pub native_session_id: String,
    pub cwd: PathBuf,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSessionOperation {
    List,
    Read,
    Resume,
    Fork,
    Archive,
    Unarchive,
    Rename,
    Delete,
    Revert,
    Unrevert,
}

#[derive(Debug, Clone)]
pub struct RuntimeInteractionResponse {
    pub interaction_id: String,
    pub process_epoch: u64,
    pub instance_epoch: Option<u64>,
    pub response: Value,
}

#[derive(Debug, Clone)]
pub struct RuntimeMcpRequest {
    pub operation: String,
    pub cwd: PathBuf,
    pub argument: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeControlSetRequest {
    pub thread_id: String,
    pub native_session_id: String,
    pub cwd: PathBuf,
    pub control_id: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAuthRequest {
    pub operation: RuntimeAuthOperation,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeAuthOperation {
    Status { refresh: bool },
    LoginChatgpt,
    LoginDeviceCode,
    Cancel { login_id: String },
    Logout,
}

#[derive(Debug, Clone)]
pub struct RuntimeExtensionRequest {
    pub namespace: String,
    pub operation: String,
    pub argument: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExecuteResult {
    Turn(RuntimeTurnResult),
    Session(RuntimeSessionResult),
    Compaction(RuntimeCompactionResult),
    Interaction(RuntimeInteractionResult),
    Mcp(Value),
    Control(RuntimeControlSetResult),
    Auth(RuntimeAuthResult),
    Extension(Value),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCompactionResult {
    pub thread_id: String,
    pub native_session_id: String,
    pub item_id: String,
    pub compacted: bool,
    pub process_epoch: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeControlSetResult {
    pub changed: bool,
    pub observed: bool,
    pub control: RuntimeControlDescriptor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAuthResult {
    pub accepted: bool,
    pub status: String,
    pub message: String,
    /// Safe adapter response metadata. Implementations must remove credentials
    /// and tokens before crossing the runtime-host seam.
    pub output: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTurnOutcome {
    Completed,
    Interrupted,
    Failed,
}

/// Product-safe classification for a failed accepted turn.
///
/// Adapter-native terminal payloads stay in `RuntimeTurnResult::metadata` and
/// must never be used as a public error surface. Implementations populate this
/// DTO from protocol state they understand, before crossing the runtime-host
/// seam.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTerminalError {
    pub code: String,
    pub stage: RuntimeErrorStage,
    pub retry_class: RetryClass,
    pub message: String,
    pub diagnostic_ref: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTurnResult {
    pub turn_id: String,
    pub thread_id: String,
    pub native_session_id: String,
    pub outcome: RuntimeTurnOutcome,
    pub final_answer: String,
    pub provider: String,
    pub model: String,
    pub history_fidelity: HistoryFidelity,
    pub process_epoch: u64,
    pub instance_epoch: Option<u64>,
    pub terminal_error: Option<RuntimeTerminalError>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryFidelity {
    Full,
    Summary,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOwnership {
    ReadWrite,
    ReadOnly,
    Active,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSession {
    pub native_session_id: String,
    pub thread_id: Option<String>,
    pub parent_native_session_id: Option<String>,
    pub title: Option<String>,
    pub cwd: Option<PathBuf>,
    pub archived: bool,
    pub updated_at_ms: Option<i64>,
    pub cursor: Option<String>,
    pub native_dedup_key: String,
    pub fidelity: HistoryFidelity,
    pub ownership: SessionOwnership,
    pub actions: Vec<String>,
    pub messages: Vec<RuntimeHistoryMessage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHistoryMessage {
    pub dedup_key: String,
    pub role: String,
    pub text: String,
    pub created_at_ms: Option<i64>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeSessionResult {
    pub changed: bool,
    pub sessions: Vec<RuntimeSession>,
    pub cursor: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeInteractionResult {
    pub accepted: bool,
    pub expired: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSessionBinding {
    pub runtime_ref: String,
    pub thread_id: String,
    pub native_session_id: String,
    pub cwd: PathBuf,
    pub binding_epoch: u64,
    pub process_epoch: u64,
    pub instance_epoch: Option<u64>,
}

type ObservationSink = Arc<dyn Fn(RuntimeObservation) + Send + Sync>;
type SessionBinder = Arc<dyn Fn(RuntimeSessionBinding) -> RuntimeFuture<()> + Send + Sync>;

#[derive(Clone)]
pub struct RuntimeObserver {
    observation_sink: ObservationSink,
    session_binder: SessionBinder,
}

impl RuntimeObserver {
    pub fn new(observer: impl Fn(RuntimeObservation) + Send + Sync + 'static) -> Self {
        Self {
            observation_sink: Arc::new(observer),
            session_binder: Arc::new(|_| Box::pin(async { Ok(()) })),
        }
    }

    pub fn emit(&self, observation: RuntimeObservation) {
        (self.observation_sink)(observation);
    }

    pub fn with_session_binder<F, Fut>(mut self, binder: F) -> Self
    where
        F: Fn(RuntimeSessionBinding) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), RuntimeError>> + Send + 'static,
    {
        self.session_binder = Arc::new(move |binding| Box::pin(binder(binding)));
        self
    }

    pub async fn bind_native_session(
        &self,
        binding: RuntimeSessionBinding,
    ) -> Result<(), RuntimeError> {
        (self.session_binder)(binding).await
    }
}

impl Default for RuntimeObserver {
    fn default() -> Self {
        Self::new(|_| {})
    }
}

impl fmt::Debug for RuntimeObserver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("RuntimeObserver")
            .field(&"<observation-and-binding-callbacks>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeObservation {
    StateChanged {
        runtime_ref: String,
        process_epoch: u64,
        instance_epoch: Option<u64>,
        state: String,
        detail: Option<String>,
    },
    ChildChanged {
        runtime_ref: String,
        parent_native_session_id: String,
        native_session_id: String,
        thread_id: Option<String>,
        status: String,
        read_only: bool,
    },
    TextDelta {
        turn_id: String,
        text: String,
    },
    ReasoningDelta {
        turn_id: String,
        text: String,
    },
    Tool {
        turn_id: String,
        item_id: String,
        name: String,
        status: String,
        detail: Option<Value>,
    },
    PlanUpdated(RuntimePlanUpdate),
    DiffUpdated(RuntimeDiffUpdate),
    UsageUpdated(RuntimeUsageUpdate),
    GoalChanged(RuntimeGoalChange),
    CompactionChanged(RuntimeCompactionChange),
    AccountRateLimitsUpdated(RuntimeAccountRateLimitsUpdate),
    Interaction(Box<RuntimeInteraction>),
    Warning {
        code: String,
        message: String,
        diagnostic_ref: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInteraction {
    pub id: String,
    pub policy: RuntimeInteractionPolicy,
    pub kind: String,
    pub runtime_ref: String,
    pub thread_id: String,
    pub native_session_id: String,
    pub parent_native_session_id: Option<String>,
    pub child_native_session_id: Option<String>,
    pub process_epoch: u64,
    pub instance_epoch: Option<u64>,
    pub prompt: String,
    pub questions: Vec<RuntimeInteractionQuestion>,
    pub choices: Vec<RuntimeInteractionChoice>,
    pub authorization_lifetime: Option<String>,
    pub expires_at_ms: Option<i64>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInteractionQuestion {
    pub header: Option<String>,
    pub question: String,
    pub options: Vec<RuntimeInteractionQuestionOption>,
    pub multiple: bool,
    pub custom: bool,
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInteractionQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInteractionChoice {
    pub id: String,
    pub label: String,
    pub decision: String,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeControl {
    inner: Arc<RuntimeControlState>,
}

#[derive(Debug, Default)]
struct RuntimeControlState {
    aborted: AtomicBool,
    notify: Notify,
    steer: Mutex<Vec<String>>,
}

impl RuntimeControl {
    pub fn abort(&self) {
        self.inner.aborted.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    pub fn is_aborted(&self) -> bool {
        self.inner.aborted.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        while !self.is_aborted() {
            self.inner.notify.notified().await;
        }
    }

    pub fn steer(&self, text: impl Into<String>) {
        self.inner
            .steer
            .lock()
            .expect("runtime steer queue poisoned")
            .push(text.into());
        self.inner.notify.notify_waiters();
    }

    pub fn take_steer(&self) -> Vec<String> {
        std::mem::take(
            &mut *self
                .inner
                .steer
                .lock()
                .expect("runtime steer queue poisoned"),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownMode {
    Graceful,
    Force,
    Runtime {
        kind: RuntimeKind,
        runtime_ref: Option<String>,
        force: bool,
    },
}

pub trait RuntimeModule: Send + Sync + fmt::Debug {
    fn snapshot(&self, query: SnapshotQuery) -> RuntimeFuture<RuntimeSnapshot>;

    fn execute(
        &self,
        request: ExecuteRequest,
        observer: RuntimeObserver,
        control: RuntimeControl,
    ) -> RuntimeFuture<ExecuteResult>;

    fn shutdown(&self, mode: ShutdownMode) -> RuntimeFuture<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorStage {
    Configuration,
    Discovery,
    Launch,
    Transport,
    Handshake,
    Authentication,
    Hydration,
    Binding,
    Prompt,
    Interaction,
    Control,
    History,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryClass {
    Never,
    UserAction,
    SafeRetry,
    Reconnect,
    UnknownDelivery,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("{message}")]
pub struct RuntimeError {
    pub code: String,
    pub stage: RuntimeErrorStage,
    pub retry_class: RetryClass,
    pub message: String,
    pub diagnostic_ref: Option<String>,
}

impl RuntimeError {
    pub fn new(
        code: impl Into<String>,
        stage: RuntimeErrorStage,
        retry_class: RetryClass,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            stage,
            retry_class,
            message: message.into(),
            diagnostic_ref: None,
        }
    }

    pub fn with_diagnostic_ref(mut self, diagnostic_ref: impl Into<String>) -> Self {
        self.diagnostic_ref = Some(diagnostic_ref.into());
        self
    }
}
