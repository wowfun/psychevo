use std::fs;
use std::path::{Path, PathBuf};

use psychevo_agent_core::{Message, UserContentBlock, now_ms, user_text_message};

use crate::error::{Error, Result};
use crate::types::{ImageInput, ModelMetadata};

pub const MAX_LOCAL_IMAGE_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSourceArgument {
    pub source: String,
    pub remainder: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptImageExtraction {
    pub images: Vec<ImageInput>,
    pub text: String,
}

pub struct PromptMessageBuild {
    pub message: Message,
}

pub fn prompt_starts_with_supported_image_path(prompt: &str) -> bool {
    split_image_source_argument(prompt)
        .is_some_and(|argument| source_is_supported_leading_image_prompt(&argument.source))
}

pub fn model_metadata_explicitly_disallows_image_input(metadata: &ModelMetadata) -> bool {
    if !metadata.capabilities.input_modalities.is_empty() {
        return !metadata
            .capabilities
            .input_modalities
            .iter()
            .any(|modality| modality.eq_ignore_ascii_case("image"));
    }
    metadata.capabilities.attachment == Some(false)
}

pub fn split_image_source_argument(input: &str) -> Option<ImageSourceArgument> {
    let token = parse_leading_source_token(input)?;
    Some(ImageSourceArgument {
        remainder: input[token.end..].trim().to_string(),
        source: token.value,
        start: token.start,
        end: token.end,
    })
}

pub fn resolve_image_source(source: &str, cwd: &Path) -> Result<ImageInput> {
    let source = strip_wrapping_quotes(source.trim());
    if source.is_empty() {
        return Err(Error::Message("image source is empty".to_string()));
    }
    if is_remote_image_url(source) || is_data_image_url(source) {
        return Ok(ImageInput::ImageUrl(source.to_string()));
    }
    if !supported_image_extension(source) {
        return Err(Error::Message(format!(
            "unsupported image type: {source}; expected png, jpg, jpeg, webp, gif, bmp, or avif"
        )));
    }
    let path = resolve_prompt_path(source, Some(cwd));
    validate_local_image_path(&path)?;
    Ok(ImageInput::LocalPath(path))
}

#[cfg(test)]
pub(crate) fn prompt_message_from_text(
    prompt: &str,
    cwd: &Path,
    metadata: &ModelMetadata,
) -> Result<Message> {
    prompt_message_from_inputs(prompt, &[], cwd, metadata).map(|build| build.message)
}

#[allow(dead_code)]
pub(crate) fn prompt_message_from_inputs(
    prompt: &str,
    image_inputs: &[ImageInput],
    cwd: &Path,
    metadata: &ModelMetadata,
) -> Result<PromptMessageBuild> {
    prompt_message_from_inputs_with_options(prompt, image_inputs, cwd, metadata, true)
}

pub fn prompt_message_from_inputs_with_options(
    prompt: &str,
    image_inputs: &[ImageInput],
    cwd: &Path,
    metadata: &ModelMetadata,
    extract_prompt_image_sources: bool,
) -> Result<PromptMessageBuild> {
    let mut images = image_inputs.to_vec();
    let prompt_text = if extract_prompt_image_sources {
        let extraction = extract_image_sources_from_prompt(prompt, cwd)?;
        images.extend(extraction.images);
        extraction.text
    } else {
        prompt.trim().to_string()
    };

    if images.is_empty() {
        return Ok(PromptMessageBuild {
            message: user_text_message(prompt.to_string()),
        });
    }

    if model_metadata_explicitly_disallows_image_input(metadata) {
        let mut lines = images
            .iter()
            .map(ImageInput::display_source)
            .collect::<Vec<_>>();
        if !prompt_text.trim().is_empty() {
            lines.push(prompt_text.trim().to_string());
        }
        return Ok(PromptMessageBuild {
            message: user_text_message(lines.join("\n")),
        });
    }

    for image in &images {
        if let ImageInput::LocalPath(path) = image {
            validate_local_image_path(path)?;
        }
    }

    let mut content = images
        .into_iter()
        .map(|image| match image {
            ImageInput::LocalPath(path) => UserContentBlock::local_image(path),
            ImageInput::ImageUrl(url) => UserContentBlock::image_url(url),
        })
        .collect::<Vec<_>>();
    if !prompt_text.trim().is_empty() {
        content.push(UserContentBlock::text(prompt_text.trim().to_string()));
    }
    Ok(PromptMessageBuild {
        message: Message::User {
            content,
            timestamp_ms: now_ms(),
        },
    })
}

pub fn extract_image_sources_from_prompt(
    prompt: &str,
    cwd: &Path,
) -> Result<PromptImageExtraction> {
    let spans = image_source_spans(prompt, cwd)?;
    if spans.is_empty() {
        return Ok(PromptImageExtraction {
            images: Vec::new(),
            text: prompt.trim().to_string(),
        });
    }

    let mut text = String::new();
    let mut images = Vec::new();
    let mut cursor = 0usize;
    for span in spans {
        text.push_str(&prompt[cursor..span.start]);
        images.push(span.image);
        cursor = span.end;
    }
    text.push_str(&prompt[cursor..]);
    Ok(PromptImageExtraction {
        images,
        text: normalize_prompt_text_after_image_extraction(&text),
    })
}

pub(crate) fn validate_local_image_path(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            Error::Message(format!("image path does not exist: {}", path.display()))
        } else {
            Error::Message(format!(
                "image path is not readable: {}: {err}",
                path.display()
            ))
        }
    })?;
    if !metadata.is_file() {
        return Err(Error::Message(format!(
            "image path is not a file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_LOCAL_IMAGE_BYTES {
        return Err(Error::Message(format!(
            "image file is too large: {} exceeds {} bytes",
            path.display(),
            MAX_LOCAL_IMAGE_BYTES
        )));
    }
    Ok(())
}

pub(crate) fn source_is_supported_leading_image_prompt(source: &str) -> bool {
    is_data_image_url(source)
        || (source.starts_with("file://") && supported_image_extension(source))
        || (supported_image_extension(source) && !looks_like_prose_prefixed_path(source))
}

pub(crate) fn source_is_supported_embedded_image_prompt(source: &str) -> bool {
    is_data_image_url(source)
        || (source.starts_with("file://") && supported_image_extension(source))
        || ((source.starts_with("~/") || Path::new(source).is_absolute())
            && supported_image_extension(source)
            && !looks_like_prose_prefixed_path(source))
}

pub(crate) fn resolve_prompt_path(token: &str, cwd: Option<&Path>) -> PathBuf {
    let value = file_url_path(token).unwrap_or_else(|| expand_home_path(token));
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        cwd.map_or(path.clone(), |cwd| cwd.join(path))
    }
}

pub(crate) fn expand_home_path(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest).to_string_lossy().to_string();
    }
    value.to_string()
}

