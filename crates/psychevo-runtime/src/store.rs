use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use psychevo_agent_core::{AssistantBlock, Message, now_ms};
use psychevo_ai::Outcome;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::messages::{sanitize_message_for_output, sanitize_message_for_tui_history};
use crate::run::normalize_session_title;
use crate::types::{SanitizedMessageSummary, SessionSummary, TuiMessageSummary};

const SQLITE_SCHEMA_VERSION: i64 = 4;
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

#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

// Store internals are split by schema, session, message, undo, and row-helper concerns.
include!("store/schema.rs");
include!("store/sessions.rs");
include!("store/undo_state.rs");
include!("store/messages.rs");
include!("store/lifecycle.rs");
include!("store/retry.rs");
include!("store/schema_helpers.rs");
include!("store/message_fields.rs");
include!("store/metadata.rs");
include!("store/undo_helpers.rs");
