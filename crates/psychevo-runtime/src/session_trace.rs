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

fn run_start_trace_draft(payload: &Value) -> SessionTraceDraft {
    SessionTraceDraft {
        kind: "run_start".to_string(),
        timestamp_ms: now_ms(),
        monotonic_offset_ms: 0,
        turn_index: None,
        correlation: json!({}),
        payload: compact_run_start_payload(payload),
    }
}

fn trace_drafts_from_agent_event(
    event: &AgentEvent,
    _accounting: Option<&MessageAccounting>,
    monotonic_offset_ms: u64,
    turn_index: Option<usize>,
    stats: &mut SessionTraceStats,
) -> Vec<SessionTraceDraft> {
    let event_kind = trace_event_kind(event);
    if should_coalesce_event(event) {
        stats.coalesce_kind(event_kind);
        stats.observe_coalesced_event(event);
        if let Some(turn_index) = turn_index {
            let turn = stats.turn_mut(turn_index);
            turn.coalesced_events = turn.coalesced_events.saturating_add(1);
        }
        return Vec::new();
    }
    let timestamp_ms = event_timestamp_ms(event).unwrap_or_else(now_ms);
    let mut correlation = Map::new();
    let (kind, payload) = match event {
        AgentEvent::AgentStart => ("agent_start", json!({})),
        AgentEvent::AgentEnd {
            outcome,
            messages,
            terminal_reason,
        } => (
            "agent_end",
            json!({
                "outcome": outcome.as_str(),
                "message_count": messages.len(),
                "terminal_reason": terminal_reason,
            }),
        ),
        AgentEvent::TurnStart { .. } => ("turn_start", json!({})),
        AgentEvent::TurnEnd { outcome, .. } => (
            "turn_end",
            json!({
                "outcome": outcome.as_str(),
            }),
        ),
        AgentEvent::GenerationStart {
            generation_id,
            message_count,
            tool_count,
            started_at_ms,
            ..
        } => {
            correlation.insert("generation_id".to_string(), json!(generation_id));
            (
                "generation_start",
                json!({
                    "message_count": message_count,
                    "tool_count": tool_count,
                    "started_at_ms": started_at_ms,
                }),
            )
        }
        AgentEvent::GenerationEnd {
            generation_id,
            provider,
            model,
            outcome,
            elapsed_ms,
            usage,
            metadata,
            error,
        } => {
            correlation.insert("generation_id".to_string(), json!(generation_id));
            let mut payload = json!({
                "provider": provider,
                "model": model,
                "outcome": outcome.as_str(),
                "elapsed_ms": elapsed_ms,
            });
            if let Some(object) = payload.as_object_mut() {
                if let Some(usage) = usage {
                    object.insert("usage".to_string(), bounded_public_value(usage));
                }
                if let Some(metadata) = metadata {
                    let summary = metadata_summary(metadata);
                    if !is_empty_object(&summary) {
                        object.insert("metadata".to_string(), summary);
                    }
                }
                if let Some(error) = error {
                    object.insert("error".to_string(), bounded_string(error));
                }
            }
            ("generation_end", payload)
        }
        AgentEvent::MessageStart { .. }
        | AgentEvent::MessageUpdate { .. }
        | AgentEvent::ReasoningDelta { .. }
        | AgentEvent::ToolCallPending { .. }
        | AgentEvent::ToolExecutionUpdate { .. } => unreachable!("coalesced above"),
        AgentEvent::MessageEnd { message, .. } => message_trace("message_end", message),
        AgentEvent::ReasoningEnd { text } => (
            "reasoning_end",
            json!({
                "chars": text.chars().count(),
            }),
        ),
        AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
            started_at_ms,
            ..
        } => {
            insert_tool_correlation(&mut correlation, tool_call_id, tool_name);
            let payload = json!({
                "args_summary": value_summary(args),
                "started_at_ms": started_at_ms,
            });
            ("tool_execution_start", payload)
        }
        AgentEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            result,
            outcome,
            elapsed_ms,
            ..
        } => {
            insert_tool_correlation(&mut correlation, tool_call_id, tool_name);
            let payload = json!({
                "result_summary": value_summary(result),
                "outcome": outcome.as_str(),
                "elapsed_ms": elapsed_ms,
            });
            ("tool_execution_end", payload)
        }
    };

    if let Some(role) = message_role_for_event(event) {
        correlation.insert("message_role".to_string(), Value::String(role.to_string()));
    }
    let draft = SessionTraceDraft {
        kind: kind.to_string(),
        timestamp_ms,
        monotonic_offset_ms,
        turn_index,
        correlation: Value::Object(correlation),
        payload,
    };
    if matches!(event, AgentEvent::AgentEnd { .. }) {
        vec![
            draft,
            SessionTraceDraft {
                kind: "run_summary".to_string(),
                timestamp_ms: now_ms(),
                monotonic_offset_ms,
                turn_index: None,
                correlation: json!({}),
                payload: stats.summary_payload(),
            },
        ]
    } else {
        vec![draft]
    }
}

