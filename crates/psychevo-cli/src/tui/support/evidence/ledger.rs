#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn evidence_kind(tool: &str) -> TranscriptKind {
    evidence_kind_from_display(&ToolDisplaySpec::for_name(tool))
}

pub(crate) fn evidence_kind_for_value(tool: &str, value: &Value) -> TranscriptKind {
    evidence_kind_from_display(&tool_display_spec(tool, value))
}

pub(crate) fn evidence_kind_from_display(display: &ToolDisplaySpec) -> TranscriptKind {
    match display.category {
        ToolDisplayCategory::Explore => TranscriptKind::Explored,
        ToolDisplayCategory::Run => TranscriptKind::Ran,
        ToolDisplayCategory::Update => TranscriptKind::Updated,
        ToolDisplayCategory::Status => TranscriptKind::Status,
    }
}

pub(crate) fn is_write_like_tool(tool: &str) -> bool {
    matches!(tool, "write" | "edit")
}

pub(crate) fn active_tool_row(row: &TranscriptRow) -> bool {
    !row.failed && !row.interrupted && row.tool_started.is_some() && row.tool_elapsed.is_none()
}

pub(crate) fn completed_live_tool_elapsed(
    row: &TranscriptRow,
    metadata: Option<&Value>,
) -> Option<Duration> {
    let runtime = metadata_elapsed_duration(metadata);
    let active = row.tool_started.map(|started| started.elapsed());
    match (runtime, active) {
        (Some(runtime), Some(active)) => Some(runtime.max(active)),
        (Some(runtime), None) => Some(runtime),
        (None, Some(active)) => Some(active),
        (None, None) => None,
    }
}

pub(crate) fn completed_tool_title_from_active(kind: TranscriptKind, title: &str) -> String {
    tool_title_as_invocation(None, kind, title, false)
}

pub(crate) fn tool_title(tool: &str, value: &Value) -> String {
    if tool == "Agent" {
        return agent_tool_title(value).unwrap_or_else(|| "Agent".to_string());
    }
    if tool == "clarify" {
        return clarify_tool_title(value);
    }
    if is_user_shell_value(value) {
        let command = value
            .get("args")
            .and_then(|args| args.get("cmd"))
            .and_then(Value::as_str)
            .and_then(first_shell_command_line);
        return user_shell_title(command);
    }
    let args = value.get("args").unwrap_or(&Value::Null);
    let result = value.get("result").unwrap_or(&Value::Null);
    let display = tool_display_spec(tool, value);
    let detail = title_detail_from_keys(&display.title_arg_keys, args).or_else(|| {
        if tool == "exec_command" {
            None
        } else {
            title_detail_from_keys(&display.title_result_keys, result)
        }
    });
    tool_name_title(tool, detail.as_deref())
}

