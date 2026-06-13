use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use futures::future::BoxFuture;
use psychevo_agent_core::{
    ControlHandle, ControlReceivers, Message, PendingInputId, TerminalReason,
};
use psychevo_ai::Outcome;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::skills::SelectedSkill;
use crate::state_runtime::StateRuntime;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SmokeControl {
    #[default]
    None,
    StopAfterTurn,
    AbortOnAgentStart,
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub state: StateRuntime,
    pub workdir: PathBuf,
    pub snapshot_root: Option<PathBuf>,
    pub session: Option<String>,
    pub continue_latest: bool,
    pub prompt: String,
    pub image_inputs: Vec<ImageInput>,
    pub extract_prompt_image_sources: bool,
    pub prompt_display: Option<PromptDisplayMetadata>,
    pub max_context_messages: Option<usize>,
    pub config_path: Option<PathBuf>,
    pub project_context_override: Option<ProjectContextInstructionMode>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub include_reasoning: bool,
    pub mode: RunMode,
    pub permission_mode: Option<PermissionMode>,
    pub approval_mode: Option<ApprovalMode>,
    pub approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub clarify_enabled: bool,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub agent: Option<String>,
    pub no_agents: bool,
    pub no_skills: bool,
    pub skill_inputs: Vec<String>,
    pub mcp_servers: Vec<McpServerInput>,
}

#[derive(Debug, Clone)]
pub struct AgentSpawnOptions {
    pub state: StateRuntime,
    pub workdir: PathBuf,
    pub parent_session: Option<String>,
    pub prompt: String,
    pub agent: String,
    pub config_path: Option<PathBuf>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub mode: RunMode,
    pub permission_mode: Option<PermissionMode>,
    pub approval_mode: Option<ApprovalMode>,
    pub approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub selected_parent_agent: Option<String>,
    pub no_skills: bool,
    pub skill_inputs: Vec<String>,
    pub mcp_servers: Vec<McpServerInput>,
}

#[derive(Debug, Clone)]
pub struct AgentSpawnResult {
    pub parent_session_id: String,
    pub agent: crate::agents::AgentRunRecord,
}

