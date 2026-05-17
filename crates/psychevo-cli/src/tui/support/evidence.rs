fn evidence_kind(tool: &str) -> TranscriptKind {
    match tool {
        "read" | "list" | "search" => TranscriptKind::Explored,
        "bash" => TranscriptKind::Ran,
        "write" | "edit" => TranscriptKind::Updated,
        _ => TranscriptKind::Status,
    }
}

fn is_write_like_tool(tool: &str) -> bool {
    matches!(tool, "write" | "edit")
}

fn active_tool_row(row: &TranscriptRow) -> bool {
    !row.failed && !row.interrupted && row.tool_started.is_some() && row.tool_elapsed.is_none()
}

fn completed_live_tool_elapsed(row: &TranscriptRow, metadata: Option<&Value>) -> Option<Duration> {
    let runtime = metadata_elapsed_duration(metadata);
    let active = row.tool_started.map(|started| started.elapsed());
    match (runtime, active) {
        (Some(runtime), Some(active)) => Some(runtime.max(active)),
        (Some(runtime), None) => Some(runtime),
        (None, Some(active)) => Some(active),
        (None, None) => None,
    }
}

fn completed_tool_title_from_active(kind: TranscriptKind, title: &str) -> String {
    let title = title.trim();
    let Some((active_prefix, completed_prefix, fallback)) = tool_title_prefixes_for_kind(kind)
    else {
        return title.to_string();
    };
    if let Some(rest) = title.strip_prefix(active_prefix) {
        let rest = rest.trim();
        if rest.is_empty() {
            completed_prefix.to_string()
        } else {
            format!("{completed_prefix} {rest}")
        }
    } else if title.is_empty() {
        fallback.to_string()
    } else {
        title.to_string()
    }
}

fn tool_title_prefixes_for_kind(
    kind: TranscriptKind,
) -> Option<(&'static str, &'static str, &'static str)> {
    match kind {
        TranscriptKind::Explored => Some(("Exploring", "Explored", "Explored")),
        TranscriptKind::Ran => Some(("Running", "Ran", "Ran command")),
        TranscriptKind::Updated => Some(("Updating", "Updated", "Updated files")),
        _ => None,
    }
}

fn tool_title(tool: &str, value: &Value) -> String {
    let args = value.get("args").unwrap_or(&Value::Null);
    let result = value.get("result").unwrap_or(&Value::Null);
    match tool {
        "read" | "list" => format!("Explored {}", path_from(args, result).unwrap_or(".")),
        "search" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .or_else(|| result.get("query").and_then(Value::as_str))
                .unwrap_or("text");
            format!("Explored search {query}")
        }
        "bash" => {
            let command = args
                .get("command")
                .and_then(Value::as_str)
                .and_then(first_shell_command_line)
                .unwrap_or("command");
            if is_user_shell_value(value) {
                format!("Ran ! {command}")
            } else {
                format!("Ran {command}")
            }
        }
        "write" | "edit" => format!("Updated {}", path_from(args, result).unwrap_or("files")),
        "Agent" => agent_tool_title(value).unwrap_or_else(|| "Agent".to_string()),
        other => format!("Tool {other}"),
    }
}

fn active_tool_title(tool: &str, value: &Value) -> String {
    let args = value.get("args").unwrap_or(&Value::Null);
    match tool {
        "read" | "list" => path_from_args(args)
            .map(|path| format!("Exploring {path}"))
            .unwrap_or_else(|| "Exploring".to_string()),
        "search" => args
            .get("query")
            .and_then(Value::as_str)
            .map(|query| format!("Exploring search {query}"))
            .unwrap_or_else(|| "Exploring search".to_string()),
        "bash" => args
            .get("command")
            .and_then(Value::as_str)
            .and_then(first_shell_command_line)
            .map(|command| {
                if is_user_shell_value(value) {
                    format!("Running ! {command}")
                } else {
                    format!("Running {command}")
                }
            })
            .unwrap_or_else(|| {
                if is_user_shell_value(value) {
                    "Running ! command".to_string()
                } else {
                    "Running command".to_string()
                }
            }),
        "write" | "edit" => path_from_args(args)
            .map(|path| format!("Updating {path}"))
            .unwrap_or_else(|| "Updating files".to_string()),
        "Agent" => active_agent_tool_title(value),
        other => format!("Using {other}"),
    }
}

