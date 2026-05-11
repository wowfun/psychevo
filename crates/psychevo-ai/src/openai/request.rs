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
    if !request.tools.is_empty() && !capability_is_false(&request.metadata, "tool_call") {
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
        .filter(|_| !capability_is_false(&request.metadata, "reasoning"))
    {
        body["reasoning_effort"] = Value::String(reasoning_effort.to_string());
    }
    body
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiChatTokenCount {
    pub encoding: String,
    pub encoding_source: String,
    pub encoding_fallback: bool,
    pub system_prompt_tokens: u64,
    pub system_tools_tokens: u64,
    pub skills_tokens: u64,
    pub messages_tokens: u64,
    pub total_estimated_tokens: u64,
    pub tool_count: usize,
    pub role_counts: BTreeMap<String, OpenAiChatRoleTokenCount>,
    pub selected_skill_context_tokens: u64,
    pub selected_skill_context_count: usize,
    pub skill_names: Vec<String>,
    pub skill_entries: Vec<OpenAiChatSkillTokenCount>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiChatRoleTokenCount {
    pub count: usize,
    pub tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiChatSkillTokenCount {
    pub name: String,
    pub tokens: u64,
}

pub fn count_openai_chat_request(
    request: &GenerationRequest,
    base_url: &str,
) -> OpenAiChatTokenCount {
    let encoding = resolve_count_encoding(&request.model.provider, &request.model.model);
    let Some(enc) = tiktoken::get_encoding(&encoding.name) else {
        return OpenAiChatTokenCount {
            encoding: "o200k_base".to_string(),
            encoding_source: "fallback".to_string(),
            encoding_fallback: true,
            ..OpenAiChatTokenCount::default()
        };
    };
    let counting = request_context_counting_metadata(request);
    let body = build_chat_request(request, base_url);
    let (system_tools_tokens, tool_count) = body
        .get("tools")
        .map(|tools| (count_value(enc, tools), tools.as_array().map_or(0, Vec::len)))
        .unwrap_or((0, 0));
    let provider_messages = body
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut system_prompt_tokens = 0u64;
    let mut skills_tokens = 0u64;
    let mut messages_tokens = 0u64;
    let mut role_counts = BTreeMap::<String, OpenAiChatRoleTokenCount>::new();
    let mut seen_provider_system_messages = 0usize;
    let mut skill_entries = Vec::new();

    for message in &provider_messages {
        let tokens = count_value(enc, message);
        if message.get("role").and_then(Value::as_str) == Some("system") {
            if seen_provider_system_messages < counting.system_prompt_message_count {
                system_prompt_tokens = system_prompt_tokens.saturating_add(tokens);
            } else if seen_provider_system_messages
                < counting
                    .system_prompt_message_count
                    .saturating_add(counting.skill_index_message_count)
            {
                skills_tokens = skills_tokens.saturating_add(tokens);
                skill_entries.extend(skill_entry_token_counts(enc, message));
            } else {
                system_prompt_tokens = system_prompt_tokens.saturating_add(tokens);
            }
            seen_provider_system_messages = seen_provider_system_messages.saturating_add(1);
            continue;
        }

        messages_tokens = messages_tokens.saturating_add(tokens);
        let role = normalized_message_role(message);
        let entry = role_counts.entry(role).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.tokens = entry.tokens.saturating_add(tokens);
    }
    let selected_skill_context_messages =
        selected_skill_context_provider_messages(request, base_url, &counting);
    let selected_skill_context_tokens = selected_skill_context_messages
        .iter()
        .map(|message| count_value(enc, message))
        .sum::<u64>();

    let total_estimated_tokens = system_prompt_tokens
        .saturating_add(system_tools_tokens)
        .saturating_add(skills_tokens)
        .saturating_add(messages_tokens);
    OpenAiChatTokenCount {
        encoding: encoding.name,
        encoding_source: encoding.source,
        encoding_fallback: encoding.fallback,
        system_prompt_tokens,
        system_tools_tokens,
        skills_tokens,
        messages_tokens,
        total_estimated_tokens,
        tool_count,
        role_counts,
        selected_skill_context_tokens,
        selected_skill_context_count: selected_skill_context_messages.len(),
        skill_names: counting.skill_names,
        skill_entries,
    }
}

#[derive(Debug, Clone)]
struct CountEncoding {
    name: String,
    source: String,
    fallback: bool,
}

fn resolve_count_encoding(provider: &str, model: &str) -> CountEncoding {
    if let Some(name) = tiktoken::model_to_encoding(model) {
        return CountEncoding {
            name: name.to_string(),
            source: "model".to_string(),
            fallback: false,
        };
    }
    let provider = provider.to_lowercase();
    let model = model.to_lowercase();
    let guessed = if provider.contains("qwen")
        || provider.contains("dashscope")
        || model.contains("qwen")
        || model.contains("qwq")
    {
        Some("qwen2")
    } else if provider.contains("deepseek") || model.contains("deepseek") {
        Some("deepseek_v3")
    } else if provider.contains("llama") || model.contains("llama") {
        Some("llama3")
    } else if provider.contains("mistral") || model.contains("mistral") {
        Some("mistral_v3")
    } else if provider.contains("openai")
        || provider.contains("openrouter")
        || model.starts_with("gpt-")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
    {
        Some("o200k_base")
    } else {
        None
    };
    if let Some(name) = guessed
        && tiktoken::get_encoding(name).is_some()
    {
        return CountEncoding {
            name: name.to_string(),
            source: "provider_model_guess".to_string(),
            fallback: false,
        };
    }
    CountEncoding {
        name: "o200k_base".to_string(),
        source: "fallback".to_string(),
        fallback: true,
    }
}

#[derive(Debug, Clone, Default)]
struct RequestContextCountingMetadata {
    system_prompt_message_count: usize,
    skill_index_message_count: usize,
    previous_message_count: usize,
    selected_skill_context_message_count: usize,
    skill_names: Vec<String>,
}

fn request_context_counting_metadata(
    request: &GenerationRequest,
) -> RequestContextCountingMetadata {
    let Some(value) = request.metadata.get("context_counting") else {
        return RequestContextCountingMetadata {
            system_prompt_message_count: request
                .messages
                .iter()
                .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
                .count(),
            ..RequestContextCountingMetadata::default()
        };
    };
    RequestContextCountingMetadata {
        system_prompt_message_count: value
            .get("system_prompt_message_count")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize,
        skill_index_message_count: value
            .get("skill_index_message_count")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize,
        previous_message_count: value
            .get("previous_message_count")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize,
        selected_skill_context_message_count: value
            .get("selected_skill_context_message_count")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize,
        skill_names: value
            .get("skill_names")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
    }
}

fn count_value(enc: &tiktoken::CoreBpe, value: &Value) -> u64 {
    serde_json::to_string(value)
        .map(|text| count_text(enc, &text))
        .unwrap_or(0)
}

fn count_text(enc: &tiktoken::CoreBpe, text: &str) -> u64 {
    enc.encode(text).len() as u64
}

fn skill_entry_token_counts(
    enc: &tiktoken::CoreBpe,
    provider_message: &Value,
) -> Vec<OpenAiChatSkillTokenCount> {
    let Some(content) = provider_message.get("content").and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut rest = content;
    let mut entries = Vec::new();
    while let Some(start_index) = rest.find("<skill>") {
        rest = &rest[start_index + "<skill>".len()..];
        let Some(end_index) = rest.find("</skill>") else {
            break;
        };
        let entry = &rest[..end_index];
        if let Some(name) = skill_entry_name(entry) {
            entries.push(OpenAiChatSkillTokenCount {
                name: name.to_string(),
                tokens: count_text(enc, entry),
            });
        }
        rest = &rest[end_index + "</skill>".len()..];
    }
    entries
}

fn skill_entry_name(entry: &str) -> Option<&str> {
    let start = entry.find("<name>")? + "<name>".len();
    let end = entry[start..].find("</name>")? + start;
    let name = entry[start..end].trim();
    (!name.is_empty()).then_some(name)
}

fn selected_skill_context_provider_messages(
    request: &GenerationRequest,
    base_url: &str,
    counting: &RequestContextCountingMetadata,
) -> Vec<Value> {
    if counting.selected_skill_context_message_count == 0 {
        return Vec::new();
    }
    let start = counting.previous_message_count;
    let end = start.saturating_add(counting.selected_skill_context_message_count);
    let mut seen_non_system_messages = 0usize;
    let mut messages = Vec::new();
    for message in &request.messages {
        if message.get("role").and_then(Value::as_str) == Some("system") {
            continue;
        }
        if seen_non_system_messages >= start && seen_non_system_messages < end {
            messages.extend(translate_message(message, &request.model, base_url));
        }
        seen_non_system_messages = seen_non_system_messages.saturating_add(1);
    }
    merge_adjacent_user_messages(messages)
}

fn normalized_message_role(message: &Value) -> String {
    match message.get("role").and_then(Value::as_str).unwrap_or("unknown") {
        "tool_result" => "tool".to_string(),
        other => other.to_string(),
    }
}

fn capability_is_false(metadata: &Value, key: &str) -> bool {
    metadata
        .get("model_metadata")
        .and_then(|metadata| metadata.get("capabilities"))
        .and_then(|capabilities| capabilities.get(key))
        .and_then(Value::as_bool)
        == Some(false)
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
