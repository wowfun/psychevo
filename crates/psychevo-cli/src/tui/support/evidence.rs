fn evidence_kind(tool: &str) -> TranscriptKind {
    match tool {
        "read" | "list" | "search" => TranscriptKind::Explored,
        "bash" => TranscriptKind::Ran,
        "write" | "edit" => TranscriptKind::Changed,
        _ => TranscriptKind::Status,
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
                .and_then(first_non_empty_line)
                .unwrap_or("command");
            format!("Ran {command}")
        }
        "write" | "edit" => format!("Changed {}", path_from(args, result).unwrap_or("files")),
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
            .and_then(first_non_empty_line)
            .map(|command| format!("Running {command}"))
            .unwrap_or_else(|| "Running command".to_string()),
        "write" | "edit" => path_from_args(args)
            .map(|path| format!("Changing {path}"))
            .unwrap_or_else(|| "Changing files".to_string()),
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
    if matches!(tool, "write" | "edit") && title == "Changed files"
        && let Some(path) = existing_title.strip_prefix("Changing ")
        && path != "files"
    {
        return format!("Changed {path}");
    }
    title
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
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
    let result = value.get("result").unwrap_or(&Value::Null);
    let full = result
        .get("content")
        .and_then(Value::as_str)
        .or_else(|| result.get("output").and_then(Value::as_str))
        .or_else(|| result.get("diff").and_then(Value::as_str))
        .or_else(|| result.get("error").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| format_tool_summary(value));
    collapse_lines(&full, 20)
}

fn collapse_lines(text: &str, max_lines: usize) -> (String, Option<String>) {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.len() <= max_lines {
        return (text.to_string(), None);
    }
    let collapsed = lines
        .iter()
        .take(max_lines)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    (
        format!("{collapsed}\n... {} more lines", lines.len() - max_lines),
        Some(text.to_string()),
    )
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
    failures: usize,
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

fn usage_total_tokens(usage: &Value) -> Option<u64> {
    usage.get("total_tokens").and_then(Value::as_u64)
}

#[derive(Debug, Clone)]
struct StreamingToolCall {
    id: Option<String>,
    position_key: String,
    tool_name: String,
    args: Value,
}

fn streaming_tool_calls_from_event(value: &Value) -> Vec<StreamingToolCall> {
    if assistant_message_stream_event_type(value).is_none() {
        return Vec::new();
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

fn tool_position_key(content_index: u64, call_index: u64) -> String {
    format!("pos:{content_index}:{call_index}")
}

fn scoped_tool_position_key(message_scope: u64, position_key: &str) -> String {
    format!("msg:{message_scope}:{position_key}")
}

fn assistant_message_stream_event_type(value: &Value) -> Option<&str> {
    let event_type = value.get("type").and_then(Value::as_str)?;
    if !matches!(event_type, "message_update" | "message_end") {
        return None;
    }
    let message = value.get("message")?;
    (message.get("role").and_then(Value::as_str) == Some("assistant")).then_some(event_type)
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
