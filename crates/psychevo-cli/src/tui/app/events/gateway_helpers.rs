fn outcome_from_str(value: &str) -> Option<Outcome> {
    match value {
        "normal" => Some(Outcome::Normal),
        "stopped" => Some(Outcome::Stopped),
        "failed" => Some(Outcome::Failed),
        "aborted" => Some(Outcome::Aborted),
        _ => None,
    }
}

fn transcript_block_text(block: &TranscriptBlock) -> String {
    block
        .body
        .as_ref()
        .or(block.detail.as_ref())
        .or(block.preview.as_ref())
        .cloned()
        .unwrap_or_default()
}

fn gateway_block_row_index(ui: &mut FullscreenUi<'_>, block_id: &str) -> Option<usize> {
    if block_id.is_empty() {
        return None;
    }
    let index = ui.gateway_item_rows.get(block_id).copied()?;
    if index < ui.transcript.len() {
        Some(index)
    } else {
        ui.gateway_item_rows.remove(block_id);
        None
    }
}

fn record_gateway_block_row(ui: &mut FullscreenUi<'_>, block_id: &str, index: usize) {
    if !block_id.is_empty() {
        ui.gateway_item_rows.insert(block_id.to_string(), index);
    }
}

fn tag_gateway_transcript_row(
    ui: &mut FullscreenUi<'_>,
    index: usize,
    entry: GatewayTranscriptEntryMeta<'_>,
    block: &TranscriptBlock,
) {
    let Some(row) = ui.transcript.get_mut(index) else {
        return;
    };
    row.transcript_turn_id = entry.turn_id.map(str::to_string);
    row.transcript_source = Some(if block.source.trim().is_empty() {
        entry.source.to_string()
    } else {
        block.source.clone()
    });
    row.transcript_entry_id = Some(entry.entry_id.to_string());
    row.transcript_block_id = Some(block.id.clone());
    row.transcript_message_seq = entry.message_seq;
}

fn clear_gateway_row_slots_for_index(ui: &mut FullscreenUi<'_>, index: usize) {
    if ui.assistant_row == Some(index) {
        ui.assistant_row = None;
    }
    if ui.assistant_preamble_row == Some(index) {
        ui.assistant_preamble_row = None;
    }
    if ui.reasoning_row == Some(index) {
        ui.reasoning_row = None;
    }
}

fn gateway_reasoning_title(block: &TranscriptBlock) -> String {
    if gateway_block_is_assistant_preamble(block) {
        return "Thinking".to_string();
    }
    block
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("Thinking")
        .to_string()
}

fn gateway_block_is_assistant_preamble(block: &TranscriptBlock) -> bool {
    block
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("projection"))
        .and_then(Value::as_str)
        .is_some_and(|projection| projection == "assistant_preamble")
        || block.title.as_deref() == Some("Preamble")
}

fn transcript_block_title(block: &TranscriptBlock) -> String {
    block.title.clone().unwrap_or_else(|| match block.kind {
        TranscriptBlockKind::Shell => "exec_command".to_string(),
        TranscriptBlockKind::File => "file".to_string(),
        TranscriptBlockKind::Web => "web".to_string(),
        TranscriptBlockKind::Mcp => "mcp".to_string(),
        TranscriptBlockKind::Clarify => "clarify".to_string(),
        TranscriptBlockKind::Permission => "permission".to_string(),
        TranscriptBlockKind::Skill => "skill".to_string(),
        TranscriptBlockKind::Agent => "Agent".to_string(),
        TranscriptBlockKind::Mailbox => "mailbox".to_string(),
        TranscriptBlockKind::Diff => "diff".to_string(),
        TranscriptBlockKind::Artifact => "artifact".to_string(),
        TranscriptBlockKind::Tool | TranscriptBlockKind::ToolCall => "tool".to_string(),
        TranscriptBlockKind::ToolResult => "result".to_string(),
        TranscriptBlockKind::Status => "status".to_string(),
        TranscriptBlockKind::Text | TranscriptBlockKind::Reasoning => String::new(),
    })
}

fn transcript_block_running_text(block: &TranscriptBlock) -> String {
    let text = transcript_block_text(block);
    if !text.trim().is_empty() {
        return text;
    }
    match block.status {
        TranscriptBlockStatus::Pending => "pending".to_string(),
        TranscriptBlockStatus::Running => "running".to_string(),
        TranscriptBlockStatus::Cancelled => "interrupted".to_string(),
        TranscriptBlockStatus::Failed => "failed".to_string(),
        TranscriptBlockStatus::NeedsInput => "needs input".to_string(),
        TranscriptBlockStatus::Info | TranscriptBlockStatus::Completed => String::new(),
    }
}

fn transcript_kind_for_block(kind: TranscriptBlockKind) -> TranscriptKind {
    match kind {
        TranscriptBlockKind::File | TranscriptBlockKind::Diff | TranscriptBlockKind::Artifact => {
            TranscriptKind::Updated
        }
        TranscriptBlockKind::Web | TranscriptBlockKind::Mcp => TranscriptKind::Explored,
        TranscriptBlockKind::Status => TranscriptKind::Status,
        _ => TranscriptKind::Ran,
    }
}