fn trace_event_kind(event: &AgentEvent) -> &'static str {
    match event {
        AgentEvent::AgentStart => "agent_start",
        AgentEvent::AgentEnd { .. } => "agent_end",
        AgentEvent::TurnStart { .. } => "turn_start",
        AgentEvent::TurnEnd { .. } => "turn_end",
        AgentEvent::GenerationStart { .. } => "generation_start",
        AgentEvent::GenerationEnd { .. } => "generation_end",
        AgentEvent::MessageStart { .. } => "message_start",
        AgentEvent::MessageUpdate { .. } => "message_update",
        AgentEvent::MessageEnd { .. } => "message_end",
        AgentEvent::ReasoningDelta { .. } => "reasoning_delta",
        AgentEvent::ReasoningEnd { .. } => "reasoning_end",
        AgentEvent::ToolCallPending { .. } => "tool_call_pending",
        AgentEvent::ToolExecutionStart { .. } => "tool_execution_start",
        AgentEvent::ToolExecutionUpdate { .. } => "tool_execution_update",
        AgentEvent::ToolExecutionEnd { .. } => "tool_execution_end",
    }
}

fn should_coalesce_event(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::MessageStart { .. }
            | AgentEvent::MessageUpdate { .. }
            | AgentEvent::ReasoningDelta { .. }
            | AgentEvent::ToolCallPending { .. }
            | AgentEvent::ToolExecutionUpdate { .. }
    )
}

fn compact_run_start_payload(payload: &Value) -> Value {
    let Some(object) = payload.as_object() else {
        return json!({});
    };
    let mut next = Map::new();
    for key in [
        "type",
        "source",
        "provider",
        "model",
        "mode",
        "permission_mode",
        "approval_mode",
        "reasoning_effort",
        "context_limit",
        "project_context",
    ] {
        if let Some(value) = object.get(key)
            && !value.is_null()
        {
            next.insert(key.to_string(), bounded_redacted_value(value));
        }
    }
    if let Some(value) = object.get("selected_agent")
        && !value.is_null()
    {
        next.insert("selected_agent".to_string(), selected_agent_summary(value));
    }
    if let Some(value) = object.get("selected_skills")
        && !value.is_null()
    {
        next.insert(
            "selected_skills".to_string(),
            selected_skills_summary(value),
        );
    }
    Value::Object(next)
}

fn selected_agent_summary(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return bounded_redacted_value(value);
    };
    let mut next = Map::new();
    for key in ["name", "source", "generated"] {
        if let Some(value) = object.get(key) {
            next.insert(key.to_string(), bounded_redacted_value(value));
        }
    }
    Value::Object(next)
}

fn selected_skills_summary(value: &Value) -> Value {
    let Some(values) = value.as_array() else {
        return bounded_redacted_value(value);
    };
    Value::Array(
        values
            .iter()
            .take(TRACE_SELECTED_SKILLS_MAX_ITEMS)
            .map(|value| {
                if let Some(object) = value.as_object() {
                    let mut next = Map::new();
                    if let Some(name) = object.get("name") {
                        next.insert("name".to_string(), bounded_redacted_value(name));
                    }
                    Value::Object(next)
                } else {
                    bounded_redacted_value(value)
                }
            })
            .collect(),
    )
}

fn metadata_summary(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return value_summary(value);
    };
    let mut next = Map::new();
    for key in [
        "elapsed_ms",
        "elapsed_ms_source",
        "reasoning_effort",
        "selected_agent",
    ] {
        if let Some(value) = object.get(key) {
            next.insert(key.to_string(), bounded_redacted_value(value));
        }
    }
    Value::Object(next)
}

fn value_summary(value: &Value) -> Value {
    match value {
        Value::Null => json!({ "type": "null" }),
        Value::Bool(_) => json!({ "type": "bool" }),
        Value::Number(_) => json!({ "type": "number" }),
        Value::String(text) if text.starts_with("data:image/") => {
            json!({ "type": "string", "chars": text.chars().count(), "redacted": "image_data_url" })
        }
        Value::String(text) => json!({ "type": "string", "chars": text.chars().count() }),
        Value::Array(values) => json!({
            "type": "array",
            "items": values.len(),
        }),
        Value::Object(object) => {
            let title = title_fields(object);
            json!({
                "type": "object",
                "field_count": object.len(),
                "title": title,
            })
        }
    }
}

fn title_fields(object: &Map<String, Value>) -> Value {
    let mut next = Map::new();
    for key in [
        "cmd",
        "command",
        "path",
        "url",
        "final_url",
        "name",
        "query",
        "status",
        "exit_code",
        "content_type",
        "bytes_written",
        "files_modified",
        "truncated",
    ] {
        if let Some(value) = object.get(key) {
            next.insert(key.to_string(), title_field_value(key, value));
        }
    }
    Value::Object(next)
}

