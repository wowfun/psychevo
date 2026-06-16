#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) fn default_title(kind: TranscriptKind) -> &'static str {
    match kind {
        TranscriptKind::Prompt => "",
        TranscriptKind::Answer => "",
        TranscriptKind::Thinking => "Thinking",
        TranscriptKind::Explored => "Explored",
        TranscriptKind::Ran => "Ran",
        TranscriptKind::Updated => "Updated",
        TranscriptKind::Meta => "",
        TranscriptKind::Command => "",
        TranscriptKind::Status => "Status",
        TranscriptKind::Error => "Error",
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UserPromptDisplay {
    pub(crate) text: String,
    pub(crate) attachment_meta: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct UserShellDisplay {
    pub(crate) command: String,
    pub(crate) result: Value,
    pub(crate) outcome: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentNotificationProjection {
    pub(crate) text: String,
    pub(crate) target: Option<String>,
}

pub(crate) fn user_display_from_message(
    message: &Value,
    metadata: Option<&Value>,
) -> Option<UserPromptDisplay> {
    if side_inherited_message(metadata) {
        return None;
    }
    if agent_notification_present(metadata) {
        return None;
    }
    if user_shell_display_from_message(message, metadata).is_some() {
        return None;
    }
    if let Some(display) = tui_display_metadata(metadata) {
        return Some(UserPromptDisplay {
            text: display.content_text,
            attachment_meta: attachment_meta_from_display(&display.attachments),
        });
    }
    let text = legacy_user_text_from_message(message).unwrap_or_default();
    let attachment_meta = legacy_attachment_meta_from_message(message);
    (!text.is_empty() || attachment_meta.is_some()).then_some(UserPromptDisplay {
        text,
        attachment_meta,
    })
}

pub(crate) fn agent_notification_display(metadata: Option<&Value>) -> Option<String> {
    agent_notification_projection(metadata).map(|projection| projection.text)
}

pub(crate) fn agent_notification_target(metadata: Option<&Value>) -> Option<String> {
    agent_notification_projection(metadata).and_then(|projection| projection.target)
}

pub(crate) fn agent_notification_present(metadata: Option<&Value>) -> bool {
    metadata
        .and_then(|metadata| metadata.get("agent_notification"))
        .is_some()
}

pub(crate) fn agent_notification_projection(
    metadata: Option<&Value>,
) -> Option<AgentNotificationProjection> {
    let notification = metadata?.get("agent_notification")?;
    if notification
        .get("hidden")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let target = notification
        .get("child_session_id")
        .and_then(Value::as_str)
        .or_else(|| notification.get("agent_id").and_then(Value::as_str))
        .map(str::to_string);
    let kind = notification
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("agent_notification");
    if kind == "missing_required_agent_call" {
        let agents = notification
            .get("agents")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        return Some(AgentNotificationProjection {
            text: format!("required agent was not called: {agents}"),
            target: None,
        });
    }
    let name = notification
        .get("agent_name")
        .and_then(Value::as_str)
        .unwrap_or("agent");
    if kind == "agent_started" {
        let summary = notification
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text = if summary.trim().is_empty() {
            format!("Agent `{name}` started in the background.")
        } else {
            format!("Agent `{name}` started in the background.\n\n{summary}")
        };
        return Some(AgentNotificationProjection { text, target });
    }
    let outcome = notification
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let summary = notification
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = if summary.trim().is_empty() {
        format!("Agent `{name}` completed with outcome {outcome}.")
    } else {
        format!("Agent `{name}` completed with outcome {outcome}.\n\n{summary}")
    };
    Some(AgentNotificationProjection { text, target })
}

pub(crate) fn user_text_from_message(message: &Value, metadata: Option<&Value>) -> Option<String> {
    if side_inherited_message(metadata) {
        return None;
    }
    if let Some(display) = user_shell_display_from_message(message, metadata) {
        return Some(format!("!{}", display.command));
    }
    user_display_from_message(message, metadata).map(|display| display.text)
}

pub(crate) fn side_inherited_message(metadata: Option<&Value>) -> bool {
    side_inherited_metadata_hidden(metadata)
}

pub(crate) fn legacy_user_text_from_message(message: &Value) -> Option<String> {
    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                return Some(text.to_string());
            }
            None
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

pub(crate) fn visible_tui_message_count(messages: &[TuiMessageSummary]) -> Result<usize> {
    messages.iter().try_fold(0, |count, summary| {
        let message = serde_json::to_value(&summary.message)?;
        Ok(count
            + match message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "user" => usize::from(
                    user_shell_display_from_message(&message, summary.metadata.as_ref()).is_some()
                        || user_display_from_message(&message, summary.metadata.as_ref()).is_some(),
                ),
                "assistant" => usize::from(
                    !side_inherited_message(summary.metadata.as_ref())
                        && assistant_text_from_message(&message).is_some(),
                ),
                _ => 0,
            })
    })
}

pub(crate) fn user_shell_display_from_message(
    message: &Value,
    metadata: Option<&Value>,
) -> Option<UserShellDisplay> {
    if let Some(display) = metadata
        .and_then(|metadata| metadata.get(USER_SHELL_METADATA_KEY))
        .and_then(user_shell_display_from_metadata)
    {
        return Some(display);
    }
    let text = legacy_user_text_from_message(message)?;
    user_shell_display_from_xml(&text)
}

pub(crate) fn user_shell_display_from_metadata(metadata: &Value) -> Option<UserShellDisplay> {
    let command = metadata.get("command")?.as_str()?.to_string();
    let result = metadata
        .get("result")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"output": "(no output)", "truncated": false}));
    let outcome = metadata
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("normal")
        .to_string();
    Some(UserShellDisplay {
        command,
        result,
        outcome,
    })
}

