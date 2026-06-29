use std::collections::BTreeMap;

use psychevo_runtime::{
    AgentEdgeRecord, AssistantBlock, GatewayTurnTerminalRecord, Message, TUI_DISPLAY_METADATA_KEY,
    TuiMessageSummary, USER_SHELL_METADATA_KEY, UserContentBlock, side_inherited_metadata_hidden,
};
use serde_json::{Value, json};

use crate::protocol::{
    TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole, TranscriptToolResult,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct TurnProjectionWindow<'a> {
    pub(crate) turn_id: &'a str,
    pub(crate) first_committed_seq: i64,
}

pub(crate) fn project_transcript_entries(
    thread_id: &str,
    summaries: &[TuiMessageSummary],
) -> Vec<TranscriptEntry> {
    let mut entries = summaries
        .iter()
        .filter(|summary| !side_inherited_metadata_hidden(summary.metadata.as_ref()))
        .filter_map(|summary| project_message_entry(thread_id, summary))
        .collect::<Vec<_>>();
    attach_tool_results(&mut entries, summaries);
    merge_write_stdin_blocks(&mut entries);
    entries
}

pub(crate) fn project_committed_turn_entries(
    thread_id: &str,
    summaries: &[TuiMessageSummary],
    first_seq: i64,
) -> Vec<TranscriptEntry> {
    project_transcript_entries(thread_id, summaries)
        .into_iter()
        .filter(|entry| entry.message_seq.is_some_and(|seq| seq >= first_seq))
        .collect()
}

pub(crate) fn project_committed_turn_window_entries(
    thread_id: &str,
    summaries: &[TuiMessageSummary],
    window: TurnProjectionWindow<'_>,
) -> Vec<TranscriptEntry> {
    let mut entries =
        project_committed_turn_entries(thread_id, summaries, window.first_committed_seq);
    stamp_committed_entries_for_turn_window(&mut entries, window);
    entries
}

pub(crate) fn stamp_committed_entries_for_turn_window(
    entries: &mut [TranscriptEntry],
    window: TurnProjectionWindow<'_>,
) {
    let mut assistant_segment = 0;
    for entry in entries {
        if entry
            .message_seq
            .is_none_or(|seq| seq < window.first_committed_seq)
        {
            continue;
        }
        entry.turn_id = Some(window.turn_id.to_string());
        if entry.role == TranscriptEntryRole::Assistant {
            entry.metadata = Some(metadata_with_live_order(
                entry.metadata.take(),
                assistant_segment,
            ));
            assistant_segment += 1;
        }
    }
}

fn metadata_with_live_order(metadata: Option<Value>, live_order: i64) -> Value {
    let mut object = metadata_object(metadata);
    object.insert("liveOrder".to_string(), json!(live_order));
    Value::Object(object)
}

pub(crate) fn project_turn_terminal_entries(
    terminals: &[GatewayTurnTerminalRecord],
) -> Vec<TranscriptEntry> {
    terminals
        .iter()
        .filter_map(project_turn_terminal_entry)
        .collect()
}

fn project_turn_terminal_entry(terminal: &GatewayTurnTerminalRecord) -> Option<TranscriptEntry> {
    let (status, title, fallback_body) = match terminal.status.as_str() {
        "failed" => (
            TranscriptBlockStatus::Failed,
            "Turn failed",
            "The turn failed before producing a final response.",
        ),
        "interrupted" => (
            TranscriptBlockStatus::Cancelled,
            "Turn interrupted",
            "The turn was interrupted.",
        ),
        _ => return None,
    };
    let body = terminal
        .error_message
        .clone()
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| fallback_body.to_string());
    let mut metadata = serde_json::Map::new();
    metadata.insert("projection".to_string(), json!("turn_terminal"));
    metadata.insert("turn_id".to_string(), json!(terminal.turn_id));
    metadata.insert("status".to_string(), json!(terminal.status));
    if let Some(outcome) = &terminal.outcome {
        metadata.insert("outcome".to_string(), json!(outcome));
    }
    if let Some(error_message) = &terminal.error_message {
        metadata.insert("error_message".to_string(), json!(error_message));
    }
    if let Some(extra) = &terminal.metadata {
        metadata.insert("terminal".to_string(), extra.clone());
    }
    Some(TranscriptEntry {
        id: format!("turn:{}:terminal", terminal.turn_id),
        thread_id: terminal.thread_id.clone(),
        turn_id: Some(terminal.turn_id.clone()),
        message_seq: None,
        role: TranscriptEntryRole::Diagnostic,
        status,
        source: "gateway.turn".to_string(),
        blocks: vec![block(
            format!("turn:{}:terminal:block", terminal.turn_id),
            TranscriptBlockKind::Status,
            status,
            0,
            "gateway.turn",
            Some(title.to_string()),
            Some(body.clone()),
            Some(body),
            Some(Value::Object(metadata.clone())),
            terminal.completed_at_ms,
        )],
        metadata: Some(Value::Object(metadata)),
        usage: None,
        accounting: None,
        created_at_ms: terminal.completed_at_ms,
        updated_at_ms: terminal.completed_at_ms,
    })
}

fn metadata_object(metadata: Option<Value>) -> serde_json::Map<String, Value> {
    match metadata {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = serde_json::Map::new();
            object.insert("value".to_string(), value);
            object
        }
        None => serde_json::Map::new(),
    }
}

fn ensure_json_object_field<'a>(
    object: &'a mut serde_json::Map<String, Value>,
    key: &str,
) -> &'a mut serde_json::Map<String, Value> {
    let entry = object
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(serde_json::Map::new());
    }
    entry.as_object_mut().expect("object field")
}

fn compact_text(text: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= max_chars {
            output.push_str("...");
            return output;
        }
        output.push(ch);
    }
    output
}

include!("transcript/agent_edges.rs");
include!("transcript/message_projection.rs");
include!("transcript/tool_results.rs");

#[cfg(test)]
mod tests {
    use super::*;

    include!("transcript/tests.rs");
}
