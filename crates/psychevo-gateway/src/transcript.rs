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

pub(crate) fn enrich_agent_blocks_from_edges(
    entries: &mut [TranscriptEntry],
    edges: &[AgentEdgeRecord],
) {
    let mut used_edges = BTreeMap::<usize, ()>::new();
    for entry in entries {
        for block in &mut entry.blocks {
            if block.kind != TranscriptBlockKind::Agent
                || block.status == TranscriptBlockStatus::Failed
            {
                continue;
            }
            let mut metadata = metadata_object(block.metadata.take());
            if agent_result_child_session_id(&metadata).is_some() {
                block.metadata = Some(Value::Object(metadata));
                continue;
            }
            let Some((edge_index, edge)) =
                matching_agent_edge_for_block(&metadata, edges, &used_edges)
            else {
                block.metadata = Some(Value::Object(metadata));
                continue;
            };
            used_edges.insert(edge_index, ());
            enrich_agent_metadata_from_edge(&mut metadata, edge);
            block.metadata = Some(Value::Object(metadata.clone()));
            if let Some(result) = &mut block.result {
                result.metadata = Some(Value::Object(metadata));
            }
        }
    }
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

fn project_message_entry(thread_id: &str, summary: &TuiMessageSummary) -> Option<TranscriptEntry> {
    match &summary.message {
        Message::User {
            content,
            timestamp_ms,
        } => {
            if let Some(shell_block) = user_shell_block(summary, *timestamp_ms) {
                return Some(entry(
                    thread_id,
                    summary,
                    TranscriptEntryRole::User,
                    shell_block.status,
                    vec![shell_block],
                    *timestamp_ms,
                ));
            }
            let text = user_content_text(content, summary.metadata.as_ref());
            let mut blocks = Vec::new();
            if !text.trim().is_empty() {
                blocks.push(block(
                    format!("message:{}:block:0", summary.session_seq),
                    TranscriptBlockKind::Text,
                    TranscriptBlockStatus::Completed,
                    0,
                    "runtime.message",
                    None,
                    Some(text.clone()),
                    Some(text),
                    summary.metadata.clone(),
                    *timestamp_ms,
                ));
            }
            Some(entry(
                thread_id,
                summary,
                TranscriptEntryRole::User,
                TranscriptBlockStatus::Completed,
                blocks,
                *timestamp_ms,
            ))
        }
        Message::Assistant {
            content,
            timestamp_ms,
            finish_reason,
            outcome,
            model,
            provider,
        } => {
            let status = if outcome.as_str() == "normal" {
                TranscriptBlockStatus::Completed
            } else {
                TranscriptBlockStatus::Failed
            };
            let is_tool_call_turn = finish_reason.as_deref() == Some("tool_calls")
                || content
                    .iter()
                    .any(|block| matches!(block, AssistantBlock::ToolCall(_)));
            let blocks = content
                .iter()
                .enumerate()
                .filter_map(|(index, content_block)| match content_block {
                    AssistantBlock::Text { text } if !text.trim().is_empty() => {
                        let mut metadata = metadata_object(summary.metadata.clone());
                        metadata.insert(
                            "message_session_seq".to_string(),
                            json!(summary.session_seq),
                        );
                        metadata.insert("content_array_index".to_string(), json!(index));
                        metadata.insert("outcome".to_string(), json!(outcome.as_str()));
                        if let Some(provider) = provider {
                            metadata.insert("provider".to_string(), json!(provider));
                        }
                        if let Some(model) = model {
                            metadata.insert("model".to_string(), json!(model));
                        }
                        if let Some(finish_reason) = finish_reason {
                            metadata.insert("finish_reason".to_string(), json!(finish_reason));
                        }
                        if let Some(accounting) = &summary.accounting {
                            metadata.insert("accounting".to_string(), accounting.clone());
                        }
                        if is_tool_call_turn {
                            metadata.insert(
                                "projection".to_string(),
                                Value::String("assistant_phase".to_string()),
                            );
                        }
                        Some(block(
                            format!("message:{}:block:{index}", summary.session_seq),
                            TranscriptBlockKind::Text,
                            status,
                            index as i64,
                            "runtime.message",
                            None,
                            Some(text.clone()),
                            Some(text.clone()),
                            Some(Value::Object(metadata)),
                            *timestamp_ms,
                        ))
                    }
                    AssistantBlock::Reasoning {
                        text,
                        provider_evidence,
                    } if !text.trim().is_empty() => {
                        let mut metadata = serde_json::Map::new();
                        metadata.insert(
                            "message_session_seq".to_string(),
                            json!(summary.session_seq),
                        );
                        metadata.insert("content_array_index".to_string(), json!(index));
                        if let Some(provider_evidence) = provider_evidence {
                            metadata
                                .insert("provider_evidence".to_string(), provider_evidence.clone());
                        }
                        Some(block(
                            format!("message:{}:block:{index}", summary.session_seq),
                            TranscriptBlockKind::Reasoning,
                            status,
                            index as i64,
                            "runtime.message",
                            Some("Thinking".to_string()),
                            Some(text.clone()),
                            Some(text.clone()),
                            Some(Value::Object(metadata)),
                            *timestamp_ms,
                        ))
                    }
                    AssistantBlock::ToolCall(call) => {
                        let mut metadata = serde_json::Map::new();
                        metadata.insert("projection".to_string(), json!("tool"));
                        metadata.insert("tool_name".to_string(), json!(call.name));
                        metadata.insert("tool_call_id".to_string(), json!(call.id));
                        metadata.insert(
                            "message_session_seq".to_string(),
                            json!(summary.session_seq),
                        );
                        metadata.insert("content_array_index".to_string(), json!(index));
                        metadata.insert("content_index".to_string(), json!(call.content_index));
                        metadata.insert("call_index".to_string(), json!(call.call_index));
                        metadata.insert("arguments".to_string(), call.arguments.clone());
                        metadata.insert("args".to_string(), call.arguments.clone());
                        metadata.insert("arguments_error".to_string(), json!(call.arguments_error));
                        Some(block(
                            format!("tool:{}", call.id),
                            tool_block_kind(&call.name),
                            TranscriptBlockStatus::Pending,
                            index as i64,
                            "runtime.message",
                            Some(call.name.clone()),
                            None,
                            Some(call.arguments_json.clone()),
                            Some(Value::Object(metadata)),
                            *timestamp_ms,
                        ))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            Some(entry(
                thread_id,
                summary,
                TranscriptEntryRole::Assistant,
                status,
                blocks,
                *timestamp_ms,
            ))
        }
        Message::ToolResult { .. } => None,
    }
}

fn user_shell_block(summary: &TuiMessageSummary, timestamp_ms: i64) -> Option<TranscriptBlock> {
    let metadata = summary
        .metadata
        .as_ref()?
        .get(USER_SHELL_METADATA_KEY)?
        .as_object()?;
    let command = metadata.get("command").and_then(Value::as_str)?.to_string();
    let result = metadata.get("result").cloned().unwrap_or(Value::Null);
    let is_error = metadata
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let outcome = metadata
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or(if is_error { "failed" } else { "normal" });
    let status = if is_error || !matches!(outcome, "normal") {
        TranscriptBlockStatus::Failed
    } else {
        TranscriptBlockStatus::Completed
    };
    let mut block_metadata = serde_json::Map::new();
    block_metadata.insert("projection".to_string(), json!("tool"));
    block_metadata.insert("tool_name".to_string(), json!("exec_command"));
    block_metadata.insert("tool_call_id".to_string(), json!("user_shell"));
    block_metadata.insert("args".to_string(), json!({"cmd": command}));
    block_metadata.insert("result".to_string(), result.clone());
    block_metadata.insert("outcome".to_string(), json!(outcome));
    block_metadata.insert("is_error".to_string(), json!(is_error));
    block_metadata.insert(
        "message_session_seq".to_string(),
        json!(summary.session_seq),
    );
    block_metadata.insert(
        USER_SHELL_METADATA_KEY.to_string(),
        Value::Object(metadata.clone()),
    );
    if let Some(elapsed_ms) = metadata.get("elapsed_ms") {
        block_metadata.insert("elapsed_ms".to_string(), elapsed_ms.clone());
    }
    let detail = serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
    Some(block(
        format!("user-shell:{}", summary.session_seq),
        TranscriptBlockKind::Shell,
        status,
        0,
        "runtime.user_shell",
        Some("exec_command".to_string()),
        Some(detail.clone()),
        Some(detail),
        Some(Value::Object(block_metadata)),
        timestamp_ms,
    ))
}

fn entry(
    thread_id: &str,
    summary: &TuiMessageSummary,
    role: TranscriptEntryRole,
    status: TranscriptBlockStatus,
    blocks: Vec<TranscriptBlock>,
    timestamp_ms: i64,
) -> TranscriptEntry {
    TranscriptEntry {
        id: format!("message:{}", summary.session_seq),
        thread_id: thread_id.to_string(),
        turn_id: Some(format!("message:{}", summary.session_seq)),
        message_seq: Some(summary.session_seq),
        role,
        status,
        source: "runtime.message".to_string(),
        blocks,
        metadata: summary.metadata.clone(),
        usage: summary.usage.clone(),
        accounting: summary.accounting.clone(),
        created_at_ms: timestamp_ms,
        updated_at_ms: timestamp_ms,
    }
}

#[allow(clippy::too_many_arguments)]
fn block(
    id: String,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    order: i64,
    source: &str,
    title: Option<String>,
    body: Option<String>,
    detail: Option<String>,
    metadata: Option<Value>,
    timestamp_ms: i64,
) -> TranscriptBlock {
    TranscriptBlock {
        id,
        kind,
        status,
        order,
        source: source.to_string(),
        title,
        preview: body
            .as_deref()
            .or(detail.as_deref())
            .map(|text| compact_text(text, 240)),
        body,
        detail,
        artifact_ids: Vec::new(),
        metadata,
        result: None,
        created_at_ms: timestamp_ms,
        updated_at_ms: timestamp_ms,
    }
}

fn attach_tool_results(entries: &mut [TranscriptEntry], summaries: &[TuiMessageSummary]) {
    let mut tool_blocks = BTreeMap::<String, (usize, usize)>::new();
    for (entry_index, entry) in entries.iter().enumerate() {
        for (block_index, block) in entry.blocks.iter().enumerate() {
            if let Some(call_id) = block_tool_call_id(block) {
                tool_blocks.insert(call_id.to_string(), (entry_index, block_index));
            }
        }
    }

    for summary in summaries {
        let Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            timestamp_ms,
        } = &summary.message
        else {
            continue;
        };
        let Some((entry_index, block_index)) = tool_blocks.get(tool_call_id).copied() else {
            continue;
        };
        let Some(block) = entries
            .get_mut(entry_index)
            .and_then(|entry| entry.blocks.get_mut(block_index))
        else {
            continue;
        };
        let status = if *is_error {
            TranscriptBlockStatus::Failed
        } else {
            TranscriptBlockStatus::Completed
        };
        let result_value = serde_json::from_str::<Value>(content).unwrap_or_else(|_| {
            json!({
                "content": content,
            })
        });
        let mut metadata = metadata_object(block.metadata.take());
        metadata.insert("projection".to_string(), json!("tool"));
        metadata.insert("tool_name".to_string(), json!(tool_name));
        metadata.insert("tool_call_id".to_string(), json!(tool_call_id));
        metadata.insert(
            "outcome".to_string(),
            json!(if *is_error { "failed" } else { "normal" }),
        );
        metadata.insert("is_error".to_string(), json!(is_error));
        metadata.insert(
            "tool_result_message_session_seq".to_string(),
            json!(summary.session_seq),
        );
        if let Some(message_metadata) = &summary.metadata {
            metadata.insert("message_metadata".to_string(), message_metadata.clone());
            if let Some(elapsed_ms) = message_metadata.get("elapsed_ms") {
                metadata.insert("elapsed_ms".to_string(), elapsed_ms.clone());
            }
        }
        if let Some(display) = result_value
            .get("display")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|display| !display.is_empty())
        {
            metadata.insert("display".to_string(), json!(display));
            block.title = Some(display.to_string());
        }
        if let Some(source) = result_value
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|source| !source.is_empty())
        {
            metadata.insert("source".to_string(), json!(source));
        }
        metadata.insert("result".to_string(), result_value);
        if tool_name == "Agent" || tool_name == "agent" {
            enrich_committed_agent_metadata(&mut metadata);
        }
        block.metadata = Some(Value::Object(metadata.clone()));
        block.result = Some(TranscriptToolResult {
            result_message_seq: summary.session_seq,
            status,
            content: content.clone(),
            is_error: *is_error,
            metadata: summary.metadata.clone(),
            created_at_ms: *timestamp_ms,
            updated_at_ms: *timestamp_ms,
        });
        block.status = status;
        block.body = Some(content.clone());
        block.detail = Some(content.clone());
        block.preview = Some(compact_text(content, 240));
        block.updated_at_ms = *timestamp_ms;
        if let Some(entry) = entries.get_mut(entry_index) {
            entry.updated_at_ms = entry.updated_at_ms.max(*timestamp_ms);
        }
    }
}

fn merge_write_stdin_blocks(entries: &mut [TranscriptEntry]) {
    let mut exec_blocks = BTreeMap::<u64, (usize, usize)>::new();
    let mut hidden_blocks = Vec::<(usize, usize)>::new();

    for entry_index in 0..entries.len() {
        for block_index in 0..entries[entry_index].blocks.len() {
            let tool = block_tool_name(&entries[entry_index].blocks[block_index]);
            match tool.as_deref() {
                Some("exec_command") => {
                    if let Some(metadata) =
                        entries[entry_index].blocks[block_index].metadata.as_ref()
                        && let Some(session_id) = exec_session_id_from_result(metadata)
                        && exec_result_running(metadata)
                    {
                        exec_blocks.insert(session_id, (entry_index, block_index));
                    }
                }
                Some("write_stdin") => {
                    let Some(metadata) = entries[entry_index].blocks[block_index].metadata.clone()
                    else {
                        continue;
                    };
                    let Some(session_id) = write_stdin_target_session_id(&metadata) else {
                        continue;
                    };
                    let Some((exec_entry_index, exec_block_index)) =
                        exec_blocks.get(&session_id).copied()
                    else {
                        continue;
                    };
                    let write_block = entries[entry_index].blocks[block_index].clone();
                    if let Some(exec_block) = entries
                        .get_mut(exec_entry_index)
                        .and_then(|entry| entry.blocks.get_mut(exec_block_index))
                    {
                        merge_write_stdin_into_exec_block(exec_block, &write_block);
                    }
                    hidden_blocks.push((entry_index, block_index));
                    if exec_result_completed(&metadata) {
                        exec_blocks.remove(&session_id);
                    }
                }
                _ => {}
            }
        }
    }

    for (entry_index, block_index) in hidden_blocks {
        if let Some(block) = entries
            .get_mut(entry_index)
            .and_then(|entry| entry.blocks.get_mut(block_index))
        {
            let mut metadata = metadata_object(block.metadata.take());
            metadata.insert("hidden".to_string(), Value::Bool(true));
            block.metadata = Some(Value::Object(metadata));
        }
    }
}

fn merge_write_stdin_into_exec_block(
    exec_block: &mut TranscriptBlock,
    write_block: &TranscriptBlock,
) {
    let Some(write_metadata) = write_block.metadata.as_ref() else {
        return;
    };
    let mut exec_metadata = metadata_object(exec_block.metadata.take());
    let output = tool_result_output(write_metadata);
    if !output.is_empty() {
        let result = ensure_json_object_field(&mut exec_metadata, "result");
        let next = match result.get("output").and_then(Value::as_str) {
            Some(existing) if existing.ends_with(&output) => existing.to_string(),
            Some(existing) => format!("{existing}{output}"),
            None => output,
        };
        result.insert("output".to_string(), Value::String(next));
    }
    if exec_result_completed(write_metadata) {
        let result = ensure_json_object_field(&mut exec_metadata, "result");
        if let Some(exit_code) = write_metadata
            .get("result")
            .and_then(|result| result.get("exit_code"))
            .filter(|value| !value.is_null())
        {
            result.insert("exit_code".to_string(), exit_code.clone());
        }
        if let Some(outcome) = write_metadata.get("outcome") {
            exec_metadata.insert("outcome".to_string(), outcome.clone());
        }
        exec_block.status = write_block.status;
    }
    if let Some(delta) = write_metadata.get("elapsed_ms").and_then(Value::as_u64) {
        let total = exec_metadata
            .get("elapsed_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .saturating_add(delta);
        exec_metadata.insert("elapsed_ms".to_string(), Value::from(total));
    }
    let text = exec_metadata
        .get("result")
        .and_then(|result| serde_json::to_string(result).ok());
    if let Some(text) = text {
        exec_block.body = Some(text.clone());
        exec_block.detail = Some(text.clone());
        exec_block.preview = Some(compact_text(&text, 240));
    }
    exec_block.metadata = Some(Value::Object(exec_metadata));
    exec_block.updated_at_ms = exec_block.updated_at_ms.max(write_block.updated_at_ms);
}

fn enrich_committed_agent_metadata(metadata: &mut serde_json::Map<String, Value>) {
    let args = metadata.get("args").cloned().unwrap_or(Value::Null);
    let result = ensure_json_object_field(metadata, "result");
    for key in [
        "agent_name",
        "agent_type",
        "name",
        "task_name",
        "parent_session_id",
        "child_session_id",
        "session_id",
    ] {
        if result.get(key).is_none()
            && let Some(value) = args.get(key).filter(|value| !value.is_null())
        {
            result.insert(key.to_string(), value.clone());
        }
    }
    if result.get("task").is_none()
        && let Some(prompt) = args
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
    {
        result.insert("task".to_string(), json!(prompt));
    }
    if result.get("child_session_id").is_none()
        && let Some(session_id) = result
            .get("session_id")
            .filter(|value| !value.is_null())
            .cloned()
    {
        result.insert("child_session_id".to_string(), session_id);
    }
    if result.get("session_id").is_none()
        && let Some(child_session_id) = result
            .get("child_session_id")
            .filter(|value| !value.is_null())
            .cloned()
    {
        result.insert("session_id".to_string(), child_session_id);
    }
}

fn matching_agent_edge_for_block<'a>(
    metadata: &serde_json::Map<String, Value>,
    edges: &'a [AgentEdgeRecord],
    used_edges: &BTreeMap<usize, ()>,
) -> Option<(usize, &'a AgentEdgeRecord)> {
    let args = metadata.get("args").unwrap_or(&Value::Null);
    let result = metadata.get("result").unwrap_or(&Value::Null);
    let tool_call_id = metadata
        .get("tool_call_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(match_by_tool_call) = edges.iter().enumerate().find(|(index, edge)| {
        !used_edges.contains_key(index)
            && tool_call_id
                .is_some_and(|id| agent_edge_string(edge, "parent_tool_call_id") == Some(id))
    }) {
        return Some(match_by_tool_call);
    }

    let result_agent_id = result
        .get("agent_id")
        .or_else(|| result.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(match_by_agent_id) = edges.iter().enumerate().find(|(index, edge)| {
        !used_edges.contains_key(index)
            && result_agent_id.is_some_and(|id| agent_edge_string(edge, "id") == Some(id))
    }) {
        return Some(match_by_agent_id);
    }

    let agent_name = result
        .get("agent_name")
        .or_else(|| result.get("agent_type"))
        .or_else(|| args.get("agent_type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let task_label = result
        .get("task_name")
        .or_else(|| args.get("task_name"))
        .or_else(|| result.get("task"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let task_prompt = result
        .get("task")
        .or_else(|| args.get("prompt"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    edges.iter().enumerate().find(|(index, edge)| {
        !used_edges.contains_key(index)
            && agent_name.is_some_and(|name| agent_edge_string(edge, "name") == Some(name))
            && (task_label.is_some_and(|label| {
                agent_edge_string(edge, "task_name") == Some(label)
                    || agent_edge_string(edge, "task") == Some(label)
            }) || task_prompt
                .is_some_and(|prompt| agent_edge_string(edge, "task") == Some(prompt)))
    })
}

fn enrich_agent_metadata_from_edge(
    metadata: &mut serde_json::Map<String, Value>,
    edge: &AgentEdgeRecord,
) {
    let result = ensure_json_object_field(metadata, "result");
    result.insert(
        "child_session_id".to_string(),
        Value::String(edge.child_session_id.clone()),
    );
    result.insert(
        "session_id".to_string(),
        Value::String(edge.child_session_id.clone()),
    );
    result.insert(
        "parent_session_id".to_string(),
        Value::String(edge.parent_session_id.clone()),
    );
    if let Some(value) = agent_edge_string(edge, "id")
        && result.get("agent_id").is_none()
    {
        result.insert("agent_id".to_string(), Value::String(value.to_string()));
    }
    for key in ["name", "task_name", "task"] {
        if let Some(value) = agent_edge_string(edge, key) {
            let result_key = if key == "name" { "agent_name" } else { key };
            result
                .entry(result_key.to_string())
                .or_insert_with(|| Value::String(value.to_string()));
        }
    }
}

fn agent_result_child_session_id(metadata: &serde_json::Map<String, Value>) -> Option<&str> {
    metadata
        .get("result")
        .and_then(|result| {
            result
                .get("child_session_id")
                .or_else(|| result.get("session_id"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn agent_edge_string<'a>(edge: &'a AgentEdgeRecord, key: &str) -> Option<&'a str> {
    edge.metadata
        .as_ref()?
        .get("agent")?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn user_content_text(content: &[UserContentBlock], metadata: Option<&Value>) -> String {
    if let Some(display) = metadata
        .and_then(|metadata| metadata.get(TUI_DISPLAY_METADATA_KEY))
        .and_then(|display| display.get("content_text"))
        .and_then(Value::as_str)
    {
        return display.to_string();
    }
    content
        .iter()
        .map(|block| match block {
            UserContentBlock::Text(block) => block.text.as_str(),
            UserContentBlock::LocalImage(block) => block.path.to_str().unwrap_or("[image]"),
            UserContentBlock::ImageUrl(block) => block.url.as_str(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn block_tool_name(block: &TranscriptBlock) -> Option<String> {
    block
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("tool_name"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| block.title.clone())
}

fn block_tool_call_id(block: &TranscriptBlock) -> Option<&str> {
    block
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("tool_call_id"))
        .and_then(Value::as_str)
}

fn tool_block_kind(tool_name: &str) -> TranscriptBlockKind {
    match tool_name {
        "exec_command" | "write_stdin" => TranscriptBlockKind::Shell,
        "read" | "write" | "edit" | "apply_patch" => TranscriptBlockKind::File,
        "web_fetch" | "web_search" => TranscriptBlockKind::Web,
        "mcp" | "mcp_call" => TranscriptBlockKind::Mcp,
        "clarify" => TranscriptBlockKind::Clarify,
        "Agent" | "agent" => TranscriptBlockKind::Agent,
        _ => TranscriptBlockKind::ToolCall,
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

fn write_stdin_target_session_id(metadata: &Value) -> Option<u64> {
    metadata
        .get("args")
        .and_then(exec_session_id_from_args)
        .or_else(|| {
            metadata
                .get("arguments")
                .and_then(exec_session_id_from_args)
        })
        .or_else(|| exec_session_id_from_result(metadata))
}

fn exec_session_id_from_args(args: &Value) -> Option<u64> {
    args.get("session_id").and_then(Value::as_u64)
}

fn exec_session_id_from_result(value: &Value) -> Option<u64> {
    value
        .get("result")
        .and_then(|result| result.get("session_id"))
        .and_then(Value::as_u64)
}

fn exec_result_running(value: &Value) -> bool {
    exec_session_id_from_result(value).is_some()
        && value
            .get("result")
            .and_then(|result| result.get("exit_code"))
            .is_none_or(Value::is_null)
}

fn exec_result_completed(value: &Value) -> bool {
    value
        .get("result")
        .and_then(|result| result.get("exit_code"))
        .is_some_and(|exit_code| !exit_code.is_null())
}

fn tool_result_output(value: &Value) -> String {
    value
        .get("result")
        .and_then(|result| result.get("output"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
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
mod tests {
    use super::*;

    include!("transcript/tests.rs");
}