pub(crate) fn clarify_no_answer_result(value: &Value) -> bool {
    if value.get("tool_name").and_then(Value::as_str) != Some("clarify") {
        return false;
    }
    let Some(error) = value
        .get("result")
        .and_then(|result| result.get("error"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    matches!(
        error,
        "clarify was cancelled by the user"
            | "timed out waiting for user input"
            | "clarify was interrupted because the turn ended"
            | "clarify response channel closed"
    )
}

pub(crate) fn active_tool_title(tool: &str, value: &Value) -> String {
    if tool == "Agent" {
        return active_agent_tool_title(value);
    }
    if tool == "clarify" {
        return "Questions pending".to_string();
    }
    if is_user_shell_value(value) {
        let command = value
            .get("args")
            .and_then(|args| args.get("cmd"))
            .and_then(Value::as_str)
            .and_then(first_shell_command_line);
        return user_shell_title(command);
    }
    let args = value.get("args").unwrap_or(&Value::Null);
    let display = tool_display_spec(tool, value);
    tool_name_title(
        tool,
        title_detail_from_keys(&display.title_arg_keys, args).as_deref(),
    )
}

pub(crate) fn tool_title_for_update(tool: &str, value: &Value, existing_title: &str) -> String {
    let title = tool_title(tool, value);
    if tool == "exec_command" && matches!(title.as_str(), "exec_command" | "!") {
        let existing = tool_title_as_invocation(
            Some(tool),
            evidence_kind_for_value(tool, value),
            existing_title,
            is_user_shell_value(value),
        );
        if existing != "exec_command" && existing != "!" {
            return existing;
        }
    }
    if title == tool {
        let existing = tool_title_as_invocation(
            Some(tool),
            evidence_kind_for_value(tool, value),
            existing_title,
            false,
        );
        if existing != tool && !existing.starts_with("Tool ") {
            return existing;
        }
    }
    title
}

pub(crate) fn tool_display_spec(tool: &str, value: &Value) -> ToolDisplaySpec {
    value
        .get("display")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_else(|| ToolDisplaySpec::for_name(tool))
}

pub(crate) fn title_detail_from_keys(keys: &[String], source: &Value) -> Option<String> {
    for key in keys {
        let Some(value) = source.get(key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        if key == "cmd" {
            if let Some(command) = value.as_str().and_then(first_shell_command_line) {
                return Some(command.to_string());
            }
            continue;
        }
        if let Some(detail) = display_value_inline(value).filter(|value| !value.trim().is_empty()) {
            return Some(detail);
        }
    }
    None
}

pub(crate) fn tool_name_title(tool: &str, detail: Option<&str>) -> String {
    let Some(detail) = detail.map(str::trim).filter(|detail| !detail.is_empty()) else {
        return tool.to_string();
    };
    format!("{tool} {detail}")
}

pub(crate) fn user_shell_title(command: Option<&str>) -> String {
    let Some(command) = command.map(str::trim).filter(|command| !command.is_empty()) else {
        return "!".to_string();
    };
    format!("! {command}")
}

pub(crate) fn tool_title_as_invocation(
    tool: Option<&str>,
    kind: TranscriptKind,
    title: &str,
    user_shell: bool,
) -> String {
    let title = title.trim();
    if title.is_empty() {
        return tool
            .map(|tool| tool_name_title(tool, None))
            .unwrap_or_default();
    }
    if user_shell
        || title == "!"
        || title.starts_with("! ")
        || title.starts_with("Ran ! ")
        || title.starts_with("Running ! ")
    {
        return legacy_user_shell_title(title);
    }
    if let Some(tool) = tool
        && (title == tool || title.starts_with(&format!("{tool} ")))
    {
        return title.to_string();
    }
    match kind {
        TranscriptKind::Explored => {
            let tool = tool.unwrap_or("read");
            if matches!(title, "Exploring" | "Explored") {
                return tool.to_string();
            }
            if let Some(rest) = title
                .strip_prefix("Exploring ")
                .or_else(|| title.strip_prefix("Explored "))
            {
                return tool_name_title(tool, Some(rest));
            }
            title.to_string()
        }
        TranscriptKind::Ran => {
            if matches!(title, "Running" | "Ran" | "Running command" | "Ran command") {
                return "exec_command".to_string();
            }
            if let Some(command) = title
                .strip_prefix("Running ")
                .or_else(|| title.strip_prefix("Ran "))
            {
                return tool_name_title("exec_command", Some(command));
            }
            title.to_string()
        }
        TranscriptKind::Updated => {
            let tool = tool.unwrap_or("write");
            if matches!(
                title,
                "Updating" | "Updated" | "Updating files" | "Updated files"
            ) {
                return tool.to_string();
            }
            if let Some(path) = title
                .strip_prefix("Updating ")
                .or_else(|| title.strip_prefix("Updated "))
            {
                let path = path.trim();
                if path == "files" {
                    return tool.to_string();
                }
                return tool_name_title(tool, Some(path));
            }
            title.to_string()
        }
        _ => title.to_string(),
    }
}

pub(crate) fn legacy_user_shell_title(title: &str) -> String {
    let title = title.trim();
    if title == "!" {
        return "!".to_string();
    }
    for prefix in ["Running ! ", "Ran ! ", "! "] {
        if let Some(command) = title.strip_prefix(prefix) {
            return user_shell_title(Some(command));
        }
    }
    title.to_string()
}

pub(crate) fn is_user_shell_value(value: &Value) -> bool {
    value.get("source").and_then(Value::as_str) == Some("user_shell")
}

pub(crate) fn first_shell_command_line(text: &str) -> Option<&str> {
    let mut first_non_empty = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        first_non_empty.get_or_insert(line);
        if !line.starts_with('#') {
            return Some(line);
        }
    }
    first_non_empty
}

pub(crate) fn tool_output_text(value: &Value) -> (String, Option<String>) {
    if let Some(timeout) = exec_timeout_output_text(value) {
        return collapse_ledger_body(&timeout);
    }
    if value.get("tool_name").and_then(Value::as_str) == Some("Agent")
        && let Some(output) = agent_tool_output_text(value)
    {
        return output;
    }
    if value.get("tool_name").and_then(Value::as_str) == Some("clarify") {
        return (clarify_tool_output_text(value), None);
    }
    let tool = value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let display = tool_display_spec(tool, value);
    let result = value.get("result").unwrap_or(&Value::Null);
    if let Some(error) = result.get("error").and_then(Value::as_str) {
        return collapse_ledger_body(error);
    }
    if display.body_policy == ToolDisplayBodyPolicy::Body
        && let Some(body) = body_text_from_keys(&display.body_keys, result)
    {
        return collapse_ledger_body(&body);
    }
    let summary = format_tool_summary(value);
    let detail = body_text_from_keys(&display.body_keys, result);
    match detail {
        Some(detail) if detail != summary => {
            (summary.clone(), Some(format!("{summary}\n\n{detail}")))
        }
        _ => (summary, None),
    }
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
    let display = tool_display_spec(tool, value);
    let summary = summarize_tool_result(&display, result);
    if summary.is_empty() {
        format!("{tool} {outcome}")
    } else {
        format!("{tool} {outcome}: {summary}")
    }
}

pub(crate) fn summarize_tool_result(display: &ToolDisplaySpec, result: &Value) -> String {
    if let Some(error) = result.get("error").and_then(Value::as_str) {
        return truncate_inline(error, 140);
    }
    let mut parts = Vec::new();
    for key in &display.summary_keys {
        let Some(value) = result.get(key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        let Some(display) = display_value_inline(value) else {
            continue;
        };
        if display.trim().is_empty() {
            continue;
        }
        parts.push(format!("{key}={}", truncate_inline(&display, 60)));
        if parts.len() >= 4 {
            break;
        }
    }
    truncate_inline(&parts.join(" "), 180)
}

pub(crate) fn body_text_from_keys(keys: &[String], result: &Value) -> Option<String> {
    let mut bodies = Vec::new();
    for key in keys {
        let Some(value) = result.get(key) else {
            continue;
        };
        let Some(text) = display_value_block(value).filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        bodies.push((key.as_str(), text));
    }
    match bodies.as_slice() {
        [] => None,
        [(_, text)] => Some(text.clone()),
        many => Some(
            many.iter()
                .map(|(key, text)| format!("{key}:\n{text}"))
                .collect::<Vec<_>>()
                .join("\n\n"),
        ),
    }
}

pub(crate) fn display_value_inline(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.split_whitespace().collect::<Vec<_>>().join(" ")),
        Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
        Value::Array(items) if items.is_empty() => None,
        Value::Object(items) if items.is_empty() => None,
        other => serde_json::to_string(other).ok(),
    }
}

pub(crate) fn display_value_block(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        Value::Array(items) if items.is_empty() => None,
        Value::Object(items) if items.is_empty() => None,
        other => serde_json::to_string_pretty(other)
            .or_else(|_| serde_json::to_string(other))
            .ok(),
    }
}

