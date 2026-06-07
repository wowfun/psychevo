use crossterm::style::Stylize;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub(crate) struct TuiRenderer {
    pub(crate) color: bool,
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

    pub(crate) fn paint(&self, text: &str, kind: StyleKind) -> String {
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
pub(crate) enum StyleKind {
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

pub(crate) fn format_session_line(
    id: &str,
    project: &str,
    provider: &str,
    model: &str,
    messages: i64,
) -> String {
    let short = &id[..id.len().min(8)];
    format!("{short} {project} {provider}/{model} messages={messages}")
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;
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
