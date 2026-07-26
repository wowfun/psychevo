use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::PermissionApprovalDecision;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookSourceDescriptor {
    pub source_id: String,
    pub source_kind: String,
    pub display_name: Option<String>,
    pub path: Option<PathBuf>,
    pub hooks: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<HookWorkerAdapter>,
}

impl HookSourceDescriptor {
    pub fn new(
        source_id: impl Into<String>,
        source_kind: impl Into<String>,
        display_name: Option<String>,
        path: Option<PathBuf>,
        hooks: Value,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            source_kind: source_kind.into(),
            display_name,
            path,
            hooks,
            worker: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookWorkerAdapter {
    pub plugin_name: String,
    pub plugin_version: String,
    pub plugin_source: String,
    pub plugin_root: PathBuf,
    pub plugin_data: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest_resources: Vec<String>,
    pub psychevo_extensions: Vec<String>,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEventName {
    SessionStart,
    SessionEnd,
    SubagentStart,
    SubagentStop,
    UserPromptSubmit,
    PreToolUse,
    PermissionRequest,
    PostToolUse,
    PostLLMCall,
    PreCompact,
    PostCompact,
    Notification,
    Stop,
}

impl HookEventName {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "SessionStart" | "session_start" | "sessionstart" => Some(Self::SessionStart),
            "SessionEnd" | "session_end" | "sessionend" => Some(Self::SessionEnd),
            "SubagentStart" | "subagent_start" | "subagentstart" => Some(Self::SubagentStart),
            "SubagentStop" | "subagent_stop" | "subagentstop" => Some(Self::SubagentStop),
            "UserPromptSubmit" | "user_prompt_submit" | "userpromptsubmit" => {
                Some(Self::UserPromptSubmit)
            }
            "PreToolUse" | "pre_tool_use" | "pretooluse" => Some(Self::PreToolUse),
            "PermissionRequest" | "permission_request" | "permissionrequest" => {
                Some(Self::PermissionRequest)
            }
            "PostToolUse" | "post_tool_use" | "posttooluse" => Some(Self::PostToolUse),
            "PostLLMCall" | "post_llm_call" | "postllmcall" => Some(Self::PostLLMCall),
            "PreCompact" | "pre_compact" | "precompact" => Some(Self::PreCompact),
            "PostCompact" | "post_compact" | "postcompact" => Some(Self::PostCompact),
            "Notification" | "notification" => Some(Self::Notification),
            "Stop" | "stop" => Some(Self::Stop),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::SubagentStart => "SubagentStart",
            Self::SubagentStop => "SubagentStop",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::PreToolUse => "PreToolUse",
            Self::PermissionRequest => "PermissionRequest",
            Self::PostToolUse => "PostToolUse",
            Self::PostLLMCall => "PostLLMCall",
            Self::PreCompact => "PreCompact",
            Self::PostCompact => "PostCompact",
            Self::Notification => "Notification",
            Self::Stop => "Stop",
        }
    }

    pub(crate) fn matcher_value(self, payload: &Value) -> Option<String> {
        match self {
            Self::PreToolUse | Self::PermissionRequest | Self::PostToolUse => payload
                .get("tool")
                .or_else(|| payload.get("tool_name"))
                .or_else(|| payload.get("toolName"))
                .and_then(Value::as_str)
                .map(str::to_string),
            Self::PreCompact | Self::PostCompact => payload
                .get("trigger")
                .and_then(Value::as_str)
                .map(str::to_string),
            Self::SessionStart => payload
                .get("source")
                .or_else(|| payload.get("start_source"))
                .and_then(Value::as_str)
                .map(str::to_string),
            Self::SubagentStart | Self::SubagentStop => payload
                .get("agent")
                .or_else(|| payload.get("agent_type"))
                .and_then(Value::as_str)
                .map(str::to_string),
            Self::SessionEnd
            | Self::UserPromptSubmit
            | Self::PostLLMCall
            | Self::Notification
            | Self::Stop => None,
        }
    }

    pub(crate) fn supports_block(self) -> bool {
        matches!(
            self,
            Self::SessionStart
                | Self::SubagentStart
                | Self::SubagentStop
                | Self::UserPromptSubmit
                | Self::PreToolUse
                | Self::PermissionRequest
                | Self::PreCompact
                | Self::PostCompact
                | Self::Stop
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookSourceKind {
    Managed,
    Profile,
    Project,
    CapabilityRoot,
    Agent,
    Plugin,
    Worker,
    Runtime,
    Unknown,
}

impl HookSourceKind {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "managed" => Self::Managed,
            "profile" | "user" | "global" => Self::Profile,
            "project" => Self::Project,
            "capability_root" | "selected-capability-root" => Self::CapabilityRoot,
            "agent" | "selected-agent" => Self::Agent,
            "plugin" => Self::Plugin,
            "worker" => Self::Worker,
            "runtime" => Self::Runtime,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::Profile => "profile",
            Self::Project => "project",
            Self::CapabilityRoot => "capability_root",
            Self::Agent => "agent",
            Self::Plugin => "plugin",
            Self::Worker => "worker",
            Self::Runtime => "runtime",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn trusted_by_source(self) -> bool {
        matches!(
            self,
            Self::Managed | Self::Profile | Self::Agent | Self::Runtime
        )
    }

    pub(crate) fn requires_hash_review(self) -> bool {
        matches!(
            self,
            Self::Project | Self::CapabilityRoot | Self::Plugin | Self::Worker
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookHandlerType {
    Command,
    Worker,
    Prompt,
    Agent,
    Unsupported,
}

impl HookHandlerType {
    pub(crate) fn parse(value: Option<&str>, has_command: bool) -> Self {
        match value.unwrap_or(if has_command { "command" } else { "" }) {
            "command" => Self::Command,
            "worker" => Self::Worker,
            "prompt" => Self::Prompt,
            "agent" => Self::Agent,
            _ => Self::Unsupported,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Worker => "worker",
            Self::Prompt => "prompt",
            Self::Agent => "agent",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookTrustStatus {
    Managed,
    Trusted,
    Untrusted,
    Modified,
}

impl HookTrustStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::Trusted => "trusted",
            Self::Untrusted => "untrusted",
            Self::Modified => "modified",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookStateRecord {
    #[serde(default = "default_hook_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted_hash: Option<String>,
}

impl Default for HookStateRecord {
    fn default() -> Self {
        Self {
            enabled: true,
            trusted_hash: None,
        }
    }
}

fn default_hook_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookStateStore {
    pub state: BTreeMap<String, HookStateRecord>,
}

impl HookStateStore {
    pub(crate) fn record_for(&self, key: &str) -> HookStateRecord {
        self.state.get(key).cloned().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookMatcherGroup {
    pub matcher: Option<String>,
    pub hooks: Vec<HookHandler>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookHandler {
    pub handler_type: HookHandlerType,
    pub command: Option<String>,
    pub timeout_secs: u64,
    pub status_message: Option<String>,
    pub prompt: Option<String>,
    pub agent: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookMetadata {
    pub key: String,
    pub event: String,
    pub matcher: Option<String>,
    pub handler_type: HookHandlerType,
    pub source_kind: String,
    pub source_id: String,
    pub source_display_name: Option<String>,
    pub plugin_id: Option<String>,
    pub source_path: Option<PathBuf>,
    pub display_order: usize,
    pub enabled: bool,
    pub managed: bool,
    pub current_hash: String,
    pub trusted_hash: Option<String>,
    pub trust_status: HookTrustStatus,
    pub timeout_secs: u64,
    pub status_message: Option<String>,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookRunSummary {
    pub run_id: String,
    pub event: String,
    pub handler_type: HookHandlerType,
    pub source_kind: String,
    pub source_id: String,
    pub display_order: usize,
    pub status: HookRunStatus,
    pub trust_status: HookTrustStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u128,
    pub diagnostics: Vec<String>,
    pub entries: Vec<HookRunEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookRunStatus {
    Completed,
    Failed,
    Blocked,
    Stopped,
    Skipped,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookRunEntry {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookResponse {
    pub blocked_reason: Option<String>,
    pub permission_decision: Option<HookPermissionDecision>,
    pub updated_input: Option<Value>,
    pub model_content: Option<String>,
    pub context: Vec<Value>,
    pub feedback: Vec<String>,
    pub compaction_guidance: Vec<String>,
    pub diagnostics: Vec<String>,
    pub summaries: Vec<HookRunSummary>,
}

impl HookResponse {
    pub fn approval_decision(&self) -> Option<PermissionApprovalDecision> {
        match self.permission_decision {
            Some(HookPermissionDecision::Allow) => Some(PermissionApprovalDecision::allow_once()),
            Some(HookPermissionDecision::Deny) => Some(PermissionApprovalDecision::deny()),
            None => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookLifecycleOutcome {
    pub response: HookResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

impl HookLifecycleOutcome {
    pub(crate) fn from_response(response: HookResponse) -> Self {
        Self {
            stop_reason: response.blocked_reason.clone(),
            response,
        }
    }

    pub fn should_stop(&self) -> bool {
        self.stop_reason.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookUserPromptSubmitOutcome {
    pub response: HookResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(default)]
    pub context: Vec<Value>,
}

impl HookUserPromptSubmitOutcome {
    pub(crate) fn from_response(response: HookResponse) -> Self {
        Self {
            block_reason: response.blocked_reason.clone(),
            context: response.context.clone(),
            response,
        }
    }

    pub fn is_blocked(&self) -> bool {
        self.block_reason.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookPreToolUseOutcome {
    pub response: HookResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<Value>,
}

impl HookPreToolUseOutcome {
    pub(crate) fn from_response(response: HookResponse) -> Self {
        Self {
            block_reason: response.blocked_reason.clone(),
            updated_input: response.updated_input.clone(),
            response,
        }
    }

    pub fn is_blocked(&self) -> bool {
        self.block_reason.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookPermissionRequestOutcome {
    pub response: HookResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<HookPermissionDecision>,
}

impl HookPermissionRequestOutcome {
    pub(crate) fn from_response(response: HookResponse) -> Self {
        Self {
            decision: response.permission_decision,
            response,
        }
    }

    pub fn approval_decision(&self) -> Option<PermissionApprovalDecision> {
        self.response.approval_decision()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookPostToolUseOutcome {
    pub response: HookResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_content: Option<String>,
}

impl HookPostToolUseOutcome {
    pub(crate) fn from_response(response: HookResponse) -> Self {
        Self {
            model_content: response.model_content.clone(),
            response,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookStopOutcome {
    pub response: HookResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(default)]
    pub continuation_context: Vec<Value>,
}

impl HookStopOutcome {
    pub(crate) fn from_response(response: HookResponse) -> Self {
        let mut continuation_context = response.context.clone();
        continuation_context.extend(response.feedback.iter().map(|text| {
            serde_json::json!({
                "source": "hook_feedback",
                "text": text,
            })
        }));
        Self {
            block_reason: response.blocked_reason.clone(),
            continuation_context,
            response,
        }
    }

    pub fn is_blocked(&self) -> bool {
        self.block_reason.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookReadOnlyOutcome {
    pub response: HookResponse,
}

impl HookReadOnlyOutcome {
    pub(crate) fn from_response(response: HookResponse) -> Self {
        Self { response }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPermissionDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Default)]
pub struct HookRuntimeConfig {
    pub sources: Vec<HookSourceDescriptor>,
    pub state: HookStateStore,
    pub bypass_trust: bool,
}
