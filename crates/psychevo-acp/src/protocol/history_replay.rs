use std::path::Path;

use agent_client_protocol::schema::v2::{
    ContentBlock, ContentChunk, ImageContent, MessageId, Meta, ResourceLink, SessionId,
    SessionUpdate, TextContent, ToolCallContent, ToolCallStatus, ToolCallUpdate,
    UpdateSessionNotification,
};
use agent_client_protocol::{Client, ConnectionTo, Error};
use psychevo::Thread;
use psychevo::application::{
    AssistantBlock, AssistantSource, HistoryReplayItem, HistoryReplayWarning,
    HistoryReplayWarningKind, Message, ProviderToolBlock, ToolCallBlock, UserContentBlock,
};
use serde_json::{Map, Value, json};

use super::core_types::{tool_kind, tool_title};

const REPLAY_PAGE_SIZE: usize = 200;
const REPLAY_WARNING_LIMIT: usize = 32;

pub(crate) async fn replay_thread_history(
    thread: &Thread,
    session_id: &SessionId,
    cx: &ConnectionTo<Client>,
) -> Result<Option<Meta>, Error> {
    let history = thread.history();
    let mut after_session_seq = None;
    let mut warnings = ReplayWarnings::default();
    loop {
        let page = history
            .replay_after(after_session_seq, Some(REPLAY_PAGE_SIZE))
            .await
            .map_err(super::core_types::acp_internal_error)?;
        for warning in page.warnings {
            warnings.push_framework(warning);
        }
        for item in page.items {
            for update in replay_item_updates(item, &mut warnings) {
                cx.send_notification(UpdateSessionNotification::new(session_id.clone(), update))?;
            }
        }
        let Some(next_after) = page.next_after else {
            break;
        };
        after_session_seq = Some(next_after);
    }
    Ok(warnings.meta())
}

fn replay_item_updates(
    item: HistoryReplayItem,
    warnings: &mut ReplayWarnings,
) -> Vec<SessionUpdate> {
    match item {
        HistoryReplayItem::Available { item } => {
            let item = *item;
            message_updates(item.session_seq, item.message, warnings)
        }
        HistoryReplayItem::Unavailable { session_seq } => vec![agent_text_update(
            session_seq,
            "unavailable",
            format!("[history item {session_seq} is unavailable]"),
        )],
    }
}

fn message_updates(
    session_seq: i64,
    message: Message,
    warnings: &mut ReplayWarnings,
) -> Vec<SessionUpdate> {
    match message {
        Message::User { content, .. } => content
            .into_iter()
            .map(|block| {
                SessionUpdate::UserMessageChunk(ContentChunk::new(
                    user_content(block),
                    MessageId::new(format!("history:{session_seq}:user")),
                ))
            })
            .collect(),
        Message::Assistant { content, .. } => {
            let mut updates = Vec::new();
            for block in content {
                match block {
                    AssistantBlock::Text { text } => {
                        updates.push(agent_text_update(session_seq, "assistant", text));
                    }
                    AssistantBlock::Reasoning { text, .. } => {
                        updates.push(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                            ContentBlock::Text(TextContent::new(text)),
                            MessageId::new(format!("history:{session_seq}:reasoning")),
                        )));
                    }
                    AssistantBlock::ToolCall(call) => {
                        updates.push(tool_call_update(call));
                    }
                    AssistantBlock::ProviderTool(tool) => {
                        updates.push(provider_tool_update(tool));
                    }
                    AssistantBlock::Source { source } => match source_update(session_seq, source) {
                        Some(update) => updates.push(update),
                        None => {
                            warnings.push_projection(session_seq, "unsupported_provider_source");
                            updates.push(agent_text_update(
                                session_seq,
                                "source",
                                "[history source is unavailable]",
                            ));
                        }
                    },
                }
            }
            if updates.is_empty() {
                warnings.push_projection(session_seq, "empty_assistant_message");
                updates.push(agent_text_update(
                    session_seq,
                    "assistant",
                    "[empty assistant history item]",
                ));
            }
            updates
        }
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            ..
        } => vec![SessionUpdate::ToolCallUpdate(
            ToolCallUpdate::new(tool_call_id)
                .title(tool_title(&tool_name))
                .kind(tool_kind(&tool_name))
                .status(if is_error {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Completed
                })
                .content(vec![ToolCallContent::from(content.clone())])
                .raw_output(parse_json_or_string(content)),
        )],
    }
}

fn user_content(block: UserContentBlock) -> ContentBlock {
    match block {
        UserContentBlock::Text(block) => ContentBlock::Text(TextContent::new(block.text)),
        UserContentBlock::LocalImage(block) => {
            let name = block
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("image")
                .to_string();
            ContentBlock::ResourceLink(ResourceLink::new(name, local_image_uri(&block.path)))
        }
        UserContentBlock::ImageUrl(block) => {
            ContentBlock::Image(ImageContent::new("", "application/octet-stream").uri(block.url))
        }
    }
}

