use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const WRITE_ARGUMENT_PREVIEW_INTERVAL: Duration = Duration::from_millis(500);
const WRITE_ARGUMENT_PREVIEW_HEAD_BYTES: usize = 2 * 1024;
const WRITE_ARGUMENT_PREVIEW_TAIL_BYTES: usize = 6 * 1024;
const WRITE_ARGUMENT_PREVIEW_PATH_CHARS: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriteArgumentPreview {
    pub path: Option<String>,
    pub text: String,
    pub bytes_seen: usize,
    pub lines_seen: usize,
    pub omitted_bytes: usize,
    pub truncated: bool,
}

#[derive(Debug, Default)]
pub struct WriteArgumentPreviewTracker {
    last_seen_len: usize,
    last_published_len: usize,
    last_published_hash: Option<u64>,
    last_published_at: Option<Instant>,
    last_preview: Option<WriteArgumentPreview>,
}

impl WriteArgumentPreviewTracker {
    pub fn observe(&mut self, arguments_json: &str, now: Instant) -> Option<WriteArgumentPreview> {
        let reset = arguments_json.len() < self.last_seen_len
            || self.published_prefix_changed(arguments_json);
        if reset {
            self.reset();
        } else if arguments_json.len() == self.last_seen_len {
            return None;
        }
        self.last_seen_len = arguments_json.len();

        let should_probe = self
            .last_preview
            .as_ref()
            .is_none_or(|preview| preview.text.is_empty())
            || self.last_published_at.is_none_or(|last| {
                now.saturating_duration_since(last) >= WRITE_ARGUMENT_PREVIEW_INTERVAL
            });
        if !should_probe {
            return None;
        }
        self.publish(arguments_json, now)
    }

    pub fn flush(&mut self, arguments_json: &str, now: Instant) -> Option<WriteArgumentPreview> {
        if arguments_json.len() < self.last_seen_len
            || self.published_prefix_changed(arguments_json)
        {
            self.reset();
        }
        self.last_seen_len = arguments_json.len();
        self.publish(arguments_json, now)
    }

    pub fn last_preview(&self) -> Option<&WriteArgumentPreview> {
        self.last_preview.as_ref()
    }

    pub fn reset(&mut self) {
        self.last_seen_len = 0;
        self.last_published_len = 0;
        self.last_published_hash = None;
        self.last_published_at = None;
        self.last_preview = None;
    }

    fn publish(&mut self, arguments_json: &str, now: Instant) -> Option<WriteArgumentPreview> {
        let preview = write_argument_preview_from_json(arguments_json)?;
        self.last_published_len = arguments_json.len();
        self.last_published_hash = Some(hash_text(arguments_json));
        self.last_published_at = Some(now);
        if self.last_preview.as_ref() == Some(&preview) {
            return None;
        }
        self.last_preview = Some(preview.clone());
        Some(preview)
    }

    fn published_prefix_changed(&self, arguments_json: &str) -> bool {
        let (Some(expected), true) = (
            self.last_published_hash,
            arguments_json.len() >= self.last_published_len && self.last_published_len > 0,
        ) else {
            return false;
        };
        arguments_json
            .get(..self.last_published_len)
            .is_none_or(|prefix| hash_text(prefix) != expected)
    }
}

pub fn write_argument_preview_from_args(arguments: &Value) -> Option<WriteArgumentPreview> {
    let object = arguments.as_object()?;
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .map(bounded_preview_path);
    let content = object.get("content").and_then(Value::as_str);
    build_preview(path, content)
}

pub fn write_argument_preview_from_json(arguments_json: &str) -> Option<WriteArgumentPreview> {
    if let Ok(arguments) = serde_json::from_str::<Value>(arguments_json) {
        return write_argument_preview_from_args(&arguments);
    }
    let fields = scan_partial_top_level_strings(arguments_json)?;
    build_preview(fields.path, fields.content.as_deref())
}

fn build_preview(path: Option<String>, content: Option<&str>) -> Option<WriteArgumentPreview> {
    if path.is_none() && content.is_none() {
        return None;
    }
    let content = content.unwrap_or_default();
    let bytes_seen = content.len();
    let lines_seen = if content.is_empty() {
        0
    } else {
        content.bytes().filter(|byte| *byte == b'\n').count() + 1
    };
    let (text, omitted_bytes) = bounded_preview_text(content);
    Some(WriteArgumentPreview {
        path,
        text,
        bytes_seen,
        lines_seen,
        omitted_bytes,
        truncated: omitted_bytes > 0,
    })
}

