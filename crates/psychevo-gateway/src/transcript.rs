use psychevo::{
    ThreadCompaction, ThreadItem, ThreadTurnTerminal, ThreadTurnTerminalStatus,
    thread_lineage::side_inherited_metadata_hidden,
    tool_argument_display::write_argument_preview_from_args,
    tool_argument_display::write_argument_preview_from_json,
};
use serde_json::{Value, json};

use psychevo_gateway_protocol::events_transcript::{
    TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole,
};

mod agent_edges;
mod message_projection;
mod tool_results;

pub(crate) use agent_edges::{
    agent_relationship_lookup_candidates, enrich_agent_blocks_from_relationships,
};
use message_projection::{block, project_message_entry};
use tool_results::{attach_tool_results, entry_status_for_tool_result, merge_write_stdin_blocks};

#[derive(Clone, Copy, Debug)]
pub(crate) struct TurnProjectionWindow<'a> {
    pub(crate) turn_id: &'a str,
    pub(crate) first_committed_seq: i64,
}

pub(crate) fn project_transcript_entries(
    thread_id: &str,
    summaries: &[ThreadItem],
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
    summaries: &[ThreadItem],
    first_seq: i64,
) -> Vec<TranscriptEntry> {
    project_transcript_entries(thread_id, summaries)
        .into_iter()
        .filter(|entry| entry.message_seq.is_some_and(|seq| seq >= first_seq))
        .collect()
}

