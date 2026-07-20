#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SettingsReadParams {
    #[serde(default)]
    pub cwd: Option<String>,
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
    pub cwd: String,
    #[serde(default)]
    pub project: Option<WorkbenchProjectView>,
    #[serde(default)]
    pub channels: ChannelListResult,
    #[ts(type = "Record<string, unknown>")]
    pub memory_resources: BTreeMap<String, Value>,
    #[ts(type = "Record<string, unknown>")]
    pub secrets: BTreeMap<String, Value>,
    #[serde(default)]
    pub controls: Option<WorkbenchControlsView>,
    #[serde(default)]
    pub web_search: Option<WebSearchSettingsView>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchSettingsView {
    pub execution: String,
    pub backend: String,
    pub external_access: String,
    pub context_size: String,
    pub return_token_budget: String,
    #[serde(default)]
    pub content_types: Vec<String>,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub blocked_domains: Vec<String>,
    pub background_storage_acknowledged: bool,
    #[ts(type = "Record<string, unknown>")]
    pub location: BTreeMap<String, Value>,
    #[ts(type = "Record<string, unknown>")]
    pub image: BTreeMap<String, Value>,
    #[ts(type = "Record<string, string>")]
    pub credentials: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchSettingsReadParams {
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchSettingsUpdateParams {
    pub scope: GatewayRequestScope,
    pub search: WebSearchSettingsView,
    #[serde(default)]
    #[ts(type = "Record<string, string>")]
    pub credential_values: BTreeMap<String, String>,
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
    pub model_status: WorkbenchModelStatus,
    #[serde(default)]
    pub model_error: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub permission_mode_options: Vec<String>,
    #[serde(default)]
    pub mode_options: Vec<String>,
    #[serde(default)]
    pub model_options: Vec<String>,
    #[serde(default)]
    pub model_details: Vec<ModelOptionView>,
    #[serde(default)]
    pub recent_models: Vec<String>,
    #[serde(default)]
    pub variant_options: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkbenchModelStatus {
    Resolved,
    #[default]
    Unconfigured,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ModelSettingsScope {
    #[default]
    Global,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettingsReadParams {
    #[serde(default)]
    pub scope: ModelSettingsScope,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderSaveParams {
    #[serde(default)]
    pub scope: ModelSettingsScope,
    pub provider_id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub api: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub no_auth: bool,
    #[serde(default)]
    pub model: Option<ModelProviderSaveModelParams>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderSaveModelParams {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub limit: ModelLimitView,
    #[serde(default)]
    pub advanced_format: Option<String>,
    #[serde(default)]
    pub advanced: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderCatalogParams {
    #[serde(default)]
    pub scope: ModelSettingsScope,
    pub provider_id: String,
    #[serde(default)]
    pub refresh: bool,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelStateReadParams {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelStateSetParams {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelStateResult {
    pub cwd: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub recent_models: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ModelAssignmentTarget {
    Default,
    Auxiliary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelAssignmentSetParams {
    #[serde(default)]
    pub scope: ModelSettingsScope,
    pub target: ModelAssignmentTarget,
    #[serde(default)]
    pub task: Option<String>,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettingsResult {
    pub scope: ModelSettingsScope,
    pub cwd: String,
    pub default_model: Option<String>,
    #[serde(default)]
    pub default_reasoning_effort: Option<String>,
    pub providers: Vec<ModelProviderView>,
    pub auxiliary: Vec<AuxiliaryModelAssignmentView>,
    pub model_options: Vec<ModelOptionView>,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub voice: Option<Value>,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub image_generation: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderView {
    pub id: String,
    pub name: String,
    pub built_in: bool,
    pub configured: bool,
    pub api: Option<String>,
    pub api_key_env: Option<String>,
    pub credential_status: ModelCredentialStatus,
    pub no_auth: bool,
    pub can_fetch_models: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ModelCredentialStatus {
    Present,
    Missing,
    NotRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelOptionView {
    pub provider: String,
    pub id: String,
    pub value: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub provider_name: Option<String>,
    #[serde(default)]
    pub free: bool,
    #[serde(default)]
    pub limit: ModelLimitView,
    #[serde(default)]
    pub reasoning_supported: Option<bool>,
    #[serde(default)]
    pub reasoning_efforts: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelLimitView {
    #[serde(default)]
    pub context: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AuxiliaryModelAssignmentView {
    pub task: String,
    pub label: String,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    pub effective_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderCatalogResult {
    pub provider_id: String,
    pub models: Vec<ModelOptionView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelAssignmentSetResult {
    pub ok: bool,
    pub target: ModelAssignmentTarget,
    #[serde(default)]
    pub task: Option<String>,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCreateParams {
    pub name: String,
    #[serde(default)]
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCreateResult {
    pub cwd: String,
    pub scope: GatewayRequestScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFolderEntry {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFolderListParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFolderListResult {
    pub root: String,
    pub current: String,
    #[serde(default)]
    pub parent: Option<String>,
    pub folders: Vec<WorkspaceFolderEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGitBranchesParams {
    pub scope: GatewayRequestScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGitCheckoutParams {
    pub scope: GatewayRequestScope,
    pub branch: String,
    #[serde(default)]
    pub create: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGitBranchesResult {
    #[serde(default)]
    pub current: Option<String>,
    pub branches: Vec<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceExternalFileAction {
    SystemDefault,
    Vscode,
    Reveal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceExternalFileCategory {
    Webpage,
    Image,
    Media,
    Pdf,
    Office,
    Text,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceExternalHostPlatform {
    Macos,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileExternalActionsParams {
    pub scope: GatewayRequestScope,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileExternalActionsResult {
    pub path: String,
    pub category: WorkspaceExternalFileCategory,
    pub text_like: bool,
    pub platform: WorkspaceExternalHostPlatform,
    pub preferred_action: WorkspaceExternalFileAction,
    pub available_actions: Vec<WorkspaceExternalFileAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileOpenExternalParams {
    pub scope: GatewayRequestScope,
    pub path: String,
    pub action: WorkspaceExternalFileAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileOpenExternalResult {
    pub path: String,
    pub action: WorkspaceExternalFileAction,
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
pub struct WorkspaceFilePreviewOpenParams {
    pub scope: GatewayRequestScope,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilePreviewOpenResult {
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
    pub media_type: String,
    pub resource_id: String,
    pub resource_path: String,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilePreviewReleaseParams {
    pub resource_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilePreviewReleaseResult {
    pub released: bool,
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
    #[serde(default)]
    pub invalidations: Vec<WorkspaceChangeInvalidationView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChangeInvalidationView {
    pub source: String,
    pub message: String,
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
    pub basis: String,
    #[serde(default)]
    pub applies_to_session_seq: Option<i64>,
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
    #[serde(default)]
    pub effective_total_tokens: Option<u64>,
    pub reported_total_tokens: u64,
    pub total_status: String,
    pub accounted_provider_call_count: u64,
    pub unaccounted_provider_call_count: u64,
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
    pub effective_total_tokens: u64,
    pub reported_total_tokens: u64,
    pub total_status: String,
    pub accounted_provider_call_count: u64,
    pub unaccounted_provider_call_count: u64,
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
    pub effective_total_tokens: u64,
    pub reported_total_tokens: u64,
    pub total_status: String,
    pub accounted_provider_call_count: u64,
    pub unaccounted_provider_call_count: u64,
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
