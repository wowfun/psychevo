#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct RunRequest {
    pub config: Option<PathBuf>,
    pub benchmark: Option<String>,
    pub task_set: Option<String>,
    pub task: Option<String>,
    pub agent: Option<String>,
    pub overwrite: bool,
    pub store_root: Option<PathBuf>,
    pub output_root: Option<PathBuf>,
    pub include_artifacts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TaskEnvCreateRequest {
    pub config: Option<PathBuf>,
    pub benchmark: Option<String>,
    pub task_set: Option<String>,
    pub task: Option<String>,
    pub store_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct TaskEnvVerifyRequest {
    pub env_root: PathBuf,
    pub duration_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEnvManifest {
    pub schema_version: u32,
    pub benchmark: String,
    pub benchmark_slug: String,
    pub project: String,
    pub env_key: String,
    pub case_id: String,
    pub task_set: TaskSetManifest,
    pub task: TaskManifest,
    pub task_set_manifest_path: PathBuf,
    pub task_manifest_path: PathBuf,
    pub task_dir: PathBuf,
    pub workspace_source: PathBuf,
    pub created_at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEnvCreateResult {
    pub schema_version: u32,
    pub benchmark: String,
    pub task_set_id: String,
    pub task_id: String,
    pub env_key: String,
    pub env_root: PathBuf,
    pub workspace: PathBuf,
    pub prompt: PathBuf,
    pub metadata: PathBuf,
    pub readme: PathBuf,
    pub verify_command: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEnvVerifyResult {
    pub schema_version: u32,
    pub benchmark: String,
    pub task_set_id: String,
    pub task_id: String,
    pub env_key: String,
    pub env_root: PathBuf,
    pub run_json: PathBuf,
    pub status: CaseStatus,
    pub passed: bool,
    pub score: Option<f64>,
    pub message: String,
    pub duration_ms: u128,
}

#[derive(Debug, Clone)]
pub struct InitStoreRequest {
    pub root: Option<PathBuf>,
    pub make_default: bool,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellRun {
    pub schema_version: u32,
    pub benchmark: String,
    pub benchmark_slug: String,
    pub cell_key: String,
    pub fingerprint: String,
    pub cell_root: PathBuf,
    pub started_at_ms: u128,
    pub finished_at_ms: u128,
    pub case: CaseResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunExecutionSummary {
    pub schema_version: u32,
    pub project: String,
    pub benchmark: String,
    pub benchmark_slug: String,
    pub selected_cells: usize,
    pub executed_cells: usize,
    pub reused_cells: usize,
    pub overwritten_cells: usize,
    pub retried_cells: usize,
    pub failed_cells: usize,
    pub passed_cells: usize,
    pub status: RunStatus,
    pub cells: Vec<RunExecutionCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunExecutionCell {
    pub cell_key: String,
    pub fingerprint: String,
    pub cell_root: PathBuf,
    pub task_set_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub status: CaseStatus,
    pub action: CellRunAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellRunAction {
    Executed,
    Reused,
    Overwritten,
    Retried,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "kebab-case")]
pub enum RunStatusFilter {
    Passed,
    Failed,
}

impl From<RunStatusFilter> for RunStatus {
    fn from(value: RunStatusFilter) -> Self {
        match value {
            RunStatusFilter::Passed => RunStatus::Passed,
            RunStatusFilter::Failed => RunStatus::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    pub schema_version: u32,
    pub identity: CaseIdentity,
    pub candidate: CandidateIdentity,
    #[serde(default)]
    pub factors: CaseFactors,
    pub case_id: String,
    pub task_set_id: String,
    pub task_id: String,
    #[serde(default = "default_task_kind")]
    pub task_family: String,
    pub agent_id: String,
    pub status: CaseStatus,
    #[serde(default)]
    pub failure_class: Option<String>,
    pub score: ScoreResult,
    pub duration_ms: u128,
    #[serde(default)]
    pub metrics: CaseMetrics,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    pub artifacts: CaseArtifacts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseIdentity {
    pub case_id: String,
    pub task_set_id: String,
    pub task_id: String,
    #[serde(default = "default_task_kind")]
    pub task_family: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateIdentity {
    pub agent_id: String,
    pub adapter: AgentKind,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaseFactors {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunMetrics {
    pub duration_ms: u128,
    pub total_tool_calls: u64,
    pub total_tool_errors: u64,
    #[serde(default)]
    pub total_turns: Option<u64>,
    #[serde(default)]
    pub usage: UsageMetrics,
    #[serde(default)]
    pub accounting: AccountingMetrics,
    #[serde(default)]
    pub cost: CostMetrics,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaseMetrics {
    pub duration_ms: u128,
    pub tool_calls: u64,
    pub tool_errors: u64,
    #[serde(default)]
    pub turns: Option<u64>,
    #[serde(default)]
    pub usage: UsageMetrics,
    #[serde(default)]
    pub accounting: AccountingMetrics,
    #[serde(default)]
    pub cost: CostMetrics,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageMetrics {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_tokens: Option<u64>,
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostMetrics {
    #[serde(default)]
    pub amount_usd: Option<f64>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountingMetrics {
    #[serde(default)]
    pub context_input_tokens: Option<u64>,
    #[serde(default)]
    pub billable_input_tokens: Option<u64>,
    #[serde(default)]
    pub billable_output_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_tokens: Option<u64>,
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    #[serde(default)]
    pub reported_total_tokens: Option<u64>,
    #[serde(default)]
    pub estimated_cost_nanodollars: Option<i64>,
    #[serde(default)]
    pub pricing_source: Option<String>,
    #[serde(default)]
    pub pricing_tier: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseStatus {
    Passed,
    Failed,
    SetupFailed,
    RuntimeFailed,
    EvaluatorFailed,
    Timeout,
}

impl CaseStatus {
    pub fn is_terminal_reusable(self) -> bool {
        matches!(
            self,
            CaseStatus::Passed
                | CaseStatus::Failed
                | CaseStatus::EvaluatorFailed
                | CaseStatus::Timeout
        )
    }

    pub fn is_passed(self) -> bool {
        self == CaseStatus::Passed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreResult {
    pub schema_version: u32,
    pub passed: bool,
    #[serde(default)]
    pub score: Option<f64>,
    pub message: String,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseArtifacts {
    pub result: PathBuf,
    pub trajectory: PathBuf,
    pub evaluator_stdout: PathBuf,
    pub evaluator_stderr: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryEvent {
    pub schema_version: u32,
    pub sequence: u64,
    pub case_id: String,
    pub kind: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u128>,
    #[serde(default)]
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct CasePlan {
    pub case_id: String,
    pub task_set: TaskSetManifest,
    pub task: TaskManifest,
    pub agent: AgentManifest,
}

#[derive(Debug)]
pub(crate) struct ProcessOutcome {
    pub(crate) success: bool,
    pub(crate) code: Option<i32>,
    pub(crate) timed_out: bool,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}