fn tool_title_for_update(tool: &str, value: &Value, existing_title: &str) -> String {
    let title = tool_title(tool, value);
    if tool == "bash" && matches!(title.as_str(), "Ran command" | "Ran ! command") {
        if existing_title.starts_with("Ran ") {
            return existing_title.to_string();
        }
        if let Some(command) = existing_title.strip_prefix("Running ") {
            return format!("Ran {command}");
        }
    }
    if matches!(tool, "write" | "edit") && title == "Updated files"
        && let Some(path) = existing_title.strip_prefix("Updating ")
        && path != "files"
    {
        return format!("Updated {path}");
    }
    title
}

fn is_user_shell_value(value: &Value) -> bool {
    value.get("source").and_then(Value::as_str) == Some("user_shell")
}

fn first_shell_command_line(text: &str) -> Option<&str> {
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

fn path_from<'a>(args: &'a Value, result: &'a Value) -> Option<&'a str> {
    args.get("path")
        .and_then(Value::as_str)
        .or_else(|| result.get("path").and_then(Value::as_str))
}

fn path_from_args(args: &Value) -> Option<&str> {
    args.get("path").and_then(Value::as_str)
}

fn tool_output_text(value: &Value) -> (String, Option<String>) {
    if let Some(timeout) = bash_timeout_output_text(value) {
        return collapse_ledger_body(&timeout);
    }
    if value.get("tool_name").and_then(Value::as_str) == Some("Agent")
        && let Some(output) = agent_tool_output_text(value)
    {
        return output;
    }
    let result = value.get("result").unwrap_or(&Value::Null);
    let full = result
        .get("content")
        .and_then(Value::as_str)
        .or_else(|| result.get("output").and_then(Value::as_str))
        .or_else(|| result.get("diff").and_then(Value::as_str))
        .or_else(|| result.get("error").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| format_tool_summary(value));
    collapse_ledger_body(&full)
}

fn active_agent_tool_title(value: &Value) -> String {
    let args = value.get("args").unwrap_or(&Value::Null);
    let agent = agent_name_from(args, &Value::Null);
    let detail = agent_detail_from(args, &Value::Null);
    agent_title(agent, detail)
}

fn agent_tool_title(value: &Value) -> Option<String> {
    let args = value.get("args").unwrap_or(&Value::Null);
    let result = value.get("result").unwrap_or(&Value::Null);
    if result.is_null() && args.is_null() {
        return None;
    }
    let agent = agent_name_from(args, result);
    let detail = agent_detail_from(args, result);
    Some(agent_title(agent, detail))
}

fn agent_session_start_title(value: &Value) -> Option<String> {
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

fn agent_session_start_name(value: &Value) -> Option<&str> {
    value
        .get("agent_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn agent_title_name(title: &str) -> &str {
    title
        .split_once('(')
        .map(|(name, _)| name)
        .unwrap_or(title)
        .trim()
}

fn agent_placeholder_title_matches(row: &TranscriptRow, agent_name: &str) -> bool {
    let title_name = agent_title_name(&row.title);
    title_name == agent_name || matches!(title_name, "agent" | "Agent")
}

fn agent_name_from<'a>(args: &'a Value, result: &'a Value) -> &'a str {
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

fn agent_detail_from<'a>(args: &'a Value, result: &'a Value) -> Option<&'a str> {
    result
        .get("agent_description")
        .and_then(Value::as_str)
        .or_else(|| args.get("description").and_then(Value::as_str))
        .or_else(|| result.get("task_name").and_then(Value::as_str))
        .or_else(|| args.get("task_name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn agent_title(agent: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) => format!("{agent}({})", single_line_preview(detail, 96)),
        None => agent.to_string(),
    }
}

fn matching_agent_edge<'a>(
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

fn agent_edge_title(edge: &AgentEdgeRecord, catalog: Option<&AgentCatalog>) -> Option<String> {
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

fn agent_edge_metadata_matches(edge: &AgentEdgeRecord, target: &str) -> bool {
    agent_edge_agent_string(edge, "id").is_some_and(|value| value == target)
        || agent_edge_agent_string(edge, "task_name").is_some_and(|value| value == target)
}

fn agent_edge_agent_name(edge: &AgentEdgeRecord) -> Option<&str> {
    agent_edge_agent_string(edge, "name")
}

fn agent_edge_agent_string<'a>(edge: &'a AgentEdgeRecord, key: &str) -> Option<&'a str> {
    edge.metadata
        .as_ref()?
        .get("agent")?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn single_line_preview(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&compact, max_chars)
}

fn agent_tool_output_text(value: &Value) -> Option<(String, Option<String>)> {
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
    let preview = if result.get("background").and_then(Value::as_bool) == Some(true)
        && status == "running"
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

fn pluralize(count: i64, singular: &str) -> String {
    if count == 1 {
        singular.to_string()
    } else {
        format!("{singular}s")
    }
}

fn agent_child_latest_tokens(summary: &Value) -> Option<u64> {
    summary
        .get("latest_total_tokens")
        .and_then(Value::as_u64)
        .or_else(|| summary.get("latest_usage").and_then(usage_total_tokens))
}

fn usage_total_tokens(usage: &Value) -> Option<u64> {
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

fn format_compact_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn status_label(status: &str, outcome: &str) -> String {
    match (status, outcome) {
        ("errored", _) | (_, "failed") => "Failed".to_string(),
        ("interrupted", _) | (_, "aborted") | (_, "interrupted") => "Interrupted".to_string(),
        ("shutdown", _) | (_, "shutdown") => "Closed".to_string(),
        ("running", _) => "Running".to_string(),
        _ => status.replace('_', " "),
    }
}

fn agent_target_from_tool_event(value: &Value) -> Option<String> {
    let result = value.get("result")?;
    result
        .get("session_id")
        .or_else(|| result.get("child_session_id"))
        .and_then(Value::as_str)
        .or_else(|| result.get("id").and_then(Value::as_str))
        .or_else(|| result.get("task_name").and_then(Value::as_str))
        .map(str::to_string)
}

fn bash_timeout_output_text(value: &Value) -> Option<String> {
    if value.get("tool_name").and_then(Value::as_str) != Some("bash") {
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

fn tool_event_interrupted(value: &Value) -> bool {
    value.get("outcome").and_then(Value::as_str) == Some("aborted")
        || value
            .get("result")
            .and_then(|result| result.get("error"))
            .and_then(Value::as_str)
            == Some("aborted")
}

fn model_label(provider: &str, model: &str) -> String {
    match (provider.is_empty(), model.is_empty()) {
        (false, false) => format!("{provider}/{model}"),
        (false, true) => provider.to_string(),
        (true, false) => model.to_string(),
        (true, true) => String::new(),
    }
}

fn model_meta_label(provider: &str, model: &str, metadata: Option<&Value>) -> String {
    let label = model_label(provider, model);
    match metadata_reasoning_effort(metadata) {
        Some(reasoning_effort) if !label.is_empty() => format!("{label} {reasoning_effort}"),
        Some(reasoning_effort) => reasoning_effort.to_string(),
        None => label,
    }
}

struct TurnMetaProjection<'a> {
    mode: &'a str,
    provider: &'a str,
    model: &'a str,
    started: Option<Instant>,
    usage: Option<&'a Value>,
    metadata: Option<&'a Value>,
    accounting: Option<&'a Value>,
    failures: usize,
    interrupted: bool,
    debug: bool,
}

fn turn_meta_text(meta: TurnMetaProjection<'_>) -> String {
    let mut parts = Vec::new();
    if !meta.provider.is_empty() || !meta.model.is_empty() {
        parts.push(model_meta_label(meta.provider, meta.model, meta.metadata));
    }
    if let Some(elapsed) = metadata_elapsed_duration(meta.metadata)
        .or_else(|| meta.started.map(|started| started.elapsed()))
    {
        parts.push(format_duration_compact(elapsed));
    }
    if meta.failures > 0 {
        let suffix = if meta.failures == 1 {
            "failure"
        } else {
            "failures"
        };
        parts.push(format!("{} {suffix}", meta.failures));
    }
    if meta.interrupted {
        parts.push("interrupted".to_string());
    }
    if meta.debug {
        if let Some(usage) = meta.usage {
            let mut usage_parts = Vec::new();
            for (key, label) in [
                ("input_tokens", "input"),
                ("output_tokens", "output"),
                ("reasoning_tokens", "reasoning"),
                ("cached_tokens", "cached"),
            ] {
                if let Some(value) = usage.get(key).and_then(Value::as_u64) {
                    usage_parts.push(format!("{value} {label}"));
                }
            }
            if !usage_parts.is_empty() {
                parts.push(format!("usage {}", usage_parts.join(" ")));
            }
        }
        if let Some(accounting) = meta.accounting.and_then(Value::as_object) {
            let mut pricing = Vec::new();
            if let Some(source) = accounting.get("pricing_source").and_then(Value::as_str) {
                pricing.push(format!("source {source}"));
            }
            if let Some(tier) = accounting.get("pricing_tier").and_then(Value::as_str) {
                pricing.push(format!("tier {tier}"));
            }
            if !pricing.is_empty() {
                parts.push(format!("pricing {}", pricing.join(" ")));
            }
        }
        if let Some(metadata) = meta.metadata.and_then(Value::as_object)
            && !metadata.is_empty()
        {
            let summary = metadata
                .iter()
                .filter(|(key, _)| !matches!(key.as_str(), "elapsed_ms" | "reasoning_effort"))
                .take(5)
                .map(|(key, value)| format!("{} {}", metadata_label(key), compact_value(value)))
                .collect::<Vec<_>>()
                .join(" ");
            if !summary.is_empty() {
                parts.push(format!("metadata {summary}"));
            }
        }
    }
    if !meta.mode.is_empty() && meta.mode != "default" {
        parts.push(meta.mode.to_string());
    }
    parts.join("  ")
}

fn usage_context_tokens(usage: &Value) -> Option<u64> {
    usage.get("input_tokens").and_then(Value::as_u64)
}

fn format_nanodollars(value: i64) -> String {
    format!("${:.6}", value as f64 / 1_000_000_000.0)
}

#[derive(Debug, Clone)]
struct StreamingToolCall {
    id: Option<String>,
    position_key: String,
    tool_name: String,
    args: Value,
}

fn streaming_tool_calls_from_event(value: &Value) -> Vec<StreamingToolCall> {
    if value.get("type").and_then(Value::as_str) == Some("tool_call_pending") {
        return streaming_tool_call_from_pending_event(value)
            .into_iter()
            .collect();
    }
    let Some(message) = value.get("message") else {
        return Vec::new();
    };
    let Some(content) = message.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(streaming_tool_call_from_block)
        .collect()
}

fn streaming_tool_call_from_pending_event(value: &Value) -> Option<StreamingToolCall> {
    let tool_name = value
        .get("tool_name")
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if tool_name.is_empty() {
        return None;
    }
    let content_index = value
        .get("content_index")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let call_index = value.get("call_index").and_then(Value::as_u64).unwrap_or(0);
    let id = value
        .get("tool_call_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let args = value
        .get("arguments")
        .filter(|value| !value.is_null())
        .cloned()
        .or_else(|| {
            value
                .get("arguments_json")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str(raw).ok())
        })
        .unwrap_or(Value::Null);
    Some(StreamingToolCall {
        id,
        position_key: tool_position_key(content_index, call_index),
        tool_name,
        args,
    })
}

fn streaming_tool_call_from_block(block: &Value) -> Option<StreamingToolCall> {
    if block.get("type").and_then(Value::as_str) != Some("tool_call") {
        return None;
    }
    let tool_name = block.get("name").and_then(Value::as_str)?.to_string();
    let content_index = block
        .get("content_index")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let call_index = block.get("call_index").and_then(Value::as_u64).unwrap_or(0);
    let id = block
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let args = block
        .get("arguments")
        .filter(|value| !value.is_null())
        .cloned()
        .or_else(|| {
            block
                .get("arguments_json")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str(raw).ok())
        })
        .unwrap_or(Value::Null);
    Some(StreamingToolCall {
        id,
        position_key: tool_position_key(content_index, call_index),
        tool_name,
        args,
    })
}

fn tool_id_key(tool_call_id: &str) -> String {
    format!("id:{tool_call_id}")
}

fn tool_intent_key(tool: &str) -> String {
    format!("intent:{tool}")
}

fn tool_position_key(content_index: u64, call_index: u64) -> String {
    format!("pos:{content_index}:{call_index}")
}

fn scoped_tool_position_key(message_scope: u64, position_key: &str) -> String {
    format!("msg:{message_scope}:{position_key}")
}

fn assistant_message_stream_event_type(value: &Value) -> Option<&str> {
    let event_type = value.get("type").and_then(Value::as_str)?;
    if event_type == "tool_call_pending" {
        return Some(event_type);
    }
    if !matches!(event_type, "message_update" | "message_end") {
        return None;
    }
    let message = value.get("message")?;
    (message.get("role").and_then(Value::as_str) == Some("assistant")).then_some(event_type)
}

fn visible_tool_intent_from_text(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    let tail = lower
        .char_indices()
        .rev()
        .nth(240)
        .map(|(index, _)| &lower[index..])
        .unwrap_or(lower.as_str());
    if [
        "let me write",
        "let me now write",
        "let me write the",
        "i'll write",
        "i will write",
        "let me create",
        "let me now create",
        "i'll create",
        "i will create",
        "write the report",
        "write the complete report",
        "write the full report",
        "write it out",
        "create the report",
    ]
    .iter()
    .any(|needle| tail.contains(needle))
    {
        return Some("write");
    }
    if [
        "let me run",
        "let me now run",
        "i'll run",
        "i will run",
        "let me execute",
        "let me now execute",
        "i'll execute",
        "i will execute",
        "run a command",
        "run the command",
        "execute a command",
        "execute the command",
    ]
    .iter()
    .any(|needle| tail.contains(needle))
    {
        return Some("bash");
    }
    if [
        "let me read",
        "let me now read",
        "i'll read",
        "i will read",
        "let me inspect",
        "let me now inspect",
        "i'll inspect",
        "i will inspect",
        "let me open",
        "i'll open",
    ]
    .iter()
    .any(|needle| tail.contains(needle))
    {
        return Some("read");
    }
    if [
        "let me search",
        "let me now search",
        "i'll search",
        "i will search",
    ]
    .iter()
    .any(|needle| tail.contains(needle))
    {
        return Some("search");
    }
    None
}

fn format_duration_compact(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    }
}

fn metadata_label(key: &str) -> &str {
    match key {
        "provider_response_id" => "response",
        "system_fingerprint" => "fingerprint",
        "model" => "model",
        other => other,
    }
}

fn format_count(value: u64) -> String {
    let text = value.to_string();
    let mut out = String::new();
    for (index, ch) in text.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn compact_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}
