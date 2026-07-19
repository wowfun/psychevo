use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::future::BoxFuture;
use psychevo_agent_core::{
    ControlHandle, ControlReceivers, Message, PendingInputId, TerminalReason, ToolBinding,
};
use psychevo_ai::{AbortSignal, Outcome};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::error::Result;
use crate::extensions::SelectedCapabilityRoot;
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
    pub cwd: PathBuf,
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
    pub sandbox_override: Option<RunSandboxOverride>,
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
    pub selected_capability_roots: Vec<SelectedCapabilityRoot>,
    pub skill_inputs: Vec<String>,
    pub mcp_servers: Vec<McpServerInput>,
    pub workspace_mutations: Option<WorkspaceMutationSink>,
    pub runtime_tools: Vec<RuntimeTool>,
}

#[derive(Clone)]
pub struct RuntimeTool {
    inner: Arc<dyn ToolBinding>,
    source_id: Option<String>,
    source_kind: Option<String>,
}

impl RuntimeTool {
    pub fn new(inner: Arc<dyn ToolBinding>) -> Self {
        Self {
            inner,
            source_id: None,
            source_kind: None,
        }
    }

    pub fn with_source(
        inner: Arc<dyn ToolBinding>,
        source_id: impl Into<String>,
        source_kind: impl Into<String>,
    ) -> Self {
        Self {
            inner,
            source_id: Some(source_id.into()),
            source_kind: Some(source_kind.into()),
        }
    }

    pub(crate) fn binding(&self) -> Arc<dyn ToolBinding> {
        Arc::clone(&self.inner)
    }

    pub fn name(&self) -> &str {
        self.inner.name()
    }

    pub(crate) fn source_id(&self) -> Option<&str> {
        self.source_id.as_deref()
    }

    pub(crate) fn source_kind(&self) -> Option<&str> {
        self.source_kind.as_deref()
    }
}

impl fmt::Debug for RuntimeTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeTool")
            .field("name", &self.inner.name())
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct ExternalAgentDelegateRequest {
    pub run_id: String,
    pub parent_session_id: String,
    pub child_session_id: String,
    pub agent_name: String,
    pub agent_description: String,
    /// Public execution identity selected by the Team member. ACP delegates use
    /// the generated or configured Profile id, never the raw backend id.
    pub runtime_ref: String,
    /// ACP implementation identity when the selected Runtime Profile is ACP.
    /// Native Runtime Profiles deliberately leave this unset.
    pub backend_ref: Option<String>,
    /// Agent Definition instructions for adapters that expose a native
    /// developer/system-instruction field.
    pub instructions: Option<String>,
    pub prompt: String,
    pub task_name: String,
    pub model: Option<String>,
    pub runtime_options: BTreeMap<String, String>,
    /// Runtime Profile revision captured by Team configuration/activation.
    /// The delegate must reject execution if the effective Profile changed.
    pub expected_runtime_profile_revision: Option<u64>,
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
    pub cwd: PathBuf,
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
    pub selected_capability_roots: Vec<SelectedCapabilityRoot>,
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
pub const EDITABLE_INPUT_METADATA_KEY: &str = "editable_input";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum StoredEditableInputPart {
    Text { text: String },
    Image { image_block_index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEditableInputEnvelope {
    pub version: u32,
    pub parts: Vec<StoredEditableInputPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptDisplayMetadata {
    pub content_text: String,
    pub attachments: Vec<PromptAttachmentDisplay>,
    #[serde(skip)]
    pub editable_input: Option<StoredEditableInputEnvelope>,
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
    pub source_id: Option<String>,
    pub source_kind: Option<String>,
    pub transport: McpTransportInput,
    pub policy: McpServerPolicy,
}

/// A fully resolved MCP declaration ready to cross an Agent-adapter boundary.
///
/// The bearer token deliberately has no serde implementation and its `Debug`
/// representation is redacted. It may live in process memory long enough to
/// build an ACP session request, but must never enter persistence or product
/// projections.
#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedMcpServerInput {
    pub server: McpServerInput,
    pub bearer_token: Option<String>,
}

impl fmt::Debug for ResolvedMcpServerInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let transport = match &self.server.transport {
            McpTransportInput::Stdio { .. } => "stdio",
            McpTransportInput::StreamableHttp { .. } => "streamable_http",
            McpTransportInput::Unsupported { .. } => "unsupported",
        };
        formatter
            .debug_struct("ResolvedMcpServerInput")
            .field("name", &self.server.name)
            .field("source_id", &self.server.source_id)
            .field("source_kind", &self.server.source_kind)
            .field("transport", &transport)
            .field("policy", &self.server.policy)
            .field(
                "bearer_token",
                &self.bearer_token.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerPolicy {
    pub enabled: bool,
    pub required: bool,
    pub enabled_tools: Option<Vec<String>>,
    pub disabled_tools: Vec<String>,
    pub supports_parallel_tool_calls: bool,
    pub startup_timeout_secs: Option<u64>,
    pub tool_timeout_secs: Option<u64>,
}

impl Default for McpServerPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            required: false,
            enabled_tools: None,
            disabled_tools: Vec::new(),
            supports_parallel_tool_calls: false,
            startup_timeout_secs: None,
            tool_timeout_secs: None,
        }
    }
}

impl McpServerInput {
    pub fn new(name: impl Into<String>, transport: McpTransportInput) -> Self {
        Self {
            name: name.into(),
            source_id: None,
            source_kind: None,
            transport,
            policy: McpServerPolicy::default(),
        }
    }

    pub fn with_source(
        name: impl Into<String>,
        transport: McpTransportInput,
        source_id: impl Into<String>,
        source_kind: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            source_id: Some(source_id.into()),
            source_kind: Some(source_kind.into()),
            transport,
            policy: McpServerPolicy::default(),
        }
    }

    pub fn with_policy(mut self, policy: McpServerPolicy) -> Self {
        self.policy = policy;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpTransportInput {
    Stdio {
        command: PathBuf,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: Option<PathBuf>,
    },
    StreamableHttp {
        url: String,
        headers: BTreeMap<String, String>,
        bearer_token_env_var: Option<String>,
        scopes: Vec<String>,
        oauth_resource: Option<String>,
        oauth_client_id: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunSandboxMode {
    WorkspaceWrite,
    ReadOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSandboxOverride {
    pub enabled: bool,
    pub mode: RunSandboxMode,
    pub writable_roots: Vec<String>,
    pub include_tmp: bool,
    pub include_common_caches: bool,
}

impl RunSandboxOverride {
    pub fn workspace_write() -> Self {
        Self {
            enabled: true,
            mode: RunSandboxMode::WorkspaceWrite,
            writable_roots: Vec::new(),
            include_tmp: true,
            include_common_caches: true,
        }
    }

    pub fn read_only() -> Self {
        Self {
            enabled: true,
            mode: RunSandboxMode::ReadOnly,
            writable_roots: Vec::new(),
            include_tmp: false,
            include_common_caches: false,
        }
    }
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
    pub web_search_queries: BTreeMap<String, PermissionAccess>,
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

include!("run_options/results.rs");