pub(crate) fn user_shell_display_from_xml(text: &str) -> Option<UserShellDisplay> {
    let trimmed = text.trim_start();
    if !trimmed.starts_with("<user_shell_command>") {
        return None;
    }
    let command = unescape_xml_text(extract_xml_tag(trimmed, "command")?);
    let result_text = unescape_xml_text(extract_xml_tag(trimmed, "result").unwrap_or_default());
    let outcome = if result_text.contains("Exit code: 0") {
        "normal"
    } else {
        "failed"
    }
    .to_string();
    Some(UserShellDisplay {
        command,
        result: serde_json::json!({
            "output": result_text,
            "truncated": false,
        }),
        outcome,
    })
}

pub(crate) fn extract_xml_tag<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let start_tag = format!("<{tag}>");
    let end_tag = format!("</{tag}>");
    let start = text.find(&start_tag)? + start_tag.len();
    let end = text[start..].find(&end_tag)? + start;
    Some(&text[start..end])
}

pub(crate) fn unescape_xml_text(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub(crate) fn tui_display_metadata(metadata: Option<&Value>) -> Option<PromptDisplayMetadata> {
    metadata
        .and_then(|metadata| metadata.get(TUI_DISPLAY_METADATA_KEY))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

pub(crate) fn attachment_meta_from_display(
    attachments: &[PromptAttachmentDisplay],
) -> Option<String> {
    if attachments.is_empty() {
        return None;
    }
    let mut lines = vec!["attachments".to_string()];
    for (index, attachment) in attachments.iter().enumerate() {
        lines.push(format!(
            "{} {}: {}",
            attachment.kind,
            index + 1,
            attachment.source
        ));
    }
    Some(lines.join("\n"))
}

pub(crate) fn legacy_attachment_meta_from_message(message: &Value) -> Option<String> {
    let content = message.get("content")?.as_array()?;
    let mut sources = Vec::new();
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("local_image") => {
                if let Some(path) = block.get("path").and_then(Value::as_str) {
                    sources.push(path.to_string());
                }
            }
            Some("image_url") => {
                if let Some(url) = block.get("url").and_then(Value::as_str) {
                    sources.push(url.to_string());
                }
            }
            _ => {}
        }
    }
    if sources.is_empty() {
        return None;
    }
    let mut lines = vec!["attachments".to_string()];
    for (index, source) in sources.iter().enumerate() {
        lines.push(format!("image {}: {source}", index + 1));
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
pub(crate) fn visible_transcript_message_count(rows: &[TranscriptRow]) -> usize {
    rows.iter()
        .filter(|row| matches!(row.kind, TranscriptKind::Prompt | TranscriptKind::Answer))
        .count()
}

pub(crate) fn assistant_text_from_message(message: &Value) -> Option<String> {
    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

pub(crate) fn assistant_reasoning_from_message(message: &Value) -> Option<String> {
    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("reasoning"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

pub(crate) fn reasoning_only_message_receives_meta(message: &Value) -> bool {
    if !assistant_message_allows_terminal_meta(message) {
        return false;
    }
    !assistant_message_has_tool_calls(message)
}

pub(crate) fn visible_answer_message_receives_meta(message: &Value) -> bool {
    assistant_message_allows_terminal_meta(message) && !assistant_message_has_tool_calls(message)
}

pub(crate) fn assistant_message_keeps_tool_calls_active(message: &Value) -> bool {
    message.get("finish_reason").and_then(Value::as_str) == Some("tool_calls")
        && message
            .get("outcome")
            .and_then(Value::as_str)
            .is_none_or(|outcome| outcome == "normal")
}

pub(crate) fn assistant_message_allows_terminal_meta(message: &Value) -> bool {
    if message
        .get("finish_reason")
        .and_then(Value::as_str)
        .is_some_and(|finish_reason| matches!(finish_reason, "tool_calls" | "aborted"))
    {
        return false;
    }
    message
        .get("outcome")
        .and_then(Value::as_str)
        .is_none_or(|outcome| outcome == "normal")
}

pub(crate) fn assistant_message_has_tool_calls(message: &Value) -> bool {
    message
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|content| {
            content.iter().any(|block| {
                block.get("type").and_then(Value::as_str) == Some("tool_call")
                    || block.get("tool_calls").is_some()
            })
        })
}

#[derive(Debug, Clone)]
pub(crate) struct HistoryToolCall {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) args: Value,
    pub(crate) active_title: String,
    pub(crate) completed_title: String,
}

pub(crate) fn history_tool_calls_from_message(message: &Value) -> Vec<HistoryToolCall> {
    let Some(content) = message.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(Value::as_str) != Some("tool_call") {
                return None;
            }
            let id = block.get("id").and_then(Value::as_str)?;
            let name = block.get("name").and_then(Value::as_str)?;
            let args = tool_call_args_from_block(block);
            let value = serde_json::json!({ "args": args.clone() });
            Some(HistoryToolCall {
                id: id.to_string(),
                name: name.to_string(),
                args,
                active_title: active_tool_title(name, &value),
                completed_title: tool_title(name, &value),
            })
        })
        .collect()
}

pub(crate) fn tool_call_args_from_block(block: &Value) -> Value {
    block
        .get("arguments")
        .cloned()
        .or_else(|| {
            block
                .get("arguments_json")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str(raw).ok())
        })
        .unwrap_or(Value::Null)
}

