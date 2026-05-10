fn default_title(kind: TranscriptKind) -> &'static str {
    match kind {
        TranscriptKind::Prompt => "",
        TranscriptKind::Answer => "",
        TranscriptKind::Thinking => "Thinking",
        TranscriptKind::Explored => "Explored",
        TranscriptKind::Ran => "Ran",
        TranscriptKind::Changed => "Changed",
        TranscriptKind::Meta => "",
        TranscriptKind::Status => "Status",
        TranscriptKind::Error => "Error",
    }
}

fn user_text_from_message(message: &Value) -> Option<String> {
    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn visible_tui_message_count(messages: &[TuiMessageSummary]) -> Result<usize> {
    messages.iter().try_fold(0, |count, summary| {
        let message = serde_json::to_value(&summary.message)?;
        Ok(count + visible_message_count_from_value(&message))
    })
}

fn visible_message_count_from_value(message: &Value) -> usize {
    match message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "user" => usize::from(user_text_from_message(message).is_some()),
        "assistant" => usize::from(assistant_text_from_message(message).is_some()),
        _ => 0,
    }
}

fn visible_transcript_message_count(rows: &[TranscriptRow]) -> usize {
    rows.iter()
        .filter(|row| matches!(row.kind, TranscriptKind::Prompt | TranscriptKind::Answer))
        .count()
}

fn assistant_text_from_message(message: &Value) -> Option<String> {
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

fn assistant_reasoning_from_message(message: &Value) -> Option<String> {
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

fn reasoning_only_message_receives_meta(message: &Value) -> bool {
    if !assistant_message_allows_terminal_meta(message) {
        return false;
    }
    !assistant_message_has_tool_calls(message)
}

fn visible_answer_message_receives_meta(message: &Value) -> bool {
    assistant_message_allows_terminal_meta(message) && !assistant_message_has_tool_calls(message)
}

fn assistant_message_keeps_tool_calls_active(message: &Value) -> bool {
    message.get("finish_reason").and_then(Value::as_str) == Some("tool_calls")
        && message
            .get("outcome")
            .and_then(Value::as_str)
            .is_none_or(|outcome| outcome == "normal")
}

fn assistant_message_allows_terminal_meta(message: &Value) -> bool {
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

fn assistant_message_has_tool_calls(message: &Value) -> bool {
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
struct HistoryToolCall {
    id: String,
    name: String,
    active_title: String,
    completed_title: String,
}

fn history_tool_calls_from_message(message: &Value) -> Vec<HistoryToolCall> {
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
            let value = serde_json::json!({ "args": args });
            Some(HistoryToolCall {
                id: id.to_string(),
                name: name.to_string(),
                active_title: active_tool_title(name, &value),
                completed_title: tool_title(name, &value),
            })
        })
        .collect()
}

fn tool_call_args_from_block(block: &Value) -> Value {
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

fn message_timestamp_ms(message: &Value) -> Option<i64> {
    message.get("timestamp_ms").and_then(Value::as_i64)
}

fn outcome_from_value(value: &Value) -> Option<Outcome> {
    match value.get("outcome").and_then(Value::as_str)? {
        "normal" => Some(Outcome::Normal),
        "stopped" => Some(Outcome::Stopped),
        "failed" => Some(Outcome::Failed),
        "aborted" => Some(Outcome::Aborted),
        _ => None,
    }
}

fn history_meta_text(
    message: &Value,
    _usage: Option<&Value>,
    metadata: Option<&Value>,
    accounting: Option<&Value>,
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
    if let Some(cost) = compact_cost(accounting) {
        parts.push(cost);
    }
    (!parts.is_empty()).then(|| parts.join("  "))
}

fn history_elapsed_duration(
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

fn metadata_elapsed_duration(metadata: Option<&Value>) -> Option<Duration> {
    metadata
        .and_then(|metadata| metadata.get("elapsed_ms"))
        .and_then(Value::as_u64)
        .map(Duration::from_millis)
}

fn metadata_reasoning_effort(metadata: Option<&Value>) -> Option<&str> {
    metadata
        .and_then(|metadata| metadata.get("reasoning_effort"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
}

fn tool_started_instant(value: &Value) -> Instant {
    let now = Instant::now();
    let Some(started_at_ms) = value.get("started_at_ms").and_then(Value::as_i64) else {
        return now;
    };
    let Some(elapsed_ms) = wall_now_ms().checked_sub(started_at_ms) else {
        return now;
    };
    if elapsed_ms <= 0 {
        return now;
    }
    now.checked_sub(Duration::from_millis(elapsed_ms as u64))
        .unwrap_or(now)
}

fn history_tool_started_instant(message: &Value) -> Instant {
    let now = Instant::now();
    let Some(started_at_ms) = message_timestamp_ms(message) else {
        return now;
    };
    let elapsed_ms = wall_now_ms().saturating_sub(started_at_ms);
    if elapsed_ms <= 0 {
        return now;
    }
    now.checked_sub(Duration::from_millis(elapsed_ms as u64))
        .unwrap_or(now)
}

fn wall_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn row_visible(row: &TranscriptRow, thinking_visible: bool) -> bool {
    thinking_visible || row.kind != TranscriptKind::Thinking
}

#[cfg(test)]
fn next_visible_row(
    rows: &[TranscriptRow],
    index: usize,
    thinking_visible: bool,
) -> Option<&TranscriptRow> {
    rows.iter()
        .skip(index + 1)
        .find(|row| row_visible(row, thinking_visible))
}

#[cfg(test)]
fn compact_trailing_for(
    rows: &[TranscriptRow],
    index: usize,
    row: &TranscriptRow,
    thinking_visible: bool,
) -> bool {
    next_visible_row(rows, index, thinking_visible)
        .is_some_and(|next| row.kind == TranscriptKind::Answer && next.kind == TranscriptKind::Meta)
}

#[cfg(test)]
fn transcript_line_count(
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
            let lines = transcript_lines(row, false, compact_trailing, width, workdir);
            wrapped_line_count(&lines, width)
        })
        .sum()
}

fn wrapped_line_count(lines: &[Line<'_>], width: u16) -> usize {
    if width == 0 {
        return 0;
    }
    Paragraph::new(Text::from(lines.to_vec()))
        .wrap(Wrap { trim: false })
        .line_count(width)
}