pub const TUI_DISPLAY_METADATA_KEY: &str = "tui_display";
pub const USER_SHELL_METADATA_KEY: &str = "user_shell";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptDisplayMetadata {
    pub content_text: String,
    pub attachments: Vec<PromptAttachmentDisplay>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptAttachmentDisplay {
    pub kind: String,
    pub placeholder: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageInput {
    LocalPath(PathBuf),
    ImageUrl(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerInput {
    pub name: String,
    pub transport: McpTransportInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpTransportInput {
    Stdio {
        command: PathBuf,
        args: Vec<String>,
        env: BTreeMap<String, String>,
    },
    StreamableHttp {
        url: String,
        headers: BTreeMap<String, String>,
    },
    Unsupported {
        kind: String,
    },
}

impl ImageInput {
    pub fn display_source(&self) -> String {
        match self {
            Self::LocalPath(path) => path.display().to_string(),
            Self::ImageUrl(url) => url.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProviderInput {
    pub home: PathBuf,
    pub provider_id: String,
    pub label: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub no_auth: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedCustomProviderInput {
    pub config_dir: PathBuf,
    pub provider_id: String,
    pub label: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    pub require_api_key: bool,
    pub no_auth: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProviderResult {
    pub provider_id: String,
    pub label: String,
    pub base_url: String,
    pub api_key_env: String,
    pub wrote_api_key: bool,
    pub reused_existing_api_key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigScope {
    Global,
    Local,
    Effective,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunMode {
    Plan,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectContextInstructionMode {
    #[default]
    GitRoot,
    Cwd,
    Off,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    #[default]
    Default,
    AcceptEdits,
    DontAsk,
    BypassPermissions,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "acceptEdits",
            Self::DontAsk => "dontAsk",
            Self::BypassPermissions => "bypassPermissions",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "acceptEdits" | "accept_edits" | "accept-edits" => Some(Self::AcceptEdits),
            "dontAsk" | "dont_ask" | "dont-ask" => Some(Self::DontAsk),
            "bypassPermissions" | "bypass_permissions" | "bypass-permissions" => {
                Some(Self::BypassPermissions)
            }
            _ => None,
        }
    }

    pub fn bypasses_prompt_asks(self) -> bool {
        matches!(self, Self::BypassPermissions)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    #[default]
    Manual,
    Smart,
}

impl ApprovalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Smart => "smart",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "smart" => Some(Self::Smart),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ApprovalPolicy {
    #[default]
    OnRequest,
    Untrusted,
    Never,
    Granular,
}

impl ApprovalPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OnRequest => "on-request",
            Self::Untrusted => "untrusted",
            Self::Never => "never",
            Self::Granular => "granular",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "on-request" | "on_request" => Some(Self::OnRequest),
            "untrusted" => Some(Self::Untrusted),
            "never" => Some(Self::Never),
            "granular" => Some(Self::Granular),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GranularApprovalConfig {
    pub filesystem: bool,
    pub network: bool,
    pub exec: bool,
    pub mcp: bool,
    pub skill: bool,
    pub request_permissions: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalsReviewer {
    #[default]
    User,
    Smart,
}

impl ApprovalsReviewer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Smart => "smart",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "smart" | "auto_review" | "guardian_subagent" => Some(Self::Smart),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoReviewConfig {
    pub model: Option<String>,
    pub timeout_secs: u64,
    pub policy: Option<String>,
}

impl Default for AutoReviewConfig {
    fn default() -> Self {
        Self {
            model: None,
            timeout_secs: 90,
            policy: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionAccess {
    Deny,
    Read,
    Write,
    Allow,
    Prompt,
}

impl PermissionAccess {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "deny" => Some(Self::Deny),
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "allow" => Some(Self::Allow),
            "prompt" | "ask" => Some(Self::Prompt),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deny => "deny",
            Self::Read => "read",
            Self::Write => "write",
            Self::Allow => "allow",
            Self::Prompt => "prompt",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionProfileConfig {
    pub extends: Option<String>,
    pub filesystem: BTreeMap<String, PermissionAccess>,
    pub network_domains: BTreeMap<String, PermissionAccess>,
    pub skill_tools: BTreeMap<String, PermissionAccess>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecPolicyDecision {
    Allow,
    Prompt,
    Deny,
}

impl ExecPolicyDecision {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "allow" => Some(Self::Allow),
            "prompt" | "ask" => Some(Self::Prompt),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Prompt => "prompt",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecPolicyPatternToken {
    Single(String),
    Alternatives(Vec<String>),
}

impl ExecPolicyPatternToken {
    pub fn matches(&self, value: &str) -> bool {
        match self {
            Self::Single(expected) => expected == value,
            Self::Alternatives(values) => values.iter().any(|expected| expected == value),
        }
    }

    pub fn alternatives(&self) -> &[String] {
        match self {
            Self::Single(value) => std::slice::from_ref(value),
            Self::Alternatives(values) => values,
        }
    }
}

impl From<String> for ExecPolicyPatternToken {
    fn from(value: String) -> Self {
        Self::Single(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecPolicyExample {
    pub raw: String,
    pub tokens: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecPolicyRule {
    pub prefix: Vec<ExecPolicyPatternToken>,
    pub decision: ExecPolicyDecision,
    pub justification: Option<String>,
    pub match_examples: Vec<ExecPolicyExample>,
    pub not_match_examples: Vec<ExecPolicyExample>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecPolicyHostExecutable {
    pub name: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecPolicyConfig {
    pub rules: Vec<ExecPolicyRule>,
    pub host_executables: Vec<ExecPolicyHostExecutable>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionConfig {
    pub approval_policy: ApprovalPolicy,
    pub approvals_reviewer: ApprovalsReviewer,
    pub granular: Option<GranularApprovalConfig>,
    pub auto_review: AutoReviewConfig,
    pub default_permissions: String,
    pub profiles: BTreeMap<String, PermissionProfileConfig>,
    pub exec_policy: ExecPolicyConfig,
    pub allow_login_shell: bool,
}

impl Default for PermissionConfig {
    fn default() -> Self {
        Self {
            approval_policy: ApprovalPolicy::OnRequest,
            approvals_reviewer: ApprovalsReviewer::User,
            granular: None,
            auto_review: AutoReviewConfig::default(),
            default_permissions: ":workspace".to_string(),
            profiles: BTreeMap::new(),
            exec_policy: ExecPolicyConfig::default(),
            allow_login_shell: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionApprovalRequest {
    pub tool_call_id: String,
    pub tool_name: String,
    pub summary: String,
    pub reason: String,
    pub matched_rule: Option<String>,
    pub suggested_rule: Option<String>,
    pub allow_always: bool,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionApprovalOutcome {
    AllowOnce,
    AllowSession,
    AllowAlways,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionApprovalDecision {
    pub outcome: PermissionApprovalOutcome,
}

impl PermissionApprovalDecision {
    pub fn allow_once() -> Self {
        Self {
            outcome: PermissionApprovalOutcome::AllowOnce,
        }
    }

    pub fn allow_session() -> Self {
        Self {
            outcome: PermissionApprovalOutcome::AllowSession,
        }
    }

    pub fn allow_always() -> Self {
        Self {
            outcome: PermissionApprovalOutcome::AllowAlways,
        }
    }

    pub fn deny() -> Self {
        Self {
            outcome: PermissionApprovalOutcome::Deny,
        }
    }
}

pub trait ApprovalHandler: Send + Sync + fmt::Debug {
    fn timeout_secs(&self) -> u64 {
        300
    }

    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> BoxFuture<'static, PermissionApprovalDecision>;
}

impl RunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Default => "default",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "plan" => Some(Self::Plan),
            "default" => Some(Self::Default),
            _ => None,
        }
    }
}

impl ProjectContextInstructionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitRoot => "git-root",
            Self::Cwd => "cwd",
            Self::Off => "off",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "git-root" | "git_root" => Some(Self::GitRoot),
            "cwd" => Some(Self::Cwd),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub session_id: String,
    pub outcome: Outcome,
    pub terminal_reason: Option<TerminalReason>,
    pub final_answer: String,
    pub db_path: PathBuf,
    pub workdir: PathBuf,
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
    pub events: Vec<Value>,
    pub warnings: Vec<RunWarning>,
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
    pub workdir: PathBuf,
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
    pub workdir: PathBuf,
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
    pub unknown_pricing_count: u64,
    pub cache_read_percent: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct UserShellResult {
    pub command: String,
    pub workdir: PathBuf,
    pub session_id: Option<String>,
    pub context_text: Option<String>,
    pub outcome: Outcome,
    pub tool_failures: usize,
    pub result: Value,
}

#[derive(Debug, Clone)]
pub struct SessionUndoOptions {
    pub state: StateRuntime,
    pub workdir: PathBuf,
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
    pub workdir: String,
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

impl fmt::Debug for ModelCatalogProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModelCatalogProvider")
            .field("provider", &self.provider)
            .field("display_label", &self.display_label)
            .field("base_url", &self.base_url)
            .field("api_key_env", &self.api_key_env)
            .field("missing_credentials", &self.missing_credentials)
            .field("unavailable_reason", &self.unavailable_reason)
            .field("no_auth", &self.no_auth)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCatalogEntry {
    pub id: String,
    pub context_limit: Option<u64>,
    pub metadata: ModelMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMetadataCacheTarget {
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub limits: ModelLimits,
    pub cost: Option<ModelCost>,
    pub capabilities: ModelCapabilities,
    pub source: Option<String>,
    pub raw: Option<Value>,
}

impl ModelMetadata {
    pub fn context_limit(&self) -> Option<u64> {
        self.limits.context
    }

    pub fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        let limits = self.limits.public_json();
        if !limits.as_object().is_none_or(|object| object.is_empty()) {
            object.insert("limit".to_string(), limits);
        }
        if let Some(cost) = &self.cost {
            object.insert("cost".to_string(), cost.public_json());
        }
        let capabilities = self.capabilities.public_json();
        if !capabilities
            .as_object()
            .is_none_or(|object| object.is_empty())
        {
            object.insert("capabilities".to_string(), capabilities);
        }
        if let Some(source) = &self.source {
            object.insert("source".to_string(), Value::String(source.clone()));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLimits {
    pub context: Option<u64>,
    pub input: Option<u64>,
    pub output: Option<u64>,
}

impl ModelLimits {
    pub(crate) fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        if let Some(value) = self.context {
            object.insert("context".to_string(), Value::from(value));
        }
        if let Some(value) = self.input {
            object.insert("input".to_string(), Value::from(value));
        }
        if let Some(value) = self.output {
            object.insert("output".to_string(), Value::from(value));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCost {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    pub context_over_200k: Option<ModelCostTier>,
    pub source: Option<String>,
}

impl ModelCost {
    pub(crate) fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        if let Some(value) = self.input {
            object.insert("input".to_string(), Value::from(value));
        }
        if let Some(value) = self.output {
            object.insert("output".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_read {
            object.insert("cache_read".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_write {
            object.insert("cache_write".to_string(), Value::from(value));
        }
        if let Some(tier) = &self.context_over_200k {
            object.insert("context_over_200k".to_string(), tier.public_json());
        }
        if let Some(source) = &self.source {
            object.insert("source".to_string(), Value::String(source.clone()));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCostTier {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

impl ModelCostTier {
    pub(crate) fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        if let Some(value) = self.input {
            object.insert("input".to_string(), Value::from(value));
        }
        if let Some(value) = self.output {
            object.insert("output".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_read {
            object.insert("cache_read".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_write {
            object.insert("cache_write".to_string(), Value::from(value));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub reasoning: Option<bool>,
    pub tool_call: Option<bool>,
    pub developer_role: Option<bool>,
    pub temperature: Option<bool>,
    pub attachment: Option<bool>,
    pub structured_output: Option<bool>,
    pub interleaved: Option<Value>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
}

impl ModelCapabilities {
    pub(crate) fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        if let Some(value) = self.reasoning {
            object.insert("reasoning".to_string(), Value::Bool(value));
        }
        if let Some(value) = self.tool_call {
            object.insert("tool_call".to_string(), Value::Bool(value));
        }
        if let Some(value) = self.developer_role {
            object.insert("developer_role".to_string(), Value::Bool(value));
        }
        if let Some(value) = self.temperature {
            object.insert("temperature".to_string(), Value::Bool(value));
        }
        if let Some(value) = self.attachment {
            object.insert("attachment".to_string(), Value::Bool(value));
        }
        if let Some(value) = self.structured_output {
            object.insert("structured_output".to_string(), Value::Bool(value));
        }
        if let Some(value) = &self.interleaved {
            object.insert("interleaved".to_string(), value.clone());
        }
        if !self.input_modalities.is_empty() || !self.output_modalities.is_empty() {
            let mut modalities = serde_json::Map::new();
            if !self.input_modalities.is_empty() {
                modalities.insert(
                    "input".to_string(),
                    Value::Array(
                        self.input_modalities
                            .iter()
                            .map(|value| Value::String(value.clone()))
                            .collect(),
                    ),
                );
            }
            if !self.output_modalities.is_empty() {
                modalities.insert(
                    "output".to_string(),
                    Value::Array(
                        self.output_modalities
                            .iter()
                            .map(|value| Value::String(value.clone()))
                            .collect(),
                    ),
                );
            }
            object.insert("modalities".to_string(), Value::Object(modalities));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageAccounting {
    pub context_input_tokens: Option<u64>,
    pub billable_input_tokens: Option<u64>,
    pub billable_output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reported_total_tokens: Option<u64>,
    pub estimated_cost_nanodollars: Option<i64>,
    pub pricing_source: Option<String>,
    pub pricing_tier: Option<String>,
}

impl MessageAccounting {
    pub fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        if let Some(value) = self.context_input_tokens {
            object.insert("context_input_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.billable_input_tokens {
            object.insert("billable_input_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.billable_output_tokens {
            object.insert("billable_output_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.reasoning_tokens {
            object.insert("reasoning_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_read_tokens {
            object.insert("cache_read_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_write_tokens {
            object.insert("cache_write_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.reported_total_tokens {
            object.insert("reported_total_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.estimated_cost_nanodollars {
            object.insert("estimated_cost_nanodollars".to_string(), Value::from(value));
        }
        if let Some(value) = &self.pricing_source {
            object.insert("pricing_source".to_string(), Value::String(value.clone()));
        }
        if let Some(value) = &self.pricing_tier {
            object.insert("pricing_tier".to_string(), Value::String(value.clone()));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SanitizedMessageSummary {
    pub message: Message,
    pub usage: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionExportMessageSummary {
    pub session_seq: i64,
    pub message: Message,
    pub usage: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TuiMessageSummary {
    pub session_seq: i64,
    pub message: Message,
    pub usage: Option<Value>,
    pub metadata: Option<Value>,
    pub accounting: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunStreamEvent {
    Event(Value),
    ReasoningDelta {
        text: String,
    },
    ReasoningEnd,
    ClarifyRequest(ClarifyRequestEvent),
    ClarifyResolved(ClarifyResolvedEvent),
    Scoped {
        session_id: String,
        event: Box<RunStreamEvent>,
    },
}

impl RunStreamEvent {
    pub fn scoped(session_id: impl Into<String>, event: RunStreamEvent) -> Self {
        Self::Scoped {
            session_id: session_id.into(),
            event: Box::new(event),
        }
    }
}

pub type RunStreamSink = Arc<dyn Fn(RunStreamEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyQuestion {
    pub question: String,
    pub options: Vec<ClarifyQuestionOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyRequestEvent {
    pub call_id: String,
    pub questions: Vec<ClarifyQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyAnswer {
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyResponse {
    pub answers: Vec<ClarifyAnswer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClarifyResult {
    Answered(ClarifyResponse),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClarifyResolvedReason {
    Answered,
    Cancelled,
    TimedOut,
    TurnFinished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyResolvedEvent {
    pub call_id: String,
    pub reason: ClarifyResolvedReason,
}

#[derive(Debug, Default)]
pub(crate) struct ClarifyControl {
    pub(crate) pending: Mutex<HashMap<String, oneshot::Sender<ClarifyResult>>>,
}

impl ClarifyControl {
    pub(crate) fn register(&self, call_id: String) -> oneshot::Receiver<ClarifyResult> {
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().expect("clarify pending map poisoned");
        pending.insert(call_id, tx);
        rx
    }

    pub(crate) fn submit(&self, call_id: &str, result: ClarifyResult) -> bool {
        let sender = self
            .pending
            .lock()
            .expect("clarify pending map poisoned")
            .remove(call_id);
        sender.is_some_and(|sender| sender.send(result).is_ok())
    }

    pub(crate) fn remove(&self, call_id: &str) -> bool {
        self.pending
            .lock()
            .expect("clarify pending map poisoned")
            .remove(call_id)
            .is_some()
    }
}

#[derive(Clone)]
pub struct RunControlHandle {
    pub(crate) inner: ControlHandle,
    pub(crate) clarify: Arc<ClarifyControl>,
}

impl RunControlHandle {
    pub fn stop(&self) {
        self.inner.stop();
    }

    pub fn abort(&self) {
        self.inner.abort();
    }

    pub fn inject_user_message(&self, message: Message) -> bool {
        self.inner.inject_user_message(message)
    }

    pub fn steer_user_message(&self, message: Message) -> Option<PendingInputId> {
        self.inner.steer_user_message(message)
    }

    pub fn update_pending_user_message(&self, id: PendingInputId, message: Message) -> bool {
        self.inner.update_pending_user_message(id, message)
    }

    pub fn cancel_pending_user_message(&self, id: PendingInputId) -> bool {
        self.inner.cancel_pending_user_message(id)
    }

    pub fn submit_clarify_result(&self, call_id: &str, result: ClarifyResult) -> bool {
        self.clarify.submit(call_id, result)
    }
}

impl fmt::Debug for RunControlHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunControlHandle")
            .finish_non_exhaustive()
    }
}

pub struct RunControl {
    pub(crate) handle: RunControlHandle,
    pub(crate) receivers: ControlReceivers,
}

pub fn run_control() -> (RunControlHandle, RunControl) {
    let (inner, receivers) = ControlHandle::new();
    let clarify = Arc::new(ClarifyControl::default());
    let handle = RunControlHandle { inner, clarify };
    (handle.clone(), RunControl { handle, receivers })
}