pub(crate) fn message_timestamp_ms(message: &Value) -> Option<i64> {
    message.get("timestamp_ms").and_then(Value::as_i64)
}

pub(crate) fn pending_input_id_from_message_end(value: &Value) -> Option<u64> {
    value
        .get("metadata")
        .and_then(|metadata| metadata.get("pending_input"))
        .and_then(|pending| pending.get("id"))
        .and_then(Value::as_u64)
}

pub(crate) fn outcome_from_value(value: &Value) -> Option<Outcome> {
    match value.get("outcome").and_then(Value::as_str)? {
        "normal" => Some(Outcome::Normal),
        "stopped" => Some(Outcome::Stopped),
        "failed" => Some(Outcome::Failed),
        "aborted" => Some(Outcome::Aborted),
        _ => None,
    }
}

pub(crate) fn history_meta_text(
    message: &Value,
    _usage: Option<&Value>,
    metadata: Option<&Value>,
    _accounting: Option<&Value>,
    prompt_started_ms: Option<i64>,
) -> Option<String> {
    let provider = message
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("");
    let model = message.get("model").and_then(Value::as_str).unwrap_or("");
    let mut parts = Vec::new();
    if !provider.is_empty() || !model.is_empty() {
        parts.push(model_meta_label(provider, model, metadata));
    }
    if let Some(elapsed) = history_elapsed_duration(message, metadata, prompt_started_ms) {
        parts.push(format_duration_compact(elapsed));
    }
    (!parts.is_empty()).then(|| parts.join("  "))
}

