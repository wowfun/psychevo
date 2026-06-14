pub fn gateway_event_from_run_stream(
    turn_id: &str,
    event: &RunStreamEvent,
) -> Option<GatewayEvent> {
    Some(match event {
        RunStreamEvent::ReasoningDelta { text } => GatewayEvent::EntryDelta {
            turn_id: turn_id.to_string(),
            entry_id: None,
            block_id: None,
            delta: text.clone(),
        },
        RunStreamEvent::ClarifyRequest(request) => GatewayEvent::ClarifyRequested {
            request_id: request.call_id.clone(),
            raw: serde_json::to_value(request).unwrap_or(Value::Null),
        },
        RunStreamEvent::ClarifyResolved(resolved) => GatewayEvent::ClarifyResolved {
            request_id: resolved.call_id.clone(),
            reason: format!("{:?}", resolved.reason),
        },
        RunStreamEvent::Scoped { event, .. } => {
            return gateway_event_from_run_stream(turn_id, event);
        }
        RunStreamEvent::Event(value) => return gateway_event_from_runtime_value(turn_id, value),
        RunStreamEvent::ReasoningEnd => return None,
    })
}

fn gateway_event_from_runtime_value(turn_id: &str, value: &Value) -> Option<GatewayEvent> {
    Some(match value.get("type").and_then(Value::as_str) {
        Some("run_start") | Some("agent_start") | Some("task_started") | Some("turn_started") => {
            GatewayEvent::TurnStarted {
                thread_id: value
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                turn_id: turn_id.to_string(),
                selected_skills: selected_skills_from_value(value),
            }
        }
        Some("task_complete") | Some("turn_complete") | Some("agent_end") => {
            GatewayEvent::TurnCompleted {
                thread_id: value
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                turn_id: turn_id.to_string(),
                outcome: value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                committed_entries: Vec::new(),
            }
        }
        Some("message_update") => {
            let message = value.get("message");
            if runtime_message_role(message) == Some("assistant") {
                let is_preamble = assistant_message_is_tool_call_turn(message);
                let text = message_text(message);
                if is_preamble && text.is_none() {
                    return None;
                }
                GatewayEvent::EntryUpdated {
                    turn_id: turn_id.to_string(),
                    entry: live_entry(
                        turn_id,
                        "assistant",
                        TranscriptEntryRole::Assistant,
                        TranscriptBlockKind::Text,
                        TranscriptBlockStatus::Running,
                        None,
                        text,
                        Some(if is_preamble {
                            assistant_phase_metadata(value)
                        } else {
                            assistant_message_metadata(value)
                        }),
                    ),
                }
            } else {
                return None;
            }
        }
        Some("message_end") => {
            let message = value.get("message");
            match runtime_message_role(message) {
                Some("assistant") => {
                    let is_preamble = assistant_message_is_tool_call_turn(message);
                    if is_preamble && message_text(message).is_none() {
                        return None;
                    } else {
                        GatewayEvent::EntryCompleted {
                            turn_id: turn_id.to_string(),
                            entry: live_entry(
                                turn_id,
                                "assistant",
                                TranscriptEntryRole::Assistant,
                                TranscriptBlockKind::Text,
                                TranscriptBlockStatus::Completed,
                                None,
                                message_text(value.get("message")),
                                Some(if is_preamble {
                                    assistant_phase_metadata(value)
                                } else {
                                    assistant_message_metadata(value)
                                }),
                            ),
                        }
                    }
                }
                Some("user") => GatewayEvent::EntryCompleted {
                    turn_id: turn_id.to_string(),
                    entry: live_entry(
                        turn_id,
                        "prompt",
                        TranscriptEntryRole::User,
                        TranscriptBlockKind::Text,
                        TranscriptBlockStatus::Completed,
                        None,
                        message_text(value.get("message")),
                        None,
                    ),
                },
                _ => return None,
            }
        }
        Some("agent_message") => GatewayEvent::EntryCompleted {
            turn_id: turn_id.to_string(),
            entry: live_entry(
                turn_id,
                "assistant",
                TranscriptEntryRole::Assistant,
                TranscriptBlockKind::Text,
                TranscriptBlockStatus::Completed,
                None,
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                None,
            ),
        },
        Some("agent_session_start") => GatewayEvent::EntryUpdated {
            turn_id: turn_id.to_string(),
            entry: live_entry(
                turn_id,
                value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("agent"),
                TranscriptEntryRole::Assistant,
                TranscriptBlockKind::Agent,
                TranscriptBlockStatus::Running,
                value
                    .get("agent_name")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                value
                    .get("agent_description")
                    .or_else(|| value.get("task_name"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                Some(runtime_value_metadata(value)),
            ),
        },
        Some("tool_call_pending" | "tool_execution_start" | "tool_execution_update")
            if tool_name_from_value(value) == "write_stdin" =>
        {
            return None;
        }
        Some("tool_execution_end")
            if tool_name_from_value(value) == "write_stdin" && !tool_event_failed(value) =>
        {
            return None;
        }
        Some("tool_call_pending") => GatewayEvent::EntryStarted {
            turn_id: turn_id.to_string(),
            entry: live_tool_entry(
                turn_id,
                value,
                TranscriptBlockStatus::Pending,
                value
                    .get("arguments_json")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            ),
        },
        Some("tool_execution_start") => GatewayEvent::EntryStarted {
            turn_id: turn_id.to_string(),
            entry: live_tool_entry(
                turn_id,
                value,
                TranscriptBlockStatus::Running,
                value.get("args").and_then(json_preview),
            ),
        },
        Some("tool_execution_update") => GatewayEvent::EntryUpdated {
            turn_id: turn_id.to_string(),
            entry: live_tool_entry(
                turn_id,
                value,
                TranscriptBlockStatus::Running,
                value.get("partial_result").and_then(json_preview),
            ),
        },
        Some("tool_execution_end") => GatewayEvent::EntryCompleted {
            turn_id: turn_id.to_string(),
            entry: live_tool_entry(
                turn_id,
                value,
                if value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .is_some_and(|outcome| outcome != "normal")
                {
                    TranscriptBlockStatus::Failed
                } else {
                    TranscriptBlockStatus::Completed
                },
                value.get("result").and_then(json_preview),
            ),
        },
        Some("user_message") => GatewayEvent::EntryCompleted {
            turn_id: turn_id.to_string(),
            entry: live_entry(
                turn_id,
                "prompt",
                TranscriptEntryRole::User,
                TranscriptBlockKind::Text,
                TranscriptBlockStatus::Completed,
                None,
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                None,
            ),
        },
        Some("warning") => serde_json::from_value::<RunWarning>(value.clone())
            .map(|warning| GatewayEvent::Warning {
                kind: warning.kind,
                message: warning.message,
                source_path: warning.source_path.map(|path| path.display().to_string()),
                suggestion: warning.suggestion,
            })
            .unwrap_or_else(|_| GatewayEvent::Warning {
                kind: "runtime_warning".to_string(),
                message: "runtime warning could not be decoded".to_string(),
                source_path: None,
                suggestion: None,
            }),
        Some("exec_approval_request") | Some("apply_patch_approval_request") => {
            GatewayEvent::PermissionRequested {
                request_id: value
                    .get("call_id")
                    .or_else(|| value.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                tool_name: value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string(),
                summary: value
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                reason: value
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                matched_rule: value
                    .get("matched_rule")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                suggested_rule: value
                    .get("suggested_rule")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                allow_always: value
                    .get("allow_always")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                timeout_secs: value
                    .get("timeout_secs")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            }
        }
        _ => return None,
    })
}
