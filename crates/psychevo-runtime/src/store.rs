pub(crate) use std::collections::BTreeSet;
pub(crate) use std::fs;
pub(crate) use std::path::Path;
pub(crate) use std::sync::atomic::{AtomicUsize, Ordering};
pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use std::thread;
pub(crate) use std::time::Duration;

pub(crate) use psychevo_agent_core::{
    AssistantBlock, Message, TerminalReason, now_ms, user_text_message,
};
pub(crate) use psychevo_ai::Outcome;
pub(crate) use rusqlite::{Connection, OptionalExtension, params};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::{Map, Value, json};
pub(crate) use uuid::Uuid;

pub(crate) use crate::error::{Error, Result};
pub(crate) use crate::messages::{sanitize_message_for_output, sanitize_message_for_tui_history};
pub(crate) use crate::run::normalize_session_title;
pub(crate) use crate::types::{
    CostStatus, MessageAccounting, SanitizedMessageSummary, SessionExportMessageSummary,
    SessionSummary, TuiMessageSummary,
};

pub(crate) const SQLITE_SCHEMA_VERSION: i64 = 26;
pub(crate) const MIN_SUPPORTED_SQLITE_SCHEMA_VERSION: i64 = 24;
pub(crate) const SESSION_REVERT_METADATA_KEY: &str = "revert";
pub(crate) const MESSAGE_UNDO_METADATA_KEY: &str = "undo";
pub(crate) const MESSAGE_PRE_SNAPSHOT_KEY: &str = "pre_snapshot";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRevertState {
    pub start_seq: i64,
    pub original_snapshot: String,
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
    pub max_context_messages: Option<usize>,
    pub inherited_message_metadata: Value,
    pub boundary_text: &'a str,
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
    pub draft_runtime_ref: Option<String>,
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
    pub draft_runtime_ref: Option<&'a str>,
    pub lineage: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewaySourceLaneRecord {
    pub source_key: String,
    pub source_kind: String,
    pub raw_identity: Value,
    pub visible_name: Option<String>,
    pub thread_id: Option<String>,
    pub draft_runtime_ref: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub lineage: Option<Value>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRuntimeBindingRecord {
    pub thread_id: String,
    pub status: GatewayRuntimeBindingStatus,
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
    pub unresolved_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayActivityClaimInput<'a> {
    pub activity_id: &'a str,
    pub thread_id: Option<&'a str>,
    pub source_key: Option<&'a str>,
    pub turn_id: Option<&'a str>,
    pub kind: &'a str,
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
    pub kind: String,
    pub status: String,
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

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayLiveEventRecord {
    pub seq: i64,
    pub activity_id: Option<String>,
    pub owner_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub event: Value,
    pub created_at_ms: i64,
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
    pub command_kind: &'a str,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayControlCommandRecord {
    pub id: i64,
    pub activity_id: String,
    pub owner_id: String,
    pub command_kind: String,
    pub status: String,
    pub payload: Value,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayTurnTerminalInput<'a> {
    pub turn_id: &'a str,
    pub thread_id: &'a str,
    pub status: &'a str,
    pub outcome: Option<&'a str>,
    pub error_message: Option<&'a str>,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: i64,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayTurnTerminalRecord {
    pub turn_id: String,
    pub thread_id: String,
    pub status: String,
    pub outcome: Option<String>,
    pub error_message: Option<String>,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: i64,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationTaskInput {
    pub id: Option<String>,
    pub cwd: String,
    pub kind: String,
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
    pub kind: String,
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
    pub last_status: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationRunRecord {
    pub id: String,
    pub automation_id: String,
    pub trigger: String,
    pub status: String,
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
    pub status: &'a str,
    pub thread_id: Option<&'a str>,
    pub source_key: Option<&'a str>,
    pub error: Option<&'a str>,
    pub metadata: Option<Value>,
    pub next_run_at_ms: Option<i64>,
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
pub struct SqliteStore {
    pub(crate) inner: Arc<SqliteStoreInner>,
}

pub(crate) struct SqliteStoreInner {
    pub(crate) conn: Mutex<Connection>,
    pub(crate) successful_writes: AtomicUsize,
}

// Store internals are split by schema, session, message, undo, and row-helper concerns.
#[path = "store/schema.rs"]
pub(crate) mod store_schema;
#[allow(unused_imports)]
use store_schema::*;
#[path = "store/sessions.rs"]
pub(crate) mod store_sessions;
#[allow(unused_imports)]
use store_sessions::*;
#[path = "store/undo_state.rs"]
pub(crate) mod store_undo_state;
#[allow(unused_imports)]
use store_undo_state::*;
#[path = "store/messages.rs"]
pub(crate) mod store_messages;
#[allow(unused_imports)]
use store_messages::*;
#[path = "store/context_evidence.rs"]
pub(crate) mod store_context_evidence;
#[allow(unused_imports)]
use store_context_evidence::*;
#[path = "store/prompt_prefix.rs"]
pub(crate) mod store_prompt_prefix;
#[allow(unused_imports)]
use store_prompt_prefix::*;
#[path = "store/agents.rs"]
pub(crate) mod store_agents;
pub use store_agents::*;
#[path = "store/agent_mailbox.rs"]
pub(crate) mod store_agent_mailbox;
#[allow(unused_imports)]
use store_agent_mailbox::*;
#[path = "store/compactions.rs"]
pub(crate) mod store_compactions;
#[allow(unused_imports)]
use store_compactions::*;
#[path = "store/automations.rs"]
pub(crate) mod store_automations;
#[path = "store/gateway_activity.rs"]
pub(crate) mod store_gateway_activity;
#[path = "store/gateway_bindings.rs"]
pub(crate) mod store_gateway_bindings;
#[path = "store/lifecycle.rs"]
pub(crate) mod store_lifecycle;
#[path = "store/runtime_bindings.rs"]
pub(crate) mod store_runtime_bindings;
#[allow(unused_imports)]
use store_lifecycle::*;
#[path = "store/retry.rs"]
pub(crate) mod store_retry;
#[allow(unused_imports)]
use store_retry::*;
#[path = "store/schema_helpers.rs"]
pub(crate) mod store_schema_helpers;
#[allow(unused_imports)]
use store_schema_helpers::*;
#[path = "store/message_fields.rs"]
pub(crate) mod store_message_fields;
#[allow(unused_imports)]
use store_message_fields::*;
#[path = "store/metadata.rs"]
pub(crate) mod store_metadata;
#[allow(unused_imports)]
use store_metadata::*;
#[path = "store/undo_helpers.rs"]
pub(crate) mod store_undo_helpers;
#[allow(unused_imports)]
use store_undo_helpers::*;