fn gateway_event_session_id(event: &GatewayEvent) -> Option<&str> {
    match event {
        GatewayEvent::TurnStarted { thread_id, .. }
        | GatewayEvent::TurnQueued { thread_id, .. }
        | GatewayEvent::TurnCompleted { thread_id, .. } => thread_id.as_deref(),
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. } => {
            (!entry.thread_id.is_empty()).then_some(entry.thread_id.as_str())
        }
        GatewayEvent::EntryDelta { .. }
        | GatewayEvent::PermissionRequested { .. }
        | GatewayEvent::PermissionResolved { .. }
        | GatewayEvent::ClarifyRequested { .. }
        | GatewayEvent::ClarifyResolved { .. }
        | GatewayEvent::Warning { .. } => None,
    }
}

fn gateway_block_tool_value(block: &TranscriptBlock) -> Option<Value> {
    let value = block.metadata.as_ref()?;
    (value.get("projection").and_then(Value::as_str) == Some("tool")).then(|| value.clone())
}

fn gateway_block_runtime_value(block: &TranscriptBlock) -> Option<Value> {
    let value = block.metadata.as_ref()?;
    (value.get("projection").and_then(Value::as_str) == Some("runtimeValue")).then(|| value.clone())
}

fn remove_visible_write_stdin_row(ui: &mut FullscreenUi<'_>, tool_call_id: &str) {
    if tool_call_id.is_empty() {
        return;
    }
    ui.remove_streaming_tool_call_row("write_stdin", tool_call_id, None);
    let key = tool_id_key(tool_call_id);
    if let Some(index) = ui.tool_rows.get(&key).copied()
        && ui
            .transcript
            .get(index)
            .is_some_and(|row| row.tool_name.as_deref() == Some("write_stdin"))
    {
        ui.remove_transcript_row(index);
    }
}

fn apply_gateway_assistant_turn_metadata(ui: &mut FullscreenUi<'_>, block: &TranscriptBlock) {
    let Some(metadata) = block.metadata.as_ref() else {
        return;
    };
    if let Some(usage) = non_null_metadata_field(metadata, "usage") {
        if let Some(tokens) = usage_context_tokens(&usage) {
            ui.sidebar_tokens = Some(tokens);
        }
        ui.turn_usage = Some(usage);
    }
    if let Some(turn_metadata) = gateway_assistant_turn_metadata(metadata) {
        ui.turn_metadata = Some(turn_metadata);
    }
    if let Some(accounting) = non_null_metadata_field(metadata, "accounting") {
        ui.add_sidebar_cost(Some(&accounting));
        ui.turn_accounting = Some(accounting);
    }
    if let Some(provider) = metadata_string_field(metadata, "provider") {
        ui.turn_provider = provider;
    }
    if let Some(model) = metadata_string_field(metadata, "model") {
        ui.turn_model = model;
    }
    if let Some(mode) = metadata_string_field(metadata, "mode") {
        ui.turn_mode = mode;
    }
}

fn push_gateway_completed_turn_meta(
    ui: &mut FullscreenUi<'_>,
    debug: bool,
    entry: GatewayTranscriptEntryMeta<'_>,
) {
    let meta = turn_meta_text(TurnMetaProjection {
        mode: &ui.turn_mode,
        provider: &ui.turn_provider,
        model: &ui.turn_model,
        started: None,
        usage: ui.turn_usage.as_ref(),
        metadata: ui.turn_metadata.as_ref(),
        accounting: ui.turn_accounting.as_ref(),
        failures: ui.turn_failures,
        interrupted: ui.turn_interrupted,
        debug,
    });
    if !meta.is_empty() {
        let mut row = TranscriptRow::with_title(TranscriptKind::Meta, "", meta);
        row.transcript_turn_id = entry.turn_id.map(str::to_string);
        row.transcript_source = Some(entry.source.to_string());
        row.transcript_entry_id = Some(entry.entry_id.to_string());
        row.transcript_block_id = Some(format!("{}:meta", entry.entry_id));
        row.transcript_message_seq = entry.message_seq;
        ui.transcript.push(row);
    }
    ui.finish_turn();
}

fn gateway_assistant_block_receives_meta(block: &TranscriptBlock) -> bool {
    if block.kind != TranscriptBlockKind::Text || block.status != TranscriptBlockStatus::Completed {
        return false;
    }
    let Some(metadata) = block.metadata.as_ref() else {
        return true;
    };
    if metadata_string_field(metadata, "finish_reason")
        .as_deref()
        .is_some_and(|finish_reason| matches!(finish_reason, "tool_calls" | "aborted"))
    {
        return false;
    }
    metadata_string_field(metadata, "outcome")
        .as_deref()
        .is_none_or(|outcome| outcome == "normal")
}

fn gateway_assistant_turn_metadata(metadata: &Value) -> Option<Value> {
    if let Some(value) = non_null_metadata_field(metadata, "metadata") {
        return Some(value);
    }
    let object = metadata.as_object()?;
    let mut projected = serde_json::Map::new();
    for (key, value) in object {
        if matches!(
            key.as_str(),
            "usage"
                | "accounting"
                | "provider"
                | "model"
                | "mode"
                | "finish_reason"
                | "outcome"
                | "message_session_seq"
                | "content_array_index"
        ) {
            continue;
        }
        if !value.is_null() {
            projected.insert(key.clone(), value.clone());
        }
    }
    (!projected.is_empty()).then_some(Value::Object(projected))
}

fn non_null_metadata_field(metadata: &Value, key: &str) -> Option<Value> {
    metadata.get(key).filter(|value| !value.is_null()).cloned()
}

fn metadata_string_field(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[path = "helpers.rs"]
pub(crate) mod helpers;
#[allow(unused_imports)]
pub use helpers::*;
