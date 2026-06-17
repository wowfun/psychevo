use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use futures::future::BoxFuture;
use psychevo_agent_core::{
    ControlHandle, ControlReceivers, Message, PendingInputId, TerminalReason,
};
use psychevo_ai::{AbortSignal, Outcome};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::error::Result;
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
    pub runtime_ref: Option<String>,
    pub runtime_session_id: Option<String>,
    pub runtime_options: BTreeMap<String, String>,
    pub include_reasoning: bool,
    pub mode: RunMode,
    pub permission_mode: Option<PermissionMode>,
    pub approval_mode: Option<ApprovalMode>,
    pub approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub clarify_enabled: bool,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub agent: Option<String>,
    pub external_agent_delegate: Option<Arc<dyn ExternalAgentDelegate>>,
    pub no_agents: bool,
    pub no_skills: bool,
    pub skill_inputs: Vec<String>,
    pub mcp_servers: Vec<McpServerInput>,
}

#[derive(Debug, Clone)]
pub struct ExternalAgentDelegateRequest {
    pub run_id: String,
    pub parent_session_id: String,
    pub child_session_id: String,
    pub agent_name: String,
    pub agent_description: String,
    pub backend_ref: String,
    pub prompt: String,
    pub task_name: String,
    pub model: Option<String>,
    pub runtime_options: BTreeMap<String, String>,
    pub abort: AbortSignal,
}

#[derive(Debug, Clone)]
pub struct ExternalAgentDelegateResult {
    pub child_session_id: String,
    pub final_answer: String,
    pub outcome: Outcome,
}

pub trait ExternalAgentDelegate: Send + Sync + fmt::Debug {
    fn run(
        &self,
        request: ExternalAgentDelegateRequest,
    ) -> BoxFuture<'static, Result<ExternalAgentDelegateResult>>;
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
