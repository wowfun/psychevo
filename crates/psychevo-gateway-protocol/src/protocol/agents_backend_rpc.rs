#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentConfigTarget {
    Project,
    Profile,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentReadParams {
    pub name: String,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentBackendRefInput {
    #[serde(rename = "ref")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct AgentDeleteParams {
    pub name: String,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct TeamListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadParams {
    pub name: String,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
    pub max_turns: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct TeamDeleteParams {
    pub name: String,
    #[serde(default)]
    pub target: Option<AgentConfigTarget>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct TeamStatusParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
    #[serde(default, rename = "threadId")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct AgentListResult {
    pub agents: Vec<AgentDefinitionView>,
    pub shadowed_agents: Vec<AgentDefinitionView>,
    pub disabled_agents: Vec<AgentDefinitionView>,
    pub diagnostics: Vec<AgentDiagnosticView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentReadResult {
    pub agent: AgentDefinitionView,
    pub instructions: String,
    pub raw_markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentWriteResult {
    pub written: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
    pub agent: AgentDefinitionView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentDeleteResult {
    pub deleted: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentSetEnabledResult {
    pub written: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
    pub agent: AgentDefinitionView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatusResult {
    pub agents: Vec<AgentRunView>,
    pub control: AgentStatusControlView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TeamListResult {
    pub teams: Vec<TeamDefinitionView>,
    pub shadowed_teams: Vec<TeamDefinitionView>,
    pub disabled_teams: Vec<TeamDefinitionView>,
    pub diagnostics: Vec<AgentDiagnosticView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadResult {
    pub team: TeamDefinitionView,
    pub instructions: String,
    pub raw_markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TeamWriteResult {
    pub written: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
    pub team: TeamDefinitionView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TeamDeleteResult {
    pub deleted: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TeamSetEnabledResult {
    pub written: bool,
    pub name: String,
    pub path: String,
    pub target: AgentConfigTarget,
    pub team: TeamDefinitionView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct AgentControlResult {
    pub accepted: bool,
    #[serde(default)]
    pub agent: Option<AgentRunView>,
    pub control: AgentStatusControlView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentBackendRefView {
    #[serde(rename = "ref")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentDiagnosticView {
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentContributionView {
    Instructions,
    Tools,
    Mcp,
    Skills,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
    pub max_turns: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
    pub max_parallel_agents: u64,
    pub diagnostics: Vec<AgentDiagnosticView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
    pub started_at_ms: i64,
    #[serde(default)]
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
pub struct AgentStatusControlView {
    pub spawning_paused: bool,
    pub max_spawn_depth_cap: u8,
    #[serde(default)]
    pub concurrency_cap: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
    pub max_parallel_agents: u64,
    pub status: String,
    pub started_at_ms: i64,
    #[serde(default)]
    pub ended_at_ms: Option<i64>,
    #[serde(default)]
    pub final_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
    pub started_at_ms: i64,
    #[serde(default)]
    pub ended_at_ms: Option<i64>,
    #[serde(default)]
    pub final_summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum BackendConfigTarget {
    Project,
    Profile,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BackendListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BackendDoctorParams {
    pub id: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct BackendDeleteParams {
    pub id: String,
    pub target: BackendConfigTarget,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileReadParams {
    pub id: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileSetEnabledParams {
    pub id: String,
    pub target: BackendConfigTarget,
    pub enabled: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileWriteParams {
    pub id: String,
    pub target: BackendConfigTarget,
    pub runtime: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
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
pub struct RuntimeProfileDeleteParams {
    pub id: String,
    pub target: BackendConfigTarget,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshotParams {
    #[serde(default, rename = "runtimeRef")]
    pub runtime_ref: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealthCheckParams {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionListParams {
    #[serde(default, rename = "runtimeRef")]
    pub runtime_ref: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionParams {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(rename = "sessionHandle")]
    pub native_session_id: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionReadParams {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(rename = "sessionHandle")]
    pub native_session_id: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionRenameParams {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(rename = "sessionHandle")]
    pub native_session_id: String,
    pub title: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionRevisionParams {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(rename = "sessionHandle")]
    pub native_session_id: String,
    #[serde(default, rename = "revisionHandle")]
    pub revision_handle: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeContextReadParams {
    #[serde(default, rename = "threadId")]
    pub thread_id: Option<String>,
    #[serde(default, rename = "runtimeRef")]
    pub runtime_ref: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeControlSetParams {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(rename = "controlId")]
    pub control_id: String,
    #[ts(type = "unknown")]
    pub value: Value,
    #[serde(rename = "expectedCapabilityRevision")]
    pub expected_capability_revision: String,
    #[serde(rename = "expectedBindingRevision")]
    pub expected_binding_revision: u64,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAuthActionParams {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    pub action: String,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub input: Option<Value>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGoalReadParams {
    #[serde(default, rename = "threadId")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGoalSetParams {
    #[serde(default, rename = "threadId")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub status: Option<RuntimeGoalStatusView>,
    #[serde(default, rename = "tokenBudget")]
    pub token_budget: Option<i64>,
    #[serde(default, rename = "clearTokenBudget")]
    pub clear_token_budget: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGoalClearParams {
    #[serde(default, rename = "threadId")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeAccountRateLimitsReadParams {
    #[serde(default, rename = "runtimeRef")]
    pub runtime_ref: Option<String>,
    #[serde(default, rename = "threadId")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginReadParams {
    pub selector: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginDoctorParams {
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
    pub adapter_mode: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SkillListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SkillReadParams {
    pub name: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct SkillWriteResult {
    pub written: bool,
    pub name: String,
    pub path: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
    pub adapter_mode: Option<String>,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginUninstallParams {
    pub selector: String,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginSetEnabledParams {
    pub selector: String,
    pub enabled: bool,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginSetTrustParams {
    pub selector: String,
    #[serde(default = "default_true")]
    pub trusted: bool,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginCatalogListParams {
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginCatalogAddParams {
    pub name: String,
    pub source: String,
    #[serde(default = "default_plugin_catalog_kind")]
    pub kind: String,
    #[serde(default)]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub npm_version: Option<String>,
    #[serde(default)]
    pub npm_registry: Option<String>,
    #[serde(default)]
    pub adapter_mode: Option<String>,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginCatalogRemoveParams {
    pub name: String,
    #[serde(default)]
    pub scope_name: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

fn default_true() -> bool {
    true
}

fn default_plugin_catalog_kind() -> String {
    "local".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ToolListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ToolReadParams {
    pub name: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct ToolRemoveParams {
    pub name: String,
    #[serde(default)]
    pub local: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpReadParams {
    pub name: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
    pub startup_timeout_secs: Option<u64>,
    #[serde(default, rename = "toolTimeoutSecs")]
    pub tool_timeout_secs: Option<u64>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpNameParams {
    pub name: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpSetEnabledParams {
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct McpOAuthStartParams {
    pub name: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpOAuthStatusParams {
    pub session_id: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct BackendListResult {
    pub backends: Vec<BackendConfigView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BackendDiagnosticView {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BackendDoctorCheck {
    pub name: String,
    pub ok: bool,
    pub message: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BackendDoctorResult {
    pub id: String,
    pub kind: String,
    pub ok: bool,
    pub checks: Vec<BackendDoctorCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BackendWriteResult {
    pub written: bool,
    pub changed: bool,
    pub path: String,
    pub target: BackendConfigTarget,
    pub backend: BackendConfigView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BackendDeleteResult {
    pub deleted: bool,
    pub changed: bool,
    pub id: String,
    pub path: String,
    pub target: BackendConfigTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileView {
    pub id: String,
    pub runtime: String,
    pub enabled: bool,
    pub label: String,
    pub generated: bool,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
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
    #[serde(default, rename = "envKeys")]
    pub env_keys: Vec<String>,
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
pub struct RuntimeHealthView {
    pub status: String,
    pub summary: String,
    #[serde(default)]
    pub command_path: Option<String>,
    #[serde(default)]
    pub checked_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct RuntimeReadinessStageView {
    pub id: String,
    pub status: RuntimeReadinessStatusView,
    pub summary: String,
    #[serde(default, rename = "observedAtMs")]
    pub observed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeStabilityView {
    Stable,
    Experimental,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilityView {
    pub id: String,
    pub enabled: bool,
    pub stability: RuntimeStabilityView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileListResult {
    pub profiles: Vec<RuntimeProfileView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileReadResult {
    pub profile: RuntimeProfileView,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub options: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileWriteResult {
    pub written: bool,
    pub changed: bool,
    pub path: String,
    pub target: BackendConfigTarget,
    pub profile: RuntimeProfileView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileDeleteResult {
    pub deleted: bool,
    pub changed: bool,
    pub id: String,
    pub path: String,
    pub target: BackendConfigTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshotResult {
    pub profiles: Vec<RuntimeProfileView>,
    #[serde(default)]
    pub agents: Vec<RuntimeSnapshotAgentView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshotAgentView {
    pub name: String,
    pub label: String,
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(default, rename = "nativeId")]
    pub native_id: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionListResult {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    pub supported: bool,
    pub sessions: Vec<RuntimeSessionView>,
    #[serde(default, rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeHistoryFidelityView {
    Full,
    Summary,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSessionOwnershipView {
    ReadWrite,
    ReadOnly,
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionRevisionView {
    #[serde(rename = "revisionHandle")]
    pub revision_handle: String,
    pub role: String,
    #[serde(default, rename = "createdAtMs")]
    pub created_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionView {
    #[serde(rename = "sessionHandle")]
    pub native_session_id: String,
    #[serde(default, rename = "threadId")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, rename = "updatedAtMs")]
    pub updated_at_ms: Option<i64>,
    #[serde(default, rename = "parentThreadId")]
    pub parent_thread_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, rename = "dedupKey")]
    pub native_dedup_key: String,
    pub fidelity: RuntimeHistoryFidelityView,
    pub ownership: RuntimeSessionOwnershipView,
    #[serde(default)]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionMutationResult {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(rename = "sessionHandle")]
    pub native_session_id: String,
    pub supported: bool,
    pub changed: bool,
    #[serde(default)]
    pub session: Option<RuntimeSessionView>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub revisions: Vec<RuntimeSessionRevisionView>,
    #[serde(default, rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeControlStateView {
    RuntimeDefault,
    ReadOnlyCurrent,
    Selectable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeControlChoiceView {
    #[ts(type = "unknown")]
    pub value: Value,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeControlDependencyView {
    #[serde(rename = "controlId")]
    pub control_id: String,
    #[ts(type = "unknown")]
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeControlDescriptorView {
    pub id: String,
    pub label: String,
    pub state: RuntimeControlStateView,
    #[serde(default, rename = "currentValue")]
    #[ts(type = "unknown | null")]
    pub current_value: Option<Value>,
    #[serde(default)]
    pub choices: Vec<RuntimeControlChoiceView>,
    #[serde(default, rename = "dependsOn")]
    pub depends_on: Option<RuntimeControlDependencyView>,
    #[serde(default, rename = "channelSafe")]
    pub channel_safe: bool,
    #[serde(rename = "capabilityRevision")]
    pub capability_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBindingView {
    #[serde(rename = "threadId")]
    pub thread_id: String,
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
    pub ownership: RuntimeSessionOwnershipView,
    #[serde(rename = "bindingRevision")]
    pub binding_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeContextReadResult {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(rename = "selectionState")]
    pub selection_state: String,
    #[serde(default)]
    pub profiles: Vec<RuntimeProfileView>,
    #[serde(default)]
    pub binding: Option<RuntimeBindingView>,
    #[serde(default)]
    pub controls: Vec<RuntimeControlDescriptorView>,
    #[serde(default)]
    pub stability: Option<RuntimeStabilityView>,
    #[serde(default)]
    pub capabilities: Vec<RuntimeCapabilityView>,
    #[serde(default, rename = "activeSession")]
    pub active_session: Option<RuntimeSessionView>,
    #[serde(default)]
    pub children: Vec<RuntimeSessionView>,
    #[serde(default)]
    pub goal: Option<RuntimeGoalView>,
    #[serde(default, rename = "accountRateLimits")]
    pub account_rate_limits: Option<RuntimeAccountRateLimitsView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeControlSetResult {
    pub changed: bool,
    pub observed: bool,
    pub control: RuntimeControlDescriptorView,
    #[serde(rename = "bindingRevision")]
    pub binding_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAuthActionResult {
    pub accepted: bool,
    pub status: String,
    pub message: String,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub output: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeGoalStatusView {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeGoalView {
    pub objective: String,
    pub status: RuntimeGoalStatusView,
    #[serde(default, rename = "tokenBudget")]
    pub token_budget: Option<i64>,
    #[serde(rename = "tokensUsed")]
    pub tokens_used: i64,
    #[serde(rename = "timeUsedSeconds")]
    pub time_used_seconds: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeGoalReadResult {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(default)]
    pub goal: Option<RuntimeGoalView>,
    #[serde(rename = "bindingRevision")]
    pub binding_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeGoalSetResult {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    pub goal: RuntimeGoalView,
    #[serde(rename = "bindingRevision")]
    pub binding_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeGoalClearResult {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    pub cleared: bool,
    #[serde(rename = "bindingRevision")]
    pub binding_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRateLimitReachedTypeView {
    RateLimitReached,
    WorkspaceOwnerCreditsDepleted,
    WorkspaceMemberCreditsDepleted,
    WorkspaceOwnerUsageLimitReached,
    WorkspaceMemberUsageLimitReached,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRateLimitWindowView {
    #[serde(rename = "usedPercent")]
    pub used_percent: i32,
    #[serde(default, rename = "windowDurationMins")]
    pub window_duration_mins: Option<i64>,
    #[serde(default, rename = "resetsAt")]
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCreditsSnapshotView {
    #[serde(rename = "hasCredits")]
    pub has_credits: bool,
    pub unlimited: bool,
    #[serde(default)]
    pub balance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSpendControlLimitSnapshotView {
    pub limit: String,
    pub used: String,
    #[serde(rename = "remainingPercent")]
    pub remaining_percent: i32,
    #[serde(rename = "resetsAt")]
    pub resets_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRateLimitSnapshotView {
    #[serde(default, rename = "limitId")]
    pub limit_id: Option<String>,
    #[serde(default, rename = "limitName")]
    pub limit_name: Option<String>,
    #[serde(default)]
    pub primary: Option<RuntimeRateLimitWindowView>,
    #[serde(default)]
    pub secondary: Option<RuntimeRateLimitWindowView>,
    #[serde(default)]
    pub credits: Option<RuntimeCreditsSnapshotView>,
    #[serde(default, rename = "individualLimit")]
    pub individual_limit: Option<RuntimeSpendControlLimitSnapshotView>,
    #[serde(default, rename = "planType")]
    pub plan_type: Option<String>,
    #[serde(default, rename = "rateLimitReachedType")]
    pub rate_limit_reached_type: Option<RuntimeRateLimitReachedTypeView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAccountRateLimitsView {
    #[serde(rename = "rateLimits")]
    pub rate_limits: RuntimeRateLimitSnapshotView,
    #[serde(default, rename = "rateLimitsByLimitId")]
    pub rate_limits_by_limit_id: BTreeMap<String, RuntimeRateLimitSnapshotView>,
    #[serde(default, rename = "resetCreditsAvailable")]
    pub reset_credits_available: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAccountRateLimitsReadResult {
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: String,
    #[serde(rename = "accountRateLimits")]
    pub account_rate_limits: RuntimeAccountRateLimitsView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeRetryClassView {
    Never,
    UserAction,
    SafeRetry,
    Reconnect,
    UnknownDelivery,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct ReadyzResult {
    pub ok: bool,
    pub server: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateLaunchParams {
    pub cwd: String,
    #[serde(default)]
    pub source: Option<GatewaySourceInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateLaunchResult {
    pub launch_id: String,
    pub expires_at_ms: i64,
    pub open_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManagedServerState {
    pub pid: u32,
    pub base_url: String,
    pub readyz_url: String,
    pub started_at_ms: i64,
    pub version: String,
    pub executable_path: Option<String>,
    pub executable_modified_ms: Option<i64>,
    pub executable_size: Option<u64>,
    pub executable_inode: Option<u64>,
    pub static_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum JsonRpcId {
    String(String),
    Number(i64),
    Null,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcSuccess {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    #[ts(type = "unknown")]
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcErrorResponse {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    pub error: JsonRpcError,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "method", content = "params")]
pub enum ClientRequest {
    #[serde(rename = "initialize")]
    Initialize(InitializeParams),
    #[serde(rename = "thread/start")]
    ThreadStart(ThreadStartParams),
    #[serde(rename = "thread/compact/start")]
    ThreadCompactStart(ThreadCompactStartParams),
    #[serde(rename = "thread/resume")]
    ThreadResume(ThreadResumeParams),
    #[serde(rename = "thread/read")]
    ThreadRead(ThreadReadParams),
    #[serde(rename = "thread/trace")]
    ThreadTrace(ThreadTraceParams),
    #[serde(rename = "thread/list")]
    ThreadList(ThreadListParams),
    #[serde(rename = "thread/browser")]
    ThreadBrowser(ThreadBrowserParams),
    #[serde(rename = "thread/rename")]
    ThreadRename(ThreadRenameParams),
    #[serde(rename = "thread/archive")]
    ThreadArchive(ThreadIdParams),
    #[serde(rename = "thread/restore")]
    ThreadRestore(ThreadIdParams),
    #[serde(rename = "thread/delete")]
    ThreadDelete(ThreadIdParams),
    #[serde(rename = "turn/start")]
    TurnStart(TurnStartParams),
    #[serde(rename = "turn/steer")]
    TurnSteer(TurnSteerParams),
    #[serde(rename = "turn/interrupt")]
    TurnInterrupt(TurnInterruptParams),
    #[serde(rename = "turn/takeover")]
    TurnTakeover(TurnTakeoverParams),
    #[serde(rename = "runtime/options")]
    RuntimeOptions(RuntimeOptionsParams),
    #[serde(rename = "runtime/context/read")]
    RuntimeContextRead(RuntimeContextReadParams),
    #[serde(rename = "runtime/control/set")]
    RuntimeControlSet(RuntimeControlSetParams),
    #[serde(rename = "runtime/auth/action")]
    RuntimeAuthAction(RuntimeAuthActionParams),
    #[serde(rename = "runtime/goal/read")]
    RuntimeGoalRead(RuntimeGoalReadParams),
    #[serde(rename = "runtime/goal/set")]
    RuntimeGoalSet(RuntimeGoalSetParams),
    #[serde(rename = "runtime/goal/clear")]
    RuntimeGoalClear(RuntimeGoalClearParams),
    #[serde(rename = "runtime/account/rateLimits/read")]
    RuntimeAccountRateLimitsRead(RuntimeAccountRateLimitsReadParams),
    #[serde(rename = "runtime/profile/list")]
    RuntimeProfileList(RuntimeProfileListParams),
    #[serde(rename = "runtime/profile/read")]
    RuntimeProfileRead(RuntimeProfileReadParams),
    #[serde(rename = "runtime/profile/write")]
    RuntimeProfileWrite(RuntimeProfileWriteParams),
    #[serde(rename = "runtime/profile/delete")]
    RuntimeProfileDelete(RuntimeProfileDeleteParams),
    #[serde(rename = "runtime/profile/setEnabled")]
    RuntimeProfileSetEnabled(RuntimeProfileSetEnabledParams),
    #[serde(rename = "runtime/snapshot")]
    RuntimeSnapshot(RuntimeSnapshotParams),
    #[serde(rename = "runtime/health/check")]
    RuntimeHealthCheck(RuntimeHealthCheckParams),
    #[serde(rename = "runtime/session/list")]
    RuntimeSessionList(RuntimeSessionListParams),
    #[serde(rename = "runtime/session/read")]
    RuntimeSessionRead(RuntimeSessionReadParams),
    #[serde(rename = "runtime/session/attach")]
    RuntimeSessionAttach(RuntimeSessionParams),
    #[serde(rename = "runtime/session/resume")]
    RuntimeSessionResume(RuntimeSessionParams),
    #[serde(rename = "runtime/session/archive")]
    RuntimeSessionArchive(RuntimeSessionParams),
    #[serde(rename = "runtime/session/unarchive")]
    RuntimeSessionUnarchive(RuntimeSessionParams),
    #[serde(rename = "runtime/session/delete")]
    RuntimeSessionDelete(RuntimeSessionParams),
    #[serde(rename = "runtime/session/rename")]
    RuntimeSessionRename(RuntimeSessionRenameParams),
    #[serde(rename = "runtime/session/fork")]
    RuntimeSessionFork(RuntimeSessionParams),
    #[serde(rename = "runtime/session/revert")]
    RuntimeSessionRevert(RuntimeSessionRevisionParams),
    #[serde(rename = "runtime/session/unrevert")]
    RuntimeSessionUnrevert(RuntimeSessionRevisionParams),
    #[serde(rename = "automation/list")]
    AutomationList(AutomationListParams),
    #[serde(rename = "automation/draft")]
    AutomationDraft(AutomationDraftParams),
    #[serde(rename = "automation/write")]
    AutomationWrite(AutomationWriteParams),
    #[serde(rename = "automation/pause")]
    AutomationPause(AutomationIdParams),
    #[serde(rename = "automation/resume")]
    AutomationResume(AutomationIdParams),
    #[serde(rename = "automation/delete")]
    AutomationDelete(AutomationIdParams),
    #[serde(rename = "automation/run")]
    AutomationRun(AutomationRunParams),
    #[serde(rename = "completion/list")]
    CompletionList(CompletionListParams),
    #[serde(rename = "command/list")]
    CommandList(CommandListParams),
    #[serde(rename = "command/execute")]
    CommandExecute(CommandExecuteParams),
    #[serde(rename = "slash/settings/read")]
    SlashSettingsRead(SlashSettingsReadParams),
    #[serde(rename = "slash/settings/update")]
    SlashSettingsUpdate(SlashSettingsUpdateParams),
    #[serde(rename = "agent/list")]
    AgentList(AgentListParams),
    #[serde(rename = "agent/read")]
    AgentRead(AgentReadParams),
    #[serde(rename = "agent/write")]
    AgentWrite(AgentWriteParams),
    #[serde(rename = "agent/setEnabled")]
    AgentSetEnabled(AgentSetEnabledParams),
    #[serde(rename = "agent/delete")]
    AgentDelete(AgentDeleteParams),
    #[serde(rename = "agent/status")]
    AgentStatus(AgentStatusParams),
    #[serde(rename = "team/list")]
    TeamList(TeamListParams),
    #[serde(rename = "team/read")]
    TeamRead(TeamReadParams),
    #[serde(rename = "team/write")]
    TeamWrite(TeamWriteParams),
    #[serde(rename = "team/setEnabled")]
    TeamSetEnabled(TeamSetEnabledParams),
    #[serde(rename = "team/delete")]
    TeamDelete(TeamDeleteParams),
    #[serde(rename = "team/status")]
    TeamStatus(TeamStatusParams),
    #[serde(rename = "agent/control")]
    AgentControl(AgentControlParams),
    #[serde(rename = "backend/list")]
    BackendList(BackendListParams),
    #[serde(rename = "backend/doctor")]
    BackendDoctor(BackendDoctorParams),
    #[serde(rename = "backend/write")]
    BackendWrite(BackendWriteParams),
    #[serde(rename = "backend/delete")]
    BackendDelete(BackendDeleteParams),
    #[serde(rename = "plugin/list")]
    PluginList(PluginListParams),
    #[serde(rename = "plugin/read")]
    PluginRead(PluginReadParams),
    #[serde(rename = "plugin/doctor")]
    PluginDoctor(PluginDoctorParams),
    #[serde(rename = "plugin/import/inspect")]
    PluginInspect(PluginInspectParams),
    #[serde(rename = "plugin/install")]
    PluginInstall(PluginInstallParams),
    #[serde(rename = "plugin/uninstall")]
    PluginUninstall(PluginUninstallParams),
    #[serde(rename = "plugin/setEnabled")]
    PluginSetEnabled(PluginSetEnabledParams),
    #[serde(rename = "plugin/setTrust")]
    PluginSetTrust(PluginSetTrustParams),
    #[serde(rename = "plugin/catalog/list")]
    PluginCatalogList(PluginCatalogListParams),
    #[serde(rename = "plugin/catalog/add")]
    PluginCatalogAdd(PluginCatalogAddParams),
    #[serde(rename = "plugin/catalog/remove")]
    PluginCatalogRemove(PluginCatalogRemoveParams),
    #[serde(rename = "skill/list")]
    SkillList(SkillListParams),
    #[serde(rename = "skill/read")]
    SkillRead(SkillReadParams),
    #[serde(rename = "skill/install")]
    SkillInstall(SkillInstallParams),
    #[serde(rename = "skill/uninstall")]
    SkillUninstall(SkillUninstallParams),
    #[serde(rename = "skill/setEnabled")]
    SkillSetEnabled(SkillSetEnabledParams),
    #[serde(rename = "skill/write")]
    SkillWrite(SkillWriteParams),
    #[serde(rename = "tool/list")]
    ToolList(ToolListParams),
    #[serde(rename = "tool/read")]
    ToolRead(ToolReadParams),
    #[serde(rename = "tool/setEnabled")]
    ToolSetEnabled(ToolSetEnabledParams),
    #[serde(rename = "tool/create")]
    ToolCreate(ToolCreateParams),
    #[serde(rename = "tool/remove")]
    ToolRemove(ToolRemoveParams),
    #[serde(rename = "mcp/list")]
    McpList(McpListParams),
    #[serde(rename = "mcp/read")]
    McpRead(McpReadParams),
    #[serde(rename = "mcp/upsert")]
    McpUpsert(McpUpsertParams),
    #[serde(rename = "mcp/remove")]
    McpRemove(McpNameParams),
    #[serde(rename = "mcp/setEnabled")]
    McpSetEnabled(McpSetEnabledParams),
    #[serde(rename = "mcp/setToolPolicy")]
    McpSetToolPolicy(McpSetToolPolicyParams),
    #[serde(rename = "mcp/test")]
    McpTest(McpNameParams),
    #[serde(rename = "mcp/oauth/start")]
    McpOAuthStart(McpOAuthStartParams),
    #[serde(rename = "mcp/oauth/status")]
    McpOAuthStatus(McpOAuthStatusParams),
    #[serde(rename = "mcp/oauth/logout")]
    McpOAuthLogout(McpNameParams),
    #[serde(rename = "channel/list")]
    ChannelList(ChannelListParams),
    #[serde(rename = "channel/show")]
    ChannelShow(ChannelIdParams),
    #[serde(rename = "channel/enable")]
    ChannelEnable(ChannelEnableParams),
    #[serde(rename = "channel/update")]
    ChannelUpdate(ChannelUpdateParams),
    #[serde(rename = "channel/delete")]
    ChannelDelete(ChannelIdParams),
    #[serde(rename = "channel/doctor")]
    ChannelDoctor(ChannelDoctorParams),
    #[serde(rename = "channel/source/list")]
    ChannelSourceList(ChannelIdParams),
    #[serde(rename = "channel/wechat-qr/start")]
    ChannelWechatQrStart(ChannelWechatQrStartParams),
    #[serde(rename = "channel/wechat-qr/poll")]
    ChannelWechatQrPoll(ChannelWechatQrPollParams),
    #[serde(rename = "shell/start")]
    ShellStart(ShellStartParams),
    #[serde(rename = "terminal/start")]
    TerminalStart(TerminalStartParams),
    #[serde(rename = "terminal/write")]
    TerminalWrite(TerminalWriteParams),
    #[serde(rename = "terminal/resize")]
    TerminalResize(TerminalResizeParams),
    #[serde(rename = "terminal/terminate")]
    TerminalTerminate(TerminalTerminateParams),
    #[serde(rename = "source/reset")]
    SourceReset(SourceResetParams),
    #[serde(rename = "permission/respond")]
    PermissionRespond(PermissionRespondParams),
    #[serde(rename = "clarify/respond")]
    ClarifyRespond(ClarifyRespondParams),
    #[serde(rename = "settings/update")]
    SettingsUpdate(SettingsUpdateParams),
    #[serde(rename = "settings/read")]
    SettingsRead(SettingsReadParams),
    #[serde(rename = "model/settings/read")]
    ModelSettingsRead(ModelSettingsReadParams),
    #[serde(rename = "model/provider/save")]
    ModelProviderSave(ModelProviderSaveParams),
    #[serde(rename = "model/provider/catalog")]
    ModelProviderCatalog(ModelProviderCatalogParams),
    #[serde(rename = "model/state/read")]
    ModelStateRead(ModelStateReadParams),
    #[serde(rename = "model/state/set")]
    ModelStateSet(ModelStateSetParams),
    #[serde(rename = "model/assignment/set")]
    ModelAssignmentSet(ModelAssignmentSetParams),
    #[serde(rename = "voice/asr/transcribe")]
    VoiceAsrTranscribe(VoiceAsrTranscribeParams),
    #[serde(rename = "voice/tts/synthesize")]
    VoiceTtsSynthesize(VoiceTtsSynthesizeParams),
    #[serde(rename = "voice/policy/read")]
    VoicePolicyRead(VoicePolicyReadParams),
    #[serde(rename = "voice/policy/update")]
    VoicePolicyUpdate(VoicePolicyUpdateParams),
    #[serde(rename = "thread/realtime/start")]
    ThreadRealtimeStart(ThreadRealtimeStartParams),
    #[serde(rename = "thread/realtime/appendAudio")]
    ThreadRealtimeAppendAudio(ThreadRealtimeAppendAudioParams),
    #[serde(rename = "thread/realtime/appendText")]
    ThreadRealtimeAppendText(ThreadRealtimeAppendTextParams),
    #[serde(rename = "thread/realtime/appendSpeech")]
    ThreadRealtimeAppendSpeech(ThreadRealtimeAppendSpeechParams),
    #[serde(rename = "thread/realtime/stop")]
    ThreadRealtimeStop(ThreadRealtimeSessionParams),
    #[serde(rename = "thread/realtime/listVoices")]
    ThreadRealtimeListVoices(ThreadRealtimeSessionParams),
    #[serde(rename = "workspace/files")]
    WorkspaceFiles(WorkspaceFilesParams),
    #[serde(rename = "workspace/file/read")]
    WorkspaceFileRead(WorkspaceFileReadParams),
    #[serde(rename = "workspace/file/write")]
    WorkspaceFileWrite(WorkspaceFileWriteParams),
    #[serde(rename = "workspace/diff")]
    WorkspaceDiff(WorkspaceDiffParams),
    #[serde(rename = "workspace/changes")]
    WorkspaceChanges(WorkspaceChangesParams),
    #[serde(rename = "workspace/change/accept")]
    WorkspaceChangeAccept(WorkspaceChangeFileParams),
    #[serde(rename = "workspace/change/reject")]
    WorkspaceChangeReject(WorkspaceChangeFileParams),
    #[serde(rename = "context/read")]
    ContextRead(ContextReadParams),
    #[serde(rename = "observability/read")]
    ObservabilityRead(ObservabilityReadParams),
    #[serde(rename = "usage/read")]
    UsageRead(UsageReadParams),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "method", content = "params")]
pub enum ServerNotification {
    #[serde(rename = "gateway/event")]
    GatewayEvent(GatewayEvent),
    #[serde(rename = "turn/result")]
    TurnResult(TurnResultPayload),
    #[serde(rename = "turn/error")]
    TurnError(TurnErrorPayload),
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