pub(crate) fn supported_image_extension(value: &str) -> bool {
    let value = file_url_path(value).unwrap_or_else(|| value.to_string());
    let Some(extension) = Path::new(&value)
        .extension()
        .and_then(|value| value.to_str())
    else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "avif"
    )
}

pub(crate) fn is_remote_image_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

pub(crate) fn is_data_image_url(value: &str) -> bool {
    value.starts_with("data:image/") && value.contains(";base64,")
}

pub(crate) fn file_url_path(value: &str) -> Option<String> {
    let rest = value.strip_prefix("file://")?;
    let path = if let Some(path) = rest.strip_prefix("localhost/") {
        format!("/{path}")
    } else if rest.starts_with('/') {
        rest.to_string()
    } else {
        return None;
    };
    Some(percent_decode_lossy(&path))
}

pub(crate) fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            output.push(high * 16 + low);
            index += 3;
            continue;
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

pub(crate) fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromptToken {
    pub(crate) value: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn parse_leading_source_token(prompt: &str) -> Option<PromptToken> {
    let start = prompt
        .char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))?;
    let rest = &prompt[start..];
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    if first == '"' || first == '\'' {
        return parse_quoted_token(prompt, start, first);
    }
    parse_unquoted_token(prompt, start)
}

pub(crate) fn parse_quoted_token(prompt: &str, start: usize, quote: char) -> Option<PromptToken> {
    let mut value = String::new();
    let mut escaped = false;
    let rest = &prompt[start + quote.len_utf8()..];
    for (offset, ch) in rest.char_indices() {
        if escaped {
            value.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return Some(PromptToken {
                value,
                start,
                end: start + quote.len_utf8() + offset + quote.len_utf8(),
            });
        }
        value.push(ch);
    }
    None
}

