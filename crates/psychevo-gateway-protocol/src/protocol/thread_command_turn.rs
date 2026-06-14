#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct InitializeParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub server: String,
    pub version: String,
    pub cwd: String,
    pub scope: GatewayRequestScope,
    pub source: GatewaySource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<GatewayProfileView>,
    #[ts(type = "Record<string, unknown>")]
    pub capabilities: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayProfileView {
    pub name: String,
    pub home: String,
    pub default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartParams {
    pub scope: GatewayRequestScope,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadResumeParams {
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReadParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListParams {
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadIdParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRenameParams {
    pub thread_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListResult {
    pub sessions: Vec<SessionSummaryView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMutationResult {
    pub session: SessionSummaryView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDeleteResult {
    pub deleted: bool,
    pub thread_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTraceParams {
    pub thread_id: String,
    #[serde(default)]
    pub after_seq: Option<u64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTraceResult {
    pub thread_id: String,
    pub available: bool,
    #[ts(type = "Array<Record<string, unknown>>")]
    pub events: Vec<Value>,
    pub warnings: Vec<String>,
    pub truncated: bool,
    #[serde(default)]
    pub next_after_seq: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CompletionListParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub text: String,
    pub cursor: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CompletionReplacement {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    pub id: String,
    pub sigil: String,
    pub label: String,
    pub insert_text: String,
    pub kind: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub target: Option<GatewayMentionTarget>,
    #[serde(default)]
    pub sort_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CompletionListResult {
    pub items: Vec<CompletionItem>,
    #[serde(default)]
    pub replacement: Option<CompletionReplacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecuteParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub command: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandAlternateAction {
    #[serde(rename = "type")]
    pub action_type: String,
    pub target: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandListItem {
    pub name: String,
    pub slash: String,
    pub usage: String,
    pub summary: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub argument_kind: String,
    pub source: String,
    #[serde(default)]
    pub presentation_kind: Option<String>,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub feedback_anchor: Option<String>,
    #[serde(default)]
    pub alternate_action: Option<CommandAlternateAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandListResult {
    pub commands: Vec<CommandListItem>,
    #[serde(default)]
    pub hidden_dynamic: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecuteResult {
    pub accepted: bool,
    pub command: String,
    #[serde(default)]
    pub known: Option<bool>,
    #[serde(default)]
    pub presentation_kind: Option<String>,
    #[serde(default)]
    pub feedback_anchor: Option<String>,
    #[serde(default)]
    pub alternate_action: Option<CommandAlternateAction>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub action: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShellStartParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShellStartResult {
    pub accepted: bool,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStartParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStartResult {
    pub terminal_id: String,
    pub cwd: String,
    #[serde(default)]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalWriteParams {
    pub terminal_id: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalResizeParams {
    pub terminal_id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalTerminateParams {
    pub terminal_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalMutationResult {
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputPayload {
    pub terminal_id: String,
    pub stream: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExitedPayload {
    pub terminal_id: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub runtime_ref: Option<String>,
    #[serde(default)]
    pub runtime_session_id: Option<String>,
    #[serde(default)]
    pub runtime_options: BTreeMap<String, String>,
    #[serde(default)]
    pub input: Vec<GatewayInputPart>,
    #[serde(default)]
    pub mentions: Vec<GatewayMention>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnSteerParams {
    #[serde(default)]
    pub thread_id: Option<String>,
    pub expected_turn_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnInterruptParams {
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub source_key: Option<SourceKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOptionsParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub runtime_ref: String,
    #[serde(default)]
    pub runtime_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeConfigOptionValueView {
    pub value: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeConfigOptionView {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub option_type: String,
    #[serde(default)]
    pub current_value: Option<String>,
    #[serde(default)]
    pub values: Vec<RuntimeConfigOptionValueView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOptionsResult {
    pub runtime_ref: String,
    #[serde(default)]
    pub runtime_session_id: Option<String>,
    pub options: Vec<RuntimeConfigOptionView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartResult {
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnControlResult {
    #[serde(default)]
    pub accepted: Option<bool>,
    #[serde(default)]
    pub interrupted: Option<bool>,
    #[serde(default)]
    pub cleared: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnResultPayload {
    pub thread: GatewayThread,
    pub turn: GatewayTurn,
    pub result: TurnRunResult,
    #[serde(rename = "committedEntries", default)]
    pub committed_entries: Vec<TranscriptEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnRunResult {
    pub session_id: String,
    pub outcome: String,
    pub final_answer: String,
    pub tool_failures: usize,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnErrorPayload {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShellResultPayload {
    pub thread: GatewayThread,
    pub command: String,
    pub outcome: String,
    pub tool_failures: usize,
    #[serde(rename = "committedEntries", default)]
    pub committed_entries: Vec<TranscriptEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShellErrorPayload {
    pub message: String,
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRespondParams {
    #[serde(default)]
    pub thread_id: Option<String>,
    pub request_id: String,
    pub decision: PermissionDecision,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ClarifyRespondParams {
    #[serde(default)]
    pub thread_id: Option<String>,
    pub request_id: String,
    #[serde(default)]
    pub answers: Option<Vec<Vec<String>>>,
    #[serde(default)]
    pub cancel: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRespondResult {
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceResetParams {
    pub scope: GatewayRequestScope,
}
