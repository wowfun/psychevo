use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{AgentBindingSnapshot, AgentSource};

#[derive(Debug, Clone)]
pub enum ThreadAgentBinding {
    Resolved {
        binding: Box<AgentBindingSnapshot>,
        writable: bool,
        thread_preferences: BTreeMap<String, Value>,
        runtime_observed: BTreeMap<String, Value>,
    },
    Unresolved {
        thread_id: String,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct UpdateThreadAgentControlState {
    pub expected_binding_revision: i64,
    pub expected_control_revision: i64,
    pub thread_preferences: Option<BTreeMap<String, Value>>,
    pub runtime_observed: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ThreadMainAgentSelection {
    Missing { base_agent: Option<String> },
    Default { base_agent: Option<String> },
    Agent { input: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetThreadMainAgentSelection {
    Default,
    Agent {
        input: String,
        name: String,
        source: AgentSource,
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadModelSelection {
    pub provider: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRelationshipStatus {
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRelationshipAgent {
    pub id: Option<String>,
    pub name: Option<String>,
    pub task_name: Option<String>,
    pub task: Option<String>,
    pub description: Option<String>,
    pub parent_tool_call_id: Option<String>,
    pub team_run_id: Option<String>,
    pub mission_run_id: Option<String>,
    pub team_name: Option<String>,
    pub team_member_id: Option<String>,
    pub runtime_ref: Option<String>,
    pub role: Option<String>,
    pub background: Option<bool>,
    pub fork_context: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRelationship {
    pub parent_thread_id: String,
    pub child_thread_id: String,
    pub status: AgentRelationshipStatus,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub agent: Option<AgentRelationshipAgent>,
}