pub(crate) fn project_committed_turn_window_entries(
    thread_id: &str,
    summaries: &[ThreadItem],
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

#[cfg(test)]
pub(crate) fn project_turn_terminal_entries(
    thread_id: &str,
    terminals: &[ThreadTurnTerminal],
) -> Vec<TranscriptEntry> {
    terminals
        .iter()
        .map(|terminal| project_turn_terminal_entry(thread_id, terminal))
        .collect()
}

pub(crate) fn reconcile_terminal_bounded_running_blocks(
    entries: &mut [TranscriptEntry],
    thread_id: &str,
    terminals: &[ThreadTurnTerminal],
) {
    for terminal in terminals {
        let Some(first_committed_seq) = terminal.first_committed_session_seq else {
            continue;
        };
        let replacement = match terminal.status {
            ThreadTurnTerminalStatus::Failed => TranscriptBlockStatus::Failed,
            ThreadTurnTerminalStatus::Interrupted => TranscriptBlockStatus::Cancelled,
        };
        for entry in entries.iter_mut() {
            let Some(message_seq) = entry.message_seq else {
                continue;
            };
            if entry.thread_id != thread_id
                || message_seq < first_committed_seq
                || entry.created_at_ms > terminal.completed_at_ms
            {
                continue;
            }
            let mut changed = false;
            for block in &mut entry.blocks {
                if matches!(
                    block.status,
                    TranscriptBlockStatus::Pending | TranscriptBlockStatus::Running
                ) {
                    block.status = replacement;
                    retain_terminal_write_preview(
                        block,
                        if replacement == TranscriptBlockStatus::Cancelled {
                            "cancelled"
                        } else {
                            "failed"
                        },
                    );
                    block.updated_at_ms = block.updated_at_ms.max(terminal.completed_at_ms);
                    changed = true;
                }
            }
            if changed {
                entry.status = entry_status_for_tool_result(&entry.blocks, entry.status);
                entry.updated_at_ms = entry.updated_at_ms.max(terminal.completed_at_ms);
            }
        }
    }
}

fn retain_terminal_write_preview(block: &mut TranscriptBlock, phase: &str) {
    let Some(metadata) = block.metadata.as_mut() else {
        return;
    };
    if metadata.get("tool_name").and_then(Value::as_str) != Some("write") {
        return;
    }
    let preview = metadata
        .get("args")
        .or_else(|| metadata.get("arguments"))
        .and_then(write_argument_preview_from_args)
        .or_else(|| {
            block
                .detail
                .as_deref()
                .and_then(write_argument_preview_from_json)
        });
    if let Some(preview) = preview {
        crate::projection::set_write_argument_preview(metadata, &preview, phase);
    }
}

pub(crate) fn project_compaction_entries(
    thread_id: &str,
    compactions: &[ThreadCompaction],
) -> Vec<TranscriptEntry> {
    compactions
        .iter()
        .map(|record| project_compaction_entry(thread_id, record))
        .collect()
}

pub(crate) fn merge_entries_at_session_boundaries(
    mut messages: Vec<TranscriptEntry>,
    mut synthetic_entries: Vec<(i64, TranscriptEntry)>,
) -> Vec<TranscriptEntry> {
    messages.sort_by(|left, right| {
        left.message_seq
            .unwrap_or(i64::MAX)
            .cmp(&right.message_seq.unwrap_or(i64::MAX))
            .then_with(|| left.id.cmp(&right.id))
    });
    synthetic_entries.sort_by(|(left_boundary, left), (right_boundary, right)| {
        left_boundary
            .cmp(right_boundary)
            .then_with(|| left.created_at_ms.cmp(&right.created_at_ms))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut merged = Vec::with_capacity(messages.len() + synthetic_entries.len());
    let mut synthetic = synthetic_entries.into_iter().peekable();
    for message in messages {
        let message_seq = message.message_seq.unwrap_or(i64::MAX);
        while synthetic
            .peek()
            .is_some_and(|(boundary, _)| *boundary < message_seq)
        {
            merged.push(synthetic.next().expect("peeked synthetic entry").1);
        }
        merged.push(message);
        while synthetic
            .peek()
            .is_some_and(|(boundary, _)| *boundary == message_seq)
        {
            merged.push(synthetic.next().expect("peeked synthetic entry").1);
        }
    }
    merged.extend(synthetic.map(|(_, entry)| entry));
    merged
}

fn project_compaction_entry(thread_id: &str, record: &ThreadCompaction) -> TranscriptEntry {
    let mut metadata = serde_json::Map::new();
    metadata.insert("projection".to_string(), json!("compaction"));
    metadata.insert("checkpoint_id".to_string(), json!(record.checkpoint_id));
    metadata.insert("reason".to_string(), json!(record.reason));
    metadata.insert("trigger".to_string(), json!(record.reason));
    metadata.insert(
        "first_kept_session_seq".to_string(),
        json!(record.first_kept_session_seq),
    );
    metadata.insert(
        "created_after_session_seq".to_string(),
        json!(record.boundary_session_seq),
    );
    metadata.insert("created_at_ms".to_string(), json!(record.created_at_ms));
    metadata.insert("tokens_before".to_string(), json!(record.tokens_before));
    metadata.insert("tokens_after".to_string(), json!(record.tokens_after));
    metadata.insert(
        "summary_provider".to_string(),
        json!(record.summary_provider),
    );
    metadata.insert("summary_model".to_string(), json!(record.summary_model));
    if let Some(instructions) = record.instructions.as_ref() {
        metadata.insert("instructions".to_string(), json!(instructions));
    }
    if let Some(extra) = record.metadata.as_ref() {
        metadata.insert("checkpoint".to_string(), extra.clone());
    }
    let preview = compaction_preview(record);
    let metadata = Value::Object(metadata);
    let mut block = block(
        format!("compaction:{}:block", record.checkpoint_id),
        TranscriptBlockKind::Compaction,
        TranscriptBlockStatus::Completed,
        0,
        "runtime.compaction",
        Some("Session compacted".to_string()),
        None,
        Some(record.summary.clone()),
        Some(metadata.clone()),
        record.created_at_ms,
    );
    block.preview = Some(preview);
    TranscriptEntry {
        id: format!("compaction:{}", record.checkpoint_id),
        thread_id: thread_id.to_string(),
        turn_id: Some(format!("compaction:{}", record.checkpoint_id)),
        message_seq: None,
        role: TranscriptEntryRole::Diagnostic,
        status: TranscriptBlockStatus::Completed,
        source: "runtime.compaction".to_string(),
        blocks: vec![block],
        metadata: Some(metadata),
        usage: None,
        accounting: None,
        created_at_ms: record.created_at_ms,
        updated_at_ms: record.created_at_ms,
    }
}

fn compaction_preview(record: &ThreadCompaction) -> String {
    let mut parts = vec![record.reason.clone()];
    match (record.tokens_before, record.tokens_after) {
        (Some(before), Some(after)) => parts.push(format!("{before} -> {after} tokens")),
        (Some(before), None) => parts.push(format!("{before} tokens before")),
        (None, Some(after)) => parts.push(format!("{after} tokens after")),
        (None, None) => {}
    }
    parts.push(format!("keeps from #{}", record.first_kept_session_seq));
    parts.join(" | ")
}

pub(crate) fn project_turn_terminal_entry(
    thread_id: &str,
    terminal: &ThreadTurnTerminal,
) -> TranscriptEntry {
    let (status, title, fallback_body) = match terminal.status {
        ThreadTurnTerminalStatus::Failed => (
            TranscriptBlockStatus::Failed,
            "Turn failed",
            "The turn failed before producing a final response.",
        ),
        ThreadTurnTerminalStatus::Interrupted => (
            TranscriptBlockStatus::Cancelled,
            "Turn interrupted",
            "The turn was interrupted.",
        ),
    };
    let body = terminal
        .error_message
        .clone()
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| fallback_body.to_string());
    let mut metadata = serde_json::Map::new();
    metadata.insert("projection".to_string(), json!("turn_terminal"));
    metadata.insert("turn_id".to_string(), json!(terminal.turn_id));
    metadata.insert("status".to_string(), json!(terminal.status.as_str()));
    if let Some(outcome) = &terminal.outcome {
        metadata.insert("outcome".to_string(), json!(outcome));
    }
    if let Some(error_message) = &terminal.error_message {
        metadata.insert("error_message".to_string(), json!(error_message));
    }
    if let Some(extra) = &terminal.metadata {
        metadata.insert("terminal".to_string(), extra.clone());
    }
    TranscriptEntry {
        id: format!("turn:{}:terminal", terminal.turn_id),
        thread_id: thread_id.to_string(),
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
    }
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

#[cfg(test)]
mod tests;
