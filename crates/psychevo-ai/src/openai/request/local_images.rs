#[allow(unused_imports)]
pub(crate) use super::*;
use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat};

pub(crate) fn local_image_data_url(path: &str) -> std::result::Result<String, String> {
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
pub(crate) enum LocalImageFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
    Bmp,
    Avif,
}

impl LocalImageFormat {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
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

    pub(crate) fn image_format(self) -> Option<ImageFormat> {
        match self {
            Self::Png => Some(ImageFormat::Png),
            Self::Jpeg => Some(ImageFormat::Jpeg),
            Self::Webp => Some(ImageFormat::WebP),
            Self::Gif => Some(ImageFormat::Gif),
            Self::Bmp => Some(ImageFormat::Bmp),
            Self::Avif => None,
        }
    }

    pub(crate) fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
            Self::Avif => "image/avif",
        }
    }

    pub(crate) fn preserve_original(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp | Self::Gif)
    }
}

pub(crate) fn decode_local_image(
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

pub(crate) fn decode_avif_image(data: &[u8]) -> std::result::Result<DynamicImage, String> {
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
        return Err(format!(
            "AVIF image could not be decoded: {}",
            stderr.trim()
        ));
    }
    image::load_from_memory_with_format(&output.stdout, ImageFormat::Png)
        .map_err(|err| format!("AVIF image could not be decoded: {err}"))
}

pub(crate) fn normalized_image_data_url(image: &DynamicImage) -> Option<String> {
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

pub(crate) fn candidate_image_sizes(width: u32, height: u32) -> Vec<(u32, u32)> {
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

pub(crate) fn encode_png(image: &DynamicImage) -> std::result::Result<Vec<u8>, image::ImageError> {
    let mut cursor = Cursor::new(Vec::new());
    image.write_to(&mut cursor, ImageFormat::Png)?;
    Ok(cursor.into_inner())
}

pub(crate) fn encode_jpeg(
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

pub(crate) fn bounded_data_url(mime_type: &str, data: &[u8]) -> Option<String> {
    let encoded = BASE64_STANDARD.encode(data);
    (encoded.len() <= MAX_IMAGE_BASE64_BYTES).then(|| format!("data:{mime_type};base64,{encoded}"))
}

pub(crate) fn assistant_messages(
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

pub(crate) fn merge_adjacent_user_messages(messages: Vec<Value>) -> Vec<Value> {
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

pub(crate) fn apply_reasoning_content_for_api(
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

pub(crate) fn projects_reasoning_content(
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
) -> bool {
    if model_interleaved_is_false(metadata) {
        return false;
    }
    if let Some(field) = model_interleaved_field(metadata) {
        return field == "reasoning_content";
    }
    capability_is_true(metadata, "reasoning")
        || needs_thinking_reasoning_pad_fallback(target, base_url)
}

pub(crate) fn model_interleaved_field(metadata: &Value) -> Option<&str> {
    model_capabilities(metadata)
        .and_then(|capabilities| capabilities.get("interleaved"))
        .and_then(|interleaved| interleaved.get("field"))
        .and_then(Value::as_str)
}

pub(crate) fn model_interleaved_is_false(metadata: &Value) -> bool {
    model_capabilities(metadata)
        .and_then(|capabilities| capabilities.get("interleaved"))
        .and_then(Value::as_bool)
        == Some(false)
}

pub(crate) fn needs_thinking_reasoning_pad_fallback(target: &ModelTarget, base_url: &str) -> bool {
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

pub(crate) fn base_url_host_matches(base_url: &str, needle: &str) -> bool {
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

pub(crate) fn tool_result_messages(message: &Value) -> Vec<Value> {
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
