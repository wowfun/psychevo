#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug)]
pub(crate) struct MessageFields {
    pub(crate) role: String,
    pub(crate) timestamp_ms: i64,
    pub(crate) content_text: Option<String>,
    pub(crate) tool_call_id: Option<String>,
    pub(crate) tool_name: Option<String>,
    pub(crate) tool_calls_json: Option<String>,
    pub(crate) finish_reason: Option<String>,
    pub(crate) outcome: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) tool_call_count: i64,
}

pub(crate) fn message_fields(message: &Message) -> Result<MessageFields> {
    match message {
        Message::User {
            content,
            timestamp_ms,
        } => Ok(MessageFields {
            role: "user".to_string(),
            timestamp_ms: *timestamp_ms,
            content_text: Some(user_content_text(content)),
            tool_call_id: None,
            tool_name: None,
            tool_calls_json: None,
            finish_reason: None,
            outcome: None,
            model: None,
            provider: None,
            tool_call_count: 0,
        }),
        Message::Assistant {
            content,
            timestamp_ms,
            finish_reason,
            outcome,
            model,
            provider,
        } => {
            let text = content
                .iter()
                .filter_map(|block| match block {
                    AssistantBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let tool_calls = content
                .iter()
                .filter_map(|block| match block {
                    AssistantBlock::ToolCall(call) => Some(call),
                    _ => None,
                })
                .collect::<Vec<_>>();
            Ok(MessageFields {
                role: "assistant".to_string(),
                timestamp_ms: *timestamp_ms,
                content_text: if text.is_empty() { None } else { Some(text) },
                tool_call_id: None,
                tool_name: None,
                tool_calls_json: if tool_calls.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string(&tool_calls)?)
                },
                finish_reason: finish_reason.clone(),
                outcome: Some(outcome.as_str().to_string()),
                model: model.clone(),
                provider: provider.clone(),
                tool_call_count: tool_calls.len() as i64,
            })
        }
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            timestamp_ms,
        } => Ok(MessageFields {
            role: "tool_result".to_string(),
            timestamp_ms: *timestamp_ms,
            content_text: Some(content.clone()),
            tool_call_id: Some(tool_call_id.clone()),
            tool_name: Some(tool_name.clone()),
            tool_calls_json: None,
            finish_reason: None,
            outcome: Some(if *is_error { "failed" } else { "normal" }.to_string()),
            model: None,
            provider: None,
            tool_call_count: 0,
        }),
    }
}

pub(crate) fn user_content_text(content: &[psychevo_agent_core::UserContentBlock]) -> String {
    let mut image_index = 0usize;
    content
        .iter()
        .map(|block| match block {
            psychevo_agent_core::UserContentBlock::Text(block) => block.text.clone(),
            psychevo_agent_core::UserContentBlock::LocalImage(_)
            | psychevo_agent_core::UserContentBlock::ImageUrl(_) => {
                image_index += 1;
                format!("[Image {image_index}]")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn optional_json_string(value: &Option<Value>) -> Result<Option<String>> {
    value
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(Into::into)
}

pub(crate) fn parse_optional_json(value: Option<String>) -> Result<Option<Value>> {
    value
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(Into::into)
}
