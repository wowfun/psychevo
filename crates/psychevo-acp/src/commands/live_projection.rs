#[derive(Debug, Default)]
pub(crate) struct AcpTurnProjection {
    terminal_output: bool,
    terminal_commands: HashMap<String, String>,
    terminal_offsets: HashMap<String, usize>,
}

impl AcpTurnProjection {
    pub(crate) fn new(terminal_output: bool) -> Self {
        Self {
            terminal_output,
            ..Self::default()
        }
    }

    fn runtime_tool_update(&mut self, data: &Value) -> Option<SessionUpdate> {
        let update = runtime_event_session_update(data)?;
        if !self.terminal_output
            || data.get("tool_name").and_then(Value::as_str) != Some("exec_command")
        {
            return Some(update);
        }
        let SessionUpdate::ToolCallUpdate(mut update) = update else {
            return Some(update);
        };
        let call_id = update.tool_call_id.0.as_ref().to_string();
        if let Some(command) = data
            .get("args")
            .and_then(|args| args.get("cmd"))
            .and_then(Value::as_str)
        {
            self.terminal_commands
                .insert(call_id.clone(), command.to_string());
        }
        let Some(command) = self.terminal_commands.get(&call_id).cloned() else {
            return Some(SessionUpdate::ToolCallUpdate(update));
        };
        update = update.content(vec![ToolCallContent::from(format!("$ {command}"))]);
        let mut meta = update.meta.value().cloned().unwrap_or_else(Meta::new);
        let first_update = !self.terminal_offsets.contains_key(&call_id);
        if first_update {
            meta.insert(
                "terminal_info".to_string(),
                json!({
                    "terminal_id": call_id,
                    "command": command,
                }),
            );
        }

        let output = tool_event_output(data).unwrap_or_default();
        let offset = self.terminal_offsets.entry(call_id.clone()).or_insert(0);
        if *offset > output.len() || !output.is_char_boundary(*offset) {
            *offset = 0;
        }
        let mut delta = String::new();
        if first_update {
            delta.push_str("$ ");
            delta.push_str(&command);
            delta.push('\n');
        }
        if let Some(tail) = output.get(*offset..) {
            delta.push_str(tail);
        }
        *offset = output.len();
        if !delta.is_empty() {
            meta.insert(
                "terminal_output".to_string(),
                json!({
                    "terminal_id": call_id,
                    "data": delta,
                }),
            );
        }
        if data.get("type").and_then(Value::as_str) == Some("tool_execution_end") {
            meta.insert(
                "terminal_exit".to_string(),
                json!({
                    "terminal_id": call_id,
                    "exit_code": data
                        .get("result")
                        .and_then(|result| result.get("exit_code"))
                        .and_then(Value::as_i64),
                    "signal": null,
                }),
            );
        }
        update = update.meta(meta);
        Some(SessionUpdate::ToolCallUpdate(update))
    }
}

fn tool_event_output(data: &Value) -> Option<String> {
    let result = match data.get("type").and_then(Value::as_str) {
        Some("tool_execution_update") => data.get("partial_result"),
        Some("tool_execution_end") => data.get("result"),
        _ => None,
    }?;
    result
        .get("model_content")
        .and_then(Value::as_str)
        .or_else(|| result.get("output").and_then(Value::as_str))
        .or_else(|| result.get("raw_output").and_then(Value::as_str))
        .or_else(|| {
            result
                .get("raw_output")
                .and_then(|raw| raw.get("output"))
                .and_then(Value::as_str)
        })
        .or_else(|| result.get("error").and_then(Value::as_str))
        .map(ToString::to_string)
        .or_else(|| Some(compact_tool_result_text(result)))
}

fn record_completed_message_usage(
    accumulator: &mut AcpUsageAccumulator,
    message: Value,
    usage: Option<Value>,
    metadata: Option<Value>,
    accounting: Option<Value>,
) {
    let mut runtime_event = serde_json::Map::new();
    runtime_event.insert("type".to_string(), json!("message_end"));
    runtime_event.insert("message".to_string(), message);
    if let Some(value) = usage {
        runtime_event.insert("usage".to_string(), value);
    }
    if let Some(value) = metadata {
        runtime_event.insert("metadata".to_string(), value);
    }
    if let Some(value) = accounting {
        runtime_event.insert("accounting".to_string(), value);
    }
    accumulator.record_stream_event(&RunStreamEvent::value(Value::Object(runtime_event)));
}

