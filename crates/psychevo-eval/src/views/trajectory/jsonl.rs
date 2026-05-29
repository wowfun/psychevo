#[allow(unused_imports)]
use super::*;

pub(crate) fn derive_jsonl_atif_step(step_id: u64, event: &TrajectoryEvent) -> Option<AtifStep> {
    let raw = event.data.get("raw_event").unwrap_or(&event.data);
    let event_type = raw
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or(&event.kind)
        .to_ascii_lowercase();
    if matches!(
        event_type.as_str(),
        "system" | "system_message" | "system_prompt"
    ) {
        return Some(AtifStep {
            step_id,
            source: "system".to_string(),
            message: Value::String(event_text(raw)),
            reasoning_content: None,
            tool_calls: Vec::new(),
            observation: None,
            metrics: None,
            extra: Some(json!({
                "source_event": event.kind,
                "timestamp_ms": event.timestamp_ms,
            })),
            llm_call_count: None,
        });
    }
    if matches!(event_type.as_str(), "user_message" | "user" | "input") {
        return Some(AtifStep {
            step_id,
            source: "user".to_string(),
            message: Value::String(event_text(raw)),
            reasoning_content: None,
            tool_calls: Vec::new(),
            observation: None,
            metrics: None,
            extra: Some(json!({
                "source_event": event.kind,
                "timestamp_ms": event.timestamp_ms,
            })),
            llm_call_count: None,
        });
    }
    if matches!(
        event_type.as_str(),
        "assistant_message" | "assistant" | "message" | "output"
    ) {
        return Some(AtifStep {
            step_id,
            source: "agent".to_string(),
            message: Value::String(event_text(raw)),
            reasoning_content: raw
                .get("reasoning")
                .or_else(|| raw.get("reasoning_content"))
                .and_then(Value::as_str)
                .map(str::to_string),
            tool_calls: Vec::new(),
            observation: None,
            metrics: atif_metrics_from_value(raw),
            extra: Some(json!({
                "source_event": event.kind,
                "timestamp_ms": event.timestamp_ms,
                "model_name": raw
                    .get("model")
                    .or_else(|| raw.get("model_name"))
                    .cloned()
                    .unwrap_or(Value::Null),
            })),
            llm_call_count: Some(1),
        });
    }
    if matches!(event_type.as_str(), "tool_call" | "tool_execution_start") {
        let tool_call = AtifToolCall {
            tool_call_id: raw
                .get("tool_call_id")
                .or_else(|| raw.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("tool-call")
                .to_string(),
            function_name: raw
                .get("function_name")
                .or_else(|| raw.get("name"))
                .or_else(|| raw.get("tool"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string(),
            arguments: raw
                .get("arguments")
                .cloned()
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({})),
            extra: Some(json!({ "source_event": event.kind })),
        };
        return Some(AtifStep {
            step_id,
            source: "agent".to_string(),
            message: Value::String(String::new()),
            reasoning_content: None,
            tool_calls: vec![tool_call],
            observation: None,
            metrics: None,
            extra: Some(json!({
                "source_event": event.kind,
                "timestamp_ms": event.timestamp_ms,
            })),
            llm_call_count: Some(0),
        });
    }
    if matches!(event_type.as_str(), "tool_result" | "tool_execution_end") {
        let (content, truncated) = bounded_atif_content(
            raw.get("result")
                .or_else(|| raw.get("output"))
                .unwrap_or(raw),
        );
        return Some(AtifStep {
            step_id,
            source: "agent".to_string(),
            message: Value::String(String::new()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            observation: Some(AtifObservation {
                results: vec![AtifObservationResult {
                    source_call_id: raw
                        .get("tool_call_id")
                        .or_else(|| raw.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    content: Some(content),
                    extra: Some(json!({
                        "source_event": event.kind,
                        "truncated": truncated,
                    })),
                }],
            }),
            metrics: None,
            extra: Some(json!({
                "source_event": event.kind,
                "timestamp_ms": event.timestamp_ms,
            })),
            llm_call_count: Some(0),
        });
    }
    None
}

pub(crate) fn acp_content_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    None
}

pub(crate) fn acp_system_text(update: &Value) -> Option<String> {
    let update_type = update
        .get("sessionUpdate")
        .or_else(|| update.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        update_type.as_str(),
        "system" | "system_prompt" | "system_message"
    ) {
        return None;
    }
    update
        .get("content")
        .and_then(acp_content_text)
        .or_else(|| update.get("message").and_then(acp_content_text))
        .or_else(|| update.get("system_prompt").and_then(acp_content_text))
        .filter(|value| !value.trim().is_empty())
}

pub(crate) fn event_text(value: &Value) -> String {
    for key in [
        "message",
        "text",
        "content",
        "prompt",
        "system_prompt",
        "output",
    ] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return text.to_string();
        }
    }
    String::new()
}

pub(crate) fn trajectory_session_id(cell: &CellRun, events: &[TrajectoryEvent]) -> Option<String> {
    events
        .iter()
        .find_map(|event| {
            event
                .data
                .get("session_id")
                .and_then(Value::as_str)
                .or_else(|| {
                    event
                        .data
                        .get("raw_event")
                        .and_then(|raw| raw.get("params"))
                        .and_then(|params| params.get("sessionId"))
                        .and_then(Value::as_str)
                })
                .map(str::to_string)
        })
        .or_else(|| Some(cell.case.case_id.clone()))
}

pub(crate) fn bounded_atif_content(value: &Value) -> (Value, bool) {
    match value {
        Value::String(text) => {
            let (preview, truncated) = truncate_chars_with_flag(text, ATIF_CONTENT_PREVIEW_CHARS);
            (Value::String(redact_preview_text(&preview)), truncated)
        }
        _ => {
            let raw = serde_json::to_string(value).unwrap_or_default();
            let (preview, truncated) = truncate_chars_with_flag(&raw, ATIF_CONTENT_PREVIEW_CHARS);
            if truncated {
                (Value::String(redact_preview_text(&preview)), true)
            } else {
                (value.clone(), false)
            }
        }
    }
}
