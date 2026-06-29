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
    EntryDelta {
        #[serde(rename = "turnId")]
        turn_id: String,
        #[serde(rename = "entryId")]
        entry_id: Option<String>,
        #[serde(rename = "blockId")]
        block_id: Option<String>,
        delta: String,
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
    PermissionRequested {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        summary: String,
        reason: String,
        #[serde(rename = "matchedRule")]
        matched_rule: Option<String>,
        #[serde(rename = "suggestedRule")]
        suggested_rule: Option<String>,
        #[serde(rename = "allowAlways")]
        allow_always: bool,
        #[serde(rename = "timeoutSecs")]
        timeout_secs: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        thread_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        turn_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        activity_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        source_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        owner_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        lease_expires_at_ms: Option<i64>,
    },
    PermissionResolved {
        #[serde(rename = "requestId")]
        request_id: String,
        decision: PermissionDecision,
    },
    ClarifyRequested {
        #[serde(rename = "requestId")]
        request_id: String,
        #[ts(type = "unknown")]
        raw: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        thread_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        turn_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        activity_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        source_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        owner_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        lease_expires_at_ms: Option<i64>,
    },
    ClarifyResolved {
        #[serde(rename = "requestId")]
        request_id: String,
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
pub struct GatewaySelectedSkill {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum PermissionDecision {
    AllowOnce,
    AllowSession,
    AllowAlways,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum TranscriptEntryRole {
    User,
    Assistant,
    Diagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
    Diff,
    Artifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
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
pub struct TranscriptToolResult {
    #[serde(rename = "resultMessageSeq")]
    pub result_message_seq: i64,
    pub status: TranscriptBlockStatus,
    pub content: String,
    #[serde(rename = "isError")]
    pub is_error: bool,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub metadata: Option<Value>,
    #[serde(rename = "createdAtMs")]
    pub created_at_ms: i64,
    #[serde(rename = "updatedAtMs")]
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptBlock {
    pub id: String,
    pub kind: TranscriptBlockKind,
    pub status: TranscriptBlockStatus,
    pub order: i64,
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
    pub created_at_ms: i64,
    #[serde(rename = "updatedAtMs")]
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntry {
    pub id: String,
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(rename = "turnId")]
    pub turn_id: Option<String>,
    #[serde(rename = "messageSeq")]
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
    pub created_at_ms: i64,
    #[serde(rename = "updatedAtMs")]
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayActivityView {
    pub running: bool,
    #[serde(default)]
    pub active_turn_id: Option<String>,
    pub queued_turns: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub started_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub updated_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub owner_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub owner_surface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lease_expires_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub takeover_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermissionView {
    pub request_id: String,
    pub tool_name: String,
    pub summary: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub matched_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub suggested_rule: Option<String>,
    pub allow_always: bool,
    pub timeout_secs: u64,
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
    pub lease_expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingClarifyView {
    pub request_id: String,
    #[ts(type = "unknown")]
    pub raw: Value,
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
    pub lease_expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    pub source: GatewaySource,
    pub scope: GatewayRequestScope,
    #[serde(default)]
    pub thread: Option<GatewayThread>,
    pub entries: Vec<TranscriptEntry>,
    pub activity: GatewayActivityView,
    pub pending_permissions: Vec<PendingPermissionView>,
    pub pending_clarifies: Vec<PendingClarifyView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionProjectView {
    pub cwd: String,
    pub label: String,
    pub display_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryView {
    pub id: String,
    pub cwd: String,
    pub project: SessionProjectView,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    pub started_at_ms: i64,
    #[serde(default)]
    pub updated_at_ms: Option<i64>,
    #[serde(default)]
    pub ended_at_ms: Option<i64>,
    #[serde(default)]
    pub end_reason: Option<String>,
    #[serde(default)]
    pub archived_at_ms: Option<i64>,
    pub message_count: usize,
    pub tool_call_count: usize,
    pub visible_entry_count: usize,
    #[serde(default)]
    pub activity: GatewayActivityView,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub display_title: Option<String>,
    #[serde(default)]
    pub preview: Option<String>,
}
