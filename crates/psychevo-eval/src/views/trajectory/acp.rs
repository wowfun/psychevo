#[allow(unused_imports)]
use super::*;

pub(crate) fn derive_acp_atif_steps(
    mut next_step_id: u64,
    events: &[TrajectoryEvent],
) -> Vec<AtifStep> {
    let mut steps = Vec::new();
    let mut current = None;
    for event in events {
        let Some(update) = event
            .data
            .get("raw_event")
            .and_then(|raw| raw.get("params"))
            .and_then(|params| params.get("update"))
        else {
            continue;
        };
        if let Some(text) = acp_system_text(update) {
            push_acp_grouped_step(&mut steps, &mut current);
            steps.push(AtifStep {
                step_id: next_step_id,
                source: "system".to_string(),
                message: Value::String(text),
                reasoning_content: None,
                tool_calls: Vec::new(),
                observation: None,
                metrics: None,
                extra: Some(json!({
                    "source": "acp",
                    "source_event": event.kind,
                    "timestamp_ms": event.timestamp_ms,
                })),
                llm_call_count: None,
            });
            next_step_id += 1;
            continue;
        }
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("agent_thought_chunk") => {
                if let Some(text) = acp_content_text(update.get("content").unwrap_or(&Value::Null))
                {
                    let step = acp_current_step(
                        &mut steps,
                        &mut current,
                        &mut next_step_id,
                        event.timestamp_ms,
                    );
                    step.reasoning.push_str(&text);
                    step.llm_call_count = Some(1);
                }
            }
            Some("agent_message_chunk") => {
                if let Some(text) = acp_content_text(update.get("content").unwrap_or(&Value::Null))
                {
                    let step = acp_current_step(
                        &mut steps,
                        &mut current,
                        &mut next_step_id,
                        event.timestamp_ms,
                    );
                    step.message.push_str(&text);
                    step.llm_call_count = Some(1);
                }
            }
            Some("tool_call") => {
                let tool_call_id = update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or("tool-call")
                    .to_string();
                let function_name = update
                    .get("kind")
                    .and_then(Value::as_str)
                    .or_else(|| update.get("title").and_then(Value::as_str))
                    .unwrap_or("tool")
                    .to_string();
                let arguments = update
                    .get("rawInput")
                    .cloned()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({}));
                let step = acp_current_step_without_boundary(
                    &mut current,
                    &mut next_step_id,
                    event.timestamp_ms,
                );
                step.upsert_tool_call(AtifToolCall {
                    tool_call_id,
                    function_name,
                    arguments,
                    extra: Some(json!({
                        "status": update.get("status").cloned().unwrap_or(Value::Null),
                        "title": update.get("title").cloned().unwrap_or(Value::Null),
                        "timestamp_ms": event.timestamp_ms,
                        "execution_start_timestamp_ms": event.timestamp_ms,
                    })),
                });
            }
            Some("tool_call_update") => {
                let tool_call_id = update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if acp_tool_update_is_pending(update) {
                    let tool_call_id = tool_call_id.unwrap_or_else(|| "tool-call".to_string());
                    let function_name = update
                        .get("kind")
                        .and_then(Value::as_str)
                        .or_else(|| update.get("title").and_then(Value::as_str))
                        .unwrap_or("tool")
                        .to_string();
                    let arguments = update
                        .get("rawInput")
                        .cloned()
                        .filter(|value| !value.is_null())
                        .unwrap_or_else(|| json!({}));
                    let step = acp_current_step_without_boundary(
                        &mut current,
                        &mut next_step_id,
                        event.timestamp_ms,
                    );
                    step.upsert_tool_call(AtifToolCall {
                        tool_call_id,
                        function_name,
                        arguments,
                        extra: Some(json!({
                            "status": update.get("status").cloned().unwrap_or(Value::Null),
                            "title": update.get("title").cloned().unwrap_or(Value::Null),
                            "timestamp_ms": event.timestamp_ms,
                            "pending_timestamp_ms": event.timestamp_ms,
                        })),
                    });
                    continue;
                }
                let (content, truncated) = bounded_atif_content(
                    update
                        .get("rawOutput")
                        .or_else(|| update.get("content"))
                        .unwrap_or(&Value::Null),
                );
                let step = acp_current_step_without_boundary(
                    &mut current,
                    &mut next_step_id,
                    event.timestamp_ms,
                );
                step.observation_results.push(AtifObservationResult {
                    source_call_id: tool_call_id,
                    content: Some(content),
                    extra: Some(json!({
                        "status": update.get("status").cloned().unwrap_or(Value::Null),
                        "title": update.get("title").cloned().unwrap_or(Value::Null),
                        "timestamp_ms": event.timestamp_ms,
                        "truncated": truncated,
                    })),
                });
            }
            _ => {}
        }
    }
    push_acp_grouped_step(&mut steps, &mut current);
    steps
}