pub(crate) fn send_turn_event_update(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    event: TurnEvent,
    usage: &Arc<Mutex<AcpUsageAccumulator>>,
    projection: &mut AcpTurnProjection,
) {
    match event {
        TurnEvent::ReasoningDelta { text } if !text.is_empty() => {
            send_session_update(
                cx,
                session_id.clone(),
                agent_thought_update(session_id, text),
            );
        }
        TurnEvent::Tool { data, .. } => {
            if let Ok(mut usage) = usage.lock() {
                usage.record_stream_event(&RunStreamEvent::value(data.clone()));
            }
            if let Some(update) = projection.runtime_tool_update(&data) {
                send_session_update(cx, session_id.clone(), update);
            }
        }
        TurnEvent::Warning { data } => {
            let message = data
                .get("message")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .unwrap_or_else(|| data.to_string());
            send_session_update(
                cx,
                session_id.clone(),
                agent_message_update(session_id, format!("warning: {message}")),
            );
        }
        TurnEvent::Message {
            stage: psychevo::ItemStage::Completed,
            message,
            usage: message_usage,
            metadata,
            accounting,
        } => {
            if let Ok(mut accumulator) = usage.lock() {
                record_completed_message_usage(
                    &mut accumulator,
                    message,
                    message_usage,
                    metadata,
                    accounting,
                );
            }
        }
        TurnEvent::Message { .. } => {}
        TurnEvent::Accepted { .. }
        | TurnEvent::Started { .. }
        | TurnEvent::ReasoningDelta { .. }
        | TurnEvent::ReasoningCompleted { .. }
        | TurnEvent::InteractionRequested { .. }
        | TurnEvent::InteractionResolved { .. }
        | TurnEvent::Completed { .. }
        | TurnEvent::Failed { .. }
        | TurnEvent::ResyncRequired { .. } => {}
    }
}

#[cfg(test)]
mod live_projection_tests {
    use super::*;

    #[test]
    fn framework_tool_event_reuses_the_acp_runtime_projection() {
        let data = json!({
            "type": "tool_execution_start",
            "tool_call_id": "call-1",
            "tool_name": "edit",
            "args": {"path": "src/lib.rs"},
            "started_at_ms": 1_234,
        });
        let update = runtime_event_session_update(&data).expect("tool projection");
        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("expected ToolCallUpdate");
        };
        assert_eq!(update.tool_call_id.0.as_ref(), "call-1");
        assert_eq!(update.status.value(), Some(&ToolCallStatus::InProgress));
    }

    #[test]
    fn framework_tool_update_projects_incremental_output() {
        let data = json!({
            "type": "tool_execution_update",
            "tool_call_id": "call-1",
            "tool_name": "exec_command",
            "partial_result": {"output": "first line\n"},
        });
        let update = runtime_event_session_update(&data).expect("tool projection");
        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("expected ToolCallUpdate");
        };
        assert_eq!(update.tool_call_id.0.as_ref(), "call-1");
        assert_eq!(update.status.value(), Some(&ToolCallStatus::InProgress));
        assert_eq!(update.raw_output.value(), Some(&json!({"output": "first line\n"})));
    }

    #[test]
    fn negotiated_terminal_output_emits_only_new_output_and_exit_metadata() {
        let mut projection = AcpTurnProjection::new(true);
        let start = projection
            .runtime_tool_update(&json!({
                "type": "tool_execution_start",
                "tool_call_id": "call-1",
                "tool_name": "exec_command",
                "args": {"cmd": "printf hello"},
            }))
            .expect("start");
        let update = projection
            .runtime_tool_update(&json!({
                "type": "tool_execution_update",
                "tool_call_id": "call-1",
                "tool_name": "exec_command",
                "partial_result": {"output": "hello"},
            }))
            .expect("update");
        let end = projection
            .runtime_tool_update(&json!({
                "type": "tool_execution_end",
                "tool_call_id": "call-1",
                "tool_name": "exec_command",
                "result": {"output": "hello!", "exit_code": 0},
                "outcome": "normal",
            }))
            .expect("end");

        let SessionUpdate::ToolCallUpdate(start) = start else {
            panic!("expected start");
        };
        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("expected update");
        };
        let SessionUpdate::ToolCallUpdate(end) = end else {
            panic!("expected end");
        };
        let start_meta = start.meta.value().expect("start meta");
        assert_eq!(
            start_meta["terminal_info"]["command"],
            json!("printf hello")
        );
        assert_eq!(
            start_meta["terminal_output"]["data"],
            json!("$ printf hello\n")
        );
        assert_eq!(
            update.meta.value().expect("update meta")["terminal_output"]["data"],
            json!("hello")
        );
        let end_meta = end.meta.value().expect("end meta");
        assert_eq!(end_meta["terminal_output"]["data"], json!("!"));
        assert_eq!(end_meta["terminal_exit"]["exit_code"], json!(0));
    }

    #[test]
    fn completed_message_records_top_level_accounting() {
        let mut usage = AcpUsageAccumulator::default();
        record_completed_message_usage(
            &mut usage,
            json!({"role": "assistant"}),
            Some(json!({"input_tokens": 4, "output_tokens": 3})),
            Some(json!({"provider": "fake"})),
            Some(json!({
                "billable_input_tokens": 4,
                "billable_output_tokens": 3,
                "reported_total_tokens": 7,
            })),
        );

        assert_eq!(usage.context_tokens_for_usage_update(), Some(7));
        assert_eq!(usage.to_usage().expect("usage").total_tokens, 7);
    }
}
