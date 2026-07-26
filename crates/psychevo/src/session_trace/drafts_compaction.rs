fn run_start_trace_draft(payload: &Value) -> SessionTraceDraft {
    SessionTraceDraft {
        kind: "run_start".to_string(),
        timestamp_ms: now_ms(),
        monotonic_offset_ms: 0,
        turn_index: None,
        correlation: json!({}),
        payload: compact_run_start_payload(payload),
    }
}

fn trace_drafts_from_agent_event(
    event: &AgentEvent,
    _accounting: Option<&MessageAccounting>,
    monotonic_offset_ms: u64,
    turn_index: Option<usize>,
    stats: &mut SessionTraceStats,
) -> Vec<SessionTraceDraft> {
    let event_kind = trace_event_kind(event);
    if should_coalesce_event(event) {
        stats.coalesce_kind(event_kind);
        stats.observe_coalesced_event(event);
        if let Some(turn_index) = turn_index {
            let turn = stats.turn_mut(turn_index);
            turn.coalesced_events = turn.coalesced_events.saturating_add(1);
        }
        return Vec::new();
    }
    let timestamp_ms = event_timestamp_ms(event).unwrap_or_else(now_ms);
    let mut correlation = Map::new();
    let (kind, payload) = match event {
        AgentEvent::AgentStart => ("agent_start", json!({})),
        AgentEvent::AgentEnd {
            outcome,
            messages,
            terminal_reason,
        } => (
            "agent_end",
            json!({
                "outcome": outcome.as_str(),
                "message_count": messages.len(),
                "terminal_reason": terminal_reason,
            }),
        ),
        AgentEvent::TurnStart { .. } => ("turn_start", json!({})),
        AgentEvent::TurnEnd { outcome, .. } => (
            "turn_end",
            json!({
                "outcome": outcome.as_str(),
            }),
        ),
        AgentEvent::GenerationStart {
            generation_id,
            message_count,
            tool_count,
            started_at_ms,
            ..
        } => {
            correlation.insert("generation_id".to_string(), json!(generation_id));
            (
                "generation_start",
                json!({
                    "message_count": message_count,
                    "tool_count": tool_count,
                    "started_at_ms": started_at_ms,
                }),
            )
        }
        AgentEvent::GenerationEnd {
            generation_id,
            provider,
            model,
            outcome,
            elapsed_ms,
            usage,
            metadata,
            error,
        } => {
            correlation.insert("generation_id".to_string(), json!(generation_id));
            let mut payload = json!({
                "provider": provider,
                "model": model,
                "outcome": outcome.as_str(),
                "elapsed_ms": elapsed_ms,
            });
            if let Some(object) = payload.as_object_mut() {
                if let Some(usage) = usage {
                    object.insert("usage".to_string(), bounded_public_value(usage));
                }
                if let Some(metadata) = metadata {
                    let summary = metadata_summary(metadata);
                    if !is_empty_object(&summary) {
                        object.insert("metadata".to_string(), summary);
                    }
                }
                if let Some(error) = error {
                    object.insert("error".to_string(), bounded_string(error));
                }
            }
            ("generation_end", payload)
        }
        AgentEvent::MessageStart { .. }
        | AgentEvent::MessageUpdate { .. }
        | AgentEvent::ReasoningDelta { .. }
        | AgentEvent::ToolCallPending { .. }
        | AgentEvent::ToolExecutionUpdate { .. } => unreachable!("coalesced above"),
        AgentEvent::MessageEnd { message, .. } => message_trace("message_end", message),
        AgentEvent::ReasoningEnd { text } => (
            "reasoning_end",
            json!({
                "chars": text.chars().count(),
            }),
        ),
        AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
            started_at_ms,
            ..
        } => {
            insert_tool_correlation(&mut correlation, tool_call_id, tool_name);
            let payload = json!({
                "args_summary": value_summary(args),
                "started_at_ms": started_at_ms,
            });
            ("tool_execution_start", payload)
        }
        AgentEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            result,
            outcome,
            elapsed_ms,
            ..
        } => {
            insert_tool_correlation(&mut correlation, tool_call_id, tool_name);
            let payload = json!({
                "result_summary": value_summary(result),
                "outcome": outcome.as_str(),
                "elapsed_ms": elapsed_ms,
            });
            ("tool_execution_end", payload)
        }
    };

    if let Some(role) = message_role_for_event(event) {
        correlation.insert("message_role".to_string(), Value::String(role.to_string()));
    }
    let draft = SessionTraceDraft {
        kind: kind.to_string(),
        timestamp_ms,
        monotonic_offset_ms,
        turn_index,
        correlation: Value::Object(correlation),
        payload,
    };
    if matches!(event, AgentEvent::AgentEnd { .. }) {
        vec![
            draft,
            SessionTraceDraft {
                kind: "run_summary".to_string(),
                timestamp_ms: now_ms(),
                monotonic_offset_ms,
                turn_index: None,
                correlation: json!({}),
                payload: stats.summary_payload(),
            },
        ]
    } else {
        vec![draft]
    }
}