fn title_field_value(key: &str, value: &Value) -> Value {
    if is_sensitive_key(key) {
        return Value::String("<redacted>".to_string());
    }
    match value {
        Value::String(text) => {
            let char_count = text.chars().count();
            if char_count <= TRACE_TITLE_STRING_CHARS {
                Value::String(text.to_string())
            } else {
                let prefix = text
                    .chars()
                    .take(TRACE_TITLE_STRING_CHARS)
                    .collect::<String>();
                json!({
                    "truncated": true,
                    "chars": char_count,
                    "prefix": prefix,
                })
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::Array(values) => json!({ "type": "array", "items": values.len() }),
        Value::Object(object) => json!({ "type": "object", "field_count": object.len() }),
    }
}

fn increment_counter(counters: &mut BTreeMap<String, u64>, key: &str) {
    let next = counters.get(key).copied().unwrap_or(0).saturating_add(1);
    counters.insert(key.to_string(), next);
}

fn is_empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(Map::is_empty)
}

fn limited_summary_items(items: impl Iterator<Item = Value>, limit: usize) -> (Value, u64) {
    let mut values = Vec::new();
    let mut omitted = 0u64;
    for item in items {
        if values.len() < limit {
            values.push(item);
        } else {
            omitted = omitted.saturating_add(1);
        }
    }
    (Value::Array(values), omitted)
}

pub fn read_session_trace(
    db_path: &Path,
    session_id: &str,
    options: SessionTraceReadOptions,
) -> SessionTraceReadResult {
    let mut warnings = Vec::new();
    let Some(path) = (match session_trace_path(db_path, session_id) {
        Ok(path) => path,
        Err(err) => {
            warnings.push(err);
            None
        }
    }) else {
        return SessionTraceReadResult {
            thread_id: session_id.to_string(),
            available: false,
            events: Vec::new(),
            warnings,
            truncated: false,
            next_after_seq: options.after_seq,
        };
    };
    if !path.exists() {
        return SessionTraceReadResult {
            thread_id: session_id.to_string(),
            available: false,
            events: Vec::new(),
            warnings,
            truncated: false,
            next_after_seq: options.after_seq,
        };
    }
    let limit = options
        .limit
        .unwrap_or(SESSION_TRACE_DEFAULT_LIMIT)
        .clamp(1, SESSION_TRACE_MAX_LIMIT);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(err) => {
            warnings.push(format!("failed to read session trace: {err}"));
            return SessionTraceReadResult {
                thread_id: session_id.to_string(),
                available: true,
                events: Vec::new(),
                warnings,
                truncated: false,
                next_after_seq: options.after_seq,
            };
        }
    };
    let mut events = VecDeque::new();
    let mut truncated = false;
    let mut pending_malformed: Option<(usize, String)> = None;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                warnings.push(format!("failed to read session trace: {err}"));
                break;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((line_no, message)) = pending_malformed.take() {
            warnings.push(format!(
                "ignored malformed session trace line {line_no}: {message}",
            ));
        }
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value @ Value::Object(_)) => value,
            Ok(_) => {
                warnings.push(format!(
                    "ignored non-object session trace line {}",
                    index + 1
                ));
                continue;
            }
            Err(err) => {
                pending_malformed = Some((index + 1, err.to_string()));
                continue;
            }
        };
        let seq = value.get("seq").and_then(Value::as_u64).unwrap_or(0);
        if options.after_seq.is_none_or(|after_seq| seq > after_seq) {
            if options.after_seq.is_some() && events.len() >= limit {
                truncated = true;
                break;
            }
            events.push_back(value);
            if options.after_seq.is_none() && events.len() > limit {
                let _ = events.pop_front();
                truncated = true;
            }
        }
    }
    if let Some((_line_no, message)) = pending_malformed {
        warnings.push(format!(
            "ignored malformed final session trace line: {message}"
        ));
    }

    let events = events.into_iter().collect::<Vec<_>>();
    let next_after_seq = events
        .last()
        .and_then(|value| value.get("seq"))
        .and_then(Value::as_u64)
        .or(options.after_seq);
    SessionTraceReadResult {
        thread_id: session_id.to_string(),
        available: true,
        events,
        warnings,
        truncated,
        next_after_seq,
    }
}

pub(crate) fn remove_session_trace_dir(db_path: &Path, session_id: &str) -> Result<(), String> {
    let Some(path) = session_trace_path(db_path, session_id)? else {
        return Ok(());
    };
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    if dir.exists() {
        fs::remove_dir_all(dir)
            .map_err(|err| format!("failed to remove session trace directory: {err}"))?;
    }
    Ok(())
}

pub fn session_trace_path(db_path: &Path, session_id: &str) -> Result<Option<PathBuf>, String> {
    if db_path == Path::new(":memory:") {
        return Ok(None);
    }
    validate_session_trace_id(session_id)?;
    let root = db_path.parent().unwrap_or_else(|| Path::new("."));
    Ok(Some(
        root.join("sessions").join(session_id).join("events.jsonl"),
    ))
}