fn bounded_preview_text(content: &str) -> (String, usize) {
    let limit = WRITE_ARGUMENT_PREVIEW_HEAD_BYTES + WRITE_ARGUMENT_PREVIEW_TAIL_BYTES;
    if content.len() <= limit {
        return (content.to_string(), 0);
    }
    let head_end = floor_char_boundary(content, WRITE_ARGUMENT_PREVIEW_HEAD_BYTES);
    let tail_start = ceil_char_boundary(
        content,
        content
            .len()
            .saturating_sub(WRITE_ARGUMENT_PREVIEW_TAIL_BYTES),
    );
    let omitted_bytes = tail_start.saturating_sub(head_end);
    (
        format!(
            "{}\n… {omitted_bytes} bytes omitted …\n{}",
            &content[..head_end],
            &content[tail_start..]
        ),
        omitted_bytes,
    )
}

fn bounded_preview_path(path: &str) -> String {
    if path.chars().count() <= WRITE_ARGUMENT_PREVIEW_PATH_CHARS {
        return path.to_string();
    }
    let mut preview = path
        .chars()
        .take(WRITE_ARGUMENT_PREVIEW_PATH_CHARS.saturating_sub(1))
        .collect::<String>();
    preview.push('…');
    preview
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while index < value.len() && !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn hash_text(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[derive(Default)]
struct PartialWriteFields {
    path: Option<String>,
    content: Option<String>,
}

fn scan_partial_top_level_strings(input: &str) -> Option<PartialWriteFields> {
    let bytes = input.as_bytes();
    let mut cursor = skip_whitespace(bytes, 0);
    if bytes.get(cursor) != Some(&b'{') {
        return None;
    }
    cursor += 1;
    let mut fields = PartialWriteFields::default();
    loop {
        cursor = skip_whitespace(bytes, cursor);
        match bytes.get(cursor) {
            Some(b'}') | None => return Some(fields),
            Some(b',') => {
                cursor += 1;
                continue;
            }
            Some(b'"') => {}
            _ => return None,
        }

        let key = parse_json_string(input, cursor)?;
        if !key.complete {
            return Some(fields);
        }
        cursor = skip_whitespace(bytes, key.next);
        if bytes.get(cursor) != Some(&b':') {
            return None;
        }
        cursor = skip_whitespace(bytes, cursor + 1);
        let key_name = key.decoded;
        if bytes.get(cursor) == Some(&b'"') {
            let value = parse_json_string(input, cursor)?;
            if key_name == "path" && value.complete {
                fields.path = Some(bounded_preview_path(&value.decoded));
            } else if key_name == "content" {
                fields.content = Some(value.decoded.clone());
            }
            if !value.complete {
                return Some(fields);
            }
            cursor = value.next;
        } else {
            let Some(next) = skip_json_value(input, cursor) else {
                return Some(fields);
            };
            cursor = next;
        }
    }
}

struct ParsedJsonString {
    decoded: String,
    next: usize,
    complete: bool,
}

fn parse_json_string(input: &str, start: usize) -> Option<ParsedJsonString> {
    if input.as_bytes().get(start) != Some(&b'"') {
        return None;
    }
    let mut decoded = String::new();
    let mut cursor = start + 1;
    while cursor < input.len() {
        let character = input[cursor..].chars().next()?;
        match character {
            '"' => {
                return Some(ParsedJsonString {
                    decoded,
                    next: cursor + 1,
                    complete: true,
                });
            }
            '\\' => {
                let escape_start = cursor;
                cursor += 1;
                let Some(escape) = input.as_bytes().get(cursor).copied() else {
                    return Some(ParsedJsonString {
                        decoded,
                        next: escape_start,
                        complete: false,
                    });
                };
                cursor += 1;
                match escape {
                    b'"' => decoded.push('"'),
                    b'\\' => decoded.push('\\'),
                    b'/' => decoded.push('/'),
                    b'b' => decoded.push('\u{0008}'),
                    b'f' => decoded.push('\u{000c}'),
                    b'n' => decoded.push('\n'),
                    b'r' => decoded.push('\r'),
                    b't' => decoded.push('\t'),
                    b'u' => {
                        let Some((unit, next)) = parse_unicode_unit(input, cursor) else {
                            return Some(ParsedJsonString {
                                decoded,
                                next: escape_start,
                                complete: false,
                            });
                        };
                        cursor = next;
                        let scalar = if (0xD800..=0xDBFF).contains(&unit) {
                            if input.as_bytes().get(cursor..cursor + 2) != Some(b"\\u") {
                                if cursor >= input.len() {
                                    return Some(ParsedJsonString {
                                        decoded,
                                        next: escape_start,
                                        complete: false,
                                    });
                                }
                                return None;
                            }
                            let Some((low, next)) = parse_unicode_unit(input, cursor + 2) else {
                                return Some(ParsedJsonString {
                                    decoded,
                                    next: escape_start,
                                    complete: false,
                                });
                            };
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return None;
                            }
                            cursor = next;
                            0x10000 + (((unit as u32 - 0xD800) << 10) | (low as u32 - 0xDC00))
                        } else if (0xDC00..=0xDFFF).contains(&unit) {
                            return None;
                        } else {
                            unit as u32
                        };
                        decoded.push(char::from_u32(scalar)?);
                    }
                    _ => return None,
                }
            }
            character if character <= '\u{001f}' => return None,
            character => {
                decoded.push(character);
                cursor += character.len_utf8();
            }
        }
    }
    Some(ParsedJsonString {
        decoded,
        next: input.len(),
        complete: false,
    })
}

fn parse_unicode_unit(input: &str, start: usize) -> Option<(u16, usize)> {
    let end = start.checked_add(4)?;
    let digits = input.as_bytes().get(start..end)?;
    let mut value = 0u16;
    for digit in digits {
        value = (value << 4)
            | match digit {
                b'0'..=b'9' => u16::from(*digit - b'0'),
                b'a'..=b'f' => u16::from(*digit - b'a' + 10),
                b'A'..=b'F' => u16::from(*digit - b'A' + 10),
                _ => return None,
            };
    }
    Some((value, end))
}

fn skip_json_value(input: &str, start: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    match bytes.get(start)? {
        b'"' => parse_json_string(input, start)
            .filter(|value| value.complete)
            .map(|value| value.next),
        b'{' | b'[' => skip_composite_value(input, start),
        _ => {
            let mut cursor = start;
            while let Some(byte) = bytes.get(cursor) {
                if matches!(byte, b',' | b'}') || byte.is_ascii_whitespace() {
                    return Some(cursor);
                }
                cursor += 1;
            }
            None
        }
    }
}

fn skip_composite_value(input: &str, start: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut stack = vec![*bytes.get(start)?];
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' => {
                let string = parse_json_string(input, cursor)?;
                if !string.complete {
                    return None;
                }
                cursor = string.next;
            }
            b'{' | b'[' => {
                stack.push(bytes[cursor]);
                cursor += 1;
            }
            b'}' if stack.last() == Some(&b'{') => {
                stack.pop();
                cursor += 1;
                if stack.is_empty() {
                    return Some(cursor);
                }
            }
            b']' if stack.last() == Some(&b'[') => {
                stack.pop();
                cursor += 1;
                if stack.is_empty() {
                    return Some(cursor);
                }
            }
            b'}' | b']' => return None,
            _ => cursor += 1,
        }
    }
    None
}

