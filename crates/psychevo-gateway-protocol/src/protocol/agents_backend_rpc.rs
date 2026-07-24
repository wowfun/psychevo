#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum AgentConfigTarget {
    Project,
    Profile,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentReadParams {
    pub name: String,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentBackendRefInput {
    #[serde(rename = "ref")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentWriteParams {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub backend: Option<AgentBackendRefInput>,
    #[serde(default)]
    pub entrypoints: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: Vec<String>,
    #[serde(default, rename = "optionalContributions")]
    pub optional_contributions: Vec<String>,
    #[serde(default)]
    pub raw_markdown: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentDeleteParams {
    pub name: String,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentSetEnabledParams {
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentStatusParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
    #[serde(default, rename = "threadId")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub all: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamReadParams {
    pub name: String,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamMemberInput {
    pub id: String,
    pub agent: String,
    #[serde(default, rename = "runtimeRef")]
    pub runtime_ref: Option<String>,
    #[serde(default, rename = "runtimeOptions")]
    pub runtime_options: BTreeMap<String, String>,
    #[serde(default, rename = "runtimeProfileRevision")]
    pub runtime_profile_revision: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "maxTurns")]
    #[serde(serialize_with = "option_json_safe_usize::serialize", deserialize_with = "option_json_safe_usize::deserialize")]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub max_turns: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamWriteParams {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub enabled: Option<bool>,
    pub leader: String,
    #[serde(default)]
    pub members: Vec<TeamMemberInput>,
    #[serde(default, rename = "maxParallelAgents")]
    #[serde(serialize_with = "option_json_safe_u64::serialize", deserialize_with = "option_json_safe_u64::deserialize")]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub max_parallel_agents: Option<u64>,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub raw_markdown: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamDeleteParams {
    pub name: String,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamSetEnabledParams {
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamStatusParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
    #[serde(default, rename = "threadId")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentControlParams {
    pub action: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentListResult {
    pub agents: Vec<AgentDefinitionView>,
    pub shadowed_agents: Vec<AgentDefinitionView>,
    pub disabled_agents: Vec<AgentDefinitionView>,
    pub diagnostics: Vec<AgentDiagnosticView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentReadResult {
    pub agent: AgentDefinitionView,
    pub instructions: String,
    pub raw_markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentWriteResult {
    pub written: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
    pub agent: AgentDefinitionView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentDeleteResult {
    pub deleted: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentSetEnabledResult {
    pub written: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
    pub agent: AgentDefinitionView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentStatusResult {
    pub agents: Vec<AgentRunView>,
    pub control: AgentStatusControlView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamListResult {
    pub teams: Vec<TeamDefinitionView>,
    pub shadowed_teams: Vec<TeamDefinitionView>,
    pub disabled_teams: Vec<TeamDefinitionView>,
    pub diagnostics: Vec<AgentDiagnosticView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamReadResult {
    pub team: TeamDefinitionView,
    pub instructions: String,
    pub raw_markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamWriteResult {
    pub written: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
    pub team: TeamDefinitionView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamDeleteResult {
    pub deleted: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamSetEnabledResult {
    pub written: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
    pub team: TeamDefinitionView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamStatusResult {
    #[serde(default)]
    pub team: Option<TeamRunView>,
    #[serde(default)]
    pub mission: Option<MissionRunView>,
    pub agents: Vec<AgentRunView>,
    pub control: AgentStatusControlView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentControlResult {
    pub accepted: bool,
    #[serde(default)]
    pub agent: Option<AgentRunView>,
    pub control: AgentStatusControlView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentBackendRefView {
    #[serde(rename = "ref")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentDiagnosticView {
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum AgentContributionView {
    Instructions,
    Tools,
    Mcp,
    Skills,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentDefinitionView {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub source: String,
    pub source_label: String,
    pub generated: bool,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    pub mutable: bool,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub backend: Option<AgentBackendRefView>,
    pub entrypoints: Vec<String>,
    pub tools: Vec<String>,
    #[serde(rename = "mcpServers")]
    pub mcp_servers: Vec<String>,
    #[serde(default)]
    pub contributions: Vec<AgentContributionView>,
    #[serde(default, rename = "optionalContributions")]
    pub optional_contributions: Vec<String>,
    pub diagnostics: Vec<AgentDiagnosticView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamMemberView {
    pub id: String,
    pub agent: String,
    #[serde(default, rename = "runtimeRef")]
    pub runtime_ref: Option<String>,
    #[serde(default, rename = "runtimeOptions")]
    pub runtime_options: BTreeMap<String, String>,
    #[serde(default, rename = "runtimeProfileRevision")]
    pub runtime_profile_revision: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "maxTurns")]
    #[serde(serialize_with = "option_json_safe_usize::serialize", deserialize_with = "option_json_safe_usize::deserialize")]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub max_turns: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamDefinitionView {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub source: String,
    pub source_label: String,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    pub mutable: bool,
    #[serde(default)]
    pub path: Option<String>,
    pub leader: String,
    pub members: Vec<TeamMemberView>,
    #[serde(rename = "maxParallelAgents")]
    #[serde(serialize_with = "json_safe_u64::serialize", deserialize_with = "json_safe_u64::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub max_parallel_agents: u64,
    pub diagnostics: Vec<AgentDiagnosticView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentRunView {
    pub id: String,
    #[serde(default)]
    pub task_name: Option<String>,
    pub agent_name: String,
    pub task: String,
    pub parent_session_id: String,
    #[serde(default)]
    pub child_session_id: Option<String>,
    pub role: String,
    pub background: bool,
    pub status: String,
    #[serde(default)]
    pub edge_status: Option<String>,
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub started_at_ms: i64,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub ended_at_ms: Option<i64>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub final_answer: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub effective_max_spawn_depth: Option<u8>,
    #[serde(default)]
    pub team_run_id: Option<String>,
    #[serde(default)]
    pub mission_run_id: Option<String>,
    #[serde(default)]
    pub team_name: Option<String>,
    #[serde(default)]
    pub team_member_id: Option<String>,
    #[serde(default)]
    pub agent_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentStatusControlView {
    pub spawning_paused: bool,
    pub max_spawn_depth_cap: u8,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_u64::serialize", deserialize_with = "option_json_safe_u64::deserialize")]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub concurrency_cap: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TeamRunView {
    pub id: String,
    pub parent_session_id: String,
    #[serde(default)]
    pub mission_run_id: Option<String>,
    pub team_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub source_path: Option<String>,
    pub leader_agent_name: String,
    pub members: Vec<TeamMemberView>,
    #[serde(rename = "maxParallelAgents")]
    #[serde(serialize_with = "json_safe_u64::serialize", deserialize_with = "json_safe_u64::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub max_parallel_agents: u64,
    pub status: String,
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub started_at_ms: i64,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub ended_at_ms: Option<i64>,
    #[serde(default)]
    pub final_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MissionRunView {
    pub id: String,
    pub parent_session_id: String,
    #[serde(default)]
    pub team_run_id: Option<String>,
    #[serde(default)]
    pub team_name: Option<String>,
    pub goal: String,
    pub lead_agent_name: String,
    pub status: String,
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub started_at_ms: i64,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub ended_at_ms: Option<i64>,
    #[serde(default)]
    pub final_summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum BackendConfigTarget {
    Project,
    Profile,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendDoctorParams {
    pub id: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendManageParams {
    pub id: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendWriteParams {
    pub id: String,
    pub target: BackendConfigTarget,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub entrypoints: Vec<String>,
    #[serde(default, rename = "clientCapabilities")]
    pub client_capabilities: Vec<String>,
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: Vec<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendDeleteParams {
    pub id: String,
    pub target: BackendConfigTarget,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeProfileListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeProfileReadParams {
    pub id: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeProfileSetEnabledParams {
    pub id: String,
    pub target: BackendConfigTarget,
    pub enabled: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeProfileWriteParams {
    pub id: String,
    pub target: BackendConfigTarget,
    pub runtime: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, rename = "backendRef")]
    pub backend_ref: Option<String>,
    #[serde(default, rename = "defaultModel")]
    pub default_model: Option<String>,
    #[serde(default, rename = "defaultMode")]
    pub default_mode: Option<String>,
    #[serde(default, rename = "defaultAgent")]
    pub default_agent: Option<String>,
    #[serde(default, rename = "approvalMode")]
    pub approval_mode: Option<String>,
    #[serde(default)]
    pub sandbox: Option<String>,
    #[serde(default, rename = "workspaceRoots")]
    pub workspace_roots: Vec<String>,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub options: Option<Value>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeProfileDeleteParams {
    pub id: String,
    pub target: BackendConfigTarget,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadContextReadParams {
    #[serde(default, rename = "threadId")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub target: Option<RunnableTargetInput>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadDraftPrepareParams {
    #[serde(rename = "targetId")]
    pub target_id: String,
    pub scope: GatewayRequestScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadControlSetParams {
    #[serde(default, rename = "threadId")]
    pub thread_id: Option<String>,
    #[serde(rename = "targetId")]
    pub target_id: String,
    #[serde(rename = "controlId")]
    pub control_id: String,
    #[ts(type = "unknown")]
    pub value: Value,
    #[serde(rename = "expectedCapabilityRevision")]
    pub expected_capability_revision: String,
    #[serde(rename = "expectedBindingRevision")]
    #[serde(serialize_with = "json_safe_u64::serialize", deserialize_with = "json_safe_u64::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub expected_binding_revision: u64,
    #[serde(rename = "expectedContextRevision")]
    pub expected_context_revision: String,
    #[serde(rename = "expectedControlRevision")]
    pub expected_control_revision: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginReadParams {
    pub selector: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginDoctorParams {
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginInspectParams {
    pub source: String,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub npm_version: Option<String>,
    #[serde(default)]
    pub npm_registry: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SkillListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SkillReadParams {
    pub name: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SkillInstallParams {
    pub source: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SkillUninstallParams {
    pub name: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SkillSetEnabledParams {
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SkillWriteParams {
    pub name: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    pub raw_markdown: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SkillWriteResult {
    pub written: bool,
    pub name: String,
    pub path: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginInstallParams {
    pub source: String,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub npm_version: Option<String>,
    #[serde(default)]
    pub npm_registry: Option<String>,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginUninstallParams {
    pub selector: String,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginSetEnabledParams {
    pub selector: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginAuthorityWriteParams {
    pub enabled: bool,
    #[serde(default)]
    pub binary: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginAuthorityRefreshParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginAuthoritySetTrustParams {
    pub selector: String,
    pub trusted: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginCatalogListParams {
    #[serde(default)]
    pub authority: Option<String>,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginCatalogAddParams {
    #[serde(default)]
    pub authority: Option<String>,
    pub name: String,
    pub source: String,
    #[serde(default = "default_plugin_catalog_kind")]
    pub kind: String,
    #[serde(default)]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub sparse_paths: Vec<String>,
    #[serde(default)]
    pub npm_version: Option<String>,
    #[serde(default)]
    pub npm_registry: Option<String>,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginCatalogRemoveParams {
    #[serde(default)]
    pub authority: Option<String>,
    pub name: String,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginCatalogUpgradeParams {
    pub name: String,
    #[serde(default)]
    pub authority: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub sparse_paths: Vec<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginConnectStartParams {
    pub selector: String,
    pub component_id: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginConnectStatusParams {
    pub session_id: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

fn default_plugin_catalog_kind() -> String {
    "local".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ToolListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ToolReadParams {
    pub name: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ToolSetEnabledParams {
    pub name: String,
    pub mode: String,
    pub enabled: bool,
    #[serde(default)]
    pub local: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ToolCreateParams {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub includes: Vec<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub local: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ToolRemoveParams {
    pub name: String,
    #[serde(default)]
    pub local: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct McpListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct McpReadParams {
    pub name: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct McpUpsertParams {
    pub name: String,
    pub transport: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default, rename = "bearerTokenEnvVar")]
    pub bearer_token_env_var: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default, rename = "oauthResource")]
    pub oauth_resource: Option<String>,
    #[serde(default, rename = "oauthClientId")]
    pub oauth_client_id: Option<String>,
    #[serde(default, rename = "enabledTools")]
    pub enabled_tools: Option<Vec<String>>,
    #[serde(default, rename = "disabledTools")]
    pub disabled_tools: Vec<String>,
    #[serde(default, rename = "supportsParallelToolCalls")]
    pub supports_parallel_tool_calls: Option<bool>,
    #[serde(default, rename = "startupTimeoutSecs")]
    #[serde(serialize_with = "option_json_safe_u64::serialize", deserialize_with = "option_json_safe_u64::deserialize")]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub startup_timeout_secs: Option<u64>,
    #[serde(default, rename = "toolTimeoutSecs")]
    #[serde(serialize_with = "option_json_safe_u64::serialize", deserialize_with = "option_json_safe_u64::deserialize")]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub tool_timeout_secs: Option<u64>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct McpNameParams {
    pub name: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct McpSetEnabledParams {
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct McpSetToolPolicyParams {
    pub name: String,
    #[serde(default, rename = "enabledTools")]
    pub enabled_tools: Option<Vec<String>>,
    #[serde(default, rename = "disabledTools")]
    pub disabled_tools: Vec<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct McpOAuthStartParams {
    pub name: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct McpOAuthStatusParams {
    pub session_id: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendConfigView {
    pub id: String,
    pub kind: String,
    pub enabled: bool,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: String,
    #[serde(default)]
    pub entrypoints: Vec<String>,
    #[serde(default, rename = "clientCapabilities")]
    pub client_capabilities: Vec<String>,
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: Vec<String>,
    #[serde(default, rename = "envKeys")]
    pub env_keys: Vec<String>,
    #[serde(default, rename = "sourceTargets")]
    pub source_targets: Vec<BackendConfigTarget>,
    pub diagnostics: Vec<BackendDiagnosticView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendListResult {
    pub backends: Vec<BackendConfigView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendDiagnosticView {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendDoctorCheck {
    pub name: String,
    pub ok: bool,
    pub message: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendDoctorResult {
    pub id: String,
    pub kind: String,
    pub ok: bool,
    pub checks: Vec<BackendDoctorCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendWriteResult {
    pub written: bool,
    pub changed: bool,
    pub path: String,
    pub target: BackendConfigTarget,
    pub backend: BackendConfigView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendDeleteResult {
    pub deleted: bool,
    pub changed: bool,
    pub id: String,
    pub path: String,
    pub target: BackendConfigTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackendManageResult {
    pub id: String,
    pub operation: String,
    pub changed: bool,
    pub status: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeProfileView {
    pub id: String,
    pub runtime: String,
    pub enabled: bool,
    pub label: String,
    pub generated: bool,
    #[serde(default)]
    pub configured: bool,
    #[serde(default, rename = "backendRef")]
    pub backend_ref: Option<String>,
    #[serde(default)]
    pub provenance: String,
    #[serde(default, rename = "profileRevision")]
    pub profile_revision: String,
    #[serde(default, rename = "capabilityRevision")]
    pub capability_revision: String,
    #[serde(default)]
    pub stability: Option<RuntimeStabilityView>,
    #[serde(default)]
    pub capabilities: Vec<RuntimeCapabilityView>,
    #[serde(default, rename = "defaultModel")]
    pub default_model: Option<String>,
    #[serde(default, rename = "defaultMode")]
    pub default_mode: Option<String>,
    #[serde(default, rename = "defaultAgent")]
    pub default_agent: Option<String>,
    #[serde(default, rename = "approvalMode")]
    pub approval_mode: Option<String>,
    #[serde(default)]
    pub sandbox: Option<String>,
    #[serde(default, rename = "workspaceRoots")]
    pub workspace_roots: Vec<String>,
    #[serde(default, rename = "optionKeys")]
    pub option_keys: Vec<String>,
    #[serde(default, rename = "sourceTargets")]
    pub source_targets: Vec<BackendConfigTarget>,
    pub health: RuntimeHealthView,
    #[serde(default, rename = "readinessStages")]
    pub readiness_stages: Vec<RuntimeReadinessStageView>,
    #[serde(default)]
    pub diagnostics: Vec<BackendDiagnosticView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeHealthView {
    pub status: String,
    pub summary: String,
    #[serde(default)]
    pub command_path: Option<String>,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub checked_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum RuntimeReadinessStatusView {
    Unchecked,
    Ready,
    Missing,
    NeedsAuth,
    Unsupported,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeReadinessStageView {
    pub id: String,
    pub status: RuntimeReadinessStatusView,
    pub summary: String,
    #[serde(default, rename = "observedAtMs")]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub observed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum RuntimeStabilityView {
    Stable,
    Experimental,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeCapabilityView {
    pub id: String,
    pub enabled: bool,
    pub stability: RuntimeStabilityView,
    #[serde(default, rename = "unavailableReason")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeProfileListResult {
    pub profiles: Vec<RuntimeProfileView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeProfileReadResult {
    pub profile: RuntimeProfileView,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub options: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeProfileWriteResult {
    pub written: bool,
    pub changed: bool,
    pub path: String,
    pub target: BackendConfigTarget,
    pub profile: RuntimeProfileView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeProfileDeleteResult {
    pub deleted: bool,
    pub changed: bool,
    pub id: String,
    pub path: String,
    pub target: BackendConfigTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum RuntimeBindingOwnershipView {
    ReadWrite,
    ReadOnly,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ThreadControlSurfaceRoleView {
    Mode,
    Model,
    Reasoning,
    Advanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ThreadControlMutabilityView {
    ReadOnly,
    Selectable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ThreadControlEffectiveSourceView {
    RuntimeDefault,
    ProfileDefault,
    SourceDraft,
    ThreadPreference,
    TurnOverride,
    RuntimeObserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ThreadControlApplyScopeView {
    TurnDraft,
    Session,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadControlChoiceView {
    #[ts(type = "unknown")]
    pub value: Value,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadControlDependencyView {
    #[serde(rename = "controlId")]
    pub control_id: String,
    #[ts(type = "unknown")]
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadControlDescriptorView {
    pub id: String,
    pub label: String,
    #[serde(rename = "surfaceRole")]
    pub surface_role: ThreadControlSurfaceRoleView,
    pub mutability: ThreadControlMutabilityView,
    pub enabled: bool,
    pub required: bool,
    #[serde(default, rename = "unavailableReason")]
    pub unavailable_reason: Option<String>,
    #[serde(default, rename = "effectiveValue")]
    #[ts(type = "unknown | null")]
    pub effective_value: Option<Value>,
    #[serde(rename = "effectiveSource")]
    pub effective_source: ThreadControlEffectiveSourceView,
    #[serde(rename = "isDefault")]
    pub is_default: bool,
    #[serde(default)]
    pub choices: Vec<ThreadControlChoiceView>,
    #[serde(default, rename = "dependsOn")]
    pub depends_on: Option<ThreadControlDependencyView>,
    #[serde(rename = "applyScope")]
    pub apply_scope: ThreadControlApplyScopeView,
    pub stability: RuntimeStabilityView,
    #[serde(default, rename = "channelSafe")]
    pub channel_safe: bool,
    #[serde(rename = "capabilityRevision")]
    pub capability_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeBindingView {
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(default, rename = "agentRef")]
    pub agent_ref: Option<String>,
    #[serde(rename = "agentFingerprint")]
    pub agent_fingerprint: String,
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(rename = "backendKind")]
    pub backend_kind: String,
    #[serde(default, rename = "nativeKind")]
    pub native_kind: Option<String>,
    #[serde(default, rename = "sessionHandle")]
    pub native_session_id: Option<String>,
    pub cwd: String,
    #[serde(rename = "profileFingerprint")]
    pub profile_fingerprint: String,
    pub ownership: RuntimeBindingOwnershipView,
    #[serde(rename = "bindingRevision")]
    #[serde(serialize_with = "json_safe_u64::serialize", deserialize_with = "json_safe_u64::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub binding_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RunnableTargetView {
    #[serde(rename = "targetId")]
    pub target_id: String,
    #[serde(default, rename = "agentRef")]
    pub agent_ref: Option<String>,
    #[serde(rename = "runtimeProfileRef")]
    pub runtime_profile_ref: String,
    #[serde(rename = "agentLabel")]
    pub agent_label: String,
    #[serde(rename = "profileLabel")]
    pub profile_label: String,
    pub label: String,
    pub ready: bool,
    #[serde(default, rename = "unavailableReason")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadInputCapabilityView {
    pub kind: String,
    pub enabled: bool,
    #[serde(default, rename = "unavailableReason")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadActionDescriptorView {
    pub id: ThreadActionKind,
    pub label: String,
    pub enabled: bool,
    pub stability: RuntimeStabilityView,
    #[serde(default, rename = "channelSafe")]
    pub channel_safe: bool,
    #[serde(default, rename = "unavailableReason")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadSendabilityView {
    pub allowed: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, rename = "recoveryAction")]
    pub recovery_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadContextReadResult {
    #[serde(default, rename = "selectedTargetId")]
    pub selected_target_id: Option<String>,
    #[serde(default, rename = "suggestedTargetId")]
    pub suggested_target_id: Option<String>,
    #[serde(rename = "runtimeProfileRef")]
    pub runtime_profile_ref: String,
    #[serde(rename = "selectionState")]
    pub selection_state: String,
    #[serde(default)]
    pub profiles: Vec<RuntimeProfileView>,
    #[serde(default)]
    pub binding: Option<RuntimeBindingView>,
    #[serde(default)]
    pub controls: Vec<ThreadControlDescriptorView>,
    #[serde(default)]
    pub stability: Option<RuntimeStabilityView>,
    #[serde(default)]
    pub capabilities: Vec<RuntimeCapabilityView>,
    #[serde(default, rename = "compatibleTargets")]
    pub compatible_targets: Vec<RunnableTargetView>,
    #[serde(default, rename = "inputCapabilities")]
    pub input_capabilities: Vec<ThreadInputCapabilityView>,
    #[serde(default)]
    pub actions: Vec<ThreadActionDescriptorView>,
    pub sendability: ThreadSendabilityView,
    pub history: ThreadHistoryView,
    #[serde(default, rename = "pendingInteractions")]
    pub pending_interactions: Vec<PendingActionView>,
    #[serde(rename = "contextRevision")]
    pub context_revision: String,
    #[serde(rename = "controlRevision")]
    pub control_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadDraftPrepareResult {
    pub context: ThreadContextReadResult,
    #[serde(default)]
    pub problem: Option<RuntimeErrorView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadDraftOpenResult {
    pub snapshot: ThreadSnapshot,
    pub context: ThreadContextReadResult,
    #[serde(default)]
    pub problem: Option<RuntimeErrorView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadImportListParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub cursors: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadImportCandidateView {
    #[serde(rename = "candidateId")]
    pub candidate_id: String,
    pub cwd: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, rename = "updatedAt")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadImportProfileView {
    #[serde(rename = "runtimeProfileRef")]
    pub runtime_profile_ref: String,
    #[serde(rename = "profileLabel")]
    pub profile_label: String,
    #[serde(default)]
    pub targets: Vec<RunnableTargetView>,
    pub status: String,
    #[serde(default)]
    pub sessions: Vec<ThreadImportCandidateView>,
    #[serde(default, rename = "nextCursor")]
    pub next_cursor: Option<String>,
    #[serde(default, rename = "alreadyImportedCount")]
    #[serde(serialize_with = "json_safe_usize::serialize", deserialize_with = "json_safe_usize::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub already_imported_count: usize,
    #[serde(default)]
    pub error: Option<AgentErrorView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadImportListResult {
    pub profiles: Vec<ThreadImportProfileView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadImportParams {
    pub scope: GatewayRequestScope,
    #[serde(rename = "candidateId")]
    pub candidate_id: String,
    #[serde(rename = "targetId")]
    pub target_id: String,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadImportResult {
    pub snapshot: Box<ThreadSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ThreadControlReceiptStatusView {
    Rejected,
    Stored,
    Applied,
    Observed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadControlSetResult {
    pub changed: bool,
    pub status: ThreadControlReceiptStatusView,
    pub control: ThreadControlDescriptorView,
    pub context: ThreadContextReadResult,
    #[serde(rename = "bindingRevision")]
    #[serde(serialize_with = "json_safe_u64::serialize", deserialize_with = "json_safe_u64::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub binding_revision: u64,
    #[serde(rename = "contextRevision")]
    pub context_revision: String,
    #[serde(rename = "controlRevision")]
    pub control_revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum RuntimeRetryClassView {
    Never,
    UserAction,
    SafeRetry,
    Reconnect,
    UnknownDelivery,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RuntimeErrorView {
    pub code: String,
    pub stage: String,
    #[serde(rename = "retryClass")]
    pub retry_class: RuntimeRetryClassView,
    pub message: String,
    #[serde(default, rename = "diagnosticRef")]
    pub diagnostic_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ReadyzResult {
    pub ok: bool,
    pub server: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CreateLaunchParams {
    pub cwd: String,
    #[serde(default)]
    pub source: Option<GatewaySourceInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CreateLaunchResult {
    pub launch_id: String,
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub expires_at_ms: i64,
    pub open_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ManagedServerState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub instance_id: Option<String>,
    pub pid: u32,
    pub base_url: String,
    pub readyz_url: String,
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub started_at_ms: i64,
    pub version: String,
    pub executable_path: Option<String>,
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub executable_modified_ms: Option<i64>,
    #[serde(serialize_with = "option_json_safe_u64::serialize", deserialize_with = "option_json_safe_u64::deserialize")]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub executable_size: Option<u64>,
    pub executable_inode: Option<String>,
    pub static_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum JsonRpcId {
    String(String),
    Number(JsonSafeI64),
    Null,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    pub method: String,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct JsonRpcSuccess {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    #[ts(type = "unknown")]
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct JsonRpcErrorResponse {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    pub error: JsonRpcError,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct JsonRpcError {
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub code: i64,
    pub message: String,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "method", content = "params")]
pub enum ServerNotification {
    #[serde(rename = "gateway/event")]
    GatewayEvent(GatewayEvent),
    #[serde(rename = "shell/result")]
    ShellResult(ShellResultPayload),
    #[serde(rename = "shell/error")]
    ShellError(ShellErrorPayload),
    #[serde(rename = "terminal/output")]
    TerminalOutput(TerminalOutputPayload),
    #[serde(rename = "terminal/exited")]
    TerminalExited(TerminalExitedPayload),
    #[serde(rename = "thread/realtime/started")]
    ThreadRealtimeStarted(ThreadRealtimeStartedNotification),
    #[serde(rename = "thread/realtime/sdp")]
    ThreadRealtimeSdp(ThreadRealtimeSdpNotification),
    #[serde(rename = "thread/realtime/itemAdded")]
    ThreadRealtimeItemAdded(ThreadRealtimeItemAddedNotification),
    #[serde(rename = "thread/realtime/transcript/delta")]
    ThreadRealtimeTranscriptDelta(ThreadRealtimeTranscriptNotification),
    #[serde(rename = "thread/realtime/transcript/done")]
    ThreadRealtimeTranscriptDone(ThreadRealtimeTranscriptNotification),
    #[serde(rename = "thread/realtime/outputAudio/delta")]
    ThreadRealtimeOutputAudioDelta(ThreadRealtimeOutputAudioDeltaNotification),
    #[serde(rename = "thread/realtime/error")]
    ThreadRealtimeError(ThreadRealtimeErrorNotification),
    #[serde(rename = "thread/realtime/closed")]
    ThreadRealtimeClosed(ThreadRealtimeClosedNotification),
}
