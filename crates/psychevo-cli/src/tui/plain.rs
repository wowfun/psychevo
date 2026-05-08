use crossterm::style::Stylize;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub(crate) struct TuiRenderer {
    color: bool,
}

impl TuiRenderer {
    pub(crate) fn new(color: bool) -> Self {
        Self { color }
    }

    pub(crate) fn dim(&self, text: &str) -> String {
        self.paint(text, StyleKind::Dim)
    }

    pub(crate) fn status(&self, text: &str) -> String {
        self.paint(text, StyleKind::Status)
    }

    pub(crate) fn success(&self, text: &str) -> String {
        self.paint(text, StyleKind::Success)
    }

    pub(crate) fn error(&self, text: &str) -> String {
        self.paint(text, StyleKind::Error)
    }

    fn paint(&self, text: &str, kind: StyleKind) -> String {
        if !self.color {
            return text.to_string();
        }
        match kind {
            StyleKind::Dim => format!("{}", text.dark_grey()),
            StyleKind::Status => format!("{}", text.cyan()),
            StyleKind::Success => format!("{}", text.green()),
            StyleKind::Error => format!("{}", text.red()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum StyleKind {
    Dim,
    Status,
    Success,
    Error,
}

pub(crate) fn assistant_text_from_event(value: &Value) -> Option<String> {
    let event_type = value.get("type")?.as_str()?;
    if event_type != "message_update" && event_type != "message_end" {
        return None;
    }
    let message = value.get("message")?;
    if message.get("role")?.as_str()? != "assistant" {
        return None;
    }
    let content = message.get("content")?.as_array()?;
    let text = content
        .iter()
        .filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(text)
}

pub(crate) fn format_tool_summary(value: &Value) -> String {
    let tool = value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let outcome = value
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("normal");
    let result = value.get("result").unwrap_or(&Value::Null);
    let summary = summarize_result(result);
    if summary.is_empty() {
        format!("{tool} {outcome}")
    } else {
        format!("{tool} {outcome}: {summary}")
    }
}

fn summarize_result(value: &Value) -> String {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return truncate_inline(error, 140);
    }
    for key in [
        "path",
        "files_modified",
        "bytes_written",
        "exit_code",
        "truncated",
    ] {
        if let Some(value) = value.get(key) {
            return truncate_inline(&format!("{key}={}", compact_json(value)), 140);
        }
    }
    if let Some(content) = value.get("content").and_then(Value::as_str) {
        return truncate_inline(content, 140);
    }
    if let Some(output) = value.get("output").and_then(Value::as_str) {
        return truncate_inline(output, 140);
    }
    String::new()
}

fn compact_json(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}

pub(crate) fn format_session_line(
    id: &str,
    source: &str,
    provider: &str,
    model: &str,
    messages: i64,
) -> String {
    let short = &id[..id.len().min(8)];
    format!("{short} {source} {provider}/{model} messages={messages}")
}

fn truncate_inline(input: &str, max_chars: usize) -> String {
    let normalized = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut out = normalized
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plain_renderer_has_no_ansi_sequences() {
        let renderer = TuiRenderer::new(false);
        assert_eq!(renderer.status("ready"), "ready");
    }

    #[test]
    fn assistant_text_event_ignores_reasoning_blocks() {
        let event = json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [
                    { "type": "reasoning", "text": "hidden" },
                    { "type": "text", "text": "visible" }
                ]
            }
        });
        assert_eq!(
            assistant_text_from_event(&event).as_deref(),
            Some("visible")
        );
    }
}
