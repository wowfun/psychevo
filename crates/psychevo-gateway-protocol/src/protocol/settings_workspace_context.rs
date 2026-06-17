#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SettingsReadParams {
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdateParams {
    pub scope: GatewayRequestScope,
    pub thread_id: String,
    #[serde(default)]
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SettingsReadResult {
    pub workdir: String,
    #[serde(default)]
    pub project: Option<WorkbenchProjectView>,
    #[ts(type = "Record<string, unknown>")]
    pub memory_resources: BTreeMap<String, Value>,
    #[ts(type = "Record<string, unknown>")]
    pub secrets: BTreeMap<String, Value>,
    #[serde(default)]
    pub controls: Option<WorkbenchControlsView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchProjectView {
    pub path: String,
    pub display_path: String,
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchControlsView {
    pub permission_mode: String,
    pub mode: String,
    #[serde(default)]
    pub runtime_ref: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub permission_mode_options: Vec<String>,
    #[serde(default)]
    pub mode_options: Vec<String>,
    #[serde(default)]
    pub model_options: Vec<String>,
    #[serde(default)]
    pub variant_options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCreateParams {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCreateResult {
    pub workdir: String,
    pub scope: GatewayRequestScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceFileKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileEntry {
    pub path: String,
    pub name: String,
    pub kind: WorkspaceFileKind,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilesParams {
    pub scope: GatewayRequestScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilesResult {
    pub root: String,
    pub entries: Vec<WorkspaceFileEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileReadParams {
    pub scope: GatewayRequestScope,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileReadResult {
    pub path: String,
    #[serde(default)]
    pub content: Option<String>,
    pub truncated: bool,
    pub binary: bool,
    #[serde(default)]
    pub editable: bool,
    #[serde(default)]
    pub editable_reason: Option<String>,
    #[serde(default)]
    pub size_bytes: usize,
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub line_ending: Option<String>,
    #[serde(default)]
    pub unreadable: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileWriteParams {
    pub scope: GatewayRequestScope,
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub expected_revision: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileWriteResult {
    pub path: String,
    pub revision: String,
    pub size_bytes: usize,
    #[serde(default)]
    pub line_ending: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceDiffFileStatusView {
    Modified,
    Added,
    Deleted,
    Untracked,
    Binary,
    Unreadable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiffFileView {
    pub path: String,
    pub status: WorkspaceDiffFileStatusView,
    pub binary: bool,
    pub unreadable: bool,
    #[serde(default)]
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiffTruncationView {
    pub truncated: bool,
    pub max_bytes: usize,
    pub max_lines: usize,
    pub omitted_bytes: usize,
    pub omitted_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiffParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiffResult {
    pub is_git_repo: bool,
    pub files: Vec<WorkspaceDiffFileView>,
    pub unified_diff: String,
    pub truncation: WorkspaceDiffTruncationView,
    #[serde(default)]
    pub selected_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceChangeReviewStatusView {
    Pending,
    Accepted,
    Rejected,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChangeFileView {
    pub path: String,
    pub status: WorkspaceDiffFileStatusView,
    pub binary: bool,
    pub unreadable: bool,
    pub review_status: WorkspaceChangeReviewStatusView,
    pub can_reject: bool,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChangeGroupView {
    pub turn_id: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub created_at_ms: i64,
    pub completed_at_ms: i64,
    pub files: Vec<WorkspaceChangeFileView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChangesParams {
    pub scope: GatewayRequestScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChangesResult {
    pub groups: Vec<WorkspaceChangeGroupView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChangeFileParams {
    pub scope: GatewayRequestScope,
    pub turn_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChangeMutationResult {
    pub accepted: bool,
    pub changes: WorkspaceChangesResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContextReadParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsageCategoryView {
    pub id: String,
    pub label: String,
    pub tokens: u64,
    pub estimated: bool,
    pub status: String,
    #[serde(default)]
    pub percent: Option<f64>,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContextReadResult {
    pub available: bool,
    pub label: String,
    pub status: String,
    pub used_tokens: u64,
    #[serde(default)]
    pub context_limit: Option<u64>,
    #[serde(default)]
    pub percent: Option<f64>,
    pub categories: Vec<ContextUsageCategoryView>,
    #[serde(default)]
    pub advice: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ObservabilityReadParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsageSummaryView {
    pub available: bool,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
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
    #[serde(default)]
    pub cache_read_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ObservabilityReadResult {
    pub context: ContextReadResult,
    pub usage: SessionUsageSummaryView,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct UsageReadParams {
    #[serde(default)]
    pub activity_days: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindowSummaryView {
    pub id: String,
    pub label: String,
    #[serde(default)]
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
    #[serde(default)]
    pub cache_read_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct UsageActivityDayView {
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct UsageActivityView {
    pub start_date: String,
    pub end_date: String,
    pub days: Vec<UsageActivityDayView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct UsageReadResult {
    pub generated_at_ms: i64,
    pub windows: Vec<UsageWindowSummaryView>,
    pub activity: UsageActivityView,
}
