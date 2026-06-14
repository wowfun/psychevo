fn acp_update_kind(update: &Value) -> String {
    update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

fn acp_content_chunk_text(chunk: ContentChunk) -> Option<String> {
    match chunk.content {
        ContentBlock::Text(text) => Some(text.text),
        _ => None,
    }
}

fn acp_v2_content_chunk_text(chunk: acp_v2::ContentChunk) -> Option<String> {
    match chunk.content {
        acp_v2::ContentBlock::Text(text) => Some(text.text),
        _ => None,
    }
}

fn acp_tool_call_id(value: &Value) -> String {
    value
        .get("toolCallId")
        .or_else(|| value.get("tool_call_id"))
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string()
}

fn acp_merge_tool_update(existing: &Value, update: &Value) -> Value {
    let mut merged = existing.as_object().cloned().unwrap_or_default();
    if let Some(update) = update.as_object() {
        for (key, value) in update {
            if key == "sessionUpdate" || value.is_null() {
                continue;
            }
            merged.insert(key.clone(), value.clone());
        }
    }
    merged.insert(
        "sessionUpdate".to_string(),
        Value::String("tool_call".to_string()),
    );
    Value::Object(merged)
}

fn acp_tool_runtime_event(local_session_id: &str, tool: &Value, was_started: bool) -> Value {
    let status = tool
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("pending");
    let event_type = match status {
        "completed" | "failed" => "tool_execution_end",
        "in_progress" if was_started => "tool_execution_update",
        "in_progress" => "tool_execution_start",
        _ => "tool_call_pending",
    };
    let mut event = Map::new();
    event.insert("type".to_string(), json!(event_type));
    event.insert("session_id".to_string(), json!(local_session_id));
    event.insert("source".to_string(), json!("acp_peer"));
    event.insert("tool_call_id".to_string(), json!(acp_tool_call_id(tool)));
    event.insert("tool_name".to_string(), json!(acp_tool_runtime_name(tool)));
    if let Some(title) = acp_tool_title(tool) {
        event.insert("display".to_string(), json!(title));
    }
    if let Some(args) = acp_tool_args(tool) {
        event.insert("args".to_string(), args.clone());
        event.insert(
            "arguments_json".to_string(),
            json!(serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string())),
        );
    }
    match event_type {
        "tool_execution_update" => {
            event.insert("partial_result".to_string(), acp_tool_result(tool));
        }
        "tool_execution_end" => {
            event.insert("result".to_string(), acp_tool_result(tool));
            event.insert(
                "outcome".to_string(),
                json!(if status == "failed" {
                    "failed"
                } else {
                    "normal"
                }),
            );
        }
        _ => {}
    }
    event.insert(
        "metadata".to_string(),
        json!({
            "origin": "acp_peer",
            "acp_update": tool,
        }),
    );
    Value::Object(event)
}

fn acp_tool_started_after_event(event: &Value) -> bool {
    matches!(
        event.get("type").and_then(Value::as_str),
        Some("tool_execution_start" | "tool_execution_update" | "tool_execution_end")
    )
}

fn acp_tool_title(tool: &Value) -> Option<String> {
    tool.get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(ToString::to_string)
}

fn acp_tool_runtime_name(tool: &Value) -> String {
    match tool.get("kind").and_then(Value::as_str).unwrap_or("other") {
        "read" => "read".to_string(),
        "edit" | "delete" | "move" => "edit".to_string(),
        "execute" => "exec_command".to_string(),
        "fetch" => "web_fetch".to_string(),
        "search" => "search".to_string(),
        "think" => "task".to_string(),
        "switch_mode" => "mode".to_string(),
        _ => acp_tool_title(tool).unwrap_or_else(|| "tool".to_string()),
    }
}

fn acp_tool_args(tool: &Value) -> Option<Value> {
    tool.get("rawInput")
        .or_else(|| tool.get("raw_input"))
        .filter(|value| !value.is_null())
        .cloned()
}

fn acp_tool_call_block(tool: &Value, content_index: usize, call_index: usize) -> ToolCallBlock {
    let arguments = acp_tool_args(tool).unwrap_or_else(|| Value::Object(Map::new()));
    let arguments_json = serde_json::to_string(&arguments).unwrap_or_else(|_| "{}".to_string());
    ToolCallBlock {
        id: acp_tool_call_id(tool),
        name: acp_tool_runtime_name(tool),
        arguments,
        arguments_json,
        arguments_error: None,
        content_index,
        call_index,
    }
}

fn acp_tool_result(tool: &Value) -> Value {
    let mut result = Map::new();
    if let Some(title) = acp_tool_title(tool) {
        result.insert("display".to_string(), json!(title));
    }
    result.insert("source".to_string(), json!("acp_peer"));
    if let Some(output) = acp_tool_output(tool) {
        result.insert("output".to_string(), json!(output));
    }
    if let Some(raw_output) = tool
        .get("rawOutput")
        .or_else(|| tool.get("raw_output"))
        .filter(|value| !value.is_null())
    {
        result.insert("raw_output".to_string(), raw_output.clone());
    }
    if let Some(content) = tool.get("content").filter(|value| !value.is_null()) {
        result.insert("content".to_string(), content.clone());
    }
    if let Some(locations) = tool.get("locations").filter(|value| !value.is_null()) {
        result.insert("locations".to_string(), locations.clone());
    }
    Value::Object(result)
}

fn acp_tool_output(tool: &Value) -> Option<String> {
    if let Some(content) = tool.get("content").and_then(Value::as_array) {
        let text = content
            .iter()
            .filter_map(acp_tool_content_text)
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    let raw_output = tool
        .get("rawOutput")
        .or_else(|| tool.get("raw_output"))
        .filter(|value| !value.is_null())?;
    if let Some(output) = raw_output.as_str() {
        return Some(output.to_string());
    }
    if let Some(output) = raw_output.get("output").and_then(Value::as_str) {
        return Some(output.to_string());
    }
    serde_json::to_string(raw_output).ok()
}

fn acp_tool_content_text(content: &Value) -> Option<String> {
    match content.get("type").and_then(Value::as_str) {
        Some("content") => {
            let content = content.get("content")?;
            match content.get("type").and_then(Value::as_str) {
                Some("text") => content
                    .get("text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                Some("image") => Some("[image]".to_string()),
                Some("resource_link") => content
                    .get("uri")
                    .and_then(Value::as_str)
                    .map(|uri| format!("Resource: {uri}")),
                Some("resource") => Some("[resource]".to_string()),
                _ => None,
            }
        }
        Some("diff") => {
            let path = content
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("file");
            Some(format!("Diff: {path}"))
        }
        Some("terminal") => {
            let terminal_id = content
                .get("terminalId")
                .or_else(|| content.get("terminal_id"))
                .and_then(Value::as_str)
                .unwrap_or("terminal");
            Some(format!("Terminal: {terminal_id}"))
        }
        _ => None,
    }
}

fn acp_plan_body(plan: &Value) -> String {
    let entries = plan
        .get("entries")
        .and_then(Value::as_array)
        .or_else(|| plan.get("plan")?.get("entries")?.as_array());
    let Some(entries) = entries else {
        return serde_json::to_string_pretty(plan).unwrap_or_else(|_| "ACP plan".to_string());
    };
    if entries.is_empty() {
        return "No plan entries.".to_string();
    }
    entries
        .iter()
        .map(|entry| {
            let status = entry
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending");
            let marker = match status {
                "completed" => "x",
                "in_progress" => "~",
                _ => " ",
            };
            let content = entry
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("Untitled task");
            format!("- [{marker}] {content}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
