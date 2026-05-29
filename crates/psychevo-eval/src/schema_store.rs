#[allow(unused_imports)]
pub(crate) use super::*;

#[allow(unused_imports)]
use anyhow::{Context, Result, bail};
#[allow(unused_imports)]
use clap::{Parser, Subcommand, ValueEnum};
#[allow(unused_imports)]
use serde_json::{Value, json};
#[allow(unused_imports)]
use std::collections::{BTreeMap, BTreeSet};
#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::ffi::OsString;
#[allow(unused_imports)]
use std::fs;
#[allow(unused_imports)]
use std::io::{BufRead, BufReader};
#[allow(unused_imports)]
use std::path::{Component, Path, PathBuf};
#[allow(unused_imports)]
use std::process::{Command, Stdio};
#[allow(unused_imports)]
use std::thread;
#[allow(unused_imports)]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[allow(unused_imports)]
use uuid::Uuid;

pub const MANIFEST_SCHEMA_VERSION: u32 = 5;
pub const EVALUATOR_RESULT_SCHEMA_VERSION: u32 = 2;
pub const ARTIFACT_SCHEMA_VERSION: u32 = 8;
pub const INDEX_SCHEMA_VERSION: u32 = 1;
pub const VIEW_SCHEMA_VERSION: u32 = 7;
pub const WORKSPACE_SCHEMA_VERSION: u32 = 2;
pub const SCHEMA_VERSION: u32 = MANIFEST_SCHEMA_VERSION;

#[derive(Debug, Clone)]
pub struct EvalProject {
    pub eval_root: Option<PathBuf>,
    pub eval_manifest_path: Option<PathBuf>,
    pub id: String,
    pub name: String,
    pub benchmark_root: PathBuf,
    pub benchmark_manifest_path: PathBuf,
    pub benchmark_id: String,
    pub benchmark_name: String,
    pub schema_version: u32,
    pub output_root: Option<PathBuf>,
    pub artifacts: ArtifactSelection,
    pub agents: BTreeMap<String, AgentManifest>,
    pub task_sets: BTreeMap<String, TaskSetManifest>,
    pub tasks: BTreeMap<String, TaskManifest>,
    pub selection: EvalSelection,
}