fn trace_event_kind(event: &AgentEvent) -> &'static str {
    match event {
        AgentEvent::AgentStart => "agent_start",
        AgentEvent::AgentEnd { .. } => "agent_end",
        AgentEvent::TurnStart { .. } => "turn_start",
        AgentEvent::TurnEnd { .. } => "turn_end",
        AgentEvent::GenerationStart { .. } => "generation_start",
        AgentEvent::GenerationEnd { .. } => "generation_end",
        AgentEvent::MessageStart { .. } => "message_start",
        AgentEvent::MessageUpdate { .. } => "message_update",
        AgentEvent::MessageEnd { .. } => "message_end",
        AgentEvent::ReasoningDelta { .. } => "reasoning_delta",
        AgentEvent::ReasoningEnd { .. } => "reasoning_end",
        AgentEvent::ToolCallPending { .. } => "tool_call_pending",
        AgentEvent::ToolExecutionStart { .. } => "tool_execution_start",
        AgentEvent::ToolExecutionUpdate { .. } => "tool_execution_update",
        AgentEvent::ToolExecutionEnd { .. } => "tool_execution_end",
    }
}

fn should_coalesce_event(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::MessageStart { .. }
            | AgentEvent::MessageUpdate { .. }
            | AgentEvent::ReasoningDelta { .. }
            | AgentEvent::ToolCallPending { .. }
            | AgentEvent::ToolExecutionUpdate { .. }
    )
}

fn compact_run_start_payload(payload: &Value) -> Value {
    let Some(object) = payload.as_object() else {
        return json!({});
    };
    let mut next = Map::new();
    for key in [
        "type",
        "source",
        "provider",
        "model",
        "mode",
        "permission_mode",
        "approval_mode",
        "reasoning_effort",
        "context_limit",
        "project_context",
    ] {
        if let Some(value) = object.get(key)
            && !value.is_null()
        {
            next.insert(key.to_string(), bounded_redacted_value(value));
        }
    }
    if let Some(value) = object.get("selected_agent")
        && !value.is_null()
    {
        next.insert("selected_agent".to_string(), selected_agent_summary(value));
    }
    if let Some(value) = object.get("selected_skills")
        && !value.is_null()
    {
        next.insert(
            "selected_skills".to_string(),
            selected_skills_summary(value),
        );
    }
    Value::Object(next)
}

fn selected_agent_summary(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return bounded_redacted_value(value);
    };
    let mut next = Map::new();
    for key in ["name", "source", "generated"] {
        if let Some(value) = object.get(key) {
            next.insert(key.to_string(), bounded_redacted_value(value));
        }
    }
    Value::Object(next)
}

fn selected_skills_summary(value: &Value) -> Value {
    let Some(values) = value.as_array() else {
        return bounded_redacted_value(value);
    };
    Value::Array(
        values
            .iter()
            .take(TRACE_SELECTED_SKILLS_MAX_ITEMS)
            .map(|value| {
                if let Some(object) = value.as_object() {
                    let mut next = Map::new();
                    if let Some(name) = object.get("name") {
                        next.insert("name".to_string(), bounded_redacted_value(name));
                    }
                    Value::Object(next)
                } else {
                    bounded_redacted_value(value)
                }
            })
            .collect(),
    )
}

fn metadata_summary(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return value_summary(value);
    };
    let mut next = Map::new();
    for key in [
        "elapsed_ms",
        "elapsed_ms_source",
        "reasoning_effort",
        "selected_agent",
    ] {
        if let Some(value) = object.get(key) {
            next.insert(key.to_string(), bounded_redacted_value(value));
        }
    }
    Value::Object(next)
}

fn value_summary(value: &Value) -> Value {
    match value {
        Value::Null => json!({ "type": "null" }),
        Value::Bool(_) => json!({ "type": "bool" }),
        Value::Number(_) => json!({ "type": "number" }),
        Value::String(text) if text.starts_with("data:image/") => {
            json!({ "type": "string", "chars": text.chars().count(), "redacted": "image_data_url" })
        }
        Value::String(text) => json!({ "type": "string", "chars": text.chars().count() }),
        Value::Array(values) => json!({
            "type": "array",
            "items": values.len(),
        }),
        Value::Object(object) => {
            let title = title_fields(object);
            json!({
                "type": "object",
                "field_count": object.len(),
                "title": title,
            })
        }
    }
}

fn title_fields(object: &Map<String, Value>) -> Value {
    let mut next = Map::new();
    for key in [
        "cmd",
        "command",
        "path",
        "url",
        "final_url",
        "name",
        "query",
        "status",
        "exit_code",
        "content_type",
        "bytes_written",
        "files_modified",
        "truncated",
    ] {
        if let Some(value) = object.get(key) {
            next.insert(key.to_string(), title_field_value(key, value));
        }
    }
    Value::Object(next)
}

fn title_field_value(key: &str, value: &Value) -> Value {
    if is_sensitive_key(key) {
        return Value::String("<redacted>".to_string());
    }
    match value {
        Value::String(text) => {
            let char_count = text.chars().count();
            if char_count <= TRACE_TITLE_STRING_CHARS {
                Value::String(text.to_string())
            } else {
                let prefix = text
                    .chars()
                    .take(TRACE_TITLE_STRING_CHARS)
                    .collect::<String>();
                json!({
                    "truncated": true,
                    "chars": char_count,
                    "prefix": prefix,
                })
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::Array(values) => json!({ "type": "array", "items": values.len() }),
        Value::Object(object) => json!({ "type": "object", "field_count": object.len() }),
    }
}

fn increment_counter(counters: &mut BTreeMap<String, u64>, key: &str) {
    let next = counters.get(key).copied().unwrap_or(0).saturating_add(1);
    counters.insert(key.to_string(), next);
}

fn is_empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(Map::is_empty)
}

fn limited_summary_items(items: impl Iterator<Item = Value>, limit: usize) -> (Value, u64) {
    let mut values = Vec::new();
    let mut omitted = 0u64;
    for item in items {
        if values.len() < limit {
            values.push(item);
        } else {
            omitted = omitted.saturating_add(1);
        }
    }
    (Value::Array(values), omitted)
}
