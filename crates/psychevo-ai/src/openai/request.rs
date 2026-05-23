use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat};

const MAX_LOCAL_IMAGE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_IMAGE_BASE64_BYTES: usize = 4_718_592;
const MAX_IMAGE_DIMENSION: u32 = 2000;
const JPEG_QUALITIES: [u8; 5] = [80, 85, 70, 55, 40];

pub fn openai_chat_completions_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

pub fn openai_chat_request_body(request: &GenerationRequest, base_url: &str) -> Value {
    let mut body = json!({
        "model": request.model.model,
        "messages": translate_messages(&request.messages, &request.model, &request.metadata, base_url),
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
        .map(|tools| (count_value(enc, tools), tools.as_array().map_or(0, Vec::len)))
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
        let provider_messages = translate_message_for_request(message, &request.model, &request.metadata, base_url);
        let tokens = provider_messages
            .iter()
            .map(|message| count_value(enc, message))
            .sum::<u64>();
        let role = message.get("role").and_then(Value::as_str).unwrap_or("unknown");
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
            entry.tokens = entry.tokens.saturating_add(count_value(enc, &provider_message));
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
    previous_message_count: usize,
    project_instruction_context_message_count: usize,
    selected_skill_context_message_count: usize,
    skill_names: Vec<String>,
}

fn request_context_counting_metadata(
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

fn normalized_message_role(message: &Value) -> String {
    match message.get("role").and_then(Value::as_str).unwrap_or("unknown") {
        "tool_result" => "tool".to_string(),
        other => other.to_string(),
    }
}

fn prompt_slot(message: &Value) -> Option<&str> {
    message
        .get("metadata")
        .and_then(|metadata| metadata.get("prompt_slot"))
        .and_then(Value::as_str)
}

fn prompt_semantic_role(message: &Value) -> Option<&str> {
    message
        .get("metadata")
        .and_then(|metadata| metadata.get("prompt_semantic_role"))
        .and_then(Value::as_str)
}

fn context_category(message: &Value) -> Option<&str> {
    message
        .get("metadata")
        .and_then(|metadata| metadata.get("context_category"))
        .and_then(Value::as_str)
}

fn capability_is_false(metadata: &Value, key: &str) -> bool {
    model_capabilities(metadata)
        .and_then(|capabilities| capabilities.get(key))
        .and_then(Value::as_bool)
        == Some(false)
}

fn capability_is_true(metadata: &Value, key: &str) -> bool {
    model_capabilities(metadata)
        .and_then(|capabilities| capabilities.get(key))
        .and_then(Value::as_bool)
        == Some(true)
}

fn model_capabilities(metadata: &Value) -> Option<&Value> {
    metadata
        .get("model_metadata")
        .and_then(|metadata| metadata.get("capabilities"))
}

fn translate_messages(
    messages: &[Value],
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
) -> Vec<Value> {
    messages
        .iter()
        .flat_map(|message| translate_message_for_request(message, target, metadata, base_url))
        .collect::<Vec<_>>()
}

fn translate_message_for_request(
    message: &Value,
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
) -> Vec<Value> {
    merge_adjacent_user_messages(translate_message(message, target, metadata, base_url))
}

fn translate_message(
    message: &Value,
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
) -> Vec<Value> {
    match message.get("role").and_then(Value::as_str) {
        Some("system") => system_messages(message),
        Some("developer") => developer_messages(message, metadata),
        Some("user") => user_messages(message, metadata),
        Some("assistant") => assistant_messages(message, target, metadata, base_url),
        Some("tool_result") => tool_result_messages(message),
        _ => Vec::new(),
    }
}

fn developer_messages(message: &Value, metadata: &Value) -> Vec<Value> {
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

fn system_messages(message: &Value) -> Vec<Value> {
    message
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| vec![json!({ "role": "system", "content": text })])
        .unwrap_or_default()
}

fn user_messages(message: &Value, metadata: &Value) -> Vec<Value> {
    let Some(content) = message.get("content") else {
        return Vec::new();
    };
    if let Some(text) = content.as_str().map(str::trim).filter(|text| !text.is_empty()) {
        return vec![json!({ "role": "user", "content": text })];
    }
    let Some(blocks) = content.as_array() else {
        return Vec::new();
    };
    if blocks.iter().any(is_image_block) {
        if capability_is_false(metadata, "attachment") {
            let text = blocks
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| {
                            is_image_block(block).then(|| {
                                "[image attachment omitted: selected model metadata disables image input]"
                                    .to_string()
                            })
                        })
                })
                .collect::<Vec<_>>()
                .join("\n");
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

fn is_local_image_block(block: &Value) -> bool {
    block.get("type").and_then(Value::as_str) == Some("local_image")
}

fn is_image_url_block(block: &Value) -> bool {
    block.get("type").and_then(Value::as_str) == Some("image_url")
}

fn is_image_block(block: &Value) -> bool {
    is_local_image_block(block) || is_image_url_block(block)
}

fn user_content_parts(blocks: &[Value]) -> Vec<Value> {
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
            let path = block.get("path").and_then(Value::as_str).unwrap_or_default();
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
            && let Some(url) = block.get("url").and_then(Value::as_str)
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

fn local_image_data_url(path: &str) -> std::result::Result<String, String> {
    let path = Path::new(path);
    let format = LocalImageFormat::from_path(path).ok_or_else(|| {
        "unsupported image type; expected png, jpg, jpeg, webp, gif, bmp, or avif".to_string()
    })?;
    let metadata = fs::metadata(path).map_err(|err| err.to_string())?;
    if !metadata.is_file() {
        return Err("image path is not a file".to_string());
    }
    if metadata.len() > MAX_LOCAL_IMAGE_BYTES {
        return Err(format!(
            "image file exceeds {} bytes",
            MAX_LOCAL_IMAGE_BYTES
        ));
    }
    let data = fs::read(path).map_err(|err| err.to_string())?;
    let original_base64 = BASE64_STANDARD.encode(&data);
    let image = match decode_local_image(format, &data) {
        Ok(image) => image,
        Err(_err)
            if format == LocalImageFormat::Avif
                && original_base64.len() <= MAX_IMAGE_BASE64_BYTES =>
        {
            return Ok(format!("data:image/avif;base64,{original_base64}"));
        }
        Err(err) => return Err(err),
    };
    let (width, height) = image.dimensions();
    if format.preserve_original()
        && width <= MAX_IMAGE_DIMENSION
        && height <= MAX_IMAGE_DIMENSION
        && original_base64.len() <= MAX_IMAGE_BASE64_BYTES
    {
        return Ok(format!(
            "data:{};base64,{original_base64}",
            format.mime_type()
        ));
    }

    normalized_image_data_url(&image).ok_or_else(|| {
        format!(
            "image {width}x{height} exceeds normalized payload limit of {} base64 bytes",
            MAX_IMAGE_BASE64_BYTES
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalImageFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
    Bmp,
    Avif,
}

impl LocalImageFormat {
    fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "webp" => Some(Self::Webp),
            "gif" => Some(Self::Gif),
            "bmp" => Some(Self::Bmp),
            "avif" => Some(Self::Avif),
            _ => None,
        }
    }

    fn image_format(self) -> Option<ImageFormat> {
        match self {
            Self::Png => Some(ImageFormat::Png),
            Self::Jpeg => Some(ImageFormat::Jpeg),
            Self::Webp => Some(ImageFormat::WebP),
            Self::Gif => Some(ImageFormat::Gif),
            Self::Bmp => Some(ImageFormat::Bmp),
            Self::Avif => None,
        }
    }

    fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
            Self::Avif => "image/avif",
        }
    }

    fn preserve_original(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp | Self::Gif)
    }
}

fn decode_local_image(
    format: LocalImageFormat,
    data: &[u8],
) -> std::result::Result<DynamicImage, String> {
    if format == LocalImageFormat::Avif {
        return decode_avif_image(data);
    }
    image::load_from_memory_with_format(
        data,
        format
            .image_format()
            .expect("non-AVIF local image format has image crate decoder"),
    )
    .map_err(|err| format!("image could not be decoded: {err}"))
}

fn decode_avif_image(data: &[u8]) -> std::result::Result<DynamicImage, String> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("AVIF decoder unavailable: {err}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "AVIF decoder stdin unavailable".to_string())?;
        stdin
            .write_all(data)
            .map_err(|err| format!("failed to send AVIF data to decoder: {err}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|err| format!("AVIF decoder failed: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("AVIF image could not be decoded: {}", stderr.trim()));
    }
    image::load_from_memory_with_format(&output.stdout, ImageFormat::Png)
        .map_err(|err| format!("AVIF image could not be decoded: {err}"))
}

fn normalized_image_data_url(image: &DynamicImage) -> Option<String> {
    let (width, height) = image.dimensions();
    for (candidate_width, candidate_height) in candidate_image_sizes(width, height) {
        let candidate = if candidate_width == width && candidate_height == height {
            image.clone()
        } else {
            image.resize_exact(candidate_width, candidate_height, FilterType::Lanczos3)
        };
        if let Ok(png) = encode_png(&candidate)
            && let Some(data_url) = bounded_data_url("image/png", &png)
        {
            return Some(data_url);
        }
        for quality in JPEG_QUALITIES {
            if let Ok(jpeg) = encode_jpeg(&candidate, quality)
                && let Some(data_url) = bounded_data_url("image/jpeg", &jpeg)
            {
                return Some(data_url);
            }
        }
    }
    None
}

fn candidate_image_sizes(width: u32, height: u32) -> Vec<(u32, u32)> {
    let scale = (MAX_IMAGE_DIMENSION as f64 / width.max(1) as f64)
        .min(MAX_IMAGE_DIMENSION as f64 / height.max(1) as f64)
        .min(1.0);
    let mut current = (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    );
    let mut sizes = Vec::new();
    for _ in 0..32 {
        if !sizes.contains(&current) {
            sizes.push(current);
        }
        if current == (1, 1) {
            break;
        }
        current = (
            if current.0 == 1 {
                1
            } else {
                ((current.0 as f64) * 0.75).floor().max(1.0) as u32
            },
            if current.1 == 1 {
                1
            } else {
                ((current.1 as f64) * 0.75).floor().max(1.0) as u32
            },
        );
    }
    sizes
}

fn encode_png(image: &DynamicImage) -> std::result::Result<Vec<u8>, image::ImageError> {
    let mut cursor = Cursor::new(Vec::new());
    image.write_to(&mut cursor, ImageFormat::Png)?;
    Ok(cursor.into_inner())
}

fn encode_jpeg(
    image: &DynamicImage,
    quality: u8,
) -> std::result::Result<Vec<u8>, image::ImageError> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut output = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut output, quality);
    encoder.encode(&rgb, width, height, image::ColorType::Rgb8.into())?;
    Ok(output)
}