pub(crate) fn truncate_inline(input: &str, max_chars: usize) -> String {
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

pub(crate) fn clarify_tool_title(value: &Value) -> String {
    if value
        .get("result")
        .and_then(|result| result.get("error"))
        .is_some()
        && !clarify_no_answer_result(value)
    {
        return "Clarify failed".to_string();
    }
    let args = value.get("args").unwrap_or(&Value::Null);
    let result = value.get("result").unwrap_or(&Value::Null);
    let total = args
        .get("questions")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    let answered = result
        .get("answers")
        .and_then(Value::as_array)
        .map(|answers| {
            answers
                .iter()
                .filter(|answer| {
                    answer
                        .get("answers")
                        .and_then(Value::as_array)
                        .is_some_and(|items| !items.is_empty())
                })
                .count()
        })
        .unwrap_or_default();
    if total == 0 && answered > 0 {
        format!("Questions {answered}/{answered} answered")
    } else if total == 0 {
        "Questions 0/0 answered".to_string()
    } else {
        format!("Questions {answered}/{total} answered")
    }
}

pub(crate) fn clarify_tool_output_text(value: &Value) -> String {
    let args = value.get("args").unwrap_or(&Value::Null);
    let result = value.get("result").unwrap_or(&Value::Null);
    if let Some(error) = result.get("error").and_then(Value::as_str)
        && !clarify_no_answer_result(value)
    {
        return error.to_string();
    }
    let questions = args
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let answers = result.get("answers").and_then(Value::as_array);
    let mut lines = Vec::new();
    for (index, question) in questions.iter().enumerate() {
        let text = question
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or("question");
        let Some(answer_items) = answers
            .and_then(|answers| answers.get(index))
            .and_then(|answer| answer.get("answers"))
            .and_then(Value::as_array)
        else {
            lines.push(format!("{text} (unanswered)"));
            continue;
        };
        if answer_items.is_empty() {
            lines.push(format!("{text} (unanswered)"));
            continue;
        }
        lines.push(text.to_string());
        for item in answer_items {
            let Some(answer) = item.as_str() else {
                continue;
            };
            if let Some(note) = answer.strip_prefix("user_note: ") {
                lines.push(format!("note: {note}"));
            } else {
                lines.push(format!("answer: {answer}"));
            }
        }
    }
    if lines.is_empty() {
        format_tool_summary(value)
    } else {
        lines.join("\n")
    }
}

pub(crate) fn active_agent_tool_title(value: &Value) -> String {
    let args = value.get("args").unwrap_or(&Value::Null);
    let agent = agent_name_from(args, &Value::Null);
    let detail = agent_detail_from(args, &Value::Null);
    agent_title(agent, detail)
}

pub(crate) fn agent_tool_title(value: &Value) -> Option<String> {
    let args = value.get("args").unwrap_or(&Value::Null);
    let result = value.get("result").unwrap_or(&Value::Null);
    if result.is_null() && args.is_null() {
        return None;
    }
    let agent = agent_name_from(args, result);
    let detail = agent_detail_from(args, result);
    Some(agent_title(agent, detail))
}

pub(crate) fn agent_session_start_title(value: &Value) -> Option<String> {
    let agent = value
        .get("agent_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let detail = value
        .get("agent_description")
        .and_then(Value::as_str)
        .or_else(|| value.get("task_name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Some(agent_title(agent, detail))
}

pub(crate) fn agent_session_start_name(value: &Value) -> Option<&str> {
    value
        .get("agent_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn agent_title_name(title: &str) -> &str {
    title
        .split_once('(')
        .map(|(name, _)| name)
        .unwrap_or(title)
        .trim()
}

pub(crate) fn agent_placeholder_title_matches(row: &TranscriptRow, agent_name: &str) -> bool {
    let title_name = agent_title_name(&row.title);
    title_name == agent_name || matches!(title_name, "agent" | "Agent")
}

pub(crate) fn agent_name_from<'a>(args: &'a Value, result: &'a Value) -> &'a str {
    result
        .get("agent_name")
        .and_then(Value::as_str)
        .or_else(|| result.get("agent_type").and_then(Value::as_str))
        .or_else(|| args.get("agent_type").and_then(Value::as_str))
        .or_else(|| args.get("name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("agent")
}

pub(crate) fn agent_detail_from<'a>(args: &'a Value, result: &'a Value) -> Option<&'a str> {
    result
        .get("agent_description")
        .and_then(Value::as_str)
        .or_else(|| args.get("description").and_then(Value::as_str))
        .or_else(|| result.get("task_name").and_then(Value::as_str))
        .or_else(|| args.get("task_name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn agent_title(agent: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) => format!("{agent}({})", single_line_preview(detail, 96)),
        None => agent.to_string(),
    }
}

pub(crate) fn matching_agent_edge<'a>(
    row: &TranscriptRow,
    edges: &'a [AgentEdgeRecord],
    used_edges: &std::collections::BTreeSet<usize>,
) -> Option<(usize, &'a AgentEdgeRecord)> {
    let row_tool_call_id = row.tool_call_id.as_deref();
    if let Some(match_by_id) = edges.iter().enumerate().find(|(index, edge)| {
        !used_edges.contains(index)
            && row_tool_call_id.is_some_and(|id| agent_edge_metadata_matches(edge, id))
    }) {
        return Some(match_by_id);
    }

    let row_name = agent_title_name(&row.title);
    if let Some(match_by_name) = edges.iter().enumerate().find(|(index, edge)| {
        !used_edges.contains(index)
            && agent_edge_agent_name(edge).is_some_and(|name| name == row_name)
    }) {
        return Some(match_by_name);
    }

    if matches!(row_name, "agent" | "Agent") {
        return edges
            .iter()
            .enumerate()
            .find(|(index, _)| !used_edges.contains(index));
    }

    None
}

pub(crate) fn agent_edge_title(
    edge: &AgentEdgeRecord,
    catalog: Option<&AgentCatalog>,
) -> Option<String> {
    let name = agent_edge_agent_name(edge)?;
    let detail = catalog
        .and_then(|catalog| {
            catalog
                .agents
                .iter()
                .find(|agent| agent.name == name)
                .map(|agent| agent.description.as_str())
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| agent_edge_agent_string(edge, "description"))
        .or_else(|| agent_edge_agent_string(edge, "task_name"))
        .or_else(|| agent_edge_agent_string(edge, "task"));
    Some(agent_title(name, detail))
}

pub(crate) fn agent_edge_metadata_matches(edge: &AgentEdgeRecord, target: &str) -> bool {
    agent_edge_agent_string(edge, "id").is_some_and(|value| value == target)
        || agent_edge_agent_string(edge, "task_name").is_some_and(|value| value == target)
}

pub(crate) fn agent_edge_agent_name(edge: &AgentEdgeRecord) -> Option<&str> {
    agent_edge_agent_string(edge, "name")
}

pub(crate) fn agent_edge_agent_string<'a>(edge: &'a AgentEdgeRecord, key: &str) -> Option<&'a str> {
    edge.metadata
        .as_ref()?
        .get("agent")?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn single_line_preview(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&compact, max_chars)
}

pub(crate) fn agent_tool_output_text(value: &Value) -> Option<(String, Option<String>)> {
    let result = value.get("result")?;
    let status = result
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let outcome = result
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or(status);
    let tool_calls = result
        .get("child_session")
        .and_then(|summary| summary.get("tool_call_count"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let token_suffix = result
        .get("child_session")
        .and_then(agent_child_latest_tokens)
        .map(|tokens| format!(" · {} tokens", format_compact_count(tokens)));
    let preview =
        if result.get("background").and_then(Value::as_bool) == Some(true) && status == "running" {
            "Started in background".to_string()
        } else if status == "completed" || outcome == "normal" {
            let tool_use_label = pluralize(tool_calls, "tool use");
            format!(
                "Done ({} {}{})",
                tool_calls,
                tool_use_label,
                token_suffix.unwrap_or_default()
            )
        } else {
            let tool_use_label = pluralize(tool_calls, "tool use");
            format!(
                "{} ({} {}{})",
                status_label(status, outcome),
                tool_calls,
                tool_use_label,
                token_suffix.unwrap_or_default()
            )
        };

    let prompt = result
        .get("task")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let response = result
        .get("final_answer")
        .or_else(|| result.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| result.get("error").and_then(Value::as_str));
    let session = result
        .get("session_id")
        .or_else(|| result.get("child_session_id"))
        .and_then(Value::as_str);

    let mut full_parts = Vec::new();
    if let Some(prompt) = prompt {
        full_parts.push(format!("Prompt:\n{prompt}"));
    }
    if let Some(response) = response {
        full_parts.push(format!("Response:\n{response}"));
    }
    if let Some(session) = session {
        full_parts.push(format!("Session: {}", short_session(session)));
    }
    let full = (!full_parts.is_empty())
        .then(|| full_parts.join("\n\n"))
        .filter(|full| full != &preview);
    Some((preview, full))
}

pub(crate) fn pluralize(count: i64, singular: &str) -> String {
    if count == 1 {
        singular.to_string()
    } else {
        format!("{singular}s")
    }
}

pub(crate) fn agent_child_latest_tokens(summary: &Value) -> Option<u64> {
    summary
        .get("latest_total_tokens")
        .and_then(Value::as_u64)
        .or_else(|| summary.get("latest_usage").and_then(usage_total_tokens))
}

pub(crate) fn usage_total_tokens(usage: &Value) -> Option<u64> {
    usage
        .get("total_tokens")
        .and_then(Value::as_u64)
        .or_else(|| {
            let mut total = 0u64;
            let mut any = false;
            for key in [
                "input_tokens",
                "output_tokens",
                "reasoning_tokens",
                "cached_tokens",
                "cache_write_tokens",
            ] {
                if let Some(value) = usage.get(key).and_then(Value::as_u64) {
                    total = total.saturating_add(value);
                    any = true;
                }
            }
            any.then_some(total)
        })
}

pub(crate) fn format_compact_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

pub(crate) fn status_label(status: &str, outcome: &str) -> String {
    match (status, outcome) {
        ("errored", _) | (_, "failed") => "Failed".to_string(),
        ("interrupted", _) | (_, "aborted") | (_, "interrupted") => "Interrupted".to_string(),
        ("shutdown", _) | (_, "shutdown") => "Closed".to_string(),
        ("running", _) => "Running".to_string(),
        _ => status.replace('_', " "),
    }
}

pub(crate) fn agent_target_from_tool_event(value: &Value) -> Option<String> {
    let result = value.get("result")?;
    result
        .get("session_id")
        .or_else(|| result.get("child_session_id"))
        .and_then(Value::as_str)
        .or_else(|| result.get("id").and_then(Value::as_str))
        .or_else(|| result.get("task_name").and_then(Value::as_str))
        .map(str::to_string)
}

pub(crate) fn exec_timeout_output_text(value: &Value) -> Option<String> {
    if value.get("tool_name").and_then(Value::as_str) != Some("exec_command") {
        return None;
    }
    let result = value.get("result").unwrap_or(&Value::Null);
    let error = result.get("error").and_then(Value::as_str)?;
    if !error.starts_with("command timed out after ") {
        return None;
    }
    let prompt = format!("timeout: {error}");
    let output = result
        .get("output")
        .and_then(Value::as_str)
        .filter(|output| {
            let trimmed = output.trim();
            !trimmed.is_empty() && trimmed != "(no output)"
        });
    Some(match output {
        Some(output) => format!("{prompt}; partial output follows\n{output}"),
        None => prompt,
    })
}

pub(crate) fn tool_event_interrupted(value: &Value) -> bool {
    value.get("outcome").and_then(Value::as_str) == Some("aborted")
        || value
            .get("result")
            .and_then(|result| result.get("error"))
            .and_then(Value::as_str)
            == Some("aborted")
}

pub(crate) fn model_label(provider: &str, model: &str) -> String {
    match (provider.is_empty(), model.is_empty()) {
        (false, false) => format!("{provider}/{model}"),
        (false, true) => provider.to_string(),
        (true, false) => model.to_string(),
        (true, true) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn body_text_from_keys_skips_null_values() {
        let keys = vec!["diff".to_string(), "error".to_string()];
        let result = json!({
            "diff": "diff text",
            "error": null
        });
        assert_eq!(
            body_text_from_keys(&keys, &result),
            Some("diff text".to_string())
        );

        let keys = vec!["error".to_string()];
        assert_eq!(body_text_from_keys(&keys, &result), None);
    }
}
