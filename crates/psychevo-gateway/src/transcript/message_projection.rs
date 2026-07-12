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
            let mut blocks = content
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
                            Some(tool_call_title(&call.name, &call.arguments)),
                            None,
                            Some(call.arguments_json.clone()),
                            Some(Value::Object(metadata)),
                            *timestamp_ms,
                        ))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if let Some(plan) = acp_plan_block(summary, status, *timestamp_ms, &blocks) {
                blocks.push(plan);
            }
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

fn acp_plan_block(
    summary: &TuiMessageSummary,
    status: TranscriptBlockStatus,
    timestamp_ms: i64,
    existing_blocks: &[TranscriptBlock],
) -> Option<TranscriptBlock> {
    let acp = summary.metadata.as_ref()?.get("acp")?;
    let turn_id = acp.get("turnId")?.as_str()?.trim();
    let projection = acp.get("plan")?;
    let body = projection.get("body")?.as_str()?.trim();
    let update = projection.get("update")?;
    if turn_id.is_empty()
        || body.is_empty()
        || update.get("sessionUpdate").and_then(Value::as_str) != Some("plan")
    {
        return None;
    }
    let order = existing_blocks
        .iter()
        .map(|block| block.order)
        .max()
        .unwrap_or(-1)
        .saturating_add(1);
    let metadata = json!({
        "projection": "acp_peer_plan",
        "origin": "acp_peer",
        "source": "acp_peer",
        "turnId": turn_id,
        "plan": update,
    });
    Some(block(
        format!("turn:{turn_id}:acp-peer-plan"),
        TranscriptBlockKind::Status,
        status,
        order,
        "runtime.message",
        Some("Plan".to_string()),
        Some(body.to_string()),
        Some(body.to_string()),
        Some(metadata),
        timestamp_ms,
    ))
}

fn tool_call_title(tool_name: &str, arguments: &Value) -> String {
    if tool_name == "exec_command"
        && let Some(command) = arguments
            .get("cmd")
            .or_else(|| arguments.get("command"))
            .and_then(Value::as_str)
            .map(first_effective_command)
            .filter(|command| !command.is_empty())
    {
        return compact_text(&format!("exec_command {command}"), 180);
    }
    if tool_name == "write_stdin"
        && let Some(session_id) = arguments
            .get("session_id")
            .and_then(|value| value.as_u64().map(|value| value.to_string()))
            .filter(|session_id| !session_id.is_empty())
    {
        return compact_text(&format!("write_stdin {session_id}"), 180);
    }
    tool_name.to_string()
}

fn first_effective_command(command: &str) -> String {
    command
        .split('\n')
        .map(|line| line.trim())
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or_else(|| command.trim())
        .to_string()
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
        phase_ordinal: None,
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

fn tool_block_kind(tool_name: &str) -> TranscriptBlockKind {
    match tool_name {
        "exec_command" | "write_stdin" => TranscriptBlockKind::Shell,
        "read" | "write" | "edit" | "apply_patch" => TranscriptBlockKind::File,
        "web_fetch" | "web_search" => TranscriptBlockKind::Web,
        "mcp" | "mcp_call" => TranscriptBlockKind::Mcp,
        "clarify" => TranscriptBlockKind::Clarify,
        "spawn_agent" => TranscriptBlockKind::Agent,
        "image_generate" | "image_generation.generate" | "image_generation__generate" => {
            TranscriptBlockKind::Artifact
        }
        _ => TranscriptBlockKind::ToolCall,
    }
}
