fn event_timestamp_ms(event: &AgentEvent) -> Option<i64> {
    match event {
        AgentEvent::GenerationStart { started_at_ms, .. } => Some(*started_at_ms),
        AgentEvent::ToolExecutionStart { started_at_ms, .. } => Some(*started_at_ms),
        AgentEvent::MessageStart { message }
        | AgentEvent::MessageUpdate { message }
        | AgentEvent::MessageEnd { message, .. } => message_timestamp_ms(message),
        _ => None,
    }
}

fn message_timestamp_ms(message: &Message) -> Option<i64> {
    match message {
        Message::User { timestamp_ms, .. }
        | Message::Assistant { timestamp_ms, .. }
        | Message::ToolResult { timestamp_ms, .. } => Some(*timestamp_ms),
    }
}

fn message_role_for_event(event: &AgentEvent) -> Option<&'static str> {
    match event {
        AgentEvent::MessageStart { message }
        | AgentEvent::MessageUpdate { message }
        | AgentEvent::MessageEnd { message, .. } => Some(message.role()),
        _ => None,
    }
}

fn message_trace(kind: &'static str, message: &Message) -> (&'static str, Value) {
    let payload = json!({
        "role": message.role(),
        "timestamp_ms": message_timestamp_ms(message),
        "summary": message_summary(message),
    });
    (kind, payload)
}

fn message_summary(message: &Message) -> Value {
    match message {
        Message::User { content, .. } => json!({
            "text_chars": content.iter().filter_map(|block| block.text_value()).map(|text| text.chars().count()).sum::<usize>(),
            "image_count": content.iter().filter(|block| block.text_value().is_none()).count(),
        }),
        Message::Assistant {
            content,
            outcome,
            finish_reason,
            ..
        } => {
            let text_chars = content
                .iter()
                .map(|block| match block {
                    AssistantBlock::Text { text } => text.chars().count(),
                    _ => 0,
                })
                .sum::<usize>();
            let reasoning_chars = content
                .iter()
                .map(|block| match block {
                    AssistantBlock::Reasoning { text, .. } => text.chars().count(),
                    _ => 0,
                })
                .sum::<usize>();
            let tool_call_count = content
                .iter()
                .filter(|block| matches!(block, AssistantBlock::ToolCall(_)))
                .count();
            json!({
                "text_chars": text_chars,
                "reasoning_chars": reasoning_chars,
                "tool_call_count": tool_call_count,
                "outcome": outcome.as_str(),
                "finish_reason": finish_reason,
            })
        }
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            ..
        } => json!({
            "tool_call_id": tool_call_id,
            "tool_name": tool_name,
            "content_chars": content.chars().count(),
            "is_error": is_error,
        }),
    }
}

fn insert_tool_correlation(
    correlation: &mut Map<String, Value>,
    tool_call_id: &str,
    tool_name: &str,
) {
    correlation.insert(
        "tool_call_id".to_string(),
        Value::String(tool_call_id.to_string()),
    );
    correlation.insert(
        "tool_name".to_string(),
        Value::String(tool_name.to_string()),
    );
}

fn bounded_redacted_value(value: &Value) -> Value {
    bounded_redacted_value_at(value, 0)
}

fn bounded_public_value(value: &Value) -> Value {
    bounded_public_value_at(value, 0)
}

fn bounded_public_value_at(value: &Value, depth: usize) -> Value {
    if depth >= TRACE_MAX_DEPTH {
        return json!({
            "truncated": true,
            "reason": "max_depth",
        });
    }
    match value {
        Value::Object(object) => {
            let mut next = Map::new();
            for (index, (key, value)) in object.iter().enumerate() {
                if index >= TRACE_MAX_OBJECT_FIELDS {
                    next.insert(
                        "_psychevo_trace_truncated".to_string(),
                        json!({
                            "omitted_fields": object.len().saturating_sub(index),
                        }),
                    );
                    break;
                }
                next.insert(key.clone(), bounded_public_value_at(value, depth + 1));
            }
            Value::Object(next)
        }
        Value::Array(values) => {
            let mut next = values
                .iter()
                .take(TRACE_MAX_ARRAY_ITEMS)
                .map(|value| bounded_public_value_at(value, depth + 1))
                .collect::<Vec<_>>();
            if values.len() > TRACE_MAX_ARRAY_ITEMS {
                next.push(json!({
                    "_psychevo_trace_truncated": {
                        "omitted_items": values.len() - TRACE_MAX_ARRAY_ITEMS,
                    },
                }));
            }
            Value::Array(next)
        }
        Value::String(value) if value.starts_with("data:image/") => {
            Value::String("[image data url redacted]".to_string())
        }
        Value::String(value) => bounded_string(value),
        other => other.clone(),
    }
}

fn bounded_redacted_value_at(value: &Value, depth: usize) -> Value {
    if depth >= TRACE_MAX_DEPTH {
        return json!({
            "truncated": true,
            "reason": "max_depth",
        });
    }
    match value {
        Value::Object(object) => {
            let mut next = Map::new();
            for (index, (key, value)) in object.iter().enumerate() {
                if index >= TRACE_MAX_OBJECT_FIELDS {
                    next.insert(
                        "_psychevo_trace_truncated".to_string(),
                        json!({
                            "omitted_fields": object.len().saturating_sub(index),
                        }),
                    );
                    break;
                }
                if is_sensitive_key(key) {
                    next.insert(key.clone(), Value::String("<redacted>".to_string()));
                } else {
                    next.insert(key.clone(), bounded_redacted_value_at(value, depth + 1));
                }
            }
            Value::Object(next)
        }
        Value::Array(values) => {
            let mut next = values
                .iter()
                .take(TRACE_MAX_ARRAY_ITEMS)
                .map(|value| bounded_redacted_value_at(value, depth + 1))
                .collect::<Vec<_>>();
            if values.len() > TRACE_MAX_ARRAY_ITEMS {
                next.push(json!({
                    "_psychevo_trace_truncated": {
                        "omitted_items": values.len() - TRACE_MAX_ARRAY_ITEMS,
                    },
                }));
            }
            Value::Array(next)
        }
        Value::String(value) if value.starts_with("data:image/") => {
            Value::String("[image data url redacted]".to_string())
        }
        Value::String(value) => bounded_string(value),
        other => other.clone(),
    }
}

fn bounded_string(value: &str) -> Value {
    let mut chars = value.chars();
    let preview = chars
        .by_ref()
        .take(TRACE_MAX_STRING_CHARS)
        .collect::<String>();
    if chars.next().is_none() {
        return Value::String(value.to_string());
    }
    json!({
        "truncated": true,
        "preview": preview,
        "original_chars_min": TRACE_MAX_STRING_CHARS + 1 + chars.count(),
    })
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("api_key")
        || key.contains("apikey")
        || key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("credential")
        || key.contains("authorization")
}

fn validate_session_trace_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty()
        || session_id == "."
        || session_id == ".."
        || !session_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(format!("invalid session trace id: {session_id}"));
    }
    Ok(())
}
