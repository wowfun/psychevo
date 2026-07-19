#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn model_meta_label(provider: &str, model: &str, metadata: Option<&Value>) -> String {
    let label = model_label(provider, model);
    match metadata_reasoning_effort(metadata) {
        Some(reasoning_effort) if !label.is_empty() => format!("{label} {reasoning_effort}"),
        Some(reasoning_effort) => reasoning_effort.to_string(),
        None => label,
    }
}

pub(crate) struct TurnMetaProjection<'a> {
    pub(crate) mode: &'a str,
    pub(crate) provider: &'a str,
    pub(crate) model: &'a str,
    pub(crate) started: Option<Instant>,
    pub(crate) usage: Option<&'a Value>,
    pub(crate) metadata: Option<&'a Value>,
    pub(crate) accounting: Option<&'a Value>,
    pub(crate) failures: usize,
    pub(crate) interrupted: bool,
    pub(crate) debug: bool,
}

pub(crate) fn turn_meta_text(meta: TurnMetaProjection<'_>) -> String {
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

pub(crate) fn usage_context_tokens(usage: &Value) -> Option<u64> {
    effective_usage_total(Some(usage)).tokens
}

pub(crate) fn format_nanodollars(value: i64) -> String {
    format!("${:.6}", value as f64 / 1_000_000_000.0)
}

#[derive(Debug, Clone)]
pub(crate) struct StreamingToolCall {
    pub(crate) id: Option<String>,
    pub(crate) position_key: String,
    pub(crate) tool_name: String,
    pub(crate) args: Value,
    pub(crate) display: Option<ToolDisplaySpec>,
}

pub(crate) fn streaming_tool_calls_from_event(value: &Value) -> Vec<StreamingToolCall> {
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

pub(crate) fn streaming_tool_call_from_pending_event(value: &Value) -> Option<StreamingToolCall> {
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
        display: value
            .get("display")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok()),
    })
}

pub(crate) fn streaming_tool_call_from_block(block: &Value) -> Option<StreamingToolCall> {
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
        display: None,
    })
}

pub(crate) fn tool_id_key(tool_call_id: &str) -> String {
    format!("id:{tool_call_id}")
}

pub(crate) fn tool_intent_key(tool: &str) -> String {
    format!("intent:{tool}")
}

pub(crate) fn tool_position_key(content_index: u64, call_index: u64) -> String {
    format!("pos:{content_index}:{call_index}")
}

pub(crate) fn scoped_tool_position_key(message_scope: u64, position_key: &str) -> String {
    format!("msg:{message_scope}:{position_key}")
}

pub(crate) fn assistant_message_stream_event_type(value: &Value) -> Option<&str> {
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

pub(crate) fn format_duration_compact(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    }
}

pub(crate) fn metadata_label(key: &str) -> &str {
    match key {
        "provider_response_id" => "response",
        "system_fingerprint" => "fingerprint",
        "model" => "model",
        other => other,
    }
}

pub(crate) fn format_count(value: u64) -> String {
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

pub(crate) fn compact_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}