fn bounded_data_url(mime_type: &str, data: &[u8]) -> Option<String> {
    let encoded = BASE64_STANDARD.encode(data);
    (encoded.len() <= MAX_IMAGE_BASE64_BYTES)
        .then(|| format!("data:{mime_type};base64,{encoded}"))
}

fn assistant_messages(
    message: &Value,
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
) -> Vec<Value> {
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
        &mut output,
        has_text,
        has_tool_calls,
        &normalized_reasoning.join("\n\n"),
        target,
        metadata,
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
            && let Some(previous) = last.get("content").and_then(Value::as_str)
            && let Some(current) = message.get("content").and_then(Value::as_str)
        {
            let previous = previous.to_string();
            last["content"] = Value::String(format!("{previous}\n\n{current}"));
            continue;
        }
        merged.push(message);
    }
    merged
}

fn apply_reasoning_content_for_api(
    output: &mut Value,
    has_text: bool,
    has_tool_calls: bool,
    normalized_reasoning: &str,
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
) {
    if !projects_reasoning_content(target, metadata, base_url) {
        return;
    }
    if !has_text && !has_tool_calls {
        return;
    }
    let value = if normalized_reasoning.trim().is_empty() {
        " ".to_string()
    } else {
        normalized_reasoning.to_string()
    };
    output["reasoning_content"] = Value::String(value);
}

fn projects_reasoning_content(target: &ModelTarget, metadata: &Value, base_url: &str) -> bool {
    if model_interleaved_is_false(metadata) {
        return false;
    }
    if let Some(field) = model_interleaved_field(metadata) {
        return field == "reasoning_content";
    }
    capability_is_true(metadata, "reasoning")
        || needs_thinking_reasoning_pad_fallback(target, base_url)
}

fn model_interleaved_field(metadata: &Value) -> Option<&str> {
    model_capabilities(metadata)
        .and_then(|capabilities| capabilities.get("interleaved"))
        .and_then(|interleaved| interleaved.get("field"))
        .and_then(Value::as_str)
}

fn model_interleaved_is_false(metadata: &Value) -> bool {
    model_capabilities(metadata)
        .and_then(|capabilities| capabilities.get("interleaved"))
        .and_then(Value::as_bool)
        == Some(false)
}

fn needs_thinking_reasoning_pad_fallback(target: &ModelTarget, base_url: &str) -> bool {
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
        || provider == "xiaomi"
        || provider == "xiaomi-token-plan"
        || provider == "xiaomi-token-plan-cn"
        || model.contains("mimo")
        || base_url_host_matches(base_url, "api.xiaomimimo.com")
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