#[derive(Debug, Clone)]
pub(crate) struct AcpGroupedStep {
    step_id: u64,
    timestamp_ms: Option<u128>,
    end_timestamp_ms: Option<u128>,
    message: String,
    reasoning: String,
    tool_calls: Vec<AtifToolCall>,
    observation_results: Vec<AtifObservationResult>,
    llm_call_count: Option<u64>,
}

impl AcpGroupedStep {
    fn new(step_id: u64, timestamp_ms: Option<u128>) -> Self {
        Self {
            step_id,
            timestamp_ms,
            end_timestamp_ms: timestamp_ms,
            message: String::new(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
            observation_results: Vec::new(),
            llm_call_count: None,
        }
    }

    fn has_observation(&self) -> bool {
        !self.observation_results.is_empty()
    }

    fn record_timestamp(&mut self, timestamp_ms: Option<u128>) {
        let Some(timestamp_ms) = timestamp_ms else {
            return;
        };
        if self.timestamp_ms.is_none() {
            self.timestamp_ms = Some(timestamp_ms);
        }
        self.end_timestamp_ms = Some(
            self.end_timestamp_ms
                .map(|current| current.max(timestamp_ms))
                .unwrap_or(timestamp_ms),
        );
    }

    fn upsert_tool_call(&mut self, tool_call: AtifToolCall) {
        if let Some(existing) = self
            .tool_calls
            .iter_mut()
            .find(|existing| existing.tool_call_id == tool_call.tool_call_id)
        {
            existing.function_name = tool_call.function_name;
            existing.arguments = tool_call.arguments;
            existing.extra = merge_tool_call_extra(existing.extra.take(), tool_call.extra);
            return;
        }
        self.tool_calls.push(tool_call);
    }

    fn into_atif_step(self) -> AtifStep {
        AtifStep {
            step_id: self.step_id,
            source: "agent".to_string(),
            message: Value::String(self.message),
            reasoning_content: (!self.reasoning.trim().is_empty()).then_some(self.reasoning),
            tool_calls: self.tool_calls,
            observation: (!self.observation_results.is_empty()).then_some(AtifObservation {
                results: self.observation_results,
            }),
            metrics: None,
            extra: Some(json!({
                "source": "acp",
                "timestamp_ms": self.timestamp_ms,
                "end_timestamp_ms": self.end_timestamp_ms,
            })),
            llm_call_count: self.llm_call_count.or(Some(0)),
        }
    }
}

pub(crate) fn acp_current_step<'a>(
    steps: &mut Vec<AtifStep>,
    current: &'a mut Option<AcpGroupedStep>,
    next_step_id: &mut u64,
    timestamp_ms: Option<u128>,
) -> &'a mut AcpGroupedStep {
    if current
        .as_ref()
        .is_some_and(AcpGroupedStep::has_observation)
    {
        push_acp_grouped_step(steps, current);
    }
    acp_current_step_without_boundary(current, next_step_id, timestamp_ms)
}

pub(crate) fn acp_current_step_without_boundary<'a>(
    current: &'a mut Option<AcpGroupedStep>,
    next_step_id: &mut u64,
    timestamp_ms: Option<u128>,
) -> &'a mut AcpGroupedStep {
    let step = current.get_or_insert_with(|| {
        let step_id = *next_step_id;
        *next_step_id += 1;
        AcpGroupedStep::new(step_id, timestamp_ms)
    });
    step.record_timestamp(timestamp_ms);
    step
}

