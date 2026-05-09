fn chat_completions_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

fn build_chat_request(request: &GenerationRequest, base_url: &str) -> Value {
    let mut body = json!({
        "model": request.model.model,
        "messages": translate_messages(&request.messages, &request.model, base_url),
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if !request.tools.is_empty() {
        body["tools"] = Value::Array(
            request
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        }
                    })
                })
                .collect(),
        );
    }
    if let Some(reasoning_effort) = request
        .metadata
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        body["reasoning_effort"] = Value::String(reasoning_effort.to_string());
    }
    body
}

fn translate_messages(messages: &[Value], target: &ModelTarget, base_url: &str) -> Vec<Value> {
    let projected = messages
        .iter()
        .flat_map(|message| translate_message(message, target, base_url))
        .collect::<Vec<_>>();
    merge_adjacent_user_messages(projected)
}

fn translate_message(message: &Value, target: &ModelTarget, base_url: &str) -> Vec<Value> {
    match message.get("role").and_then(Value::as_str) {
        Some("system") => system_messages(message),
        Some("user") => user_messages(message),
        Some("assistant") => assistant_messages(message, target, base_url),
        Some("tool_result") => tool_result_messages(message),
        _ => Vec::new(),
    }
}

fn system_messages(message: &Value) -> Vec<Value> {
    message
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| vec![json!({ "role": "system", "content": text })])
        .unwrap_or_default()
}

fn user_messages(message: &Value) -> Vec<Value> {
    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .map(|text| json!({ "role": "user", "content": text }))
        .collect()
}

fn assistant_messages(message: &Value, target: &ModelTarget, base_url: &str) -> Vec<Value> {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut normalized_reasoning = Vec::new();
    if let Some(blocks) = message.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(value) = block.get("text").and_then(Value::as_str) {
                        text.push_str(value);
                    }
                }
                Some("tool_call") => {
                    let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let arguments = block
                        .get("arguments_json")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    if !id.is_empty() && !name.is_empty() {
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": arguments,
                            }
                        }));
                    }
                }
                Some("reasoning") => {
                    if let Some(value) = block.get("text").and_then(Value::as_str)
                        && !value.is_empty()
                    {
                        normalized_reasoning.push(value.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    if text.is_empty() && tool_calls.is_empty() {
        return Vec::new();
    }
    let has_text = !text.is_empty();
    let mut output = json!({
        "role": "assistant",
        "content": has_text.then_some(text),
    });
    if !tool_calls.is_empty() {
        output["tool_calls"] = Value::Array(tool_calls);
    }
    let has_tool_calls = output
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|calls| !calls.is_empty());
    apply_reasoning_content_for_api(
        message,
        &mut output,
        has_text,
        has_tool_calls,
        &normalized_reasoning.join("\n\n"),
        target,
        base_url,
    );
    vec![output]
}

fn merge_adjacent_user_messages(messages: Vec<Value>) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::new();
    for message in messages {
        let is_user = message.get("role").and_then(Value::as_str) == Some("user");
        if is_user
            && let Some(last) = merged.last_mut()
            && last.get("role").and_then(Value::as_str) == Some("user")
        {
            let previous = last
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let current = message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            last["content"] = Value::String(format!("{previous}\n\n{current}"));
            continue;
        }
        merged.push(message);
    }
    merged
}

fn apply_reasoning_content_for_api(
    source: &Value,
    output: &mut Value,
    has_text: bool,
    has_tool_calls: bool,
    normalized_reasoning: &str,
    target: &ModelTarget,
    base_url: &str,
) {
    if !needs_thinking_reasoning_pad(target, base_url) {
        return;
    }
    if !has_text && !has_tool_calls {
        return;
    }
    if !source_provider_matches_target(source, target) {
        output["reasoning_content"] = Value::String(" ".to_string());
        return;
    }
    let value = if normalized_reasoning.trim().is_empty() {
        " ".to_string()
    } else {
        normalized_reasoning.to_string()
    };
    output["reasoning_content"] = Value::String(value);
}

fn source_provider_matches_target(source: &Value, target: &ModelTarget) -> bool {
    source
        .get("provider")
        .and_then(Value::as_str)
        .is_some_and(|provider| provider.eq_ignore_ascii_case(&target.provider))
}

fn needs_thinking_reasoning_pad(target: &ModelTarget, base_url: &str) -> bool {
    let provider = target.provider.to_lowercase();
    let model = target.model.to_lowercase();
    provider == "deepseek"
        || model.contains("deepseek")
        || base_url_host_matches(base_url, "api.deepseek.com")
        || provider == "kimi-coding"
        || provider == "kimi-coding-cn"
        || base_url_host_matches(base_url, "api.kimi.com")
        || base_url_host_matches(base_url, "moonshot.ai")
        || base_url_host_matches(base_url, "moonshot.cn")
}

fn base_url_host_matches(base_url: &str, needle: &str) -> bool {
    let lower = base_url.to_lowercase();
    lower
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(lower.as_str())
        .split('/')
        .next()
        .unwrap_or_default()
        .ends_with(needle)
}

fn tool_result_messages(message: &Value) -> Vec<Value> {
    let tool_call_id = message
        .get("tool_call_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if tool_call_id.is_empty() {
        return Vec::new();
    }
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    vec![json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": content,
    })]
}
