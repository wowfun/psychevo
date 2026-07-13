pub(crate) fn render_markdown_message(out: &mut String, record: &ExportMessageRecord) {
    match &record.message {
        Message::User { content, .. } => {
            push_line(out, &format!("### {}. User", record.session_seq));
            let text = user_content_markdown(content);
            if text.trim().is_empty() {
                push_line(out, "_No text content._");
            } else {
                push_line(out, "");
                push_line(out, &text);
            }
        }
        Message::Assistant { content, .. } => {
            push_line(out, &format!("### {}. Assistant", record.session_seq));
            for block in content {
                match block {
                    AssistantBlock::Text { text } => {
                        if !text.trim().is_empty() {
                            push_line(out, "");
                            push_line(out, text);
                        }
                    }
                    AssistantBlock::Reasoning { text, .. } => {
                        if !text.trim().is_empty() {
                            push_line(out, "");
                            push_line(out, "#### Reasoning");
                            push_fenced_text(out, text);
                        }
                    }
                    AssistantBlock::ToolCall(call) => {
                        push_line(out, "");
                        push_line(
                            out,
                            &format!("#### Tool call: `{}` (`{}`)", call.name, call.id),
                        );
                        push_fenced_json(out, &call.arguments);
                    }
                    AssistantBlock::ProviderTool(call) => {
                        push_line(out, "");
                        push_line(out, &format!("#### Hosted tool: `{}` (`{}`, {})", call.name, call.id, call.status));
                        if let Some(action) = &call.action { push_fenced_json(out, action); }
                    }
                    AssistantBlock::Source(psychevo_ai::AssistantSource::UrlCitation(source)) => {
                        push_line(out, &format!("- Source: [{}]({})", source.title, source.url));
                    }
                    AssistantBlock::Source(psychevo_ai::AssistantSource::Image(source)) => {
                        push_line(out, &format!("- Image source: {}", source.source_website_url));
                    }
                    AssistantBlock::Source(psychevo_ai::AssistantSource::Provider { kind, .. }) => {
                        push_line(out, &format!("- Provider source: {kind}"));
                    }
                }
            }
        }
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            ..
        } => {
            push_line(
                out,
                &format!(
                    "### {}. Tool result: `{}` (`{}`)",
                    record.session_seq, tool_name, tool_call_id
                ),
            );
            push_line(out, &format!("- error: `{is_error}`"));
            push_fenced_text(out, content);
        }
    }
}

pub(crate) fn sanitize_message_without_reasoning(message: &Message) -> Message {
    match message {
        Message::Assistant {
            content,
            timestamp_ms,
            finish_reason,
            outcome,
            model,
            provider,
        } => Message::Assistant {
            content: content
                .iter()
                .filter(|block| !matches!(block, AssistantBlock::Reasoning { .. }))
                .cloned()
                .collect(),
            timestamp_ms: *timestamp_ms,
            finish_reason: finish_reason.clone(),
            outcome: *outcome,
            model: model.clone(),
            provider: provider.clone(),
        },
        other => other.clone(),
    }
}