pub(crate) fn parse_unquoted_token(prompt: &str, start: usize) -> Option<PromptToken> {
    let rest = &prompt[start..];
    let mut value = String::new();
    let mut escaped = false;
    for (offset, ch) in rest.char_indices() {
        if escaped {
            value.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch.is_whitespace() {
            return Some(PromptToken {
                value,
                start,
                end: start + offset,
            });
        }
        value.push(ch);
    }
    Some(PromptToken {
        value,
        start,
        end: prompt.len(),
    })
}

#[derive(Debug)]
pub(crate) struct ImageSourceSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) image: ImageInput,
}

pub(crate) fn image_source_spans(prompt: &str, cwd: &Path) -> Result<Vec<ImageSourceSpan>> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    let first_non_whitespace = prompt
        .char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index));

    while cursor < prompt.len() {
        let Some((offset, ch)) = prompt[cursor..].char_indices().next() else {
            break;
        };
        let index = cursor + offset;
        let token = if Some(index) == first_non_whitespace {
            parse_leading_source_token(prompt)
                .filter(|token| source_is_supported_leading_image_prompt(&token.value))
        } else if ch == '"' || ch == '\'' {
            parse_quoted_token(prompt, index, ch)
                .filter(|token| source_is_supported_embedded_image_prompt(&token.value))
        } else if prompt[index..].starts_with("file://")
            || prompt[index..].starts_with("data:image/")
            || prompt[index..].starts_with("~/")
            || (ch == '/' && !index_is_inside_http_url_token(prompt, index))
        {
            parse_unquoted_token(prompt, index)
                .filter(|token| source_is_supported_embedded_image_prompt(&token.value))
        } else {
            None
        };

        if let Some(token) = token
            && (source_is_supported_leading_image_prompt(&token.value)
                || source_is_supported_embedded_image_prompt(&token.value))
        {
            let image = resolve_image_source(&token.value, cwd)?;
            spans.push(ImageSourceSpan {
                start: token.start,
                end: token.end,
                image,
            });
            cursor = token.end;
            continue;
        }
        cursor = index + ch.len_utf8();
    }
    Ok(spans)
}

pub(crate) fn index_is_inside_http_url_token(prompt: &str, index: usize) -> bool {
    let token_start = prompt[..index]
        .char_indices()
        .rev()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index + ch.len_utf8()))
        .unwrap_or(0);
    let token_prefix = &prompt[token_start..index];
    token_prefix.contains("http:") || token_prefix.contains("https:")
}

pub(crate) fn looks_like_prose_prefixed_path(source: &str) -> bool {
    source.find(":/").is_some_and(|index| index > 1)
        || source.find("：/").is_some_and(|index| index > 0)
}