fn append_trace_record(
    path: &Path,
    session_id: &str,
    invocation_id: &str,
    seq: u64,
    draft: SessionTraceDraft,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create session trace directory: {err}"))?;
    }
    let record = json!({
        "schema_version": SESSION_TRACE_SCHEMA_VERSION,
        "seq": seq,
        "event_id": Uuid::now_v7().to_string(),
        "session_id": session_id,
        "invocation_id": invocation_id,
        "turn_index": draft.turn_index,
        "kind": draft.kind,
        "timestamp_ms": draft.timestamp_ms,
        "monotonic_offset_ms": draft.monotonic_offset_ms,
        "source": "runtime",
        "correlation": draft.correlation,
        "redaction_state": "redacted",
        "payload": draft.payload,
    });
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("failed to open session trace: {err}"))?;
    serde_json::to_writer(&mut file, &record)
        .map_err(|err| format!("failed to encode session trace event: {err}"))?;
    file.write_all(b"\n")
        .map_err(|err| format!("failed to write session trace event: {err}"))?;
    Ok(())
}

fn max_valid_seq(path: &Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    let file =
        fs::File::open(path).map_err(|err| format!("failed to read session trace seq: {err}"))?;
    let mut seq = 0;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|err| format!("failed to read session trace seq: {err}"))?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(value) = value.get("seq").and_then(Value::as_u64) {
            seq = seq.max(value);
        }
    }
    Ok(seq)
}

fn set_last_error(slot: &Arc<Mutex<Option<String>>>, message: String) {
    if let Ok(mut current) = slot.lock() {
        *current = Some(message);
    }
}

fn event_timestamp_ms(event: &AgentEvent) -> Option<i64> {
    match event {
        AgentEvent::GenerationStart { started_at_ms, .. } => Some(*started_at_ms),
        AgentEvent::ToolExecutionStart { started_at_ms, .. } => Some(*started_at_ms),
        AgentEvent::MessageStart { message }
        | AgentEvent::MessageUpdate { message }
        | AgentEvent::MessageEnd { message, .. } => message_timestamp_ms(message),
        _ => None,
    }
}

fn message_timestamp_ms(message: &Message) -> Option<i64> {
    match message {
        Message::User { timestamp_ms, .. }
        | Message::Assistant { timestamp_ms, .. }
        | Message::ToolResult { timestamp_ms, .. } => Some(*timestamp_ms),
    }
}

fn message_role_for_event(event: &AgentEvent) -> Option<&'static str> {
    match event {
        AgentEvent::MessageStart { message }
        | AgentEvent::MessageUpdate { message }
        | AgentEvent::MessageEnd { message, .. } => Some(message.role()),
        _ => None,
    }
}

fn message_trace(kind: &'static str, message: &Message) -> (&'static str, Value) {
    let payload = json!({
        "role": message.role(),
        "timestamp_ms": message_timestamp_ms(message),
        "summary": message_summary(message),
    });
    (kind, payload)
}

fn message_summary(message: &Message) -> Value {
    match message {
        Message::User { content, .. } => json!({
            "text_chars": content.iter().filter_map(|block| block.text_value()).map(|text| text.chars().count()).sum::<usize>(),
            "image_count": content.iter().filter(|block| block.text_value().is_none()).count(),
        }),
        Message::Assistant {
            content,
            outcome,
            finish_reason,
            ..
        } => {
            let text_chars = content
                .iter()
                .map(|block| match block {
                    AssistantBlock::Text { text } => text.chars().count(),
                    _ => 0,
                })
                .sum::<usize>();
            let reasoning_chars = content
                .iter()
                .map(|block| match block {
                    AssistantBlock::Reasoning { text, .. } => text.chars().count(),
                    _ => 0,
                })
                .sum::<usize>();
            let tool_call_count = content
                .iter()
                .filter(|block| matches!(block, AssistantBlock::ToolCall(_)))
                .count();
            json!({
                "text_chars": text_chars,
                "reasoning_chars": reasoning_chars,
                "tool_call_count": tool_call_count,
                "outcome": outcome.as_str(),
                "finish_reason": finish_reason,
            })
        }
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            ..
        } => json!({
            "tool_call_id": tool_call_id,
            "tool_name": tool_name,
            "content_chars": content.chars().count(),
            "is_error": is_error,
        }),
    }
}

fn insert_tool_correlation(
    correlation: &mut Map<String, Value>,
    tool_call_id: &str,
    tool_name: &str,
) {
    correlation.insert(
        "tool_call_id".to_string(),
        Value::String(tool_call_id.to_string()),
    );
    correlation.insert(
        "tool_name".to_string(),
        Value::String(tool_name.to_string()),
    );
}

fn bounded_redacted_value(value: &Value) -> Value {
    bounded_redacted_value_at(value, 0)
}

fn bounded_public_value(value: &Value) -> Value {
    bounded_public_value_at(value, 0)
}

