#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) const MAX_LOCAL_IMAGE_BYTES: u64 = 50 * 1024 * 1024;
pub(crate) const MAX_IMAGE_BASE64_BYTES: usize = 4_718_592;
pub(crate) const MAX_IMAGE_DIMENSION: u32 = 2000;
pub(crate) const JPEG_QUALITIES: [u8; 5] = [80, 85, 70, 55, 40];

pub fn openai_chat_completions_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageInputTranslationMode {
    ModelMetadata,
    ForceText,
}

pub fn openai_chat_request_body(request: &GenerationRequest, base_url: &str) -> Value {
    openai_chat_request_body_with_image_mode(
        request,
        base_url,
        ImageInputTranslationMode::ModelMetadata,
    )
}

pub(crate) fn openai_chat_request_body_text_only_images(
    request: &GenerationRequest,
    base_url: &str,
) -> Value {
    openai_chat_request_body_with_image_mode(
        request,
        base_url,
        ImageInputTranslationMode::ForceText,
    )
}

pub(crate) fn openai_chat_request_body_with_image_mode(
    request: &GenerationRequest,
    base_url: &str,
    image_mode: ImageInputTranslationMode,
) -> Value {
    let mut body = json!({
        "model": request.model.model,
        "messages": translate_messages(
            &request.messages,
            &request.model,
            &request.metadata,
            base_url,
            image_mode,
        ),
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
    pub base_policy_tokens: u64,
    pub developer_prompt_tokens: u64,
    pub project_context_tokens: u64,
    pub history_tokens: u64,
    pub turn_context_tokens: u64,
    pub current_prompt_tokens: u64,
    pub system_prompt_tokens: u64,
    pub system_tools_tokens: u64,
    pub skills_tokens: u64,
    pub messages_tokens: u64,
    pub total_estimated_tokens: u64,
    pub tool_count: usize,
    pub role_counts: BTreeMap<String, OpenAiChatRoleTokenCount>,
    pub project_instruction_context_tokens: u64,
    pub project_instruction_context_count: usize,
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
    let body = openai_chat_request_body(request, base_url);
    let (system_tools_tokens, tool_count) = body
        .get("tools")
        .map(|tools| {
            (
                count_value(enc, tools),
                tools.as_array().map_or(0, Vec::len),
            )
        })
        .unwrap_or((0, 0));

    let mut base_policy_tokens = 0u64;
    let mut developer_prompt_tokens = 0u64;
    let skills_tokens = 0u64;
    let mut project_context_tokens = 0u64;
    let mut history_tokens = 0u64;
    let mut turn_context_tokens = 0u64;
    let mut current_prompt_tokens = 0u64;
    let mut role_counts = BTreeMap::<String, OpenAiChatRoleTokenCount>::new();
    let mut skill_entries = Vec::new();
    let mut transcript_message_count = 0usize;

    for message in &request.messages {
        let provider_messages = translate_message_for_request(
            message,
            &request.model,
            &request.metadata,
            base_url,
            ImageInputTranslationMode::ModelMetadata,
        );
        let tokens = provider_messages
            .iter()
            .map(|message| count_value(enc, message))
            .sum::<u64>();
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if role == "system" || role == "developer" {
            match prompt_semantic_role(message).unwrap_or("developer_prompt") {
                "base_policy" => base_policy_tokens = base_policy_tokens.saturating_add(tokens),
                "developer_prompt" => {
                    developer_prompt_tokens = developer_prompt_tokens.saturating_add(tokens);
                    if prompt_slot(message) == Some("skill_index") {
                        for provider_message in &provider_messages {
                            skill_entries.extend(skill_entry_token_counts(enc, provider_message));
                        }
                    }
                }
                _ => developer_prompt_tokens = developer_prompt_tokens.saturating_add(tokens),
            }
            continue;
        }

        if context_category(message) == Some("project_context") {
            project_context_tokens = project_context_tokens.saturating_add(tokens);
        } else if context_category(message) == Some("turn_context") {
            turn_context_tokens = turn_context_tokens.saturating_add(tokens);
        } else if transcript_message_count < counting.previous_message_count {
            history_tokens = history_tokens.saturating_add(tokens);
            transcript_message_count = transcript_message_count.saturating_add(1);
        } else {
            current_prompt_tokens = current_prompt_tokens.saturating_add(tokens);
            transcript_message_count = transcript_message_count.saturating_add(1);
        }
        for provider_message in provider_messages {
            let role = normalized_message_role(&provider_message);
            let entry = role_counts.entry(role).or_default();
            entry.count = entry.count.saturating_add(1);
            entry.tokens = entry
                .tokens
                .saturating_add(count_value(enc, &provider_message));
        }
    }
    let system_prompt_tokens = base_policy_tokens.saturating_add(developer_prompt_tokens);
    let messages_tokens = project_context_tokens
        .saturating_add(history_tokens)
        .saturating_add(turn_context_tokens)
        .saturating_add(current_prompt_tokens);
    let selected_skill_context_tokens = turn_context_tokens;
    let project_instruction_context_tokens = project_context_tokens;

    let total_estimated_tokens = base_policy_tokens
        .saturating_add(developer_prompt_tokens)
        .saturating_add(system_tools_tokens)
        .saturating_add(project_context_tokens)
        .saturating_add(history_tokens)
        .saturating_add(turn_context_tokens)
        .saturating_add(current_prompt_tokens);
    OpenAiChatTokenCount {
        encoding: encoding.name,
        encoding_source: encoding.source,
        encoding_fallback: encoding.fallback,
        base_policy_tokens,
        developer_prompt_tokens,
        project_context_tokens,
        history_tokens,
        turn_context_tokens,
        current_prompt_tokens,
        system_prompt_tokens,
        system_tools_tokens,
        skills_tokens,
        messages_tokens,
        total_estimated_tokens,
        tool_count,
        role_counts,
        project_instruction_context_tokens,
        project_instruction_context_count: counting.project_instruction_context_message_count,
        selected_skill_context_tokens,
        selected_skill_context_count: counting.selected_skill_context_message_count,
        skill_names: counting.skill_names,
        skill_entries,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CountEncoding {
    pub(crate) name: String,
    pub(crate) source: String,
    pub(crate) fallback: bool,
}

pub(crate) fn resolve_count_encoding(provider: &str, model: &str) -> CountEncoding {
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
pub(crate) struct RequestContextCountingMetadata {
    pub(crate) previous_message_count: usize,
    pub(crate) project_instruction_context_message_count: usize,
    pub(crate) selected_skill_context_message_count: usize,
    pub(crate) skill_names: Vec<String>,
}

pub(crate) fn request_context_counting_metadata(
    request: &GenerationRequest,
) -> RequestContextCountingMetadata {
    let Some(value) = request.metadata.get("context_counting") else {
        return RequestContextCountingMetadata {
            ..RequestContextCountingMetadata::default()
        };
    };
    RequestContextCountingMetadata {
        previous_message_count: value
            .get("previous_message_count")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize,
        project_instruction_context_message_count: value
            .get("project_instruction_context_message_count")
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

pub(crate) fn count_value(enc: &tiktoken::CoreBpe, value: &Value) -> u64 {
    serde_json::to_string(value)
        .map(|text| count_text(enc, &text))
        .unwrap_or(0)
}

pub(crate) fn count_text(enc: &tiktoken::CoreBpe, text: &str) -> u64 {
    enc.encode(text).len() as u64
}

pub(crate) fn skill_entry_token_counts(
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

pub(crate) fn skill_entry_name(entry: &str) -> Option<&str> {
    let start = entry.find("<name>")? + "<name>".len();
    let end = entry[start..].find("</name>")? + start;
    let name = entry[start..end].trim();
    (!name.is_empty()).then_some(name)
}

pub(crate) fn normalized_message_role(message: &Value) -> String {
    match message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
    {
        "tool_result" => "tool".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn prompt_slot(message: &Value) -> Option<&str> {
    message
        .get("metadata")
        .and_then(|metadata| metadata.get("prompt_slot"))
        .and_then(Value::as_str)
}

pub(crate) fn prompt_semantic_role(message: &Value) -> Option<&str> {
    message
        .get("metadata")
        .and_then(|metadata| metadata.get("prompt_semantic_role"))
        .and_then(Value::as_str)
}

pub(crate) fn context_category(message: &Value) -> Option<&str> {
    message
        .get("metadata")
        .and_then(|metadata| metadata.get("context_category"))
        .and_then(Value::as_str)
}

pub(crate) fn capability_is_false(metadata: &Value, key: &str) -> bool {
    model_capabilities(metadata)
        .and_then(|capabilities| capabilities.get(key))
        .and_then(Value::as_bool)
        == Some(false)
}

pub(crate) fn capability_is_true(metadata: &Value, key: &str) -> bool {
    model_capabilities(metadata)
        .and_then(|capabilities| capabilities.get(key))
        .and_then(Value::as_bool)
        == Some(true)
}

pub(crate) fn model_metadata_disables_image_input(metadata: &Value) -> bool {
    capability_modalities_without_image(metadata) || capability_is_false(metadata, "attachment")
}

pub(crate) fn capability_modalities_without_image(metadata: &Value) -> bool {
    let Some(capabilities) = model_capabilities(metadata) else {
        return false;
    };
    let modal_input = capabilities
        .get("modalities")
        .and_then(|modalities| modalities.get("input"));
    let legacy_input = capabilities.get("input_modalities");
    input_modalities_without_image(modal_input) || input_modalities_without_image(legacy_input)
}

pub(crate) fn input_modalities_without_image(value: Option<&Value>) -> bool {
    let Some(modalities) = value.and_then(Value::as_array) else {
        return false;
    };
    !modalities
        .iter()
        .filter_map(Value::as_str)
        .any(|modality| modality.eq_ignore_ascii_case("image"))
}

pub(crate) fn model_capabilities(metadata: &Value) -> Option<&Value> {
    metadata
        .get("model_metadata")
        .and_then(|metadata| metadata.get("capabilities"))
}

pub(crate) fn translate_messages(
    messages: &[Value],
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
    image_mode: ImageInputTranslationMode,
) -> Vec<Value> {
    messages
        .iter()
        .flat_map(|message| {
            translate_message_for_request(message, target, metadata, base_url, image_mode)
        })
        .collect::<Vec<_>>()
}

pub(crate) fn translate_message_for_request(
    message: &Value,
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
    image_mode: ImageInputTranslationMode,
) -> Vec<Value> {
    merge_adjacent_user_messages(translate_message(
        message, target, metadata, base_url, image_mode,
    ))
}

pub(crate) fn translate_message(
    message: &Value,
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
    image_mode: ImageInputTranslationMode,
) -> Vec<Value> {
    match message.get("role").and_then(Value::as_str) {
        Some("system") => system_messages(message),
        Some("developer") => developer_messages(message, metadata),
        Some("user") => user_messages(message, metadata, image_mode),
        Some("assistant") => assistant_messages(message, target, metadata, base_url),
        Some("tool_result") => tool_result_messages(message),
        _ => Vec::new(),
    }
}

pub(crate) fn developer_messages(message: &Value, metadata: &Value) -> Vec<Value> {
    let role = if capability_is_true(metadata, "developer_role") {
        "developer"
    } else {
        "system"
    };
    message
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| vec![json!({ "role": role, "content": text })])
        .unwrap_or_default()
}

pub(crate) fn system_messages(message: &Value) -> Vec<Value> {
    message
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| vec![json!({ "role": "system", "content": text })])
        .unwrap_or_default()
}

pub(crate) fn user_messages(
    message: &Value,
    metadata: &Value,
    image_mode: ImageInputTranslationMode,
) -> Vec<Value> {
    let Some(content) = message.get("content") else {
        return Vec::new();
    };
    if let Some(text) = content
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return vec![json!({ "role": "user", "content": text })];
    }
    let Some(blocks) = content.as_array() else {
        return Vec::new();
    };
    if blocks.iter().any(is_image_block) {
        if image_mode == ImageInputTranslationMode::ForceText
            || model_metadata_disables_image_input(metadata)
        {
            let text = degraded_user_content_text(blocks);
            return if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![json!({ "role": "user", "content": text })]
            };
        }
        let parts = user_content_parts(blocks);
        if parts.is_empty() {
            Vec::new()
        } else {
            vec![json!({ "role": "user", "content": parts })]
        }
    } else {
        blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .map(|text| json!({ "role": "user", "content": text }))
            .collect()
    }
}

pub(crate) fn is_local_image_block(block: &Value) -> bool {
    block.get("type").and_then(Value::as_str) == Some("local_image")
}

pub(crate) fn is_image_url_block(block: &Value) -> bool {
    block.get("type").and_then(Value::as_str) == Some("image_url")
}

pub(crate) fn is_image_block(block: &Value) -> bool {
    is_local_image_block(block) || is_image_url_block(block)
}

pub(crate) fn request_has_image_blocks(request: &GenerationRequest) -> bool {
    request.messages.iter().any(message_has_image_blocks)
}

pub(crate) fn message_has_image_blocks(message: &Value) -> bool {
    message
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|blocks| blocks.iter().any(is_image_block))
}

pub(crate) fn degraded_user_content_text(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter_map(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .or_else(|| image_block_source_text(block))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn image_block_source_text(block: &Value) -> Option<String> {
    if is_local_image_block(block) {
        return block
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(str::to_string)
            .or_else(|| Some("[image attachment omitted: missing local path]".to_string()));
    }
    if is_image_url_block(block) {
        let Some(url) = image_url_block_url(block).filter(|url| !url.is_empty()) else {
            return Some("[image attachment omitted: missing image URL]".to_string());
        };
        if url.starts_with("data:image/") {
            return Some("[image attachment omitted: data image]".to_string());
        }
        return Some(url.to_string());
    }
    None
}

pub(crate) fn image_url_block_url(block: &Value) -> Option<&str> {
    block.get("url").and_then(Value::as_str).or_else(|| {
        block
            .get("image_url")
            .and_then(|image_url| image_url.get("url"))
            .and_then(Value::as_str)
    })
}

pub(crate) fn user_content_parts(blocks: &[Value]) -> Vec<Value> {
    let mut parts = Vec::new();
    for block in blocks {
        if let Some(text) = block
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            parts.push(json!({ "type": "text", "text": text }));
            continue;
        }
        if is_local_image_block(block) {
            let path = block
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match local_image_data_url(path) {
                Ok(data_url) => {
                    parts.push(json!({
                        "type": "image_url",
                        "image_url": { "url": data_url },
                    }));
                }
                Err(err) => {
                    parts.push(json!({
                        "type": "text",
                        "text": format!("Image at `{path}` could not be attached: {err}"),
                    }));
                }
            }
        }
        if is_image_url_block(block)
            && let Some(url) = image_url_block_url(block)
            && !url.is_empty()
        {
            parts.push(json!({
                "type": "image_url",
                "image_url": { "url": url },
            }));
        }
    }
    parts
}

#[path = "request/local_images.rs"]
pub(crate) mod local_images;
#[allow(unused_imports)]
pub use local_images::*;
