#[derive(Debug, Clone)]
pub struct RunResult {
    pub session_id: String,
    pub outcome: Outcome,
    pub terminal_reason: Option<TerminalReason>,
    pub final_answer: String,
    pub db_path: PathBuf,
    pub cwd: PathBuf,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub reasoning_effort: Option<String>,
    pub context_limit: Option<u64>,
    pub tool_failures: usize,
    pub selected_agent: Option<SelectedAgent>,
    pub selected_skills: Vec<SelectedSkill>,
    pub context_snapshot: Option<crate::context_usage::ContextSnapshot>,
    pub terminal_error: Option<RunTerminalError>,
    pub events: Vec<Value>,
    pub warnings: Vec<RunWarning>,
}

/// Product-safe direct-runtime terminal classification carried through the
/// ordinary run result without exposing adapter-native terminal metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTerminalError {
    pub code: String,
    pub stage: String,
    pub retry_class: String,
    pub message: String,
    pub diagnostic_ref: String,
}

#[derive(Debug, Clone)]
pub struct ReloadContextOptions {
    pub state: StateRuntime,
    pub session: String,
    pub config_path: Option<PathBuf>,
    pub mode: Option<RunMode>,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub agent: Option<String>,
    pub no_agents: bool,
    pub no_skills: bool,
    pub invalidation_reason: String,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReloadContextResult {
    pub session_id: String,
    pub prefix_hash: String,
    pub version: i64,
    pub provider: String,
    pub model: String,
    pub invalidation_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedAgent {
    pub name: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunWarning {
    pub kind: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UserShellOptions {
    pub cwd: PathBuf,
    pub command: String,
    pub context: Option<UserShellContextOptions>,
    pub inject_into: Option<RunControlHandle>,
}

#[derive(Debug, Clone)]
pub struct UserShellContextOptions {
    pub state: StateRuntime,
    pub session: Option<String>,
    pub continue_latest: bool,
    pub source: String,
    pub continue_sources: Vec<String>,
    pub config_path: Option<PathBuf>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub mode: RunMode,
    pub inherited_env: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone)]
pub struct StatsOptions {
    pub state: StateRuntime,
    pub cwd: PathBuf,
    pub all: bool,
    pub days: Option<u64>,
    pub limit: usize,
}

#[derive(Debug, Clone)]
pub struct SessionUsageOptions {
    pub state: StateRuntime,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionUsageSummary {
    pub session_id: String,
    pub provider: String,
    pub model: String,
    pub message_count: u64,
    pub assistant_message_count: u64,
    pub context_input_tokens: u64,
    pub billable_input_tokens: u64,
    pub billable_output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reported_total_tokens: u64,
    pub estimated_cost_nanodollars: i64,
    pub cost_status: String,
    pub estimated_pricing_count: u64,
    pub free_pricing_count: u64,
    pub included_pricing_count: u64,
    pub unknown_pricing_count: u64,
    pub cache_read_percent: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct UsageReadOptions {
    pub state: StateRuntime,
    pub activity_days: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageReadResult {
    pub generated_at_ms: i64,
    pub windows: Vec<UsageWindowSummary>,
    pub activity: UsageActivity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageWindowSummary {
    pub id: String,
    pub label: String,
    pub since_ms: Option<i64>,
    pub session_count: u64,
    pub message_count: u64,
    pub assistant_message_count: u64,
    pub context_input_tokens: u64,
    pub billable_input_tokens: u64,
    pub billable_output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reported_total_tokens: u64,
    pub estimated_cost_nanodollars: i64,
    pub cost_status: String,
    pub estimated_pricing_count: u64,
    pub free_pricing_count: u64,
    pub included_pricing_count: u64,
    pub unknown_pricing_count: u64,
    pub cache_read_percent: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageActivity {
    pub start_date: String,
    pub end_date: String,
    pub days: Vec<UsageActivityDay>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageActivityDay {
    pub date: String,
    pub session_count: u64,
    pub message_count: u64,
    pub reported_total_tokens: u64,
    pub context_input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub estimated_cost_nanodollars: i64,
    pub cost_status: String,
    pub estimated_pricing_count: u64,
    pub free_pricing_count: u64,
    pub included_pricing_count: u64,
    pub unknown_pricing_count: u64,
}

#[derive(Debug, Clone)]
pub struct UserShellResult {
    pub command: String,
    pub cwd: PathBuf,
    pub session_id: Option<String>,
    pub context_text: Option<String>,
    pub outcome: Outcome,
    pub tool_failures: usize,
    pub result: Value,
}

#[derive(Debug, Clone)]
pub struct SessionUndoOptions {
    pub state: StateRuntime,
    pub cwd: PathBuf,
    pub snapshot_root: PathBuf,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUndoResult {
    pub session_id: String,
    pub prompt: String,
    pub reverted_messages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRedoResult {
    pub session_id: String,
    pub restored_messages: usize,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub source: String,
    pub parent_session_id: Option<String>,
    pub cwd: String,
    pub model: String,
    pub provider: String,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub end_reason: Option<String>,
    pub archived_at_ms: Option<i64>,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfiguredModel {
    pub provider: String,
    pub provider_label: String,
    pub model: String,
    pub model_name: Option<String>,
    pub reasoning_effort: Option<String>,
    pub context_limit: Option<u64>,
    pub metadata: ModelMetadata,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ModelCatalogProvider {
    pub provider: String,
    pub display_label: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub missing_credentials: Option<String>,
    pub unavailable_reason: Option<String>,
    pub no_auth: bool,
    pub(crate) api_key: Option<String>,
}

impl ModelCatalogProvider {
    pub fn fetchable(&self) -> bool {
        self.missing_credentials.is_none() && self.unavailable_reason.is_none()
    }
}