fn bounded_public_value_at(value: &Value, depth: usize) -> Value {
    if depth >= TRACE_MAX_DEPTH {
        return json!({
            "truncated": true,
            "reason": "max_depth",
        });
    }
    match value {
        Value::Object(object) => {
            let mut next = Map::new();
            for (index, (key, value)) in object.iter().enumerate() {
                if index >= TRACE_MAX_OBJECT_FIELDS {
                    next.insert(
                        "_psychevo_trace_truncated".to_string(),
                        json!({
                            "omitted_fields": object.len().saturating_sub(index),
                        }),
                    );
                    break;
                }
                next.insert(key.clone(), bounded_public_value_at(value, depth + 1));
            }
            Value::Object(next)
        }
        Value::Array(values) => {
            let mut next = values
                .iter()
                .take(TRACE_MAX_ARRAY_ITEMS)
                .map(|value| bounded_public_value_at(value, depth + 1))
                .collect::<Vec<_>>();
            if values.len() > TRACE_MAX_ARRAY_ITEMS {
                next.push(json!({
                    "_psychevo_trace_truncated": {
                        "omitted_items": values.len() - TRACE_MAX_ARRAY_ITEMS,
                    },
                }));
            }
            Value::Array(next)
        }
        Value::String(value) if value.starts_with("data:image/") => {
            Value::String("[image data url redacted]".to_string())
        }
        Value::String(value) => bounded_string(value),
        other => other.clone(),
    }
}

fn bounded_redacted_value_at(value: &Value, depth: usize) -> Value {
    if depth >= TRACE_MAX_DEPTH {
        return json!({
            "truncated": true,
            "reason": "max_depth",
        });
    }
    match value {
        Value::Object(object) => {
            let mut next = Map::new();
            for (index, (key, value)) in object.iter().enumerate() {
                if index >= TRACE_MAX_OBJECT_FIELDS {
                    next.insert(
                        "_psychevo_trace_truncated".to_string(),
                        json!({
                            "omitted_fields": object.len().saturating_sub(index),
                        }),
                    );
                    break;
                }
                if is_sensitive_key(key) {
                    next.insert(key.clone(), Value::String("<redacted>".to_string()));
                } else {
                    next.insert(key.clone(), bounded_redacted_value_at(value, depth + 1));
                }
            }
            Value::Object(next)
        }
        Value::Array(values) => {
            let mut next = values
                .iter()
                .take(TRACE_MAX_ARRAY_ITEMS)
                .map(|value| bounded_redacted_value_at(value, depth + 1))
                .collect::<Vec<_>>();
            if values.len() > TRACE_MAX_ARRAY_ITEMS {
                next.push(json!({
                    "_psychevo_trace_truncated": {
                        "omitted_items": values.len() - TRACE_MAX_ARRAY_ITEMS,
                    },
                }));
            }
            Value::Array(next)
        }
        Value::String(value) if value.starts_with("data:image/") => {
            Value::String("[image data url redacted]".to_string())
        }
        Value::String(value) => bounded_string(value),
        other => other.clone(),
    }
}

fn bounded_string(value: &str) -> Value {
    let mut chars = value.chars();
    let preview = chars
        .by_ref()
        .take(TRACE_MAX_STRING_CHARS)
        .collect::<String>();
    if chars.next().is_none() {
        return Value::String(value.to_string());
    }
    json!({
        "truncated": true,
        "preview": preview,
        "original_chars_min": TRACE_MAX_STRING_CHARS + 1 + chars.count(),
    })
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("api_key")
        || key.contains("apikey")
        || key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("credential")
        || key.contains("authorization")
}

