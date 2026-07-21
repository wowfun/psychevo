#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct InitializeParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub server: String,
    pub version: String,
    pub cwd: String,
    pub display_cwd: String,
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
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ThreadDraftTargetIntent {
    Default,
    Exact {
        #[serde(rename = "targetId")]
        target_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDraftOpenParams {
    pub origin: GatewayRequestScope,
    #[serde(rename = "targetIntent")]
    pub target_intent: ThreadDraftTargetIntent,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCompactionCheckpointView {
    pub checkpoint_id: i64,
    pub reason: String,
    pub created_at_ms: i64,
    pub first_kept_session_seq: i64,
    pub tokens_before: Option<u64>,
    pub tokens_after: Option<u64>,
    pub summary_provider: Option<String>,
    pub summary_model: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCompactionResult {
    pub accepted: bool,
    pub thread_id: Option<String>,
    pub compacted: bool,
    pub reason: String,
    pub message: String,
    #[serde(default)]
    pub checkpoint: Option<ThreadCompactionCheckpointView>,
    pub tokens_before: Option<u64>,
    pub tokens_after: Option<u64>,
    pub summary_provider: Option<String>,
    pub summary_model: Option<String>,
    #[serde(default)]
    pub unavailable: bool,
    #[serde(default)]
    pub error: Option<String>,
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
    pub cwd: Option<String>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadBrowserParams {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub cursor: Option<ThreadBrowserCursor>,
    #[serde(default)]
    pub include_session_ids: Vec<String>,
    #[serde(default)]
    pub recent_days: Option<i64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadBrowserCursor {
    pub cwd: String,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadBrowserResult {
    pub workspaces: Vec<ThreadBrowserWorkspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadBrowserWorkspace {
    pub cwd: String,
    pub project: SessionProjectView,
    pub sessions: Vec<SessionSummaryView>,
    #[serde(default)]
    pub hidden_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<ThreadBrowserCursor>,
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
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub group_label: Option<String>,
    #[serde(default)]
    pub scope_label: Option<String>,
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
    pub expands_to: Option<String>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SlashSettingsReadParams {
    #[serde(default)]
    pub scope: Option<ModelSettingsScope>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SlashSettingsUpdateParams {
    pub scope: ModelSettingsScope,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub leader_key: Option<String>,
    #[serde(default)]
    pub leader_timeout_ms: Option<u64>,
    #[serde(default)]
    pub aliases: Vec<SlashAliasSetting>,
    #[serde(default)]
    pub keybinds: Vec<SlashKeybindSetting>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SlashAliasSetting {
    pub alias: String,
    pub target: String,
    #[serde(default)]
    pub target_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SlashKeybindSetting {
    pub shortcut: String,
    pub target: String,
    #[serde(default)]
    pub target_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SlashSettingsResult {
    pub scope: ModelSettingsScope,
    pub cwd: String,
    pub leader_key: String,
    pub leader_timeout_ms: u64,
    pub aliases: Vec<SlashAliasSetting>,
    pub keybinds: Vec<SlashKeybindSetting>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
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
pub struct RunnableTargetInput {
    #[serde(default, rename = "agentRef")]
    pub agent_ref: Option<String>,
    #[serde(rename = "runtimeProfileRef")]
    pub runtime_profile_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ThreadActionKind {
    Interrupt,
    Steer,
    Compact,
    Fork,
    ForkBefore,
    RevertConversation,
    UnrevertConversation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ThreadEditableInputPart {
    Text { text: String },
    Image { input: GatewayImageInput },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEditableDraft {
    #[serde(default)]
    pub parts: Vec<ThreadEditableInputPart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ThreadEditableDraftFidelity {
    Exact,
    BestEffort,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ThreadActionInput {
    Interrupt,
    Steer {
        expected_turn_id: String,
        text: String,
    },
    Compact {
        #[serde(default)]
        instructions: Option<String>,
    },
    Fork,
    ForkBefore {
        message_id: String,
    },
    RevertConversation {
        message_id: String,
        draft: ThreadEditableDraft,
    },
    UnrevertConversation,
}

impl ThreadActionInput {
    pub fn kind(&self) -> ThreadActionKind {
        match self {
            Self::Interrupt => ThreadActionKind::Interrupt,
            Self::Steer { .. } => ThreadActionKind::Steer,
            Self::Compact { .. } => ThreadActionKind::Compact,
            Self::Fork => ThreadActionKind::Fork,
            Self::ForkBefore { .. } => ThreadActionKind::ForkBefore,
            Self::RevertConversation { .. } => ThreadActionKind::RevertConversation,
            Self::UnrevertConversation => ThreadActionKind::UnrevertConversation,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadActionRunParams {
    pub scope: GatewayRequestScope,
    pub thread_id: String,
    pub action: ThreadActionInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ThreadActionRunResult {
    Interrupt {
        thread_id: String,
        interrupted: bool,
        cleared: usize,
    },
    Steer {
        thread_id: String,
        accepted: bool,
    },
    Compact {
        thread_id: String,
        result: Box<ThreadCompactionResult>,
    },
    Fork {
        #[serde(rename = "sourceThreadId")]
        source_thread_id: String,
        snapshot: Box<ThreadSnapshot>,
    },
    ForkBefore {
        #[serde(rename = "sourceThreadId")]
        source_thread_id: String,
        snapshot: Box<ThreadSnapshot>,
    },
    RevertConversation {
        thread_id: String,
        staged: bool,
        #[serde(rename = "noOp")]
        no_op: bool,
        snapshot: Box<ThreadSnapshot>,
    },
    UnrevertConversation {
        thread_id: String,
        draft: ThreadEditableDraft,
        snapshot: Box<ThreadSnapshot>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ThreadInteractionResponse {
    Permission {
        decision: PermissionDecision,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        directory: Option<String>,
    },
    Clarify { answers: Vec<Vec<String>> },
    CancelClarify,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadInteractionRespondParams {
    pub scope: GatewayRequestScope,
    pub thread_id: String,
    pub interaction_id: String,
    pub response: ThreadInteractionResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadInteractionRespondResult {
    pub accepted: bool,
    pub interaction_id: String,
    pub outcome: GatewayActionOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadHistoryReadParams {
    pub scope: GatewayRequestScope,
    pub thread_id: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadHistoryReadResult {
    pub thread_id: String,
    pub history: ThreadHistoryView,
    pub entries: Vec<TranscriptEntry>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadHistoryDraftReadParams {
    pub scope: GatewayRequestScope,
    pub thread_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadHistoryDraftReadResult {
    pub thread_id: String,
    pub message_id: String,
    #[serde(default)]
    pub message_seq: Option<i64>,
    #[serde(default)]
    pub parts: Vec<ThreadEditableInputPart>,
    pub fidelity: ThreadEditableDraftFidelity,
    #[serde(default)]
    pub warning: Option<String>,
    #[serde(default, rename = "unavailableReason")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartParams {
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub target: Option<RunnableTargetInput>,
    #[serde(default)]
    pub input: Vec<GatewayInputPart>,
    #[serde(default)]
    pub mentions: Vec<GatewayMention>,
    #[serde(default, rename = "turnOverrides")]
    #[ts(type = "Record<string, unknown>")]
    pub turn_overrides: BTreeMap<String, Value>,
    #[serde(default, rename = "expectedContextRevision")]
    pub expected_context_revision: Option<String>,
    #[serde(default, rename = "expectedControlRevision")]
    pub expected_control_revision: Option<String>,
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
pub struct TurnStartResult {
    pub accepted: bool,
    pub thread_id: String,
    pub turn_id: String,
    pub thread: GatewayThread,
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
pub struct SourceResetParams {
    pub scope: GatewayRequestScope,
}