pub(crate) fn history_elapsed_duration(
    message: &Value,
    metadata: Option<&Value>,
    prompt_started_ms: Option<i64>,
) -> Option<Duration> {
    if let Some(elapsed) = metadata_elapsed_duration(metadata) {
        return Some(elapsed);
    }
    let started = prompt_started_ms?;
    let ended = message_timestamp_ms(message)?;
    let elapsed = ended.checked_sub(started)?;
    (elapsed >= 0).then_some(Duration::from_millis(elapsed as u64))
}

pub(crate) fn metadata_elapsed_duration(metadata: Option<&Value>) -> Option<Duration> {
    metadata
        .and_then(|metadata| metadata.get("elapsed_ms"))
        .and_then(Value::as_u64)
        .map(Duration::from_millis)
}

pub(crate) fn metadata_reasoning_effort(metadata: Option<&Value>) -> Option<&str> {
    metadata
        .and_then(|metadata| metadata.get("reasoning_effort"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
}

pub(crate) fn instant_from_wall_timestamp_ms(started_at_ms: i64) -> Option<Instant> {
    let now = Instant::now();
    let elapsed_ms = wall_now_ms().checked_sub(started_at_ms)?;
    if elapsed_ms <= 0 {
        return Some(now);
    }
    now.checked_sub(Duration::from_millis(elapsed_ms as u64))
        .or(Some(now))
}

pub(crate) fn tool_started_instant(value: &Value) -> Instant {
    let Some(started_at_ms) = value.get("started_at_ms").and_then(Value::as_i64) else {
        return Instant::now();
    };
    instant_from_wall_timestamp_ms(started_at_ms).unwrap_or_else(Instant::now)
}

pub(crate) fn history_tool_started_instant(message: &Value) -> Instant {
    let Some(started_at_ms) = message_timestamp_ms(message) else {
        return Instant::now();
    };
    instant_from_wall_timestamp_ms(started_at_ms).unwrap_or_else(Instant::now)
}

pub(crate) fn wall_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(crate) fn row_visible(row: &TranscriptRow, thinking_visible: bool) -> bool {
    thinking_visible || row.kind != TranscriptKind::Thinking
}

#[cfg(test)]
pub(crate) fn next_visible_row(
    rows: &[TranscriptRow],
    index: usize,
    thinking_visible: bool,
) -> Option<&TranscriptRow> {
    rows.iter()
        .skip(index + 1)
        .find(|row| row_visible(row, thinking_visible))
}

#[cfg(test)]
pub(crate) fn compact_trailing_for(
    rows: &[TranscriptRow],
    index: usize,
    row: &TranscriptRow,
    thinking_visible: bool,
) -> bool {
    next_visible_row(rows, index, thinking_visible)
        .is_some_and(|next| row.kind == TranscriptKind::Answer && next.kind == TranscriptKind::Meta)
}

#[cfg(test)]
pub(crate) fn transcript_line_count(
    rows: &[TranscriptRow],
    width: u16,
    thinking_visible: bool,
    workdir: &Path,
) -> usize {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| row_visible(row, thinking_visible))
        .map(|(index, row)| {
            let compact_trailing = compact_trailing_for(rows, index, row, thinking_visible);
            let lines = transcript_lines(row, false, compact_trailing, width, workdir, false);
            wrapped_line_count(&lines, width)
        })
        .sum()
}

pub(crate) fn wrapped_line_count(lines: &[Line<'_>], width: u16) -> usize {
    if width == 0 {
        return 0;
    }
    Paragraph::new(Text::from(lines.to_vec()))
        .wrap(Wrap { trim: false })
        .line_count(width)
}
