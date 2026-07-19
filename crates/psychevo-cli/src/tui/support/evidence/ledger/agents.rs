#[allow(unused_imports)]
pub(crate) use super::*;

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
    let task_name = value
        .get("task_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|task_name| !generated_agent_task_name(agent, task_name));
    let detail = task_name
        .or_else(|| value.get("agent_description").and_then(Value::as_str))
        .or_else(|| value.get("message").and_then(Value::as_str))
        .or_else(|| value.get("task").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Some(agent_title(agent, detail))
}

pub(crate) fn agent_title_name(title: &str) -> &str {
    title
        .split_once('(')
        .map(|(name, _)| name)
        .unwrap_or(title)
        .trim()
}

pub(crate) fn agent_title_detail(title: &str) -> Option<&str> {
    title
        .split_once('(')
        .and_then(|(_, detail)| detail.strip_suffix(')').or(Some(detail)))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn agent_name_from<'a>(args: &'a Value, result: &'a Value) -> &'a str {
    result
        .get("agent_name")
        .and_then(Value::as_str)
        .or_else(|| result.get("agent_type").and_then(Value::as_str))
        .or_else(|| args.get("agent_type").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("agent")
}

pub(crate) fn agent_detail_from<'a>(args: &'a Value, result: &'a Value) -> Option<&'a str> {
    let agent = agent_name_from(args, result);
    let task_name = result
        .get("task_name")
        .and_then(Value::as_str)
        .or_else(|| args.get("task_name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|task_name| !generated_agent_task_name(agent, task_name));
    task_name
        .or_else(|| result.get("agent_description").and_then(Value::as_str))
        .or_else(|| args.get("description").and_then(Value::as_str))
        .or_else(|| result.get("message").and_then(Value::as_str))
        .or_else(|| args.get("message").and_then(Value::as_str))
        .or_else(|| result.get("task").and_then(Value::as_str))
        .or_else(|| args.get("task").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn agent_title(agent: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) => format!("{agent}({})", single_line_preview(detail, 96)),
        None => agent.to_string(),
    }
}

fn generated_agent_task_name(agent: &str, task_name: &str) -> bool {
    let Some(suffix) = task_name
        .strip_prefix(agent)
        .and_then(|rest| rest.strip_prefix('_'))
    else {
        return false;
    };
    suffix.len() >= 8 && suffix.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub(crate) fn matching_agent_edge<'a>(
    row: &TranscriptRow,
    edges: &'a [AgentEdgeRecord],
    used_edges: &std::collections::BTreeSet<usize>,
) -> Option<(usize, &'a AgentEdgeRecord)> {
    if row.failed {
        return None;
    }
    let row_tool_call_id = row.tool_call_id.as_deref();
    if let Some(match_by_id) = edges.iter().enumerate().find(|(index, edge)| {
        !used_edges.contains(index)
            && row_tool_call_id.is_some_and(|id| agent_edge_metadata_matches(edge, id))
    }) {
        return Some(match_by_id);
    }

    let row_name = agent_title_name(&row.title);
    if let Some(match_by_task) = edges.iter().enumerate().find(|(index, edge)| {
        !used_edges.contains(index)
            && agent_edge_agent_name(edge).is_some_and(|name| name == row_name)
            && agent_row_task_matches(row, edge)
    }) {
        return Some(match_by_task);
    }

    None
}

pub(crate) fn agent_edge_title(
    edge: &AgentEdgeRecord,
    catalog: Option<&AgentCatalog>,
) -> Option<String> {
    let name = agent_edge_agent_name(edge)?;
    let detail = agent_edge_agent_string(edge, "task_name")
        .or_else(|| agent_edge_agent_string(edge, "message"))
        .or_else(|| {
            catalog.and_then(|catalog| {
                catalog
                    .agents
                    .iter()
                    .find(|agent| agent.name == name)
                    .map(|agent| agent.description.as_str())
            })
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| agent_edge_agent_string(edge, "description"))
        .or_else(|| agent_edge_agent_string(edge, "task"));
    Some(agent_title(name, detail))
}

pub(crate) fn agent_edge_metadata_matches(edge: &AgentEdgeRecord, target: &str) -> bool {
    agent_edge_agent_string(edge, "id").is_some_and(|value| value == target)
        || agent_edge_agent_string(edge, "parent_tool_call_id").is_some_and(|value| value == target)
}

pub(crate) fn agent_edge_task_matches(edge: &AgentEdgeRecord, target: &str) -> bool {
    agent_edge_agent_string(edge, "task_name").is_some_and(|value| value == target)
        || agent_edge_agent_string(edge, "message").is_some_and(|value| value == target)
        || agent_edge_agent_string(edge, "task").is_some_and(|value| value == target)
}

fn agent_row_task_matches(row: &TranscriptRow, edge: &AgentEdgeRecord) -> bool {
    agent_title_detail(&row.title).is_some_and(|detail| agent_edge_task_matches(edge, detail))
        || agent_row_prompt_detail(row).is_some_and(|detail| agent_edge_task_matches(edge, detail))
}

fn agent_row_prompt_detail(row: &TranscriptRow) -> Option<&str> {
    row.full_text
        .as_deref()?
        .strip_prefix("Prompt:\n")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn agent_edge_agent_name(edge: &AgentEdgeRecord) -> Option<&str> {
    agent_edge_agent_string(edge, "agent_type").or_else(|| agent_edge_agent_string(edge, "name"))
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
    let preview = if status == "running"
        && value.get("type").and_then(Value::as_str) != Some("agent_session_start")
    {
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

    let prompt = agent_tool_prompt(value);
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

pub(crate) fn running_agent_tool_full_text(value: &Value) -> Option<String> {
    let prompt = agent_tool_prompt(value)?;
    Some(format!("Prompt:\n{prompt}"))
}

fn agent_tool_prompt(value: &Value) -> Option<&str> {
    let args = value.get("args").unwrap_or(&Value::Null);
    let result = value.get("result").unwrap_or(&Value::Null);
    args.get("message")
        .or_else(|| args.get("prompt"))
        .or_else(|| args.get("task"))
        .or_else(|| result.get("message"))
        .or_else(|| result.get("prompt"))
        .or_else(|| result.get("task"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
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
    effective_usage_total(Some(usage)).tokens
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
    value
        .get("child_thread_id")
        .or_else(|| value.get("child_session_id"))
        .or_else(|| {
            value
                .get("result")
                .and_then(|result| result.get("child_thread_id"))
        })
        .or_else(|| {
            value
                .get("result")
                .and_then(|result| result.get("child_session_id"))
        })
        .or_else(|| {
            value
                .get("result")
                .and_then(|result| result.get("session_id"))
        })
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn background_running_agent_result(tool: &str, value: &Value) -> bool {
    tool == "spawn_agent"
        && value.get("type").and_then(Value::as_str) != Some("agent_session_start")
        && value
            .get("result")
            .and_then(|result| result.get("status"))
            .and_then(Value::as_str)
            == Some("running")
}
