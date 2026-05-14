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
            format!("Ran {command}")
        }
        "write" | "edit" => format!("Updated {}", path_from(args, result).unwrap_or("files")),
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
            .map(|command| format!("Running {command}"))
            .unwrap_or_else(|| "Running command".to_string()),
        "write" | "edit" => path_from_args(args)
            .map(|path| format!("Updating {path}"))
            .unwrap_or_else(|| "Updating files".to_string()),
        other => format!("Using {other}"),
    }
}

fn tool_title_for_update(tool: &str, value: &Value, existing_title: &str) -> String {
    let title = tool_title(tool, value);
    if tool == "bash" && title == "Ran command" {
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