pub(crate) fn push_acp_grouped_step(
    steps: &mut Vec<AtifStep>,
    current: &mut Option<AcpGroupedStep>,
) {
    let Some(step) = current.take() else {
        return;
    };
    if step.message.trim().is_empty()
        && step.reasoning.trim().is_empty()
        && step.tool_calls.is_empty()
        && step.observation_results.is_empty()
    {
        return;
    }
    steps.push(step.into_atif_step());
}

pub(crate) fn acp_tool_update_is_pending(update: &Value) -> bool {
    update
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("pending"))
        && update.get("rawOutput").is_none()
        && update.get("content").is_none()
}

pub(crate) fn merge_tool_call_extra(
    existing: Option<Value>,
    incoming: Option<Value>,
) -> Option<Value> {
    let mut merged = existing.unwrap_or_else(|| json!({}));
    let Some(incoming) = incoming else {
        return Some(merged);
    };
    let Some(merged_object) = merged.as_object_mut() else {
        return Some(incoming);
    };
    let Some(incoming_object) = incoming.as_object() else {
        return Some(merged);
    };
    for (key, value) in incoming_object {
        if matches!(key.as_str(), "timestamp_ms" | "pending_timestamp_ms")
            && merged_object
                .get(key)
                .is_some_and(|existing| !existing.is_null())
        {
            continue;
        }
        if !value.is_null() {
            merged_object.insert(key.clone(), value.clone());
        }
    }
    Some(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_pending_tool_updates_merge_into_single_tool_call() {
        let steps = derive_acp_atif_steps(
            1,
            &[
                acp_update(
                    1,
                    1_000,
                    json!({
                        "sessionUpdate": "agent_thought_chunk",
                        "content": { "type": "text", "text": "Need edit." },
                    }),
                ),
                acp_update(
                    2,
                    1_100,
                    json!({
                        "sessionUpdate": "tool_call_update",
                        "toolCallId": "call-1",
                        "kind": "edit",
                        "status": "pending",
                        "title": "Tool: edit",
                        "rawInput": { "arguments_json": "{\"path\":\"add.py\"", "partial": true },
                    }),
                ),
                acp_update(
                    3,
                    2_000,
                    json!({
                        "sessionUpdate": "tool_call",
                        "toolCallId": "call-1",
                        "kind": "edit",
                        "status": "in_progress",
                        "title": "Tool: edit",
                        "rawInput": { "path": "add.py", "old_string": "a - b", "new_string": "a + b" },
                    }),
                ),
                acp_update(
                    4,
                    2_005,
                    json!({
                        "sessionUpdate": "tool_call_update",
                        "toolCallId": "call-1",
                        "status": "completed",
                        "title": "Tool: edit",
                        "rawOutput": { "success": true },
                    }),
                ),
            ],
        );

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].tool_calls.len(), 1);
        assert_eq!(steps[0].tool_calls[0].function_name, "edit");
        assert_eq!(steps[0].tool_calls[0].arguments["path"], "add.py");
        let tool_extra = steps[0].tool_calls[0].extra.as_ref().expect("tool extra");
        assert_eq!(tool_extra["timestamp_ms"], json!(1_100));
        assert_eq!(tool_extra["pending_timestamp_ms"], json!(1_100));
        assert_eq!(tool_extra["execution_start_timestamp_ms"], json!(2_000));
        assert_eq!(tool_extra["status"], "in_progress");
        let observation = steps[0].observation.as_ref().expect("observation");
        assert_eq!(observation.results.len(), 1);
        assert_eq!(
            observation.results[0].extra.as_ref().unwrap()["timestamp_ms"],
            json!(2_005)
        );
    }

    fn acp_update(sequence: u64, timestamp_ms: u128, update: Value) -> TrajectoryEvent {
        TrajectoryEvent {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            sequence,
            case_id: "case".to_string(),
            kind: "acp_session_update".to_string(),
            message: "ACP agent protocol message".to_string(),
            timestamp_ms: Some(timestamp_ms),
            data: json!({
                "raw_event": {
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {
                        "sessionId": "s",
                        "update": update,
                    },
                },
            }),
        }
    }
}
