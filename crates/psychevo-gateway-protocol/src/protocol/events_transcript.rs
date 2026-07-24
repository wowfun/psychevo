#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GatewayEvent {
    TurnStarted {
        #[serde(rename = "threadId")]
        thread_id: Option<String>,
        #[serde(rename = "turnId")]
        turn_id: String,
        #[serde(rename = "selectedSkills", default)]
        selected_skills: Vec<GatewaySelectedSkill>,
    },
    TurnQueued {
        #[serde(rename = "threadId")]
        thread_id: Option<String>,
        #[serde(rename = "turnId")]
        turn_id: String,
        #[serde(rename = "queuePosition")]
        #[serde(serialize_with = "json_safe_usize::serialize", deserialize_with = "json_safe_usize::deserialize")]
        #[schemars(with = "JsonSafeU64")]
        #[ts(type = "number")]
        queue_position: usize,
    },
    TurnCompleted {
        #[serde(rename = "threadId")]
        thread_id: Option<String>,
        #[serde(rename = "turnId")]
        turn_id: String,
        turn: GatewayTurn,
        #[serde(rename = "committedEntries", default)]
        committed_entries: Vec<TranscriptEntry>,
    },
    EntryStarted {
        #[serde(rename = "turnId")]
        turn_id: String,
        entry: TranscriptEntry,
    },
    EntryUpdated {
        #[serde(rename = "turnId")]
        turn_id: String,
        entry: TranscriptEntry,
    },
    EntryCompleted {
        #[serde(rename = "turnId")]
        turn_id: String,
        entry: TranscriptEntry,
    },
    ActionRequested {
        action: PendingActionView,
    },
    ActionUpdated {
        action: PendingActionView,
    },
    ActionResolved {
        #[serde(rename = "actionId")]
        action_id: String,
        kind: GatewayActionKind,
        outcome: GatewayActionOutcome,
        #[serde(default)]
        #[ts(type = "unknown")]
        payload: Value,
    },
    ActionCancelled {
        #[serde(rename = "actionId")]
        action_id: String,
        kind: GatewayActionKind,
        reason: String,
    },
    Warning {
        kind: String,
        message: String,
        #[serde(rename = "sourcePath")]
        source_path: Option<String>,
        suggestion: Option<String>,
    },
    ActivityChanged {
        #[serde(rename = "threadId")]
        thread_id: Option<String>,
        activity: GatewayActivityView,
    },
    TitleChanged {
        #[serde(rename = "threadId")]
        thread_id: String,
        title: Option<String>,
        #[serde(rename = "displayTitle")]
        display_title: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct GatewaySelectedSkill {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum PermissionDecision {
    AllowOnce,
    AllowTurn,
    AllowSession,
    AllowAlways,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum GatewayActionKind {
    Permission,
    Clarify,
    CustomTool,
    UserInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum GatewayActionOutcome {
    Accepted,
    Rejected,
    Cancelled,
    TimedOut,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum TranscriptEntryRole {
    User,
    Assistant,
    Diagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum TranscriptBlockKind {
    Text,
    Reasoning,
    ToolCall,
    ToolResult,
    Tool,
    Shell,
    File,
    Web,
    Mcp,
    Clarify,
    Permission,
    Skill,
    Agent,
    Mailbox,
    Status,
    Compaction,
    Diff,
    Artifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum TranscriptBlockStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    NeedsInput,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TranscriptToolResult {
    #[serde(rename = "resultMessageSeq")]
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub result_message_seq: i64,
    pub status: TranscriptBlockStatus,
    pub content: String,
    #[serde(rename = "isError")]
    pub is_error: bool,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub metadata: Option<Value>,
    #[serde(rename = "createdAtMs")]
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub created_at_ms: i64,
    #[serde(rename = "updatedAtMs")]
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TranscriptBlock {
    pub id: String,
    pub kind: TranscriptBlockKind,
    pub status: TranscriptBlockStatus,
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub order: i64,
    #[serde(default, rename = "phaseOrdinal", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub phase_ordinal: Option<u32>,
    pub source: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub preview: Option<String>,
    pub detail: Option<String>,
    #[serde(rename = "artifactIds")]
    pub artifact_ids: Vec<String>,
    #[ts(type = "unknown | null")]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub result: Option<TranscriptToolResult>,
    #[serde(rename = "createdAtMs")]
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub created_at_ms: i64,
    #[serde(rename = "updatedAtMs")]
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TranscriptEntry {
    pub id: String,
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(rename = "turnId")]
    pub turn_id: Option<String>,
    #[serde(rename = "messageSeq")]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub message_seq: Option<i64>,
    pub role: TranscriptEntryRole,
    pub status: TranscriptBlockStatus,
    pub source: String,
    pub blocks: Vec<TranscriptBlock>,
    #[ts(type = "unknown | null")]
    pub metadata: Option<Value>,
    #[ts(type = "unknown | null")]
    pub usage: Option<Value>,
    #[ts(type = "unknown | null")]
    pub accounting: Option<Value>,
    #[serde(rename = "createdAtMs")]
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub created_at_ms: i64,
    #[serde(rename = "updatedAtMs")]
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct GatewayActivityView {
    pub running: bool,
    #[serde(default)]
    pub active_turn_id: Option<String>,
    #[serde(serialize_with = "json_safe_usize::serialize", deserialize_with = "json_safe_usize::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub queued_turns: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub started_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub updated_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub owner_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub owner_surface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub lease_expires_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub takeover_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PendingActionView {
    pub action_id: String,
    pub kind: GatewayActionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub summary: Option<String>,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub activity_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub owner_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub lease_expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TurnStartReceipt {
    pub client_turn_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    pub source: GatewaySource,
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub thread: Option<GatewayThread>,
    pub history: ThreadHistoryView,
    pub entries: Vec<TranscriptEntry>,
    pub activity: GatewayActivityView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub turn_start_receipts: Option<Vec<TurnStartReceipt>>,
    pub pending_actions: Vec<PendingActionView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub history_editing: Option<ThreadHistoryEditingView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ThreadHistoryEditingKind {
    WorkspaceUndo,
    ConversationEdit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ThreadHistoryRecoveryActionKind {
    RedoWorkspace,
    RestoreHistory,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadHistoryEditingView {
    pub kind: ThreadHistoryEditingKind,
    #[serde(default, rename = "boundaryMessageId")]
    pub boundary_message_id: Option<String>,
    #[serde(default, rename = "hiddenEntryCount")]
    #[serde(serialize_with = "json_safe_usize::serialize", deserialize_with = "json_safe_usize::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub hidden_entry_count: usize,
    #[serde(default, rename = "replacementDraft")]
    pub replacement_draft: Option<ThreadEditableDraft>,
    #[serde(default, rename = "availableActions")]
    pub available_actions: Vec<ThreadHistoryRecoveryActionKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ThreadHistoryOwnerView {
    Psychevo,
    Agent,
    Process,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ThreadHistoryFidelityView {
    Full,
    Summary,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ThreadHistoryView {
    pub owner: ThreadHistoryOwnerView,
    pub fidelity: ThreadHistoryFidelityView,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SessionProjectView {
    pub cwd: String,
    pub label: String,
    pub display_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum SessionLifecycleActionKind {
    Fork,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SessionLifecycleActionView {
    pub id: SessionLifecycleActionKind,
    pub enabled: bool,
    #[serde(default, rename = "unavailableReason")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SessionLifecycleView {
    #[serde(default, rename = "targetLabel")]
    pub target_label: Option<String>,
    #[serde(default)]
    pub actions: Vec<SessionLifecycleActionView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SessionSummaryView {
    pub id: String,
    pub cwd: String,
    pub project: SessionProjectView,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub started_at_ms: i64,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub updated_at_ms: Option<i64>,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub ended_at_ms: Option<i64>,
    #[serde(default)]
    pub end_reason: Option<String>,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub archived_at_ms: Option<i64>,
    #[serde(serialize_with = "json_safe_usize::serialize", deserialize_with = "json_safe_usize::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub message_count: usize,
    #[serde(serialize_with = "json_safe_usize::serialize", deserialize_with = "json_safe_usize::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub tool_call_count: usize,
    #[serde(default)]
    pub activity: GatewayActivityView,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub display_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lifecycle: Option<SessionLifecycleView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub forked_from_thread_id: Option<String>,
}
