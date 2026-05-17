use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use psychevo_agent_core::{AssistantBlock, Message, TerminalReason, now_ms};
use psychevo_ai::Outcome;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::messages::{sanitize_message_for_output, sanitize_message_for_tui_history};
use crate::run::normalize_session_title;
use crate::types::{
    MessageAccounting, SanitizedMessageSummary, SessionExportMessageSummary, SessionSummary,
    TuiMessageSummary,
};

const SQLITE_SCHEMA_VERSION: i64 = 9;
const SESSION_REVERT_METADATA_KEY: &str = "revert";
const MESSAGE_UNDO_METADATA_KEY: &str = "undo";
const MESSAGE_PRE_SNAPSHOT_KEY: &str = "pre_snapshot";

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

#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

// Store internals are split by schema, session, message, undo, and row-helper concerns.
include!("store/schema.rs");
include!("store/sessions.rs");
include!("store/undo_state.rs");
include!("store/messages.rs");
include!("store/context_evidence.rs");
include!("store/prompt_prefix.rs");
include!("store/agents.rs");
include!("store/lifecycle.rs");
include!("store/retry.rs");
include!("store/schema_helpers.rs");
include!("store/message_fields.rs");
include!("store/metadata.rs");
include!("store/undo_helpers.rs");