fn validate_session_trace_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty()
        || session_id == "."
        || session_id == ".."
        || !session_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(format!("invalid session trace id: {session_id}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use psychevo_ai::Outcome;

    #[test]
    fn trace_path_is_disabled_for_memory_db() {
        assert_eq!(
            session_trace_path(Path::new(":memory:"), "session-1").expect("path"),
            None
        );
    }

    #[test]
    fn trace_path_rejects_unsafe_session_id() {
        assert!(session_trace_path(Path::new("/tmp/state.db"), "../bad").is_err());
    }

    #[test]
    fn read_session_trace_ignores_malformed_final_line() {
        let temp = tempfile::tempdir().expect("temp");
        let db = temp.path().join("state.db");
        let trace = temp.path().join("sessions").join("s1");
        fs::create_dir_all(&trace).expect("trace dir");
        fs::write(
            trace.join("events.jsonl"),
            concat!(
                "{\"schema_version\":1,\"seq\":1,\"kind\":\"agent_start\"}\n",
                "{\"schema_version\":1,\"seq\":"
            ),
        )
        .expect("trace");

        let result = read_session_trace(
            &db,
            "s1",
            SessionTraceReadOptions {
                after_seq: None,
                limit: Some(10),
            },
        );
        assert!(result.available);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0]["seq"], 1);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn append_trace_record_writes_redacted_bounded_event() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("sessions").join("s1").join("events.jsonl");
        append_trace_record(
            &path,
            "s1",
            "invocation-1",
            1,
            SessionTraceDraft {
                kind: "tool_execution_start".to_string(),
                timestamp_ms: 10,
                monotonic_offset_ms: 0,
                turn_index: Some(0),
                correlation: json!({"tool_call_id": "call-1"}),
                payload: bounded_redacted_value(&json!({
                    "args": {
                        "api_key": "secret",
                        "path": "README.md"
                    }
                })),
            },
        )
        .expect("append");

        let result = read_session_trace(
            &temp.path().join("state.db"),
            "s1",
            SessionTraceReadOptions::default(),
        );
        assert_eq!(result.events.len(), 1);
        let event = &result.events[0];
        assert_eq!(event["schema_version"], SESSION_TRACE_SCHEMA_VERSION);
        assert_eq!(event["seq"], 1);
        assert_eq!(event["payload"]["args"]["api_key"], "<redacted>");
        assert_eq!(event["payload"]["args"]["path"], "README.md");
    }

    #[test]
    fn compact_trace_coalesces_high_frequency_events_into_run_summary() {
        let mut stats = SessionTraceStats::default();
        let high_frequency_events = [
            AgentEvent::MessageUpdate {
                message: assistant_message("partial"),
            },
            AgentEvent::ReasoningDelta {
                text: "thinking".to_string(),
            },
            AgentEvent::ToolCallPending {
                tool_call_id: "call-1".to_string(),
                tool_name: "read".to_string(),
                arguments_json: "{\"path\":\"README.md\"}".to_string(),
                content_index: 0,
                call_index: 0,
                display: None,
            },
            AgentEvent::ToolExecutionUpdate {
                tool_call_id: "call-1".to_string(),
                tool_name: "read".to_string(),
                partial_result: json!({
                    "status": "streaming",
                    "output": "z".repeat(10_000),
                }),
            },
        ];
        for event in &high_frequency_events {
            assert!(trace_drafts_from_agent_event(event, None, 0, Some(0), &mut stats).is_empty());
        }

        let drafts = trace_drafts_from_agent_event(
            &AgentEvent::AgentEnd {
                outcome: Outcome::Normal,
                messages: Vec::new(),
                terminal_reason: None,
            },
            None,
            10,
            Some(0),
            &mut stats,
        );

        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].kind, "agent_end");
        assert_eq!(drafts[1].kind, "run_summary");
        let summary = &drafts[1].payload;
        assert_eq!(summary["summary_kind"], "accounting_footer");
        assert_eq!(summary["coalesced_counts"]["message_update"], 1);
        assert_eq!(summary["coalesced_counts"]["reasoning_delta"], 1);
        assert_eq!(summary["coalesced_counts"]["tool_call_pending"], 1);
        assert_eq!(summary["coalesced_counts"]["tool_execution_update"], 1);
        assert_eq!(summary["turns"][0]["coalesced_events"], 4);
        assert_eq!(summary["turns"][0]["turn_index"], 0);
        assert_eq!(
            summary["coalesced_by_tool_name"]["read"]["tool_call_pending"],
            1
        );
        assert_eq!(
            summary["coalesced_by_tool_name"]["read"]["tool_execution_update"],
            1
        );
        assert!(summary.get("event_counts").is_none());
        assert!(summary.get("persisted_counts").is_none());
        assert!(summary.get("coalesced_tool_events").is_none());
        assert!(summary.get("tools").is_none());
        assert!(summary.get("generations").is_none());
        let encoded = serde_json::to_string(summary).expect("summary json");
        assert!(!encoded.contains("call-1"));
        assert!(!encoded.contains(&"z".repeat(128)));
    }

    #[test]
    fn compact_trace_keeps_minimal_message_end_summary_without_full_message() {
        let mut stats = SessionTraceStats::default();
        let accounting = MessageAccounting {
            context_input_tokens: Some(1),
            billable_input_tokens: Some(2),
            billable_output_tokens: Some(3),
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reported_total_tokens: None,
            estimated_cost_nanodollars: None,
            pricing_source: Some("test".to_string()),
            pricing_tier: Some("standard".to_string()),
        };
        let drafts = trace_drafts_from_agent_event(
            &AgentEvent::MessageEnd {
                message: assistant_message("final answer"),
                usage: Some(json!({"input_tokens": 3, "output_tokens": 4})),
                metadata: Some(json!({
                    "elapsed_ms": 123,
                    "debug_body": "x".repeat(10_000),
                })),
            },
            Some(&accounting),
            12,
            Some(0),
            &mut stats,
        );

        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].kind, "message_end");
        let payload = &drafts[0].payload;
        assert_eq!(payload["role"], "assistant");
        assert_eq!(payload["summary"]["text_chars"], 12);
        assert_eq!(payload["summary"]["finish_reason"], "stop");
        assert_eq!(payload["summary"]["outcome"], "normal");
        assert!(payload["summary"].get("model").is_none());
        assert!(payload["summary"].get("provider").is_none());
        assert!(payload.get("usage").is_none());
        assert!(payload.get("metadata").is_none());
        assert!(payload.get("accounting").is_none());
        assert!(payload.get("message").is_none());
    }

    #[test]
    fn compact_trace_summarizes_tool_payloads_without_body_preview_or_display() {
        let mut stats = SessionTraceStats::default();
        let long_body = "c".repeat(10_000);
        let long_cmd = "run ".to_string() + &"x".repeat(220);
        let start = trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionStart {
                tool_call_id: "call-1".to_string(),
                tool_name: "write".to_string(),
                args: json!({
                    "cmd": long_cmd,
                    "path": "README.md",
                    "content": long_body,
                    "api_key": "secret",
                }),
                started_at_ms: 20,
                display: Some(psychevo_agent_core::ToolDisplaySpec::for_name("write")),
            },
            None,
            20,
            Some(0),
            &mut stats,
        );

        assert_eq!(start.len(), 1);
        let payload = &start[0].payload;
        assert!(payload.get("args").is_none());
        assert_eq!(payload["args_summary"]["type"], "object");
        assert_eq!(payload["args_summary"]["field_count"], 4);
        assert_eq!(payload["args_summary"]["title"]["path"], "README.md");
        assert_eq!(payload["args_summary"]["title"]["cmd"]["truncated"], true);
        assert_eq!(payload["args_summary"]["title"]["cmd"]["chars"], 224);
        assert_eq!(
            payload["args_summary"]["title"]["cmd"]["prefix"]
                .as_str()
                .expect("cmd prefix")
                .chars()
                .count(),
            TRACE_TITLE_STRING_CHARS
        );
        assert!(payload["args_summary"].get("fields").is_none());
        assert!(payload.get("display").is_none());
        let encoded = serde_json::to_string(payload).expect("payload json");
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains(&"c".repeat(128)));

        let result = trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionEnd {
                tool_call_id: "call-1".to_string(),
                tool_name: "write".to_string(),
                result: json!({
                    "status": "ok",
                    "output": "y".repeat(10_000),
                }),
                outcome: Outcome::Normal,
                elapsed_ms: 34,
                display: Some(psychevo_agent_core::ToolDisplaySpec::for_name("write")),
            },
            None,
            54,
            Some(0),
            &mut stats,
        );

        assert_eq!(result.len(), 1);
        let payload = &result[0].payload;
        assert!(payload.get("result").is_none());
        assert_eq!(payload["result_summary"]["type"], "object");
        assert_eq!(payload["result_summary"]["title"]["status"], "ok");
        assert!(payload["result_summary"].get("fields").is_none());
        assert!(payload.get("display").is_none());
        let encoded = serde_json::to_string(payload).expect("payload json");
        assert!(!encoded.contains(&"y".repeat(128)));
    }

    #[test]
    fn compact_trace_keeps_event_types_with_trimmed_payloads() {
        let mut stats = SessionTraceStats::default();
        let mut drafts = Vec::new();
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::TurnStart { turn_index: 7 },
            None,
            1,
            Some(7),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::GenerationStart {
                generation_id: "generation-1".to_string(),
                provider: "provider".to_string(),
                model: "model".to_string(),
                message_count: 2,
                tool_count: 1,
                started_at_ms: 10,
            },
            None,
            10,
            Some(7),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::GenerationEnd {
                generation_id: "generation-1".to_string(),
                provider: "provider".to_string(),
                model: "model".to_string(),
                outcome: Outcome::Normal,
                elapsed_ms: 30,
                usage: Some(json!({"input_tokens": 2})),
                metadata: Some(json!({})),
                error: None,
            },
            None,
            40,
            Some(7),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::ReasoningEnd {
                text: "hidden".to_string(),
            },
            None,
            41,
            Some(7),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::TurnEnd {
                turn_index: 7,
                outcome: Outcome::Normal,
            },
            None,
            42,
            Some(7),
            &mut stats,
        ));

        let kinds = drafts
            .iter()
            .map(|draft| draft.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                "turn_start",
                "generation_start",
                "generation_end",
                "reasoning_end",
                "turn_end"
            ]
        );
        assert_eq!(drafts[0].payload, json!({}));
        assert_eq!(drafts[1].payload["message_count"], 2);
        assert!(drafts[1].payload.get("provider").is_none());
        assert!(drafts[1].payload.get("model").is_none());
        assert_eq!(drafts[2].payload["elapsed_ms"], 30);
        assert_eq!(drafts[2].payload["usage"]["input_tokens"], 2);
        assert!(drafts[2].payload.get("metadata").is_none());
        assert_eq!(drafts[4].payload, json!({"outcome": "normal"}));
    }

    #[test]
    fn run_summary_does_not_duplicate_lifecycle_fact_details() {
        let mut stats = SessionTraceStats::default();
        let mut drafts = Vec::new();
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::GenerationStart {
                generation_id: "generation-1".to_string(),
                provider: "provider".to_string(),
                model: "model".to_string(),
                message_count: 2,
                tool_count: 1,
                started_at_ms: 10,
            },
            None,
            10,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::GenerationEnd {
                generation_id: "generation-1".to_string(),
                provider: "provider".to_string(),
                model: "model".to_string(),
                outcome: Outcome::Normal,
                elapsed_ms: 30,
                usage: None,
                metadata: None,
                error: None,
            },
            None,
            40,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionStart {
                tool_call_id: "call-1".to_string(),
                tool_name: "write".to_string(),
                args: json!({"path": "README.md", "content": "x".repeat(10_000)}),
                started_at_ms: 45,
                display: None,
            },
            None,
            45,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionEnd {
                tool_call_id: "call-1".to_string(),
                tool_name: "write".to_string(),
                result: json!({"status": "ok", "output": "y".repeat(10_000)}),
                outcome: Outcome::Normal,
                elapsed_ms: 25,
                display: None,
            },
            None,
            70,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::AgentEnd {
                outcome: Outcome::Normal,
                messages: Vec::new(),
                terminal_reason: None,
            },
            None,
            75,
            Some(0),
            &mut stats,
        ));

        let summary = drafts
            .iter()
            .find(|draft| draft.kind == "run_summary")
            .expect("run summary");
        assert_eq!(summary.payload["summary_kind"], "accounting_footer");
        assert!(summary.payload.get("event_counts").is_none());
        assert!(summary.payload.get("persisted_counts").is_none());
        assert!(summary.payload.get("coalesced_tool_events").is_none());
        assert_eq!(summary.payload["coalesced_by_tool_name"], json!({}));
        assert_eq!(summary.payload["omitted_counts"]["turns"], 0);
        assert!(summary.payload.get("tools").is_none());
        assert!(summary.payload.get("generations").is_none());
        let encoded = serde_json::to_string(&summary.payload).expect("summary json");
        assert!(!encoded.contains("args_summary"));
        assert!(!encoded.contains("result_summary"));
        assert!(!encoded.contains("elapsed_ms"));
        assert!(!encoded.contains("outcome"));
        assert!(!encoded.contains(&"x".repeat(128)));
        assert!(!encoded.contains(&"y".repeat(128)));
    }

    #[test]
    fn compact_trace_hackernews_like_pending_updates_stay_bounded() {
        let mut stats = SessionTraceStats::default();
        let mut drafts = Vec::new();
        for index in 0..2_500 {
            drafts.extend(trace_drafts_from_agent_event(
                &AgentEvent::ToolCallPending {
                    tool_call_id: "call-1".to_string(),
                    tool_name: "hackernews-daily".to_string(),
                    arguments_json: format!("{{\"query\":\"item-{index}\"}}"),
                    content_index: 0,
                    call_index: 0,
                    display: None,
                },
                None,
                index,
                Some(0),
                &mut stats,
            ));
        }
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionStart {
                tool_call_id: "call-1".to_string(),
                tool_name: "hackernews-daily".to_string(),
                args: json!({"query": "frontpage"}),
                started_at_ms: 2_600,
                display: None,
            },
            None,
            2_600,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionEnd {
                tool_call_id: "call-1".to_string(),
                tool_name: "hackernews-daily".to_string(),
                result: json!({"status": "ok", "items": 500}),
                outcome: Outcome::Normal,
                elapsed_ms: 50,
                display: None,
            },
            None,
            2_650,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::AgentEnd {
                outcome: Outcome::Normal,
                messages: Vec::new(),
                terminal_reason: None,
            },
            None,
            2_700,
            Some(0),
            &mut stats,
        ));

        assert!(drafts.len() <= 100);
        assert!(!drafts.iter().any(|draft| draft.kind == "tool_call_pending"));
        let summary = drafts
            .iter()
            .find(|draft| draft.kind == "run_summary")
            .expect("run summary");
        assert_eq!(
            summary.payload["coalesced_counts"]["tool_call_pending"],
            2_500
        );
        assert_eq!(summary.payload["summary_kind"], "accounting_footer");
        assert_eq!(
            summary.payload["coalesced_by_tool_name"]["hackernews-daily"]["tool_call_pending"],
            2_500
        );
        assert_eq!(
            summary.payload["coalesced_by_tool_name"]["hackernews-daily"]["tool_execution_update"],
            0
        );
        assert!(summary.payload.get("event_counts").is_none());
        assert!(summary.payload.get("persisted_counts").is_none());
        assert!(summary.payload.get("coalesced_tool_events").is_none());
        assert!(summary.payload.get("tools").is_none());
        assert!(summary.payload.get("generations").is_none());
        let encoded = serde_json::to_string(&summary.payload).expect("summary json");
        assert!(!encoded.contains("call-1"));
    }

    fn assistant_message(text: &str) -> Message {
        Message::Assistant {
            content: vec![AssistantBlock::Text {
                text: text.to_string(),
            }],
            timestamp_ms: 1,
            finish_reason: Some("stop".to_string()),
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        }
    }
}
