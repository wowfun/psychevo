use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

pub const JSONRPC_VERSION: &str = "2.0";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS)]
#[serde(transparent)]
pub struct SourceKey(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum GatewaySourceLifetime {
    Invocation,
    Process,
    Persistent,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySource {
    pub kind: String,
    pub raw_id: String,
    pub lifetime: GatewaySourceLifetime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown | null")]
    pub raw_identity: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_name: Option<String>,
}

impl GatewaySource {
    pub fn new(kind: impl Into<String>, raw_id: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            raw_id: raw_id.into(),
            lifetime: GatewaySourceLifetime::Invocation,
            raw_identity: None,
            visible_name: None,
        }
    }

    pub fn invocation(mut self) -> Self {
        self.lifetime = GatewaySourceLifetime::Invocation;
        self
    }

    pub fn process(mut self) -> Self {
        self.lifetime = GatewaySourceLifetime::Process;
        self
    }

    pub fn persistent(mut self) -> Self {
        self.lifetime = GatewaySourceLifetime::Persistent;
        self
    }

    pub fn with_raw_identity(mut self, raw_identity: Value) -> Self {
        self.raw_identity = Some(raw_identity);
        self
    }

    pub fn with_visible_name(mut self, visible_name: impl Into<String>) -> Self {
        self.visible_name = Some(visible_name.into());
        self
    }

    pub fn source_key(&self) -> SourceKey {
        SourceKey(format!("{}:{}", self.kind, self.raw_id))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySourceInput {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<GatewaySourceLifetime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown | null")]
    pub raw_identity: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRequestScope {
    pub workdir: String,
    pub source: GatewaySourceInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GatewayThreadSelector {
    ThreadId {
        #[serde(rename = "threadId")]
        thread_id: String,
    },
    Source {
        #[serde(rename = "sourceKey")]
        source_key: SourceKey,
    },
}

impl GatewayThreadSelector {
    pub fn thread_id(thread_id: impl Into<String>) -> Self {
        Self::ThreadId {
            thread_id: thread_id.into(),
        }
    }

    pub fn source(source_key: SourceKey) -> Self {
        Self::Source { source_key }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum BackendKind {
    Psychevo,
    PeerAgent,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Psychevo => "psychevo",
            Self::PeerAgent => "peer_agent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayBackendInfo {
    pub kind: BackendKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayThread {
    pub id: String,
    pub backend: GatewayBackendInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_key: Option<SourceKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayTurn {
    pub id: String,
    pub thread_id: String,
    pub status: GatewayTurnStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum GatewayTurnStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GatewayInputPart {
    Text {
        text: String,
    },
    Image {
        input: GatewayImageInput,
    },
    Context {
        label: String,
        text: String,
        #[serde(rename = "visibleToModel")]
        visible_to_model: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GatewayImageInput {
    LocalPath { path: String },
    Url { url: String },
}

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
        outcome: Option<String>,
    },
    ItemDelta {
        #[serde(rename = "turnId")]
        turn_id: String,
        #[serde(rename = "itemId")]
        item_id: Option<String>,
        delta: String,
    },
    ItemStarted {
        #[serde(rename = "turnId")]
        turn_id: String,
        item: TimelineItem,
    },
    ItemUpdated {
        #[serde(rename = "turnId")]
        turn_id: String,
        item: TimelineItem,
    },
    ItemCompleted {
        #[serde(rename = "turnId")]
        turn_id: String,
        item: TimelineItem,
    },
    PermissionRequested {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        reason: String,
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
    DebugAvailable {
        #[serde(rename = "turnId")]
        turn_id: String,
    },
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
pub enum TimelineItemKind {
    Prompt,
    Assistant,
    Reasoning,
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
pub enum TimelineItemStatus {
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
pub struct TimelineItem {
    pub id: String,
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(rename = "turnId")]
    pub turn_id: Option<String>,
    pub sequence: i64,
    pub kind: TimelineItemKind,
    pub status: TimelineItemStatus,
    pub source: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub preview: Option<String>,
    pub detail: Option<String>,
    #[serde(rename = "artifactIds")]
    pub artifact_ids: Vec<String>,
    #[ts(type = "unknown | null")]
    pub metadata: Option<Value>,
    #[serde(rename = "createdAtMs")]
    pub created_at_ms: i64,
    #[serde(rename = "updatedAtMs")]
    pub updated_at_ms: i64,
}

pub type ThreadItem = TimelineItem;
pub type TurnItem = TimelineItem;

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayActivityView {
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_turn_id: Option<String>,
    pub queued_turns: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermissionView {
    pub request_id: String,
    pub tool_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingClarifyView {
    pub request_id: String,
    #[ts(type = "unknown")]
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    pub source: GatewaySource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<GatewayThread>,
    pub items: Vec<ThreadItem>,
    pub activity: GatewayActivityView,
    pub pending_permissions: Vec<PendingPermissionView>,
    pub pending_clarifies: Vec<PendingClarifyView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryView {
    pub id: String,
    pub source: String,
    pub workdir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub started_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at_ms: Option<i64>,
    pub message_count: usize,
    pub tool_call_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

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
    #[ts(type = "Record<string, unknown>")]
    pub capabilities: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartParams {
    pub scope: GatewayRequestScope,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadResumeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartParams {
    pub scope: GatewayRequestScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub input: Vec<GatewayInputPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnSteerParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub expected_turn_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnInterruptParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_key: Option<SourceKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartResult {
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnControlResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleared: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnResultPayload {
    pub thread: GatewayThread,
    pub turn: GatewayTurn,
    pub result: TurnRunResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnRunResult {
    pub session_id: String,
    pub outcome: String,
    pub final_answer: String,
    pub tool_failures: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnErrorPayload {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRespondParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub request_id: String,
    pub decision: PermissionDecision,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ClarifyRespondParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<Vec<Vec<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SettingsReadParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SettingsReadResult {
    pub workdir: String,
    #[ts(type = "Record<string, unknown>")]
    pub memory_resources: BTreeMap<String, Value>,
    #[ts(type = "Record<string, unknown>")]
    pub secrets: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct DebugEventsParams {
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct TimelineDebugEvent {
    pub id: i64,
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(rename = "turnId")]
    pub turn_id: Option<String>,
    #[serde(rename = "eventType")]
    pub event_type: String,
    pub source: String,
    #[ts(type = "unknown | null")]
    pub scope: Option<Value>,
    pub status: Option<String>,
    pub summary: Option<String>,
    #[ts(type = "unknown | null")]
    pub payload: Option<Value>,
    #[serde(rename = "createdAtMs")]
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct DebugEventsResult {
    pub events: Vec<TimelineDebugEvent>,
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
    pub workdir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown | null")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(rename = "thread/list")]
    ThreadList(ThreadListParams),
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
    #[serde(rename = "source/reset")]
    SourceReset(SourceResetParams),
    #[serde(rename = "permission/respond")]
    PermissionRespond(PermissionRespondParams),
    #[serde(rename = "clarify/respond")]
    ClarifyRespond(ClarifyRespondParams),
    #[serde(rename = "settings/read")]
    SettingsRead(SettingsReadParams),
    #[serde(rename = "debug/events")]
    DebugEvents(DebugEventsParams),
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
}

#[derive(Debug, Clone, Copy)]
struct ExportedType {
    name: &'static str,
    ts_decl: fn() -> Result<String>,
    schema: fn() -> Result<Value>,
}

pub fn generate_typescript_and_schema(repo_root: &Path, check: bool) -> Result<()> {
    let package_root = repo_root.join("packages/protocol");
    let generated_dir = package_root.join("src/generated");
    let schema_dir = package_root.join("schema");
    let mut ts = String::from("// @generated by psychevo-xtask gateway-protocol generate\n\n");
    for exported in exported_types() {
        ts.push_str(&(exported.ts_decl)()?);
        ts.push_str("\n\n");
    }
    write_checked(&generated_dir.join("types.ts"), &ts, check)?;

    let mut schema_ts =
        String::from("// @generated by psychevo-xtask gateway-protocol generate\n\n");
    schema_ts.push_str("export const gatewaySchemas = {\n");
    for exported in exported_types() {
        let schema = (exported.schema)()?;
        let json = serde_json::to_string_pretty(&schema)?;
        write_checked(
            &schema_dir.join(format!("{}.json", exported.name)),
            &(json.clone() + "\n"),
            check,
        )?;
        schema_ts.push_str("  ");
        schema_ts.push_str(exported.name);
        schema_ts.push_str(": ");
        schema_ts.push_str(&json);
        schema_ts.push_str(",\n");
    }
    schema_ts.push_str("} as const;\n\n");
    schema_ts.push_str("export type GatewaySchemaName = keyof typeof gatewaySchemas;\n");
    write_checked(&generated_dir.join("schemas.ts"), &schema_ts, check)?;

    let index = "// @generated by psychevo-xtask gateway-protocol generate\nexport * from './types';\nexport * from './schemas';\n";
    write_checked(&generated_dir.join("index.ts"), index, check)?;
    Ok(())
}

fn write_checked(path: &Path, content: &str, check: bool) -> Result<()> {
    if check {
        let existing = fs::read_to_string(path).with_context(|| {
            format!(
                "generated file is missing or unreadable: {}",
                path.display()
            )
        })?;
        if existing != content {
            bail!("generated file is out of date: {}", path.display());
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn ts_decl<T>() -> Result<String>
where
    T: TS,
{
    let decl = T::decl();
    Ok(export_ts_decl(camelize_ts_decl_numbers(decl)))
}

fn export_ts_decl(decl: String) -> String {
    if decl.starts_with("type ") {
        format!("export {decl}")
    } else if decl.starts_with("interface ") {
        format!("export {decl}")
    } else {
        decl
    }
}

fn camelize_ts_decl_numbers(mut decl: String) -> String {
    for (from, to) in [
        ("thread_id", "threadId"),
        ("source_key", "sourceKey"),
        ("visible_to_model", "visibleToModel"),
        ("turn_id", "turnId"),
        ("item_id", "itemId"),
        ("queue_position", "queuePosition"),
        ("request_id", "requestId"),
        ("tool_name", "toolName"),
        ("source_path", "sourcePath"),
        ("call_id", "callId"),
        ("native_id", "nativeId"),
        ("raw_id", "rawId"),
        ("raw_identity", "rawIdentity"),
        ("visible_name", "visibleName"),
        ("artifact_ids", "artifactIds"),
        ("created_at_ms", "createdAtMs"),
        ("updated_at_ms", "updatedAtMs"),
        ("event_type", "eventType"),
    ] {
        decl = decl.replace(from, to);
    }
    decl.replace("bigint", "number")
}

fn schema<T>() -> Result<Value>
where
    T: JsonSchema,
{
    serde_json::to_value(schemars::schema_for!(T)).map_err(Into::into)
}

macro_rules! exported_type {
    ($ty:ty) => {
        ExportedType {
            name: stringify!($ty),
            ts_decl: ts_decl::<$ty>,
            schema: schema::<$ty>,
        }
    };
}

fn exported_types() -> Vec<ExportedType> {
    vec![
        exported_type!(SourceKey),
        exported_type!(GatewaySourceLifetime),
        exported_type!(GatewaySource),
        exported_type!(GatewaySourceInput),
        exported_type!(GatewayRequestScope),
        exported_type!(GatewayThreadSelector),
        exported_type!(BackendKind),
        exported_type!(GatewayBackendInfo),
        exported_type!(GatewayThread),
        exported_type!(GatewayTurn),
        exported_type!(GatewayTurnStatus),
        exported_type!(GatewayInputPart),
        exported_type!(GatewayImageInput),
        exported_type!(GatewayEvent),
        exported_type!(PermissionDecision),
        exported_type!(TimelineItemKind),
        exported_type!(TimelineItemStatus),
        exported_type!(TimelineItem),
        exported_type!(GatewayActivityView),
        exported_type!(PendingPermissionView),
        exported_type!(PendingClarifyView),
        exported_type!(ThreadSnapshot),
        exported_type!(SessionSummaryView),
        exported_type!(InitializeParams),
        exported_type!(InitializeResult),
        exported_type!(ThreadStartParams),
        exported_type!(ThreadResumeParams),
        exported_type!(ThreadReadParams),
        exported_type!(ThreadListParams),
        exported_type!(ThreadIdParams),
        exported_type!(ThreadRenameParams),
        exported_type!(ThreadListResult),
        exported_type!(ThreadMutationResult),
        exported_type!(ThreadDeleteResult),
        exported_type!(TurnStartParams),
        exported_type!(TurnSteerParams),
        exported_type!(TurnInterruptParams),
        exported_type!(TurnStartResult),
        exported_type!(TurnControlResult),
        exported_type!(TurnResultPayload),
        exported_type!(TurnRunResult),
        exported_type!(TurnErrorPayload),
        exported_type!(PermissionRespondParams),
        exported_type!(ClarifyRespondParams),
        exported_type!(InteractionRespondResult),
        exported_type!(SourceResetParams),
        exported_type!(SettingsReadParams),
        exported_type!(SettingsReadResult),
        exported_type!(DebugEventsParams),
        exported_type!(TimelineDebugEvent),
        exported_type!(DebugEventsResult),
        exported_type!(ReadyzResult),
        exported_type!(CreateLaunchParams),
        exported_type!(CreateLaunchResult),
        exported_type!(ManagedServerState),
        exported_type!(JsonRpcId),
        exported_type!(JsonRpcRequest),
        exported_type!(JsonRpcNotification),
        exported_type!(JsonRpcSuccess),
        exported_type!(JsonRpcErrorResponse),
        exported_type!(JsonRpcError),
        exported_type!(ClientRequest),
        exported_type!(ServerNotification),
    ]
}
