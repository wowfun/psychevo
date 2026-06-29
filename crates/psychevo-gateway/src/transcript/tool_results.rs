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
        if tool_name == "spawn_agent" {
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
