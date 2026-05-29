#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct ViewRequest {
    pub config: Option<PathBuf>,
    pub benchmark: Option<String>,
    pub report: Option<String>,
    pub store_root: Option<PathBuf>,
    pub path: Option<PathBuf>,
    pub task_set: Option<String>,
    pub agent: Option<String>,
    pub task: Option<String>,
    pub status: Option<CaseStatusFilter>,
    pub group_by: Vec<ViewGroupBy>,
    pub include: Vec<ViewInclude>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum ViewInclude {
    Summary,
    Matrix,
    Usage,
    Warnings,
    Artifacts,
    Trajectory,
    TrajectoryMeta,
    Analysis,
}

pub(crate) fn all_view_includes() -> Vec<ViewInclude> {
    vec![
        ViewInclude::Summary,
        ViewInclude::Matrix,
        ViewInclude::Usage,
        ViewInclude::Warnings,
        ViewInclude::Artifacts,
        ViewInclude::Trajectory,
        ViewInclude::TrajectoryMeta,
        ViewInclude::Analysis,
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum ViewFormat {
    Json,
    Html,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum ViewGroupBy {
    Agent,
    Task,
    TaskSet,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "kebab-case")]
pub enum CaseStatusFilter {
    Passed,
    Failed,
    SetupFailed,
    RuntimeFailed,
    EvaluatorFailed,
    Timeout,
}

impl From<CaseStatusFilter> for CaseStatus {
    fn from(value: CaseStatusFilter) -> Self {
        match value {
            CaseStatusFilter::Passed => CaseStatus::Passed,
            CaseStatusFilter::Failed => CaseStatus::Failed,
            CaseStatusFilter::SetupFailed => CaseStatus::SetupFailed,
            CaseStatusFilter::RuntimeFailed => CaseStatus::RuntimeFailed,
            CaseStatusFilter::EvaluatorFailed => CaseStatus::EvaluatorFailed,
            CaseStatusFilter::Timeout => CaseStatus::Timeout,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewReport {
    pub schema_version: u32,
    pub includes: Vec<ViewInclude>,
    pub scope: ViewScope,
    pub summary: ViewSummary,
    pub groups: Vec<ViewGroupRow>,
    pub matrix: ViewMatrix,
    pub leaderboard: ViewLeaderboard,
    pub trials: Vec<ViewTrial>,
    pub usage: Vec<ViewUsageRow>,
    pub warnings: Vec<ViewWarningRow>,
    pub artifacts: Vec<ViewArtifactIndex>,
    pub trajectory: Vec<AtifTrajectory>,
    pub trajectory_meta: Vec<ViewTrajectoryMetaReport>,
    pub analysis: Vec<ViewAnalysisReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewScope {
    pub workspace_root: PathBuf,
    pub path: PathBuf,
    pub benchmark: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewSummary {
    pub total_trials: usize,
    pub passed_trials: usize,
    pub failed_trials: usize,
    pub status: RunStatus,
    pub metrics: RunMetrics,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ViewMatrix {
    pub task_axis: Vec<ViewMatrixAxisEntry>,
    pub agent_axis: Vec<ViewMatrixAxisEntry>,
    pub cells: Vec<ViewMatrixCell>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewMatrixAxisEntry {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewMatrixCell {
    pub benchmark: String,
    pub matrix_cell_key: String,
    pub trial_keys: Vec<String>,
    pub representative_trial_key: String,
    pub agent_axis_id: String,
    pub task_set_id: String,
    pub task_id: String,
    pub task_family: String,
    pub agent_id: String,
    pub adapter: AgentKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    pub status: CaseStatus,
    pub failure_class: Option<String>,
    pub score: Option<f64>,
    pub duration_ms: u128,
    pub turns: Option<u64>,
    pub tool_calls: u64,
    pub tool_errors: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewTrial {
    pub benchmark: String,
    pub trial_key: String,
    pub matrix_cell_key: String,
    pub cell_root_relative: PathBuf,
    pub case_id: String,
    pub started_at_ms: u128,
    pub finished_at_ms: u128,
    pub task_set_id: String,
    pub task_id: String,
    pub task_family: String,
    pub agent_id: String,
    pub adapter: AgentKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    pub status: CaseStatus,
    pub failure_class: Option<String>,
    pub score_passed: bool,
    pub score: Option<f64>,
    pub score_message: String,
    pub score_details: Value,
    pub duration_ms: u128,
    pub turns: Option<u64>,
    pub tool_calls: u64,
    pub tool_errors: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_ref: Option<ViewDataRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_preview: Option<String>,
    pub prompt_truncated: bool,
    pub artifact_refs: Vec<ViewDataRef>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ViewLeaderboard {
    pub entries: Vec<ViewLeaderboardEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewLeaderboardEntry {
    pub rank: usize,
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    pub total_trials: usize,
    pub successes: usize,
    pub failures: usize,
    pub pass_rate: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_duration_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    pub tasks: Vec<ViewLeaderboardTask>,
    pub trial_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewLeaderboardTask {
    pub task_id: String,
    pub task_family: String,
    pub total_trials: usize,
    pub successes: usize,
    pub pass_rate: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_duration_ms: Option<f64>,
    pub trial_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewDataRef {
    pub kind: String,
    pub label: String,
    pub relative_path: PathBuf,
    pub mime: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<u128>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewUsageRow {
    pub trial_key: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub accounting: AccountingMetrics,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewWarningRow {
    pub trial_key: String,
    pub warning: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewGroupRow {
    pub key: String,
    pub total_trials: usize,
    pub passed_trials: usize,
    pub failed_trials: usize,
    pub status: RunStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewArtifactIndex {
    pub trial_key: String,
    pub paths: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewArtifactFile {
    #[serde(flatten)]
    pub data_ref: ViewDataRef,
    pub previewable: bool,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewTrajectoryMetaReport {
    pub trial_key: String,
    pub data_ref: ViewDataRef,
    pub total_events: usize,
    pub unmapped_events: usize,
    pub total_steps: usize,
    pub duration_ms: u128,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub token_total: Option<u64>,
    pub cost_usd: Option<f64>,
    pub prompt_unavailable: bool,
    pub system_exposed: bool,
    pub reasoning_exposed: bool,
    pub steps: Vec<ViewTrajectoryStepMeta>,
    pub graph: ViewTrajectoryGraph,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewTrajectoryStepMeta {
    pub step_id: u64,
    pub source: String,
    pub label: String,
    pub summary: String,
    pub tool_names: Vec<String>,
    pub tool_calls: Vec<ViewTrajectoryToolMeta>,
    pub observations: Vec<ViewTrajectoryObservationMeta>,
    pub tool_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u128>,
    pub duration_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    pub token_total: Option<u64>,
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_call_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_preview: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewTrajectoryToolMeta {
    pub tool_call_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_start_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_duration_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_duration_ms: Option<u128>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewTrajectoryObservationMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u128>,
    pub tool_error: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ViewTrajectoryGraph {
    pub nodes: Vec<ViewTrajectoryGraphNode>,
    pub edges: Vec<ViewTrajectoryGraphEdge>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewTrajectoryGraphNode {
    pub id: String,
    pub step_id: u64,
    pub label: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewTrajectoryGraphEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtifTrajectory {
    pub schema_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    pub agent: AtifAgent,
    pub steps: Vec<AtifStep>,
    #[serde(skip)]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_metrics: Option<AtifFinalMetrics>,
    #[serde(skip)]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtifAgent {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip)]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtifStep {
    pub step_id: u64,
    pub source: String,
    pub message: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<AtifToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<AtifObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<AtifMetrics>,
    #[serde(skip)]
    pub extra: Option<Value>,
    #[serde(skip)]
    pub llm_call_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtifToolCall {
    pub tool_call_id: String,
    pub function_name: String,
    pub arguments: Value,
    #[serde(skip)]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtifObservation {
    pub results: Vec<AtifObservationResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtifObservationResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip)]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtifMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_errors: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accounting: Option<AccountingMetrics>,
    #[serde(skip)]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtifFinalMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cached_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_turns: Option<u64>,
    pub total_tool_calls: u64,
    pub total_tool_errors: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accounting: Option<AccountingMetrics>,
    pub total_steps: u64,
    #[serde(skip)]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewAnalysisReport {
    pub trial_key: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_ref: Option<ViewDataRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewDiffReport {
    pub trial_key: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_ref: Option<ViewDataRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
