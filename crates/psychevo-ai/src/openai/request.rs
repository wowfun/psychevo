use serde_json::{Value, json};

use crate::sdk_types::{LanguageSettings, ResponseFormat, ToolChoice};
use crate::types::{GenerationRequest, GenerationTool, ModelTarget};

use local_images::local_image_data_url;
use messages::{assistant_messages, merge_adjacent_user_messages, tool_result_messages};

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
                .filter_map(|tool| match tool {
                    GenerationTool::Function { declaration: tool } => Some(tool),
                    GenerationTool::WebSearch(_) => None,
                })
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
    apply_openai_chat_settings(&mut body, &request.metadata);
    body
}

pub(crate) fn apply_openai_chat_settings(body: &mut Value, metadata: &Value) {
    let Some(settings) = metadata
        .get("_psychevo_ai_settings")
        .and_then(|value| serde_json::from_value::<LanguageSettings>(value.clone()).ok())
    else {
        return;
    };
    if let Some(value) = settings.max_output_tokens {
        body["max_tokens"] = json!(value);
    }
    if let Some(value) = settings.temperature {
        body["temperature"] = json!(value);
    }
    if let Some(value) = settings.top_p {
        body["top_p"] = json!(value);
    }
    if let Some(value) = settings.frequency_penalty {
        body["frequency_penalty"] = json!(value);
    }
    if let Some(value) = settings.presence_penalty {
        body["presence_penalty"] = json!(value);
    }
    if !settings.stop_sequences.is_empty() {
        body["stop"] = json!(settings.stop_sequences);
    }
    if let Some(value) = settings.seed {
        body["seed"] = json!(value);
    }
    if let Some(format) = settings.response_format {
        body["response_format"] = match format {
            ResponseFormat::Text => json!({"type": "text"}),
            ResponseFormat::JsonObject => json!({"type": "json_object"}),
            ResponseFormat::JsonSchema {
                name,
                schema,
                strict,
            } => json!({
                "type": "json_schema",
                "json_schema": {
                    "name": name,
                    "schema": schema,
                    "strict": strict,
                }
            }),
        };
    }
    if let Some(choice) = settings.tool_choice {
        body["tool_choice"] = match choice {
            ToolChoice::Auto => json!("auto"),
            ToolChoice::None => json!("none"),
            ToolChoice::Required => json!("required"),
            ToolChoice::Tool { name } => json!({
                "type": "function",
                "function": {"name": name},
            }),
        };
    }
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

mod local_images;
mod messages;
