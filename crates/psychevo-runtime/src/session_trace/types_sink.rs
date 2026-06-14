use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;

use psychevo_agent_core::{AgentEvent, AssistantBlock, Message, now_ms};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::types::MessageAccounting;

pub const SESSION_TRACE_SCHEMA_VERSION: u64 = 2;
pub const SESSION_TRACE_DEFAULT_LIMIT: usize = 200;
pub const SESSION_TRACE_MAX_LIMIT: usize = 1_000;

const TRACE_CHANNEL_CAPACITY: usize = 512;
const TRACE_WRITER_STACK_BYTES: usize = 128 * 1024;
const TRACE_MAX_DEPTH: usize = 8;
const TRACE_MAX_OBJECT_FIELDS: usize = 64;
const TRACE_MAX_ARRAY_ITEMS: usize = 64;
const TRACE_MAX_STRING_CHARS: usize = 4 * 1024;
const TRACE_TITLE_STRING_CHARS: usize = 160;
const TRACE_SELECTED_SKILLS_MAX_ITEMS: usize = 500;
const TRACE_SUMMARY_MAX_TURNS: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionTraceReadOptions {
    pub after_seq: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTraceReadResult {
    pub thread_id: String,
    pub available: bool,
    pub events: Vec<Value>,
    pub warnings: Vec<String>,
    pub truncated: bool,
    pub next_after_seq: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionTraceDraft {
    pub(crate) kind: String,
    pub(crate) timestamp_ms: i64,
    pub(crate) monotonic_offset_ms: u64,
    pub(crate) turn_index: Option<usize>,
    pub(crate) correlation: Value,
    pub(crate) payload: Value,
}

#[derive(Clone)]
pub(crate) struct SessionTraceSink {
    inner: Arc<SessionTraceSinkInner>,
}

struct SessionTraceSinkInner {
    sender: SyncSender<SessionTraceDraft>,
    last_error: Arc<Mutex<Option<String>>>,
    stats: Arc<Mutex<SessionTraceStats>>,
}

#[derive(Debug, Default)]
struct SessionTraceStats {
    coalesced_by_kind: BTreeMap<String, u64>,
    dropped_by_reason: BTreeMap<String, u64>,
    dropped_by_kind: BTreeMap<String, u64>,
    turns: BTreeMap<usize, TurnTraceStats>,
    coalesced_by_tool_name: BTreeMap<String, CoalescedToolNameStats>,
}

#[derive(Debug, Default)]
struct TurnTraceStats {
    coalesced_events: u64,
}

#[derive(Debug, Default, Clone, Serialize)]
struct CoalescedToolNameStats {
    tool_call_pending: u64,
    tool_execution_update: u64,
}

impl SessionTraceStats {
    fn coalesce_kind(&mut self, kind: &str) {
        increment_counter(&mut self.coalesced_by_kind, kind);
    }

    fn drop_kind(&mut self, reason: &str, kind: &str) {
        increment_counter(&mut self.dropped_by_reason, reason);
        increment_counter(&mut self.dropped_by_kind, kind);
    }

    fn observe_coalesced_event(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::ToolCallPending { tool_name, .. } => {
                let tool = self.coalesced_tool_name_mut(tool_name);
                tool.tool_call_pending = tool.tool_call_pending.saturating_add(1);
            }
            AgentEvent::ToolExecutionUpdate { tool_name, .. } => {
                let tool = self.coalesced_tool_name_mut(tool_name);
                tool.tool_execution_update = tool.tool_execution_update.saturating_add(1);
            }
            _ => {}
        }
    }

    fn summary_payload(&self) -> Value {
        let (turns, omitted_turns) = limited_summary_items(
            self.turns.iter().map(|(turn_index, stats)| {
                json!({
                    "turn_index": turn_index,
                    "coalesced_events": stats.coalesced_events,
                })
            }),
            TRACE_SUMMARY_MAX_TURNS,
        );
        json!({
            "summary_kind": "accounting_footer",
            "coalesced_counts": self.coalesced_by_kind.clone(),
            "dropped_counts": {
                "by_reason": self.dropped_by_reason.clone(),
                "by_kind": self.dropped_by_kind.clone(),
            },
            "turns": turns,
            "coalesced_by_tool_name": self.coalesced_by_tool_name.clone(),
            "omitted_counts": {
                "turns": omitted_turns,
            },
        })
    }

    fn turn_mut(&mut self, turn_index: usize) -> &mut TurnTraceStats {
        self.turns.entry(turn_index).or_default()
    }

    fn coalesced_tool_name_mut(&mut self, tool_name: &str) -> &mut CoalescedToolNameStats {
        self.coalesced_by_tool_name
            .entry(tool_name.to_string())
            .or_default()
    }
}

impl SessionTraceSink {
    pub(crate) fn open(
        db_path: &Path,
        session_id: &str,
        invocation_id: String,
    ) -> Result<Option<Self>, String> {
        let Some(path) = session_trace_path(db_path, session_id)? else {
            return Ok(None);
        };
        let (sender, receiver) = sync_channel(TRACE_CHANNEL_CAPACITY);
        let last_error = Arc::new(Mutex::new(None));
        let stats = Arc::new(Mutex::new(SessionTraceStats::default()));
        let writer_error = Arc::clone(&last_error);
        let session_id = session_id.to_string();
        thread::Builder::new()
            .name("psychevo-session-trace".to_string())
            .stack_size(TRACE_WRITER_STACK_BYTES)
            .spawn(move || {
                let mut next_seq = match max_valid_seq(&path) {
                    Ok(seq) => seq.saturating_add(1),
                    Err(err) => {
                        set_last_error(&writer_error, err);
                        1
                    }
                };
                for draft in receiver {
                    match append_trace_record(&path, &session_id, &invocation_id, next_seq, draft) {
                        Ok(()) => {
                            next_seq = next_seq.saturating_add(1);
                        }
                        Err(err) => set_last_error(&writer_error, err),
                    }
                }
            })
            .map_err(|err| format!("failed to start session trace writer: {err}"))?;
        Ok(Some(Self {
            inner: Arc::new(SessionTraceSinkInner {
                sender,
                last_error,
                stats,
            }),
        }))
    }

    pub(crate) fn enqueue(&self, draft: SessionTraceDraft) -> Option<String> {
        let kind = draft.kind.clone();
        match self.inner.sender.try_send(draft) {
            Ok(()) => None,
            Err(TrySendError::Full(_draft)) => {
                let message =
                    "session trace queue is full; dropping observability event".to_string();
                self.record_dropped("queue_full", &kind);
                set_last_error(&self.inner.last_error, message.clone());
                Some(message)
            }
            Err(TrySendError::Disconnected(_draft)) => {
                let message =
                    "session trace writer is unavailable; dropping observability event".to_string();
                self.record_dropped("writer_disconnected", &kind);
                set_last_error(&self.inner.last_error, message.clone());
                Some(message)
            }
        }
    }

    pub(crate) fn enqueue_run_start(&self, payload: &Value) -> Option<String> {
        let draft = run_start_trace_draft(payload);
        self.enqueue(draft)
    }

    pub(crate) fn observe_agent_event(
        &self,
        event: &AgentEvent,
        accounting: Option<&MessageAccounting>,
        monotonic_offset_ms: u64,
        turn_index: Option<usize>,
    ) -> Option<String> {
        let drafts = match self.inner.stats.lock() {
            Ok(mut stats) => trace_drafts_from_agent_event(
                event,
                accounting,
                monotonic_offset_ms,
                turn_index,
                &mut stats,
            ),
            Err(_) => {
                let message = "session trace stats are unavailable".to_string();
                set_last_error(&self.inner.last_error, message.clone());
                return Some(message);
            }
        };
        let mut warning = None;
        for draft in drafts {
            if let Some(message) = self.enqueue(draft)
                && warning.is_none()
            {
                warning = Some(message);
            }
        }
        warning
    }

    pub(crate) fn take_error(&self) -> Option<String> {
        self.inner.last_error.lock().ok()?.take()
    }

    fn record_dropped(&self, reason: &'static str, kind: &str) {
        if let Ok(mut stats) = self.inner.stats.lock() {
            stats.drop_kind(reason, kind);
        }
    }
}
