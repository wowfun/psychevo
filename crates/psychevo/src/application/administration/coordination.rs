use serde_json::Value;

#[derive(Debug, Clone)]
pub struct AgentTeamRegistration {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source_path: Option<String>,
    pub leader_agent_name: String,
    pub members: Value,
    pub max_parallel_agents: u64,
}

#[derive(Debug, Clone)]
pub struct AgentMissionRegistration {
    pub id: String,
    pub goal: String,
    pub lead_agent_name: String,
    pub team: Option<AgentTeamRegistration>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCoordinationStatus {
    pub team: Option<AgentTeamRunStatus>,
    pub mission: Option<AgentMissionRunStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTeamRunStatus {
    pub id: String,
    pub parent_thread_id: String,
    pub mission_run_id: Option<String>,
    pub team_name: String,
    pub description: Option<String>,
    pub source_path: Option<String>,
    pub leader_agent_name: String,
    pub members: Vec<crate::agents::AgentTeamMember>,
    pub max_parallel_agents: u64,
    pub status: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub final_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMissionRunStatus {
    pub id: String,
    pub parent_thread_id: String,
    pub team_run_id: Option<String>,
    pub team_name: Option<String>,
    pub goal: String,
    pub lead_agent_name: String,
    pub status: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub final_summary: Option<String>,
}