pub(crate) fn normalize_prompt_text_after_image_extraction(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn strip_wrapping_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if matches!(
            (bytes[0], bytes[value.len() - 1]),
            (b'"', b'"') | (b'\'', b'\'')
        ) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;
    use crate::types::{ModelCapabilities, ModelMetadata};

    #[test]
    fn detects_supported_image_path_prefixes() {
        assert!(prompt_starts_with_supported_image_path(
            "/tmp/image.avif describe"
        ));
        assert!(prompt_starts_with_supported_image_path(
            "\"image one.webp\" describe"
        ));
        assert!(prompt_starts_with_supported_image_path(
            "file:///tmp/image%20one.png describe"
        ));
        assert!(prompt_starts_with_supported_image_path(
            "/tmp/image.bmp describe"
        ));
        assert!(!prompt_starts_with_supported_image_path(
            "https://example.com/image.png describe"
        ));
        assert!(!prompt_starts_with_supported_image_path("/unknown"));
        assert!(!prompt_starts_with_supported_image_path(
            "docs/readme.md explain"
        ));
    }

    #[test]
    fn splits_image_source_argument() {
        assert_eq!(
            split_image_source_argument("\"image one.webp\" describe"),
            Some(ImageSourceArgument {
                source: "image one.webp".to_string(),
                remainder: "describe".to_string(),
                start: 0,
                end: "\"image one.webp\"".len(),
            })
        );
        assert_eq!(
            split_image_source_argument("image\\ one.webp describe")
                .expect("argument")
                .source,
            "image one.webp"
        );
    }

    #[test]
    fn creates_local_image_user_message_from_prompt_prefix() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("image.avif");
        fs::write(&path, [1, 2, 3]).expect("image");

        let message = prompt_message_from_text(
            &format!("{} 描述该图片", path.display()),
            temp.path(),
            &ModelMetadata::default(),
        )
        .expect("message");

        let Message::User { content, .. } = message else {
            panic!("user message");
        };
        assert_eq!(content.len(), 2);
        assert_eq!(content[0], UserContentBlock::local_image(path));
        assert_eq!(content[1], UserContentBlock::text("描述该图片"));
    }

    #[test]
    fn creates_local_image_user_message_from_embedded_absolute_path() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("image.avif");
        fs::write(&path, [1, 2, 3]).expect("image");

        let message = prompt_message_from_text(
            &format!("描述这张图片的内容：{}", path.display()),
            temp.path(),
            &ModelMetadata::default(),
        )
        .expect("message");

        let Message::User { content, .. } = message else {
            panic!("user message");
        };
        assert_eq!(content.len(), 2);
        assert_eq!(content[0], UserContentBlock::local_image(path));
        assert_eq!(content[1], UserContentBlock::text("描述这张图片的内容："));
    }

    #[test]
    fn can_disable_prompt_image_source_extraction_for_tui_display_text() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("image.avif");
        fs::write(&path, [1, 2, 3]).expect("image");
        let prompt = format!("{} 描述该图片", path.display());

        let message = prompt_message_from_inputs_with_options(
            &prompt,
            &[],
            temp.path(),
            &ModelMetadata::default(),
            false,
        )
        .expect("message")
        .message;

        let Message::User { content, .. } = message else {
            panic!("user message");
        };
        assert_eq!(content, vec![UserContentBlock::text(prompt)]);
    }

    #[test]
    fn middle_relative_image_path_remains_text() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("image.png"), [1]).expect("image");

        let message =
            prompt_message_from_text("describe image.png", temp.path(), &ModelMetadata::default())
                .expect("message");

        let Message::User { content, .. } = message else {
            panic!("user message");
        };
        assert_eq!(content, vec![UserContentBlock::text("describe image.png")]);
    }

    #[test]
    fn http_urls_remain_text_during_prompt_extraction() {
        let prompt = concat!(
            "https://example.com/image.png describe it\n",
            "See https://developers.openai.com/codex/hooks for docs\n",
            "Markdown ![diagram](https://example.com/diagram.jpg) and ",
            "`https://example.com/quoted.png` stay text"
        );
        let message = prompt_message_from_text(prompt, Path::new("."), &ModelMetadata::default())
            .expect("message");

        let Message::User { content, .. } = message else {
            panic!("user message");
        };
        assert_eq!(content, vec![UserContentBlock::text(prompt)]);
    }

    #[test]
    fn data_image_url_prompt_still_becomes_image_url() {
        let message = prompt_message_from_text(
            "data:image/png;base64,aGVsbG8= describe it",
            Path::new("."),
            &ModelMetadata::default(),
        )
        .expect("message");

        let Message::User { content, .. } = message else {
            panic!("user message");
        };
        assert_eq!(
            content,
            vec![
                UserContentBlock::image_url("data:image/png;base64,aGVsbG8="),
                UserContentBlock::text("describe it"),
            ]
        );
    }

    #[test]
    fn creates_image_url_message_from_explicit_inputs() {
        let message = prompt_message_from_inputs(
            "describe it",
            &[ImageInput::ImageUrl(
                "https://example.com/image.png".to_string(),
            )],
            Path::new("."),
            &ModelMetadata::default(),
        )
        .expect("message")
        .message;

        let Message::User { content, .. } = message else {
            panic!("user message");
        };
        assert_eq!(
            content,
            vec![
                UserContentBlock::image_url("https://example.com/image.png"),
                UserContentBlock::text("describe it"),
            ]
        );
    }

    #[test]
    fn unsupported_image_model_degrades_explicit_inputs_to_source_and_text() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("image.png");
        fs::write(&path, [1, 2, 3]).expect("image");
        let metadata = ModelMetadata {
            capabilities: ModelCapabilities {
                input_modalities: vec!["text".to_string()],
                attachment: None,
                tool_call: None,
                ..ModelCapabilities::default()
            },
            ..ModelMetadata::default()
        };

        let message = prompt_message_from_inputs_with_options(
            "describe it",
            &[ImageInput::LocalPath(path.clone())],
            temp.path(),
            &metadata,
            false,
        )
        .expect("message")
        .message;

        let Message::User { content, .. } = message else {
            panic!("user message");
        };
        assert_eq!(
            content,
            vec![UserContentBlock::text(format!(
                "{}\ndescribe it",
                path.display()
            ))]
        );
    }

    #[test]
    fn resolves_cwd_relative_and_quoted_image_paths() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("image one.webp");
        fs::write(&path, [1]).expect("image");

        let message = prompt_message_from_text(
            "\"image one.webp\" describe",
            temp.path(),
            &Default::default(),
        )
        .expect("message");

        let Message::User { content, .. } = message else {
            panic!("user message");
        };
        assert_eq!(content[0], UserContentBlock::local_image(path));
        assert_eq!(content[1], UserContentBlock::text("describe"));
    }

    #[test]
    fn missing_image_path_returns_bounded_error() {
        let temp = tempfile::tempdir().expect("temp");
        let err = prompt_message_from_text(
            "missing.avif describe",
            temp.path(),
            &ModelMetadata::default(),
        )
        .expect_err("missing file");

        assert!(err.to_string().contains("image path does not exist"));
    }

    #[test]
    fn embedded_missing_image_path_returns_bounded_error() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("missing.avif");
        let err = prompt_message_from_text(
            &format!("describe: {}", path.display()),
            temp.path(),
            &ModelMetadata::default(),
        )
        .expect_err("missing file");

        assert!(err.to_string().contains("image path does not exist"));
    }

    #[test]
    fn directory_image_path_returns_bounded_error() {
        let temp = tempfile::tempdir().expect("temp");
        let dir = temp.path().join("dir.avif");
        fs::create_dir(&dir).expect("dir");

        let err = prompt_message_from_text(
            &format!("{} describe", dir.display()),
            temp.path(),
            &ModelMetadata::default(),
        )
        .expect_err("directory");

        assert!(err.to_string().contains("image path is not a file"));
    }

    #[test]
    fn unsupported_model_metadata_degrades_image_prompt_to_text() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("image.png");
        fs::write(&path, [1]).expect("image");
        let metadata = ModelMetadata {
            capabilities: ModelCapabilities {
                input_modalities: vec!["text".to_string()],
                ..ModelCapabilities::default()
            },
            ..ModelMetadata::default()
        };

        let build = prompt_message_from_text(
            &format!("{} describe", path.display()),
            temp.path(),
            &metadata,
        )
        .expect("degraded message");

        let Message::User { content, .. } = build else {
            panic!("user message");
        };
        assert_eq!(
            content,
            vec![UserContentBlock::text(format!(
                "{}\ndescribe",
                path.display()
            ))]
        );
    }
}