fn local_image_uri(path: &Path) -> String {
    if path.is_absolute() {
        format!("file://{}", path.display())
    } else {
        path.display().to_string()
    }
}

fn agent_text_update(session_seq: i64, stream: &str, text: impl Into<String>) -> SessionUpdate {
    SessionUpdate::AgentMessageChunk(ContentChunk::new(
        ContentBlock::Text(TextContent::new(text)),
        MessageId::new(format!("history:{session_seq}:{stream}")),
    ))
}

fn tool_call_update(call: ToolCallBlock) -> SessionUpdate {
    SessionUpdate::ToolCallUpdate(
        ToolCallUpdate::new(call.id)
            .title(tool_title(&call.name))
            .kind(tool_kind(&call.name))
            .status(ToolCallStatus::Pending)
            .raw_input(call.arguments),
    )
}

fn provider_tool_update(tool: ProviderToolBlock) -> SessionUpdate {
    SessionUpdate::ToolCallUpdate(
        ToolCallUpdate::new(tool.id)
            .title(tool_title(&tool.name))
            .kind(tool_kind(&tool.name))
            .status(replay_tool_status(&tool.status))
            .raw_input(tool.action),
    )
}

fn replay_tool_status(status: &str) -> ToolCallStatus {
    match status {
        "completed" | "complete" | "succeeded" => ToolCallStatus::Completed,
        "failed" | "error" => ToolCallStatus::Failed,
        "pending" => ToolCallStatus::Pending,
        _ => ToolCallStatus::InProgress,
    }
}

fn source_update(session_seq: i64, source: AssistantSource) -> Option<SessionUpdate> {
    let content = match source {
        AssistantSource::UrlCitation(source) => {
            ContentBlock::ResourceLink(ResourceLink::new(source.title, source.url))
        }
        AssistantSource::Image(source) => {
            ContentBlock::Image(ImageContent::new("", "image/*").uri(source.image_url))
        }
        AssistantSource::Provider { .. } => return None,
    };
    Some(SessionUpdate::AgentMessageChunk(ContentChunk::new(
        content,
        MessageId::new(format!("history:{session_seq}:source")),
    )))
}

fn parse_json_or_string(value: String) -> Value {
    serde_json::from_str(&value).unwrap_or(Value::String(value))
}

#[derive(Debug, Default)]
struct ReplayWarnings {
    items: Vec<Value>,
    omitted_count: usize,
}

impl ReplayWarnings {
    fn push_framework(&mut self, warning: HistoryReplayWarning) {
        self.push(
            warning.session_seq,
            match warning.kind {
                HistoryReplayWarningKind::InvalidMessage => "invalid_message",
                HistoryReplayWarningKind::InvalidUsage => "invalid_usage",
                HistoryReplayWarningKind::InvalidMetadata => "invalid_metadata",
                HistoryReplayWarningKind::InvalidAccounting => "invalid_accounting",
            },
        );
    }

    fn push_projection(&mut self, session_seq: i64, kind: &str) {
        self.push(session_seq, kind);
    }

    fn push(&mut self, session_seq: i64, kind: &str) {
        if self.items.len() < REPLAY_WARNING_LIMIT {
            self.items.push(json!({
                "sessionSeq": session_seq,
                "kind": kind,
            }));
        } else {
            self.omitted_count += 1;
        }
    }

    fn meta(self) -> Option<Meta> {
        if self.items.is_empty() && self.omitted_count == 0 {
            return None;
        }
        let mut meta = Map::new();
        meta.insert(
            "psychevo".to_string(),
            json!({
                "replay_warnings": {
                    "items": self.items,
                    "omitted_count": self.omitted_count,
                }
            }),
        );
        Some(meta)
    }
}

#[cfg(test)]
mod tests {
    use psychevo::application::{
        HistoryReplayItem, HistoryReplayWarning, HistoryReplayWarningKind,
    };

    use super::{REPLAY_WARNING_LIMIT, ReplayWarnings, replay_item_updates};

    #[test]
    fn replay_warning_summary_is_globally_bounded() {
        let mut warnings = ReplayWarnings::default();
        for session_seq in 1..=(REPLAY_WARNING_LIMIT as i64 + 7) {
            warnings.push_framework(HistoryReplayWarning {
                session_seq,
                kind: HistoryReplayWarningKind::InvalidMessage,
            });
            let updates = replay_item_updates(
                HistoryReplayItem::Unavailable { session_seq },
                &mut warnings,
            );
            assert_eq!(updates.len(), 1);
        }
        let meta = warnings.meta().expect("warning metadata");
        let summary = &meta["psychevo"]["replay_warnings"];
        assert_eq!(
            summary["items"].as_array().map(Vec::len),
            Some(REPLAY_WARNING_LIMIT)
        );
        assert_eq!(summary["omitted_count"].as_u64(), Some(7));
    }
}