fn skip_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    cursor
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn previews_complete_write_arguments() {
        assert_eq!(
            write_argument_preview_from_args(&json!({
                "path": "notes/report.md",
                "content": "first\nsecond",
            })),
            Some(WriteArgumentPreview {
                path: Some("notes/report.md".to_string()),
                text: "first\nsecond".to_string(),
                bytes_seen: 12,
                lines_seen: 2,
                omitted_bytes: 0,
                truncated: false,
            })
        );
    }

    #[test]
    fn decodes_partial_content_with_escapes_and_unicode() {
        let preview = write_argument_preview_from_json(
            r#"{"path":"report.md","content":"line\nquote: \"ok\" slash \\ 你好 \uD83D\uDE80"#,
        )
        .expect("preview");
        assert_eq!(preview.path.as_deref(), Some("report.md"));
        assert_eq!(preview.text, "line\nquote: \"ok\" slash \\ 你好 🚀");
        assert_eq!(preview.lines_seen, 2);

        let incomplete_escape = write_argument_preview_from_json(r#"{"content":"ready \u4f"#)
            .expect("preview before incomplete escape");
        assert_eq!(incomplete_escape.text, "ready ");
    }

    #[test]
    fn finds_target_fields_in_either_order_and_skips_nested_values() {
        let preview = write_argument_preview_from_json(
            r#"{"metadata":{"content":"ignore"},"content":"body","path":"x.txt"}"#,
        )
        .expect("preview");
        assert_eq!(preview.path.as_deref(), Some("x.txt"));
        assert_eq!(preview.text, "body");
    }

    #[test]
    fn bounds_preview_to_utf8_safe_head_and_tail() {
        let content = format!("{}中{}尾", "a".repeat(3_000), "b".repeat(7_000));
        let preview = write_argument_preview_from_args(&json!({
            "path": "large.txt",
            "content": content,
        }))
        .expect("preview");
        assert!(preview.truncated);
        assert!(preview.omitted_bytes > 0);
        assert!(preview.text.starts_with(&"a".repeat(2_048)));
        assert!(preview.text.ends_with('尾'));
        assert!(preview.text.contains("bytes omitted"));
        assert_eq!(preview.bytes_seen, content.len());
    }

    #[test]
    fn tracker_publishes_first_content_then_throttles_and_flushes() {
        let start = Instant::now();
        let mut tracker = WriteArgumentPreviewTracker::default();
        let path = tracker
            .observe(r#"{"path":"a.txt""#, start)
            .expect("path preview");
        assert_eq!(path.path.as_deref(), Some("a.txt"));
        let first_content = tracker
            .observe(
                r#"{"path":"a.txt","content":"one"#,
                start + Duration::from_millis(1),
            )
            .expect("first content is immediate");
        assert_eq!(first_content.text, "one");
        assert!(
            tracker
                .observe(
                    r#"{"path":"a.txt","content":"one two"#,
                    start + Duration::from_millis(499),
                )
                .is_none()
        );
        let paced = tracker
            .observe(
                r#"{"path":"a.txt","content":"one two three"#,
                start + Duration::from_millis(501),
            )
            .expect("paced preview");
        assert_eq!(paced.text, "one two three");
        let flushed = tracker
            .flush(
                r#"{"path":"a.txt","content":"one two three four"}"#,
                start + Duration::from_millis(502),
            )
            .expect("final preview");
        assert_eq!(flushed.text, "one two three four");
    }

    #[test]
    fn tracker_resets_for_a_shorter_cumulative_snapshot() {
        let start = Instant::now();
        let mut tracker = WriteArgumentPreviewTracker::default();
        let _ = tracker.observe(r#"{"content":"first"#, start);
        let reset = tracker
            .observe(r#"{"content":"new"#, start + Duration::from_millis(1))
            .expect("reset preview");
        assert_eq!(reset.text, "new");
    }

    #[test]
    fn tracker_replays_every_valid_utf8_chunk_boundary() {
        let complete = r#"{"content":"line\n你好 \uD83D\uDE80","path":"report.md"}"#;
        let start = Instant::now();
        let mut tracker = WriteArgumentPreviewTracker::default();
        for (step, end) in complete
            .char_indices()
            .map(|(index, character)| index + character.len_utf8())
            .enumerate()
        {
            let _ = tracker.flush(&complete[..end], start + Duration::from_millis(step as u64));
        }
        let preview = tracker.last_preview().expect("final preview");
        assert_eq!(preview.path.as_deref(), Some("report.md"));
        assert_eq!(preview.text, "line\n你好 🚀");
        assert_eq!(preview.lines_seen, 2);
    }

    #[test]
    fn tracker_resets_for_same_length_divergent_snapshot() {
        let start = Instant::now();
        let mut tracker = WriteArgumentPreviewTracker::default();
        let first = tracker
            .observe(r#"{"content":"first"#, start)
            .expect("first preview");
        assert_eq!(first.text, "first");
        let reset = tracker
            .observe(r#"{"content":"other"#, start + Duration::from_millis(1))
            .expect("divergent preview");
        assert_eq!(reset.text, "other");
    }

    #[test]
    fn bounds_display_path_by_characters() {
        let path = format!("{}终", "路".repeat(600));
        let preview = write_argument_preview_from_args(&json!({
            "path": path,
            "content": "body",
        }))
        .expect("preview");
        let bounded = preview.path.expect("path");
        assert_eq!(bounded.chars().count(), WRITE_ARGUMENT_PREVIEW_PATH_CHARS);
        assert!(bounded.ends_with('…'));
    }

    #[test]
    fn malformed_escape_fails_closed() {
        assert!(write_argument_preview_from_json(r#"{"content":"bad\q"#).is_none());
    }
}
