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
    pub instructions: String,
    #[serde(default)]
    pub backend: Option<AgentBackendRefInput>,
    #[serde(default)]
    pub entrypoints: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: Vec<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentDeleteParams {
    pub name: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentListResult {
    pub agents: Vec<AgentDefinitionView>,
    pub shadowed_agents: Vec<AgentDefinitionView>,
    pub diagnostics: Vec<AgentDiagnosticView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentReadResult {
    pub agent: AgentDefinitionView,
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentWriteResult {
    pub written: bool,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentDeleteResult {
    pub deleted: bool,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatusResult {
    pub agents: Vec<AgentRunView>,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefinitionView {
    pub name: String,
    pub description: String,
    pub source: String,
    pub generated: bool,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub backend: Option<AgentBackendRefView>,
    pub entrypoints: Vec<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatusControlView {
    pub spawning_paused: bool,
    pub max_spawn_depth_cap: u8,
    #[serde(default)]
    pub concurrency_cap: Option<u64>,
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
    #[serde(rename = "agent/delete")]
    AgentDelete(AgentDeleteParams),
    #[serde(rename = "agent/status")]
    AgentStatus(AgentStatusParams),
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
}