#[derive(Debug, Clone)]
pub struct BenchmarkManifest {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub sources: Vec<BenchmarkSourceSummary>,
    pub task_sets: BTreeMap<String, TaskSetManifest>,
    pub tasks: BTreeMap<String, TaskManifest>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalSelection {
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub sets: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArtifactSelection {
    #[serde(default)]
    pub include: Vec<String>,
}

impl EvalSelection {
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty() && self.sets.is_empty() && self.tasks.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    #[serde(default = "current_manifest_schema_version")]
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub kind: AgentKind,
    #[serde(default)]
    pub fake: FakeAgentOptions,
    #[serde(default)]
    pub command: CommandAgentOptions,
    #[serde(default)]
    pub acp: AcpAgentOptions,
    #[serde(default)]
    pub psychevo: PsychevoAgentOptions,
    #[serde(default)]
    pub opencode: WrapperAgentOptions,
    #[serde(default)]
    pub hermes: WrapperAgentOptions,
    #[serde(skip)]
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    Fake,
    Command,
    Acp,
    Psychevo,
    Opencode,
    Hermes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FakeAgentOptions {
    #[serde(default = "default_fake_behavior")]
    pub behavior: FakeBehavior,
}

impl Default for FakeAgentOptions {
    fn default() -> Self {
        Self {
            behavior: FakeBehavior::Pass,
        }
    }
}

pub(crate) fn default_fake_behavior() -> FakeBehavior {
    FakeBehavior::Pass
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FakeBehavior {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PsychevoAgentOptions {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandAgentOptions {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_agent_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub model: Option<String>,
}

impl Default for CommandAgentOptions {
    fn default() -> Self {
        Self {
            command: None,
            args: Vec::new(),
            timeout_seconds: default_agent_timeout_seconds(),
            model: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpAgentOptions {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_agent_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub permission: Option<String>,
}

impl Default for AcpAgentOptions {
    fn default() -> Self {
        Self {
            command: None,
            args: Vec::new(),
            timeout_seconds: default_agent_timeout_seconds(),
            model: None,
            mode: None,
            permission: None,
        }
    }
}

pub(crate) fn default_agent_timeout_seconds() -> u64 {
    600
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WrapperAgentOptions {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub collector: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSetManifest {
    #[serde(default = "current_manifest_schema_version")]
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tasks: Vec<String>,
    #[serde(skip)]
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskSourceKind {
    #[default]
    PevalAgent,
    Harbor,
    SweBench,
    Tau2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSourceSummary {
    pub id: String,
    pub kind: TaskSourceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskManifest {
    #[serde(default = "current_manifest_schema_version")]
    pub schema_version: u32,
    #[serde(rename = "task_id")]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_task_kind")]
    pub kind: String,
    pub problem_statement: String,
    pub workspace: WorkspaceManifest,
    pub test_spec: TestSpecManifest,
    #[serde(default)]
    pub source_kind: TaskSourceKind,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub native_id: String,
    #[serde(default)]
    pub verifier_timeout_seconds: Option<u64>,
    #[serde(skip)]
    pub manifest_path: PathBuf,
    #[serde(skip)]
    pub dir: PathBuf,
}

pub(crate) fn current_manifest_schema_version() -> u32 {
    MANIFEST_SCHEMA_VERSION
}

pub(crate) fn default_task_kind() -> String {
    "swe-style".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub source: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSpecManifest {
    #[serde(default)]
    pub checks: Vec<LocalCodingCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LocalCodingCheck {
    PythonFunctionCases {
        module: PathBuf,
        function: String,
        cases: Vec<PythonFunctionCase>,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    ExactFile {
        path: PathBuf,
        expected: String,
    },
    CargoTest {
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonFunctionCase {
    #[serde(default)]
    pub args: Vec<Value>,
    #[serde(default)]
    pub kwargs: BTreeMap<String, Value>,
    pub expected: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandManifest {
    pub command: Vec<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FakeTaskCommands {
    #[serde(default)]
    pub pass: Option<CommandManifest>,
    #[serde(default)]
    pub fail: Option<CommandManifest>,
}

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
    pub timestamp_ms: u128,
    #[serde(default)]
    pub data: Value,
}

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
    Atif,
    Logs,
    Analysis,
    Diff,
}

pub(crate) fn all_view_includes() -> Vec<ViewInclude> {
    vec![
        ViewInclude::Summary,
        ViewInclude::Matrix,
        ViewInclude::Usage,
        ViewInclude::Warnings,
        ViewInclude::Artifacts,
        ViewInclude::Trajectory,
        ViewInclude::Atif,
        ViewInclude::Logs,
        ViewInclude::Analysis,
        ViewInclude::Diff,
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum ViewFormat {
    Markdown,
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
    pub trials: Vec<ViewTrial>,
    pub usage: Vec<ViewUsageRow>,
    pub warnings: Vec<ViewWarningRow>,
    pub artifacts: Vec<ViewArtifactIndex>,
    pub trajectory: Vec<ViewTrajectoryReport>,
    pub atif: Vec<ViewAtifReport>,
    pub logs: Vec<ViewLogIndex>,
    pub analysis: Vec<ViewAnalysisReport>,
    pub diff: Vec<ViewDiffReport>,
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
    pub task_set_id: String,
    pub task_id: String,
    pub task_family: String,
    pub agent_id: String,
    pub adapter: AgentKind,
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
    pub case_id: String,
    pub task_set_id: String,
    pub task_id: String,
    pub task_family: String,
    pub agent_id: String,
    pub adapter: AgentKind,
    pub status: CaseStatus,
    pub failure_class: Option<String>,
    pub score: Option<f64>,
    pub duration_ms: u128,
    pub turns: Option<u64>,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub artifact_refs: Vec<ViewDataRef>,
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
    pub benchmark: String,
    pub trial_key: String,
    pub matrix_cell_key: String,
    pub case_id: String,
    pub agent_id: String,
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
    pub benchmark: String,
    pub trial_key: String,
    pub matrix_cell_key: String,
    pub case_id: String,
    pub agent_id: String,
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
    pub benchmark: String,
    pub trial_key: String,
    pub matrix_cell_key: String,
    pub files: Vec<ViewArtifactFile>,
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
pub struct ViewTrajectoryReport {
    pub benchmark: String,
    pub trial_key: String,
    pub matrix_cell_key: String,
    pub data_ref: ViewDataRef,
    pub total_events: usize,
    pub unmapped_events: usize,
    pub total_steps: usize,
    pub duration_ms: u128,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub token_total: Option<u64>,
    pub cost_usd: Option<f64>,
    pub steps: Vec<ViewTrajectoryStep>,
    pub graph: ViewTrajectoryGraph,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewTrajectoryStep {
    pub step_id: u64,
    pub source: String,
    pub label: String,
    pub summary: String,
    pub tool_names: Vec<String>,
    pub tool_error: bool,
    pub duration_ms: Option<u128>,
    pub token_total: Option<u64>,
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_preview: Option<String>,
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
pub struct ViewAtifReport {
    pub benchmark: String,
    pub trial_key: String,
    pub matrix_cell_key: String,
    pub trajectory: AtifTrajectory,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_metrics: Option<AtifFinalMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtifAgent {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_call_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtifToolCall {
    pub tool_call_id: String,
    pub function_name: String,
    pub arguments: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewLogIndex {
    pub benchmark: String,
    pub trial_key: String,
    pub matrix_cell_key: String,
    pub files: Vec<ViewArtifactFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewAnalysisReport {
    pub benchmark: String,
    pub trial_key: String,
    pub matrix_cell_key: String,
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
    pub benchmark: String,
    pub trial_key: String,
    pub matrix_cell_key: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_ref: Option<ViewDataRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RunSelectorFilters {
    pub task_set: Option<String>,
    pub agent: Option<String>,
    pub status: Option<RunStatusFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub payload: PathBuf,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub split: Option<String>,
    #[serde(default)]
    pub sample_limit: Option<usize>,
    #[serde(default)]
    pub cache_key: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetEntry {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub payload: PathBuf,
    pub payload_exists: bool,
    pub manifest_path: PathBuf,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub split: Option<String>,
    #[serde(default)]
    pub sample_limit: Option<usize>,
    #[serde(default)]
    pub cache_key: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DatasetImportRequest {
    pub store_root: Option<PathBuf>,
    pub path: PathBuf,
    pub id: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub loader: Option<String>,
    pub split: Option<String>,
    pub sample_limit: Option<usize>,
    pub cache_key: Option<String>,
    pub license: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EvalStore {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitStoreResult {
    pub schema_version: u32,
    pub root: PathBuf,
    pub default_workspace: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PevalGlobalConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub default_workspace: Option<PathBuf>,
    #[serde(default)]
    pub analysis: Option<PevalAnalysisConfig>,
    #[serde(default)]
    pub reports: BTreeMap<String, PevalReportProfile>,
    #[serde(default)]
    pub agents: Vec<AgentManifest>,
    #[serde(default)]
    pub benchmarks: Vec<RegistryBenchmark>,
}

impl Default for PevalGlobalConfig {
    fn default() -> Self {
        Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            default_workspace: None,
            analysis: None,
            reports: BTreeMap::new(),
            agents: Vec::new(),
            benchmarks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PevalWorkspaceConfig {
    pub schema_version: u32,
    pub kind: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub analysis: Option<PevalAnalysisConfig>,
    #[serde(default)]
    pub reports: BTreeMap<String, PevalReportProfile>,
    #[serde(default)]
    pub agents: Vec<AgentManifest>,
    #[serde(default)]
    pub benchmarks: Vec<RegistryBenchmark>,
}

impl Default for PevalWorkspaceConfig {
    fn default() -> Self {
        Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            kind: "workspace".to_string(),
            name: None,
            analysis: None,
            reports: BTreeMap::new(),
            agents: Vec::new(),
            benchmarks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PevalReportProfile {
    #[serde(default)]
    pub analysis: Option<PevalAnalysisConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PevalAnalysisConfig {
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub concurrency: Option<usize>,
    #[serde(default)]
    pub rubric_path: Option<PathBuf>,
    #[serde(default)]
    pub rubric: Option<PevalAnalysisRubric>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PevalAnalysisRubric {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub checks: Vec<PevalAnalysisRubricCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PevalAnalysisRubricCheck {
    pub name: String,
    pub guidance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryBenchmark {
    pub id: String,
    pub path: PathBuf,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct EvalConfigManifest {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) output_root: Option<PathBuf>,
    pub(crate) artifacts: ArtifactSelection,
    pub(crate) analysis: Option<PevalAnalysisConfig>,
    pub(crate) reports: BTreeMap<String, PevalReportProfile>,
    pub(crate) benchmark: BenchmarkReference,
    pub(crate) selection: EvalSelection,
    pub(crate) agents: Vec<AgentManifest>,
    pub(crate) benchmarks: Vec<RegistryBenchmark>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawEvalConfigManifest {
    pub(crate) schema_version: u32,
    #[serde(default = "default_eval_id")]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) output_root: Option<PathBuf>,
    #[serde(default)]
    pub(crate) artifacts: ArtifactSelection,
    #[serde(default)]
    pub(crate) analysis: Option<PevalAnalysisConfig>,
    #[serde(default)]
    pub(crate) reports: BTreeMap<String, PevalReportProfile>,
    pub(crate) benchmark: BenchmarkReference,
    pub(crate) select: EvalSelection,
    #[serde(default)]
    pub(crate) agents: Vec<AgentManifest>,
    #[serde(default)]
    pub(crate) benchmarks: Vec<RegistryBenchmark>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkReference {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawBenchmarkManifestSerde {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) sources: BenchmarkSources,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ManifestVersion {
    pub(crate) schema_version: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BenchmarkSources {
    #[serde(default)]
    pub(crate) peval_agent: Vec<PevalAgentSourceManifest>,
    #[serde(default)]
    pub(crate) harbor: Vec<HarborSourceManifest>,
    #[serde(default)]
    pub(crate) swe_bench: Vec<SweBenchSourceManifest>,
    #[serde(default)]
    pub(crate) tau2: Vec<Tau2SourceManifest>,
}

impl BenchmarkSources {
    pub(crate) fn is_empty(&self) -> bool {
        self.peval_agent.is_empty()
            && self.harbor.is_empty()
            && self.swe_bench.is_empty()
            && self.tau2.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PevalAgentSourceManifest {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    #[serde(default)]
    pub(crate) verifier_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) sets: Vec<SourceSetManifest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HarborSourceManifest {
    pub(crate) id: String,
    pub(crate) root: PathBuf,
    pub(crate) path: PathBuf,
    #[serde(default)]
    pub(crate) sets: Vec<SourceSetManifest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SweBenchSourceManifest {
    pub(crate) id: String,
    pub(crate) root: PathBuf,
    pub(crate) dataset: String,
    pub(crate) split: String,
    #[serde(default)]
    pub(crate) sets: Vec<SourceSetManifest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tau2SourceManifest {
    pub(crate) id: String,
    pub(crate) root: PathBuf,
    pub(crate) domain: String,
    #[serde(default)]
    pub(crate) split: Option<String>,
    #[serde(default)]
    pub(crate) task_set: Option<String>,
    #[serde(default)]
    pub(crate) sets: Vec<SourceSetManifest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceSetManifest {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) include: Vec<String>,
    #[serde(default)]
    pub(crate) exclude: Vec<String>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

pub(crate) fn default_eval_id() -> String {
    "evaluation".to_string()
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

impl EvalProject {
    pub fn load(start: impl AsRef<Path>) -> Result<Self> {
        let manifest_path = discover_manifest(start.as_ref())?;
        load_eval_config(&manifest_path, None)
    }

    pub fn namespace(&self) -> Result<PathBuf> {
        self.output_root
            .as_ref()
            .map(|path| validate_store_namespace(path))
            .transpose()?
            .map(Ok)
            .unwrap_or_else(|| Ok(PathBuf::from("runs").join(self.slug())))
    }

    pub fn slug(&self) -> String {
        slugify(&self.benchmark_id)
    }
}

impl BenchmarkManifest {
    pub fn load(start: impl AsRef<Path>) -> Result<Self> {
        let manifest_path = discover_benchmark_manifest(start.as_ref())?;
        let root = manifest_path
            .parent()
            .context("benchmark.toml has no parent directory")?
            .to_path_buf();
        let manifest_raw = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let version: ManifestVersion = toml::from_str(&manifest_raw).with_context(|| {
            format!(
                "failed to parse schema_version in {}",
                manifest_path.display()
            )
        })?;
        reject_unsupported(version.schema_version, &manifest_path)?;
        let raw: RawBenchmarkManifestSerde = toml::from_str(&manifest_raw)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
        let (sources, task_sets, tasks) =
            load_benchmark_sources(&root, &manifest_path, raw.sources)?;
        Ok(BenchmarkManifest {
            root,
            manifest_path,
            schema_version: raw.schema_version,
            id: slugify(&raw.id),
            name: raw.name.unwrap_or(raw.id),
            sources,
            task_sets,
            tasks,
        })
    }
}

impl EvalStore {
    pub fn resolve(store_root: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            root: resolve_store_root(store_root)?,
        })
    }

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn run_output_base(&self, project: &EvalProject) -> Result<PathBuf> {
        Ok(self.root.join(project.namespace()?))
    }

    pub fn cell_runs_root(&self, project: &EvalProject) -> PathBuf {
        self.root.join("runs").join(project.slug())
    }

    pub fn cell_root(&self, project: &EvalProject, case: &CasePlan, cell_key: &str) -> PathBuf {
        self.cell_runs_root(project)
            .join(sanitize_id(&case.agent.id))
            .join(sanitize_id(&case.task.id))
            .join(cell_key)
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.root.join("runs"))
            .with_context(|| format!("failed to create {}", self.root.join("runs").display()))?;
        fs::create_dir_all(self.root.join("datasets")).with_context(|| {
            format!("failed to create {}", self.root.join("datasets").display())
        })?;
        fs::create_dir_all(self.root.join("scripts"))
            .with_context(|| format!("failed to create {}", self.root.join("scripts").display()))?;
        Ok(())
    }

    pub fn list_datasets(&self) -> Result<Vec<DatasetEntry>> {
        let mut entries = Vec::new();
        let datasets_dir = self.root.join("datasets");
        if !datasets_dir.is_dir() {
            return Ok(entries);
        }
        for entry in fs::read_dir(&datasets_dir)
            .with_context(|| format!("failed to read {}", datasets_dir.display()))?
        {
            let path = entry?.path().join("dataset.toml");
            if path.is_file() {
                entries.push(read_dataset_entry(&path)?);
            }
        }
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(entries)
    }

    pub fn refresh_after_dataset_change(&self) -> Result<()> {
        Ok(())
    }

    pub fn scan_cell_runs(&self, scope: &Path) -> Result<Vec<CellRun>> {
        let scope = if scope.is_absolute() {
            scope.to_path_buf()
        } else {
            self.root.join(scope)
        };
        let mut cells = Vec::new();
        if !scope.is_dir() {
            return Ok(cells);
        }
        self.scan_cell_runs_in(&scope, &mut cells)?;
        cells.sort_by(|left, right| {
            left.benchmark
                .cmp(&right.benchmark)
                .then_with(|| left.case.agent_id.cmp(&right.case.agent_id))
                .then_with(|| left.case.task_id.cmp(&right.case.task_id))
                .then_with(|| left.cell_key.cmp(&right.cell_key))
        });
        Ok(cells)
    }

    fn scan_cell_runs_in(&self, dir: &Path, cells: &mut Vec<CellRun>) -> Result<()> {
        let run_json = dir.join("run.json");
        if run_json.is_file() {
            if let Ok(cell) = read_cell_run(dir) {
                cells.push(cell);
            }
            return Ok(());
        }
        for entry in
            fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                self.scan_cell_runs_in(&path, cells)?;
            }
        }
        Ok(())
    }
}

pub(crate) fn workspace_config_path(root: &Path) -> PathBuf {
    root.join("peval.toml")
}

pub(crate) fn global_peval_config_path(home: &Path) -> PathBuf {
    home.join("peval-config.toml")
}

pub(crate) fn ensure_workspace_config(root: &Path) -> Result<()> {
    let path = workspace_config_path(root);
    if path.is_file() {
        let _ = read_workspace_config(root)?;
        return Ok(());
    }
    let config = PevalWorkspaceConfig::default();
    write_toml_pretty(&path, &config)
}

pub(crate) fn read_workspace_config(root: &Path) -> Result<PevalWorkspaceConfig> {
    let path = workspace_config_path(root);
    let config: PevalWorkspaceConfig = read_toml(&path)?;
    reject_unsupported_workspace(config.schema_version, &path)?;
    if config.kind != "workspace" {
        bail!(
            "{} is not a peval workspace config; expected kind = \"workspace\"",
            path.display()
        );
    }
    Ok(config)
}

pub(crate) fn read_global_peval_config(home: &Path) -> Result<PevalGlobalConfig> {
    let path = global_peval_config_path(home);
    if !path.is_file() {
        return Ok(PevalGlobalConfig::default());
    }
    let config: PevalGlobalConfig = read_toml(&path)?;
    reject_unsupported_workspace(config.schema_version, &path)?;
    Ok(config)
}

pub(crate) fn write_global_peval_config(home: &Path, config: &PevalGlobalConfig) -> Result<()> {
    fs::create_dir_all(home).with_context(|| format!("failed to create {}", home.display()))?;
    write_toml_pretty(&global_peval_config_path(home), config)
}

pub(crate) fn write_default_workspace(home: &Path, root: &Path, force: bool) -> Result<()> {
    let mut config = read_global_peval_config(home)?;
    if let Some(existing) = &config.default_workspace
        && existing != root
        && !force
    {
        bail!(
            "{} already points to {}; rerun `peval init --default --force --root {}` to replace it",
            global_peval_config_path(home).display(),
            existing.display(),
            root.display()
        );
    }
    config.default_workspace = Some(root.to_path_buf());
    write_global_peval_config(home, &config)
}

pub(crate) fn copy_workspace_templates(root: &Path) -> Result<()> {
    let scripts = root.join("scripts");
    fs::create_dir_all(&scripts).with_context(|| format!("failed to create {}", scripts.display()))
}

pub fn init_eval_store(request: InitStoreRequest) -> Result<InitStoreResult> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let root = if let Some(path) = request.root {
        resolve_explicit_path(&path, &env_map, &cwd)?
    } else {
        cwd
    };
    fs::create_dir_all(&root).with_context(|| format!("failed to create {}", root.display()))?;
    let root = absolute_path(&root);
    ensure_workspace_config(&root)?;
    copy_workspace_templates(&root)?;
    EvalStore::new(root.clone()).ensure_layout()?;

    let mut default_workspace = false;
    if request.make_default {
        write_default_workspace(&home, &root, request.force)?;
        default_workspace = true;
    }

    Ok(InitStoreResult {
        schema_version: WORKSPACE_SCHEMA_VERSION,
        root,
        default_workspace,
    })
}

pub(crate) fn check_project(
    project: &EvalProject,
    task_set_filter: Option<&str>,
    task_filter: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<Vec<CaseResult>> {
    let cases = expand_matrix(project, task_set_filter, task_filter, agent_filter)?;
    for case in &cases {
        validate_case(case)?;
    }
    Ok(cases
        .into_iter()
        .map(|case| CaseResult {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            identity: CaseIdentity {
                case_id: case.case_id.clone(),
                task_set_id: case.task_set.id.clone(),
                task_id: case.task.id.clone(),
                task_family: case.task.kind.clone(),
            },
            candidate: CandidateIdentity {
                agent_id: case.agent.id.clone(),
                adapter: case.agent.kind,
                model: agent_model(&case.agent),
            },
            factors: CaseFactors::default(),
            case_id: case.case_id,
            task_set_id: case.task_set.id,
            task_id: case.task.id,
            task_family: case.task.kind,
            agent_id: case.agent.id,
            status: CaseStatus::Passed,
            failure_class: None,
            score: ScoreResult {
                schema_version: EVALUATOR_RESULT_SCHEMA_VERSION,
                passed: true,
                score: None,
                message: "validated".to_string(),
                details: Value::Null,
            },
            duration_ms: 0,
            metrics: CaseMetrics::default(),
            warnings: Vec::new(),
            artifacts: CaseArtifacts {
                result: PathBuf::new(),
                trajectory: PathBuf::new(),
                evaluator_stdout: PathBuf::new(),
                evaluator_stderr: PathBuf::new(),
            },
        })
        .collect())
}
