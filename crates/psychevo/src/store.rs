use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use psychevo_agent_core::Message;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session_trace::{
    SessionTraceReadOptions, SessionTraceReadResult, read_session_trace, remove_session_trace_dir,
};
use crate::types::SessionSummary;

pub(crate) const SQLITE_SCHEMA_VERSION: i64 = 32;
pub(crate) const MIN_SUPPORTED_SQLITE_SCHEMA_VERSION: i64 = 29;
pub(crate) const SESSION_REVERT_METADATA_KEY: &str = "revert";
pub(crate) const MESSAGE_UNDO_METADATA_KEY: &str = "undo";
pub(crate) const MESSAGE_PRE_SNAPSHOT_KEY: &str = "pre_snapshot";

fn invalid_persisted_domain_value(table: &str, field: &str, value: &str) -> crate::Error {
    crate::Error::structured(
        "Persisted domain value is invalid.",
        serde_json::json!({
            "kind": "invalid_persisted_domain_value",
            "table": table,
            "field": field,
            "value": value,
        }),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConversationDraftPart {
    Text { text: String },
    LocalImage { path: String },
    ImageUrl { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRevertKind {
    WorkspaceUndo {
        original_snapshot: String,
    },
    ConversationEdit {
        boundary_message_id: String,
        draft: Vec<ConversationDraftPart>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRevertState {
    pub start_seq: i64,
    pub kind: SessionRevertKind,
}

#[derive(Debug, Clone, Copy)]
pub struct NativeSessionForkInput<'a> {
    pub source_session_id: &'a str,
    pub before_session_seq: Option<i64>,
}

impl SessionRevertState {
    pub fn workspace_undo(start_seq: i64, original_snapshot: String) -> Self {
        Self {
            start_seq,
            kind: SessionRevertKind::WorkspaceUndo { original_snapshot },
        }
    }

    pub fn conversation_edit(
        start_seq: i64,
        boundary_message_id: String,
        draft: Vec<ConversationDraftPart>,
    ) -> Self {
        Self {
            start_seq,
            kind: SessionRevertKind::ConversationEdit {
                boundary_message_id,
                draft,
            },
        }
    }

    pub fn original_snapshot(&self) -> Option<&str> {
        match &self.kind {
            SessionRevertKind::WorkspaceUndo { original_snapshot } => Some(original_snapshot),
            SessionRevertKind::ConversationEdit { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoTarget {
    pub seq: i64,
    pub prompt: String,
    pub snapshot: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextEvidenceInput {
    pub role: String,
    pub source_kind: String,
    pub source_name: Option<String>,
    pub source_path: Option<String>,
    pub provider_group: Option<String>,
    pub provider_block_index: Option<i64>,
    pub context_kind: Option<String>,
    pub content_text: String,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextEvidenceRecord {
    pub id: i64,
    pub session_id: String,
    pub prompt_session_seq: i64,
    pub context_seq: i64,
    pub role: String,
    pub source_kind: String,
    pub source_name: Option<String>,
    pub source_path: Option<String>,
    pub provider_group: Option<String>,
    pub provider_block_index: Option<i64>,
    pub context_kind: Option<String>,
    pub timestamp_ms: i64,
    pub content_text: String,
    pub metadata: Option<Value>,
}

pub struct ChildSessionSnapshotInput<'a> {
    pub parent_session_id: &'a str,
    pub cwd: &'a Path,
    pub source: &'a str,
    pub model: &'a str,
    pub provider: &'a str,
    pub metadata: Option<Value>,
    pub inherited_message_metadata: Value,
    pub boundary_text: &'a str,
    pub runtime_binding: Option<ChildSessionRuntimeBindingSnapshotInput<'a>>,
}

pub struct ChildSessionRuntimeBindingSnapshotInput<'a> {
    pub expected_binding_revision: i64,
    pub expected_control_revision: i64,
    pub effective_controls: &'a BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewaySourceBindingInput<'a> {
    pub source_key: &'a str,
    pub source_kind: &'a str,
    pub raw_identity: Value,
    pub visible_name: Option<&'a str>,
    pub thread_id: &'a str,
    pub backend_kind: &'a str,
    pub backend_native_id: Option<&'a str>,
    pub lineage: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewaySourceBindingRecord {
    pub source_key: String,
    pub source_kind: String,
    pub raw_identity: Value,
    pub visible_name: Option<String>,
    pub thread_id: String,
    pub backend_kind: String,
    pub backend_native_id: Option<String>,
    pub draft_agent_ref: Option<String>,
    pub draft_profile_ref: Option<String>,
    pub draft_control_values: BTreeMap<String, String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub lineage: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewaySourceLaneInput<'a> {
    pub source_key: &'a str,
    pub source_kind: &'a str,
    pub raw_identity: Value,
    pub visible_name: Option<&'a str>,
    pub thread_id: Option<&'a str>,
    pub draft_agent_ref: Option<&'a str>,
    pub draft_profile_ref: Option<&'a str>,
    pub draft_control_values: &'a BTreeMap<String, String>,
    pub lineage: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewaySourceLaneRecord {
    pub source_key: String,
    pub source_kind: String,
    pub raw_identity: Value,
    pub visible_name: Option<String>,
    pub thread_id: Option<String>,
    pub draft_agent_ref: Option<String>,
    pub draft_profile_ref: Option<String>,
    pub draft_control_values: BTreeMap<String, String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub lineage: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayTurnDeliveryInput<'a> {
    pub turn_id: &'a str,
    pub thread_id: &'a str,
    pub runtime_ref: &'a str,
    pub input_json: &'a str,
    pub input_hash: &'a str,
}

pub(crate) struct ExistingFrameworkThreadTurnInput<'a> {
    pub delivery: GatewayTurnDeliveryInput<'a>,
    pub client_turn_id: Option<&'a str>,
    pub runtime_binding: Option<GatewayRuntimeBindingInput<'a>>,
    pub initial_thread_preferences: &'a BTreeMap<String, Value>,
    pub mission: Option<crate::application::AgentMissionRegistration>,
}

pub(crate) struct NewFrameworkThreadTurnInput<'a> {
    pub thread_id: &'a str,
    pub cwd: &'a Path,
    pub source: &'a str,
    pub metadata: Option<Value>,
    pub delivery: GatewayTurnDeliveryInput<'a>,
    pub client_turn_id: Option<&'a str>,
    pub source_lane: Option<GatewaySourceLaneInput<'a>>,
    pub runtime_binding: Option<GatewayRuntimeBindingInput<'a>>,
    pub initial_thread_preferences: &'a BTreeMap<String, Value>,
    pub mission: Option<crate::application::AgentMissionRegistration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayTurnDeliveryRecord {
    pub turn_id: String,
    pub thread_id: String,
    pub runtime_ref: String,
    pub status: GatewayTurnDeliveryStatus,
    pub input_json: Option<String>,
    pub input_hash: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub delivery_confirmed_at_ms: Option<i64>,
    pub terminal_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayTurnDeliveryStatus {
    NotDelivered,
    Delivered,
    Unknown,
    Terminal,
}

impl GatewayTurnDeliveryStatus {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "not_delivered" => Some(Self::NotDelivered),
            "delivered" => Some(Self::Delivered),
            "unknown" => Some(Self::Unknown),
            "terminal" => Some(Self::Terminal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayChannelOutboxInput<'a> {
    pub delivery_id: &'a str,
    pub thread_id: &'a str,
    pub turn_id: &'a str,
    pub connection_id: &'a str,
    pub source_key: &'a str,
    pub payload_text: &'a str,
    pub payload_hash: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayChannelOutboxRecord {
    pub delivery_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub connection_id: String,
    pub source_key: String,
    pub status: GatewayChannelOutboxStatus,
    pub payload_text: Option<String>,
    pub payload_hash: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub acknowledged_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayChannelOutboxStatus {
    Pending,
    Acknowledged,
    Failed,
}

impl GatewayChannelOutboxStatus {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "acknowledged" => Some(Self::Acknowledged),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayRuntimeBindingStatus {
    Resolved,
    Unresolved,
}

impl GatewayRuntimeBindingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Unresolved => "unresolved",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "resolved" => Some(Self::Resolved),
            "unresolved" => Some(Self::Unresolved),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayRuntimeBindingOwnership {
    ReadWrite,
    ReadOnly,
}

impl GatewayRuntimeBindingOwnership {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadWrite => "read_write",
            Self::ReadOnly => "read_only",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "read_write" => Some(Self::ReadWrite),
            "read_only" => Some(Self::ReadOnly),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRuntimeBindingInput<'a> {
    pub thread_id: &'a str,
    pub agent_ref: Option<&'a str>,
    pub agent_fingerprint: &'a str,
    pub agent_definition_json: &'a str,
    pub runtime_ref: &'a str,
    pub backend_kind: &'a str,
    pub native_kind: &'a str,
    pub native_session_id: Option<&'a str>,
    pub cwd: &'a str,
    pub profile_fingerprint: &'a str,
    pub profile_revision: &'a str,
    pub profile_config_json: &'a str,
    pub adapter_kind: &'a str,
    pub adapter_revision: &'a str,
    pub ownership: GatewayRuntimeBindingOwnership,
    pub parent_thread_id: Option<&'a str>,
}

pub(crate) struct AgentThreadImportCommitInput<'a> {
    pub(crate) thread_id: &'a str,
    pub(crate) parent_thread_id: Option<&'a str>,
    pub(crate) cwd: &'a Path,
    pub(crate) source: &'a str,
    pub(crate) binding: GatewayRuntimeBindingInput<'a>,
    pub(crate) messages: &'a [AgentThreadImportMessageInput<'a>],
    pub(crate) metadata: &'a BTreeMap<String, Value>,
    pub(crate) title: Option<&'a str>,
}

pub(crate) struct AgentThreadImportMessageInput<'a> {
    pub(crate) message: &'a Message,
    pub(crate) usage: &'a Option<Value>,
    pub(crate) metadata: &'a Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentThreadImportCommit {
    Published,
    Existing { thread_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRuntimeBindingRecord {
    pub thread_id: String,
    pub status: GatewayRuntimeBindingStatus,
    pub agent_ref: Option<String>,
    pub agent_fingerprint: Option<String>,
    pub agent_definition_json: Option<String>,
    pub runtime_ref: Option<String>,
    pub backend_kind: Option<String>,
    pub native_kind: Option<String>,
    pub native_session_id: Option<String>,
    pub cwd: String,
    pub profile_fingerprint: Option<String>,
    pub profile_revision: Option<String>,
    pub profile_config_json: Option<String>,
    pub adapter_kind: Option<String>,
    pub adapter_revision: Option<String>,
    pub ownership: GatewayRuntimeBindingOwnership,
    pub parent_thread_id: Option<String>,
    pub binding_revision: i64,
    pub thread_preferences: BTreeMap<String, Value>,
    pub runtime_observed: BTreeMap<String, Value>,
    pub control_revision: i64,
    pub unresolved_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct GatewayRuntimeControlStatePatch<'a> {
    /// Replaces the complete stored Thread preference map when present.
    pub thread_preferences: Option<&'a BTreeMap<String, Value>>,
    /// Replaces the complete Adapter-observed map when present.
    pub runtime_observed: Option<&'a BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayActivityClaimInput<'a> {
    pub activity_id: &'a str,
    pub thread_id: Option<&'a str>,
    pub source_key: Option<&'a str>,
    pub turn_id: Option<&'a str>,
    pub kind: GatewayActivityKind,
    pub owner_id: &'a str,
    pub owner_surface: Option<&'a str>,
    pub lease_expires_at_ms: i64,
    pub queued_turns: usize,
    pub superseded_activity_id: Option<&'a str>,
    pub intent: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayActivityRecord {
    pub activity_id: String,
    pub thread_id: Option<String>,
    pub source_key: Option<String>,
    pub turn_id: Option<String>,
    pub kind: GatewayActivityKind,
    pub status: GatewayActivityState,
    pub owner_id: String,
    pub owner_surface: Option<String>,
    pub generation: i64,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
    pub lease_expires_at_ms: i64,
    pub queued_turns: usize,
    pub superseded_activity_id: Option<String>,
    pub intent: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayActivityKind {
    Turn,
    Shell,
}

impl GatewayActivityKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Shell => "shell",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "turn" => Some(Self::Turn),
            "shell" => Some(Self::Shell),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayActivityState {
    Running,
    Queued,
    Superseded,
    Completed,
    Failed,
    Interrupted,
}

impl GatewayActivityState {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "running" => Some(Self::Running),
            "queued" => Some(Self::Queued),
            "superseded" => Some(Self::Superseded),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayActivityTerminalStatus {
    Completed,
    Failed,
    Interrupted,
}

impl GatewayActivityTerminalStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayTurnStartReceiptRecord {
    pub(crate) client_turn_id: String,
    pub(crate) turn_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionListProjection {
    pub(crate) summary: SessionSummary,
    pub(crate) first_user_text: Option<String>,
    pub(crate) metadata: Option<Value>,
    pub(crate) runtime_backend_kind: Option<String>,
    pub(crate) runtime_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionListCursor {
    pub(crate) updated_at_ms: i64,
    pub(crate) id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionSummaryPage {
    pub(crate) summaries: Vec<SessionSummary>,
    pub(crate) next_cursor: Option<SessionListCursor>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionListProjectionPage {
    pub(crate) sessions: Vec<SessionListProjection>,
    pub(crate) next_cursor: Option<SessionListCursor>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionBrowserRequest<'a> {
    pub(crate) cwd: Option<&'a str>,
    pub(crate) archived: bool,
    pub(crate) cursor_cwd: Option<&'a str>,
    pub(crate) cursor_offset: usize,
    pub(crate) limit: usize,
    pub(crate) recent_since_ms: i64,
    pub(crate) include_session_ids: &'a [String],
    pub(crate) active_session_ids: &'a [String],
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionBrowserWorkspaceProjection {
    pub(crate) cwd: String,
    pub(crate) sessions: Vec<SessionListProjection>,
    pub(crate) hidden_count: usize,
    pub(crate) next_offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayLiveEventRecord {
    pub seq: i64,
    pub activity_id: Option<String>,
    pub owner_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub event: Value,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayLiveEventCommit {
    pub seq: i64,
    pub idempotency_key: Option<String>,
    pub inserted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayLiveSnapshotInput<'a> {
    pub snapshot_key: &'a str,
    pub activity_id: Option<&'a str>,
    pub owner_id: Option<&'a str>,
    pub thread_id: Option<&'a str>,
    pub turn_id: Option<&'a str>,
    pub event_kind: &'a str,
    pub event: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayLiveSnapshotRecord {
    pub snapshot_key: String,
    pub activity_id: Option<String>,
    pub owner_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub event_kind: String,
    pub event: Value,
    pub revision: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayControlCommandInput<'a> {
    pub activity_id: &'a str,
    pub owner_id: &'a str,
    pub command_kind: GatewayControlCommandKind,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayControlCommandRecord {
    pub id: i64,
    pub activity_id: String,
    pub owner_id: String,
    pub command_kind: GatewayControlCommandKind,
    pub status: GatewayControlCommandStatus,
    pub payload: Value,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayControlCommandKind {
    Interrupt,
    Steer,
    Permission,
    Clarify,
}

impl GatewayControlCommandKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Interrupt => "interrupt",
            Self::Steer => "steer",
            Self::Permission => "permission",
            Self::Clarify => "clarify",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "interrupt" => Some(Self::Interrupt),
            "steer" => Some(Self::Steer),
            "permission" => Some(Self::Permission),
            "clarify" => Some(Self::Clarify),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayControlCommandStatus {
    Pending,
    Applying,
    Applied,
    Failed,
    OutcomeIndeterminate,
}

impl GatewayControlCommandStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applying => "applying",
            Self::Applied => "applied",
            Self::Failed => "failed",
            Self::OutcomeIndeterminate => "outcome_indeterminate",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "applying" => Some(Self::Applying),
            "applied" => Some(Self::Applied),
            "failed" => Some(Self::Failed),
            "outcome_indeterminate" => Some(Self::OutcomeIndeterminate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayTurnTerminalInput<'a> {
    pub turn_id: &'a str,
    pub thread_id: &'a str,
    pub status: crate::application::FrameworkTurnTerminalStatus,
    pub outcome: Option<crate::application::FrameworkTurnTerminalOutcome>,
    pub error_message: Option<&'a str>,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: i64,
    /// Last committed message sequence visible when this terminal settles.
    /// `None` captures the Thread's current boundary in the same transaction.
    pub boundary_session_seq: Option<i64>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayTurnTerminalRecord {
    pub turn_id: String,
    pub thread_id: String,
    pub status: crate::application::FrameworkTurnTerminalStatus,
    pub outcome: Option<crate::application::FrameworkTurnTerminalOutcome>,
    pub error_message: Option<String>,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: i64,
    pub boundary_session_seq: i64,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrameworkInteractionRecord {
    pub interaction_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub kind: crate::types::BlockingActionKind,
    pub status: FrameworkInteractionStatus,
    pub payload: Value,
    pub resolution: Option<Value>,
    pub requested_at_ms: i64,
    pub resolved_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameworkInteractionStatus {
    Pending,
    Resolved,
    Cancelled,
}

impl FrameworkInteractionStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Resolved => "resolved",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "resolved" => Some(Self::Resolved),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationTaskInput {
    pub id: Option<String>,
    pub cwd: String,
    pub kind: AutomationTaskKind,
    pub target_thread_id: Option<String>,
    pub title: String,
    pub prompt: String,
    pub schedule: Value,
    pub enabled: bool,
    pub execution: Value,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub source_key: Option<String>,
    pub next_run_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationTaskRecord {
    pub id: String,
    pub cwd: String,
    pub kind: AutomationTaskKind,
    pub target_thread_id: Option<String>,
    pub title: String,
    pub prompt: String,
    pub schedule: Value,
    pub enabled: bool,
    pub execution: Value,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub source_key: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub last_run_at_ms: Option<i64>,
    pub next_run_at_ms: Option<i64>,
    pub last_status: Option<AutomationRunStatus>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationRunRecord {
    pub id: String,
    pub automation_id: String,
    pub trigger: String,
    pub status: AutomationRunStatus,
    pub started_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub thread_id: Option<String>,
    pub source_key: Option<String>,
    pub error: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationRunRecoveryCandidate {
    pub task: AutomationTaskRecord,
    pub run: AutomationRunRecord,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationRunFinishInput<'a> {
    pub run_id: &'a str,
    pub status: AutomationRunTerminalStatus,
    pub thread_id: Option<&'a str>,
    pub source_key: Option<&'a str>,
    pub error: Option<&'a str>,
    pub metadata: Option<Value>,
    pub next_run_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationTaskKind {
    Project,
    ThreadHeartbeat,
}

impl AutomationTaskKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::ThreadHeartbeat => "thread_heartbeat",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "project" => Some(Self::Project),
            "thread_heartbeat" => Some(Self::ThreadHeartbeat),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationRunStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationRunTerminalStatus {
    Completed,
    Failed,
    Interrupted,
}

impl AutomationRunTerminalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

impl AutomationRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptPrefixSlotRecord {
    pub slot: String,
    pub tier: String,
    pub semantic_role: String,
    pub provider_role: String,
    pub order: usize,
    pub content: String,
    pub content_hash: String,
    pub source_kind: Option<String>,
    pub source_name: Option<String>,
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPrefixRecord {
    pub session_id: String,
    pub version: i64,
    pub created_at_ms: i64,
    pub provider: String,
    pub model: String,
    pub prefix_hash: String,
    pub tool_declarations_hash: String,
    pub invalidation_reason: Option<String>,
    pub slots: Vec<PromptPrefixSlotRecord>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentMailboxEventRecord {
    pub id: i64,
    pub parent_session_id: String,
    pub child_session_id: Option<String>,
    pub agent_id: String,
    pub task_name: Option<String>,
    pub agent_name: String,
    pub created_at_ms: i64,
    pub delivered_at_ms: Option<i64>,
    pub delivered_prompt_session_seq: Option<i64>,
    pub delivered_after_session_seq: Option<i64>,
    pub delivered_tool_call_id: Option<String>,
    pub content_text: String,
    pub payload: Value,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentMailboxEventInput {
    pub parent_session_id: String,
    pub child_session_id: Option<String>,
    pub agent_id: String,
    pub task_name: Option<String>,
    pub agent_name: String,
    pub content_text: String,
    pub payload: Value,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionCompactionInput {
    pub session_id: String,
    pub reason: String,
    pub summary_text: String,
    pub first_kept_session_seq: i64,
    pub created_after_session_seq: i64,
    pub tokens_before: Option<u64>,
    pub tokens_after: Option<u64>,
    pub summary_provider: String,
    pub summary_model: String,
    pub instructions: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionCompactionRecord {
    pub id: i64,
    pub session_id: String,
    pub created_at_ms: i64,
    pub reason: String,
    pub summary_text: String,
    pub first_kept_session_seq: i64,
    pub created_after_session_seq: i64,
    pub tokens_before: Option<u64>,
    pub tokens_after: Option<u64>,
    pub summary_provider: String,
    pub summary_model: String,
    pub instructions: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionMessageRecord {
    pub session_seq: i64,
    pub message: Message,
}

#[derive(Clone)]
pub struct StateRuntime {
    pub(crate) inner: Arc<StateRuntimeInner>,
}

pub(crate) struct StateRuntimeInner {
    pub(crate) db_path: PathBuf,
    pub(crate) pool: sqlx::SqlitePool,
    pub(crate) connection_limit: u32,
    pub(crate) in_flight_operations: AtomicU64,
    pub(crate) completed_operations: AtomicU64,
    pub(crate) failed_operations: AtomicU64,
    pub(crate) busy_operations: AtomicU64,
    pub(crate) acquire_latency_micros: AtomicU64,
    pub(crate) execute_latency_micros: AtomicU64,
    pub(crate) filesystem_grants: Mutex<HashMap<String, crate::sandbox::SandboxWriteGrants>>,
    #[cfg(test)]
    pub(crate) fail_next_framework_terminal: AtomicU64,
    #[cfg(test)]
    pub(crate) fail_next_agent_terminal: AtomicU64,
    #[cfg(test)]
    pub(crate) fail_next_agent_thread_import_commit: AtomicU64,
    #[cfg(test)]
    pub(crate) gateway_turn_acceptance_barrier:
        Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
    #[cfg(test)]
    pub(crate) native_history_fork_barrier:
        Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
    #[cfg(test)]
    pub(crate) state_close_barrier:
        Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateRuntimeDiagnostics {
    pub connection_limit: u32,
    pub pool_size: u32,
    pub pool_idle: usize,
    pub in_flight_operations: u64,
    pub completed_operations: u64,
    pub failed_operations: u64,
    pub busy_operations: u64,
    pub acquire_latency_micros: u64,
    pub execute_latency_micros: u64,
}

impl StateRuntime {
    pub fn db_path(&self) -> &Path {
        &self.inner.db_path
    }

    pub fn diagnostics(&self) -> StateRuntimeDiagnostics {
        let completed_operations = self
            .inner
            .completed_operations
            .load(std::sync::atomic::Ordering::Relaxed);
        let failed_operations = self
            .inner
            .failed_operations
            .load(std::sync::atomic::Ordering::Relaxed);
        StateRuntimeDiagnostics {
            connection_limit: self.inner.connection_limit,
            pool_size: self.inner.pool.size(),
            pool_idle: self.inner.pool.num_idle(),
            in_flight_operations: self
                .inner
                .in_flight_operations
                .load(std::sync::atomic::Ordering::Relaxed),
            completed_operations,
            failed_operations,
            busy_operations: self
                .inner
                .busy_operations
                .load(std::sync::atomic::Ordering::Relaxed),
            acquire_latency_micros: self
                .inner
                .acquire_latency_micros
                .load(std::sync::atomic::Ordering::Relaxed),
            execute_latency_micros: self
                .inner
                .execute_latency_micros
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    pub fn read_session_trace(
        &self,
        session_id: &str,
        options: SessionTraceReadOptions,
    ) -> SessionTraceReadResult {
        read_session_trace(self.db_path(), session_id, options)
    }

    pub(crate) fn filesystem_grants(&self, session_id: &str) -> crate::sandbox::SandboxWriteGrants {
        let mut grants = self
            .inner
            .filesystem_grants
            .lock()
            .expect("filesystem grant map poisoned");
        grants.entry(session_id.to_string()).or_default().clone()
    }

    pub(crate) fn turn_filesystem_grant_guard(
        &self,
        session_id: impl Into<String>,
    ) -> TurnFilesystemGrantGuard {
        TurnFilesystemGrantGuard {
            state: self.clone(),
            session_id: session_id.into(),
        }
    }

    fn clear_turn_filesystem_grants(&self, session_id: &str) {
        if let Ok(grants) = self.inner.filesystem_grants.lock()
            && let Some(grants) = grants.get(session_id)
        {
            grants.clear_turn_scopes();
        }
    }

    pub(crate) fn clear_session_filesystem_grants(&self, session_id: &str) {
        if let Ok(mut grants) = self.inner.filesystem_grants.lock()
            && let Some(grants) = grants.remove(session_id)
        {
            grants.clear_session_scopes();
        }
    }

    pub(crate) fn remove_session_trace(&self, session_id: &str) {
        let _ = remove_session_trace_dir(self.db_path(), session_id);
    }
}

pub(crate) const DEFAULT_STATE_CONNECTION_LIMIT: u32 = 5;

pub(crate) struct TurnFilesystemGrantGuard {
    state: StateRuntime,
    session_id: String,
}

impl Drop for TurnFilesystemGrantGuard {
    fn drop(&mut self) {
        self.state.clear_turn_filesystem_grants(&self.session_id);
    }
}

impl fmt::Debug for StateRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateRuntime")
            .field("db_path", &self.inner.db_path)
            .finish_non_exhaustive()
    }
}

// Store internals are split by schema, session, message, undo, and row-helper concerns.
#[path = "store/agents.rs"]
pub(crate) mod store_agents;
#[path = "store/context_evidence.rs"]
pub(crate) mod store_context_evidence;
#[path = "store/history_fork.rs"]
pub(crate) mod store_history_fork;
#[path = "store/messages.rs"]
pub(crate) mod store_messages;
#[path = "store/prompt_prefix.rs"]
pub(crate) mod store_prompt_prefix;
#[path = "store/schema.rs"]
pub(crate) mod store_schema;
#[path = "store/sessions.rs"]
pub(crate) mod store_sessions;
#[path = "store/undo_state.rs"]
pub(crate) mod store_undo_state;
#[cfg(test)]
pub(crate) use store_agents::{
    AgentCoordinationRunStatus, AgentMissionRunInput, AgentTeamRunInput,
};
pub(crate) use store_agents::{
    AgentEdgeRecord, AgentEdgeStatus, AgentMissionRunRecord, AgentTeamRunRecord,
};
#[path = "store/agent_mailbox.rs"]
pub(crate) mod store_agent_mailbox;
#[path = "store/agent_thread_import.rs"]
pub(crate) mod store_agent_thread_import;
#[path = "store/automations.rs"]
pub(crate) mod store_automations;
#[path = "store/compactions.rs"]
pub(crate) mod store_compactions;
#[path = "store/framework_interactions.rs"]
pub(crate) mod store_framework_interactions;
#[path = "store/gateway_activity.rs"]
pub(crate) mod store_gateway_activity;
#[path = "store/gateway_bindings.rs"]
pub(crate) mod store_gateway_bindings;
#[path = "store/gateway_control.rs"]
pub(crate) mod store_gateway_control;
#[path = "store/gateway_live_state.rs"]
pub(crate) mod store_gateway_live_state;
#[path = "store/lifecycle.rs"]
pub(crate) mod store_lifecycle;
#[path = "store/message_fields.rs"]
pub(crate) mod store_message_fields;
#[path = "store/metadata.rs"]
pub(crate) mod store_metadata;
#[path = "store/runtime_bindings.rs"]
pub(crate) mod store_runtime_bindings;
#[path = "store/sqlx_runtime.rs"]
pub(crate) mod store_sqlx_runtime;
#[path = "store/turn_delivery.rs"]
pub(crate) mod store_turn_delivery;
#[path = "store/undo_helpers.rs"]
pub(crate) mod store_undo_helpers;

#[cfg(test)]
mod state_runtime_tests {
    use super::*;
    use crate::types::{FilesystemApprovalLifetime, FilesystemApprovalScope};

    #[tokio::test]
    async fn filesystem_grants_follow_turn_and_session_lifecycles() {
        let temp = tempfile::tempdir().expect("temp");
        let turn_root = temp.path().join("turn");
        let session_root = temp.path().join("session");
        std::fs::create_dir_all(&turn_root).expect("turn root");
        std::fs::create_dir_all(&session_root).expect("session root");
        let state = StateRuntime::open(temp.path().join("state.db"))
            .await
            .expect("state");
        let grants = state.filesystem_grants("session-1");
        let turn_guard = state.turn_filesystem_grant_guard("session-1");
        grants
            .grant_scope(&FilesystemApprovalScope {
                directory: turn_root.display().to_string(),
                lifetime: FilesystemApprovalLifetime::Turn,
            })
            .expect("turn grant");
        grants
            .grant_scope(&FilesystemApprovalScope {
                directory: session_root.display().to_string(),
                lifetime: FilesystemApprovalLifetime::Session,
            })
            .expect("session grant");

        drop(turn_guard);

        assert_eq!(
            grants.scoped_roots(),
            vec![crate::host_paths::normalized_native_path(
                &session_root.canonicalize().unwrap()
            )]
        );
        state.clear_session_filesystem_grants("session-1");
        assert!(grants.scoped_roots().is_empty());
    }
}
