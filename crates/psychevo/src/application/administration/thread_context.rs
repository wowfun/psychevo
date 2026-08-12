use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{AgentBindingSnapshot, ThreadModelSelection};
use crate::context_usage::ContextSnapshot;
use crate::types::{PermissionMode, RunMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideConversationSurface {
    Tui,
    Web,
}

impl SideConversationSurface {
    pub(super) fn source(self) -> &'static str {
        match self {
            Self::Tui => crate::thread_lineage::TUI_SIDE_CONVERSATION_SESSION_SOURCE,
            Self::Web => crate::thread_lineage::WEB_SIDE_CONVERSATION_SESSION_SOURCE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SideConversationAgentBindingSnapshot {
    pub(super) parent_thread_id: String,
    pub(super) expected_binding_revision: i64,
    pub(super) expected_control_revision: i64,
    pub(super) effective_controls: BTreeMap<String, Value>,
}

impl SideConversationAgentBindingSnapshot {
    pub fn new(
        binding: &AgentBindingSnapshot,
        effective_controls: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            parent_thread_id: binding.thread_id.clone(),
            expected_binding_revision: binding.binding_revision,
            expected_control_revision: binding.control_revision,
            effective_controls,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StartSideConversationRequest {
    pub surface: SideConversationSurface,
    pub model: ThreadModelSelection,
    pub mode: RunMode,
    pub permission_mode: PermissionMode,
    pub selected_agent: Option<String>,
    pub agent_binding: Option<SideConversationAgentBindingSnapshot>,
}

#[derive(Debug, Clone)]
pub struct AutoCompactionRequest {
    pub snapshot: ContextSnapshot,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub inherited_env: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone)]
pub struct RefreshThreadContextRequest {
    pub mode: Option<RunMode>,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub agent: Option<String>,
    pub no_agents: bool,
    pub no_skills: bool,
    pub invalidation_reason: String,
    pub notice: Option<String>,
}

impl Default for RefreshThreadContextRequest {
    fn default() -> Self {
        Self {
            mode: None,
            inherited_env: None,
            agent: None,
            no_agents: false,
            no_skills: false,
            invalidation_reason: "manual_reload".to_string(),
            notice: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshThreadContextResult {
    pub thread_id: String,
    pub prefix_hash: String,
    pub version: i64,
    pub provider: String,
    pub model: String,
    pub invalidation_reason: Option<String>,
}
