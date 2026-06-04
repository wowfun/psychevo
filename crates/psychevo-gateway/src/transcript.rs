use std::collections::BTreeMap;

use psychevo_runtime::{
    AssistantBlock, Message, TUI_DISPLAY_METADATA_KEY, TuiMessageSummary, UserContentBlock,
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

fn project_message_entry(thread_id: &str, summary: &TuiMessageSummary) -> Option<TranscriptEntry> {
    match &summary.message {
        Message::User {
            content,
            timestamp_ms,
        } => {
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
        metadata.insert("result".to_string(), result_value);
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
    use psychevo_runtime::{Outcome, UserContentBlock};

    #[test]
    fn projector_preserves_assistant_block_order_and_attaches_tool_result() {
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![
                        AssistantBlock::Reasoning {
                            text: "think first".to_string(),
                            provider_evidence: None,
                        },
                        AssistantBlock::Text {
                            text: "I will run date.".to_string(),
                        },
                        tool_call("call_exec", "exec_command", json!({"cmd": "date"})),
                    ],
                    timestamp_ms: 10,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_exec".to_string(),
                    tool_name: "exec_command".to_string(),
                    content: "{\"exit_code\":0,\"output\":\"today\\n\"}".to_string(),
                    is_error: false,
                    timestamp_ms: 20,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_seq, Some(1));
        assert_eq!(
            entries[0]
                .blocks
                .iter()
                .map(|block| block.kind)
                .collect::<Vec<_>>(),
            vec![
                TranscriptBlockKind::Reasoning,
                TranscriptBlockKind::Text,
                TranscriptBlockKind::Shell,
            ]
        );
        assert_eq!(entries[0].blocks[0].body.as_deref(), Some("think first"));
        assert_eq!(
            entries[0].blocks[1].body.as_deref(),
            Some("I will run date.")
        );
        assert_eq!(
            entries[0].blocks[1].metadata.as_ref().unwrap()["projection"],
            "assistant_phase"
        );
        let tool = &entries[0].blocks[2];
        assert_eq!(tool.status, TranscriptBlockStatus::Completed);
        assert_eq!(tool.result.as_ref().unwrap().result_message_seq, 2);
        assert_eq!(tool.metadata.as_ref().unwrap()["args"]["cmd"], "date");
        assert_eq!(
            tool.metadata.as_ref().unwrap()["result"]["output"],
            "today\n"
        );
    }

    #[test]
    fn projector_does_not_create_top_level_entry_for_unmatched_tool_result() {
        let entries = project_transcript_entries(
            "thread-1",
            &[summary(
                1,
                Message::ToolResult {
                    tool_call_id: "missing".to_string(),
                    tool_name: "exec_command".to_string(),
                    content: "orphan".to_string(),
                    is_error: true,
                    timestamp_ms: 10,
                },
            )],
        );

        assert!(entries.is_empty());
    }

    #[test]
    fn projector_merges_write_stdin_poll_into_exec_command_block() {
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_exec",
                        "exec_command",
                        json!({"cmd": "printf first"}),
                    )],
                    timestamp_ms: 1,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_exec".to_string(),
                    tool_name: "exec_command".to_string(),
                    content: "{\"session_id\":7,\"exit_code\":null,\"output\":\"first\\n\"}"
                        .to_string(),
                    is_error: false,
                    timestamp_ms: 2,
                },
            ),
            summary(
                3,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_poll",
                        "write_stdin",
                        json!({"session_id": 7, "yield_time_ms": 60000}),
                    )],
                    timestamp_ms: 3,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                4,
                Message::ToolResult {
                    tool_call_id: "call_poll".to_string(),
                    tool_name: "write_stdin".to_string(),
                    content: "{\"session_id\":null,\"exit_code\":0,\"output\":\"second\\n\"}"
                        .to_string(),
                    is_error: false,
                    timestamp_ms: 4,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);
        let exec = entries
            .iter()
            .flat_map(|entry| entry.blocks.iter())
            .find(|block| block.title.as_deref() == Some("exec_command"))
            .expect("exec block");
        let poll = entries
            .iter()
            .flat_map(|entry| entry.blocks.iter())
            .find(|block| block.title.as_deref() == Some("write_stdin"))
            .expect("write_stdin block");

        assert_eq!(exec.status, TranscriptBlockStatus::Completed);
        assert_eq!(
            exec.metadata.as_ref().unwrap()["result"]["output"],
            "first\nsecond\n"
        );
        assert_eq!(exec.metadata.as_ref().unwrap()["result"]["exit_code"], 0);
        assert_eq!(poll.metadata.as_ref().unwrap()["hidden"], true);
    }

    #[test]
    fn projector_keeps_selected_skill_metadata_on_user_entry() {
        let mut user = summary(
            1,
            Message::User {
                content: vec![UserContentBlock::text("$x-daily")],
                timestamp_ms: 1,
            },
        );
        user.metadata = Some(json!({
            "prompt_prefix": {
                "selected_skills": [{"name": "x-daily", "path": "/tmp/x/SKILL.md"}]
            }
        }));

        let entries = project_transcript_entries("thread-1", &[user]);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].role, TranscriptEntryRole::User);
        assert_eq!(
            entries[0].metadata.as_ref().unwrap()["prompt_prefix"]["selected_skills"][0]["name"],
            "x-daily"
        );
        assert_eq!(
            entries[0].blocks[0].metadata.as_ref().unwrap()["prompt_prefix"]["selected_skills"][0]
                ["path"],
            "/tmp/x/SKILL.md"
        );
    }

    #[test]
    fn committed_turn_projection_filters_by_first_message_sequence() {
        let summaries = vec![
            summary(
                1,
                Message::User {
                    content: vec![UserContentBlock::text("old")],
                    timestamp_ms: 1,
                },
            ),
            summary(
                2,
                Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "new".to_string(),
                    }],
                    timestamp_ms: 2,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
        ];

        let entries = project_committed_turn_entries("thread-1", &summaries, 2);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_seq, Some(2));
        assert_eq!(entries[0].blocks[0].body.as_deref(), Some("new"));
    }

    fn summary(session_seq: i64, message: Message) -> TuiMessageSummary {
        TuiMessageSummary {
            session_seq,
            message,
            usage: None,
            metadata: None,
            accounting: None,
        }
    }

    fn tool_call(id: &str, name: &str, arguments: Value) -> AssistantBlock {
        let arguments_json = arguments.to_string();
        serde_json::from_value(json!({
            "type": "tool_call",
            "id": id,
            "name": name,
            "arguments": arguments,
            "arguments_json": arguments_json,
            "arguments_error": null,
            "content_index": 0,
            "call_index": 0
        }))
        .expect("tool call block")
    }
}
