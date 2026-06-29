#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn auxiliary_agent_live_for_session(
    agent: &AuxiliaryAgentTask,
    session_id: &str,
) -> bool {
    if !agent.visible_live {
        return false;
    }
    agent.child_session_id.as_deref() == Some(session_id)
        || agent.session_id.as_deref() == Some(session_id)
}

pub(crate) fn current_session_matches(
    owner_session: Option<&str>,
    current_session: Option<&str>,
) -> bool {
    match owner_session {
        Some(owner_session) => current_session == Some(owner_session),
        None => true,
    }
}

fn completed_agent_invocation_row(row: &TranscriptRow) -> bool {
    row.tool_name.as_deref() == Some("spawn_agent")
        && !active_tool_row(row)
        && (row.tool_started.is_none()
            || row.tool_elapsed.is_some()
            || row.agent_target.is_some()
            || row.failed
            || row.interrupted)
}

pub(crate) fn apply_agent_child_value_preview(row: &mut TranscriptRow, value: &Value) -> bool {
    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "tool_execution_start" => {
            let tool = value
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            append_agent_child_live_line(
                &mut row.agent_child_live_text,
                active_tool_title(tool, value),
            );
            true
        }
        "tool_execution_end" => {
            row.agent_child_tool_uses = row.agent_child_tool_uses.saturating_add(1);
            let tool = value
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            append_agent_child_live_line(&mut row.agent_child_live_text, tool_title(tool, value));
            true
        }
        "message_end" => {
            if let Some(usage) = value.get("usage") {
                row.agent_child_latest_tokens =
                    usage_total_tokens(usage).or(row.agent_child_latest_tokens);
            }
            if let Some(text) =
                assistant_text_from_event(value).filter(|text| !text.trim().is_empty())
            {
                append_agent_child_live_line(
                    &mut row.agent_child_live_text,
                    format!("Response: {}", single_line_preview(&text, 160)),
                );
            }
            true
        }
        "agent_end" => true,
        _ => false,
    }
}

pub(crate) fn append_agent_child_live_line(buffer: &mut String, line: impl AsRef<str>) {
    let line = line.as_ref().trim();
    if line.is_empty() {
        return;
    }
    if !buffer.is_empty() {
        buffer.push('\n');
    }
    buffer.push_str(line);
}

pub(crate) fn append_agent_child_live_fragment(
    buffer: &mut String,
    label: &str,
    fragment: &str,
) -> bool {
    if fragment.trim().is_empty() {
        return false;
    }
    let prefix = format!("{label}: ");
    let last_line_start = buffer.rfind('\n').map(|index| index + 1).unwrap_or(0);
    if buffer
        .get(last_line_start..)
        .is_some_and(|line| line.starts_with(&prefix))
    {
        buffer.push_str(fragment);
        return true;
    }
    append_agent_child_live_line(buffer, format!("{prefix}{}", fragment.trim_start()));
    true
}

pub(crate) fn refresh_agent_child_preview(row: &mut TranscriptRow) {
    let status = if active_tool_row(row) {
        "Running"
    } else if row.interrupted {
        "Interrupted"
    } else if row.failed {
        "Failed"
    } else {
        "Done"
    };
    let status = agent_child_status_text(
        status,
        row.agent_child_tool_uses,
        row.agent_child_latest_tokens,
    );
    if row.agent_child_live_text.trim().is_empty() {
        row.text = status;
        row.full_text = None;
        return;
    }
    let full = format!("{status}\n{}", row.agent_child_live_text);
    row.set_evidence_body_text(full);
}

pub(crate) fn agent_child_status_text(status: &str, tool_uses: i64, tokens: Option<u64>) -> String {
    let token_suffix = tokens
        .map(|tokens| format!(" · {} tokens", format_compact_count(tokens)))
        .unwrap_or_default();
    format!(
        "{status} ({} {}{})",
        tool_uses,
        pluralize(tool_uses, "tool use"),
        token_suffix
    )
}

pub(crate) fn exec_session_id_from_args(args: &Value) -> Option<u64> {
    args.get("session_id").and_then(Value::as_u64)
}

pub(crate) fn exec_session_id_from_result(value: &Value) -> Option<u64> {
    value
        .get("result")
        .and_then(|result| result.get("session_id"))
        .and_then(Value::as_u64)
}

pub(crate) fn exec_result_running(value: &Value) -> bool {
    exec_session_id_from_result(value).is_some()
        && value
            .get("result")
            .and_then(|result| result.get("exit_code"))
            .is_none_or(Value::is_null)
}

pub(crate) fn exec_result_completed(value: &Value) -> bool {
    value
        .get("result")
        .and_then(|result| result.get("exit_code"))
        .is_some_and(|exit_code| !exit_code.is_null())
}

pub(crate) fn tool_result_output(value: &Value) -> String {
    value
        .get("result")
        .and_then(|result| result.get("output"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn write_stdin_non_empty_chars(args: &Value) -> Option<&str> {
    args.get("chars")
        .and_then(Value::as_str)
        .filter(|chars| !chars.is_empty())
}

pub(crate) fn bounded_stdin_display(chars: &str) -> String {
    const MAX_CHARS: usize = 4096;
    if chars.chars().count() <= MAX_CHARS {
        return chars.to_string();
    }
    let mut output = chars.chars().take(MAX_CHARS).collect::<String>();
    output.push_str("\n... truncated");
    output
}
