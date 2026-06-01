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

pub(crate) use crate::capabilities::CapabilitySnapshot;
pub(crate) use crate::error::{Error, Result};
pub(crate) use crate::messages::{sanitize_message_for_output, sanitize_message_for_tui_history};
pub(crate) use crate::run::normalize_session_title;
pub(crate) use crate::types::{
    MessageAccounting, SanitizedMessageSummary, SessionExportMessageSummary, SessionSummary,
    TuiMessageSummary,
};

pub(crate) const SQLITE_SCHEMA_VERSION: i64 = 15;
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
    pub workdir: &'a Path,
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
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub lineage: Option<Value>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

impl TimelineItemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Assistant => "assistant",
            Self::Reasoning => "reasoning",
            Self::Tool => "tool",
            Self::Shell => "shell",
            Self::File => "file",
            Self::Web => "web",
            Self::Mcp => "mcp",
            Self::Clarify => "clarify",
            Self::Permission => "permission",
            Self::Skill => "skill",
            Self::Agent => "agent",
            Self::Mailbox => "mailbox",
            Self::Status => "status",
            Self::Diff => "diff",
            Self::Artifact => "artifact",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "prompt" => Some(Self::Prompt),
            "assistant" => Some(Self::Assistant),
            "reasoning" => Some(Self::Reasoning),
            "tool" => Some(Self::Tool),
            "shell" => Some(Self::Shell),
            "file" => Some(Self::File),
            "web" => Some(Self::Web),
            "mcp" => Some(Self::Mcp),
            "clarify" => Some(Self::Clarify),
            "permission" => Some(Self::Permission),
            "skill" => Some(Self::Skill),
            "agent" => Some(Self::Agent),
            "mailbox" => Some(Self::Mailbox),
            "status" => Some(Self::Status),
            "diff" => Some(Self::Diff),
            "artifact" => Some(Self::Artifact),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineItemStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    NeedsInput,
    Info,
}

impl TimelineItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::NeedsInput => "needs_input",
            Self::Info => "info",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "needs_input" => Some(Self::NeedsInput),
            "info" => Some(Self::Info),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItemInput {
    pub session_id: String,
    pub item_id: String,
    pub turn_id: Option<String>,
    pub kind: TimelineItemKind,
    pub status: TimelineItemStatus,
    pub source: String,
    pub title: Option<String>,
    pub body_text: Option<String>,
    pub preview_text: Option<String>,
    pub detail_text: Option<String>,
    pub artifact_ids: Vec<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItemRecord {
    pub id: i64,
    pub session_id: String,
    pub item_seq: i64,
    pub item_id: String,
    pub turn_id: Option<String>,
    pub kind: TimelineItemKind,
    pub status: TimelineItemStatus,
    pub source: String,
    pub title: Option<String>,
    pub body_text: Option<String>,
    pub preview_text: Option<String>,
    pub detail_text: Option<String>,
    pub artifact_ids: Vec<String>,
    pub metadata: Option<Value>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineArtifactInput {
    pub session_id: String,
    pub artifact_id: String,
    pub kind: String,
    pub mime_type: Option<String>,
    pub title: Option<String>,
    pub preview_text: Option<String>,
    pub path: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineArtifactRecord {
    pub id: i64,
    pub session_id: String,
    pub artifact_id: String,
    pub kind: String,
    pub mime_type: Option<String>,
    pub title: Option<String>,
    pub preview_text: Option<String>,
    pub path: Option<String>,
    pub metadata: Option<Value>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineDebugEventInput {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub event_type: String,
    pub source: String,
    pub scope: Option<Value>,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineDebugEventRecord {
    pub id: i64,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub event_type: String,
    pub source: String,
    pub scope: Option<Value>,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub payload: Option<Value>,
    pub created_at_ms: i64,
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
#[path = "store/capability_snapshots.rs"]
pub(crate) mod store_capability_snapshots;
#[allow(unused_imports)]
use store_capability_snapshots::*;
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
#[path = "store/gateway_bindings.rs"]
pub(crate) mod store_gateway_bindings;
#[path = "store/timeline.rs"]
pub(crate) mod store_timeline;
#[allow(unused_imports)]
use store_timeline::*;
#[path = "store/lifecycle.rs"]
pub(crate) mod store_lifecycle;
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
