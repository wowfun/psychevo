pub fn gateway_event_from_run_stream(
    turn_id: &str,
    event: &RunStreamEvent,
) -> Option<GatewayEvent> {
    Some(match event {
        RunStreamEvent::ReasoningDelta { text } => GatewayEvent::EntryUpdated {
            turn_id: turn_id.to_string(),
            entry: live_entry(
                turn_id,
                "assistant:reasoning",
                TranscriptEntryRole::Assistant,
                TranscriptBlockKind::Reasoning,
                TranscriptBlockStatus::Running,
                Some("Thinking".to_string()),
                Some(text.clone()),
                Some(json!({
                    "projection": "reasoning",
                    "origin": "run_stream_reasoning",
                })),
            ),
        },
        RunStreamEvent::ClarifyRequest(request) => GatewayEvent::ActionRequested {
            action: clarify_action(
                request.call_id.clone(),
                serde_json::to_value(request).unwrap_or(Value::Null),
                None,
                Some(turn_id.to_string()),
            ),
        },
        RunStreamEvent::ClarifyResolved(resolved) => GatewayEvent::ActionResolved {
            action_id: resolved.call_id.clone(),
            kind: GatewayActionKind::Clarify,
            outcome: clarify_resolution_outcome(&resolved.reason),
            payload: json!({
                "reason": format!("{:?}", resolved.reason),
            }),
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
            let thread_id = value
                .get("session_id")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let outcome = value
                .get("outcome")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let status = gateway_turn_status_from_runtime_outcome(outcome.as_deref());
            let error = gateway_turn_error_from_runtime_value(value, status);
            GatewayEvent::TurnCompleted {
                thread_id: thread_id.clone(),
                turn_id: turn_id.to_string(),
                turn: GatewayTurn {
                    id: turn_id.to_string(),
                    thread_id,
                    status,
                    outcome,
                    error,
                    started_at_ms: None,
                    completed_at_ms: None,
                },
                committed_entries: Vec::new(),
            }
        }
        Some("session_title_changed") => {
            let thread_id = value
                .get("session_id")
                .and_then(Value::as_str)
                .filter(|thread_id| !thread_id.trim().is_empty())?
                .to_string();
            let title = value
                .get("title")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            GatewayEvent::TitleChanged {
                thread_id,
                title: title.clone(),
                display_title: title,
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
        Some("action_requested") => GatewayEvent::ActionRequested {
            action: action_view_from_runtime_value(value, turn_id)?,
        },
        Some("action_updated") => GatewayEvent::ActionUpdated {
            action: action_view_from_runtime_value(value, turn_id)?,
        },
        Some("action_resolved") => GatewayEvent::ActionResolved {
            action_id: action_id_from_runtime_value(value)?,
            kind: gateway_action_kind_from_runtime_value(value),
            outcome: action_outcome_from_runtime_value(value),
            payload: action_resolution_payload(value),
        },
        Some("action_cancelled") => GatewayEvent::ActionCancelled {
            action_id: action_id_from_runtime_value(value)?,
            kind: gateway_action_kind_from_runtime_value(value),
            reason: value
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        Some("exec_approval_request") | Some("apply_patch_approval_request") => {
            GatewayEvent::ActionRequested {
                action: permission_action(
                    value
                        .get("call_id")
                        .or_else(|| value.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    value
                        .get("tool_name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string(),
                    value
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    value
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    value
                        .get("matched_rule")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    value
                        .get("suggested_rule")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    value
                        .get("allow_always")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    value
                        .get("timeout_secs")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    None,
                    Some(turn_id.to_string()),
                ),
            }
        }
        _ => return None,
    })
}

fn action_view_from_runtime_value(value: &Value, turn_id: &str) -> Option<PendingActionView> {
    let action_id = action_id_from_runtime_value(value)?;
    let kind = gateway_action_kind_from_runtime_value(value);
    let payload = value.get("payload").cloned().unwrap_or(Value::Null);
    let thread_id = value
        .get("thread_id")
        .or_else(|| value.get("threadId"))
        .or_else(|| value.get("session_id"))
        .or_else(|| value.get("sessionId"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let turn_id = value
        .get("turn_id")
        .or_else(|| value.get("turnId"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| Some(turn_id.to_string()));
    Some(match kind {
        GatewayActionKind::Clarify => {
            let raw = payload
                .get("raw")
                .cloned()
                .unwrap_or_else(|| payload.clone());
            clarify_action(action_id, raw, thread_id, turn_id)
        }
        GatewayActionKind::Permission => PendingActionView {
            action_id,
            kind,
            title: action_payload_string(&payload, "toolName")
                .or_else(|| action_payload_string(&payload, "tool_name")),
            summary: action_payload_string(&payload, "summary")
                .or_else(|| action_payload_string(&payload, "reason")),
            payload,
            thread_id,
            turn_id,
            activity_id: None,
            source_key: None,
            owner_id: None,
            lease_expires_at_ms: None,
        },
        GatewayActionKind::CustomTool | GatewayActionKind::UserInput => PendingActionView {
            action_id,
            kind,
            title: action_payload_string(&payload, "title"),
            summary: action_payload_string(&payload, "summary"),
            payload,
            thread_id,
            turn_id,
            activity_id: None,
            source_key: None,
            owner_id: None,
            lease_expires_at_ms: None,
        },
    })
}

fn action_id_from_runtime_value(value: &Value) -> Option<String> {
    value
        .get("action_id")
        .or_else(|| value.get("actionId"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .filter(|id| !id.is_empty())
}

fn gateway_action_kind_from_runtime_value(value: &Value) -> GatewayActionKind {
    match value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "permission" => GatewayActionKind::Permission,
        "custom_tool" | "customTool" => GatewayActionKind::CustomTool,
        "user_input" | "userInput" => GatewayActionKind::UserInput,
        _ => GatewayActionKind::Clarify,
    }
}

fn action_payload_string(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn action_outcome_from_runtime_value(value: &Value) -> GatewayActionOutcome {
    match value
        .get("reason")
        .or_else(|| value.get("outcome"))
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "accepted" | "answered" | "allow_once" | "allow_session" | "allow_always" => {
            GatewayActionOutcome::Accepted
        }
        "rejected" | "denied" | "deny" => GatewayActionOutcome::Rejected,
        "cancelled" | "canceled" => GatewayActionOutcome::Cancelled,
        "timed_out" | "timedOut" => GatewayActionOutcome::TimedOut,
        _ => GatewayActionOutcome::Completed,
    }
}

fn action_resolution_payload(value: &Value) -> Value {
    value.get("payload").cloned().unwrap_or_else(|| {
        json!({
            "reason": value.get("reason").and_then(Value::as_str),
        })
    })
}

#[allow(clippy::too_many_arguments)]
fn permission_action(
    action_id: String,
    tool_name: String,
    summary: String,
    reason: String,
    matched_rule: Option<String>,
    suggested_rule: Option<String>,
    allow_always: bool,
    timeout_secs: u64,
    thread_id: Option<String>,
    turn_id: Option<String>,
) -> PendingActionView {
    PendingActionView {
        action_id,
        kind: GatewayActionKind::Permission,
        title: Some(tool_name.clone()),
        summary: Some(if summary.trim().is_empty() {
            reason.clone()
        } else {
            summary.clone()
        }),
        payload: json!({
            "toolName": tool_name,
            "summary": summary,
            "reason": reason,
            "matchedRule": matched_rule,
            "suggestedRule": suggested_rule,
            "allowSession": true,
            "allowAlways": allow_always,
            "authorizationLifetime": "psychevo_session",
            "alwaysAuthorizationLifetime": allow_always.then_some("permanent"),
            "timeoutSecs": timeout_secs,
        }),
        thread_id,
        turn_id,
        activity_id: None,
        source_key: None,
        owner_id: None,
        lease_expires_at_ms: None,
    }
}

fn clarify_action(
    action_id: String,
    raw: Value,
    thread_id: Option<String>,
    turn_id: Option<String>,
) -> PendingActionView {
    PendingActionView {
        action_id,
        kind: GatewayActionKind::Clarify,
        title: Some("Clarify".to_string()),
        summary: clarify_summary(&raw),
        payload: json!({ "raw": raw }),
        thread_id,
        turn_id,
        activity_id: None,
        source_key: None,
        owner_id: None,
        lease_expires_at_ms: None,
    }
}

fn clarify_summary(raw: &Value) -> Option<String> {
    raw.get("questions")
        .and_then(Value::as_array)
        .and_then(|questions| questions.first())
        .and_then(|question| question.get("question"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|question| !question.is_empty())
        .map(ToString::to_string)
}

fn clarify_resolution_outcome(
    reason: &psychevo_runtime::types::ClarifyResolvedReason,
) -> GatewayActionOutcome {
    match reason {
        psychevo_runtime::types::ClarifyResolvedReason::Answered => GatewayActionOutcome::Accepted,
        psychevo_runtime::types::ClarifyResolvedReason::Cancelled => GatewayActionOutcome::Cancelled,
        psychevo_runtime::types::ClarifyResolvedReason::TimedOut => GatewayActionOutcome::TimedOut,
        psychevo_runtime::types::ClarifyResolvedReason::TurnFinished => GatewayActionOutcome::Completed,
    }
}

fn gateway_turn_status_from_runtime_outcome(outcome: Option<&str>) -> GatewayTurnStatus {
    match outcome {
        Some("failed") | Some("error") => GatewayTurnStatus::Failed,
        Some("stopped") | Some("aborted") | Some("interrupted") | Some("cancelled") => {
            GatewayTurnStatus::Interrupted
        }
        _ => GatewayTurnStatus::Completed,
    }
}

fn gateway_turn_error_from_runtime_value(
    value: &Value,
    status: GatewayTurnStatus,
) -> Option<GatewayTurnError> {
    if !matches!(
        status,
        GatewayTurnStatus::Failed | GatewayTurnStatus::Interrupted
    ) {
        return None;
    }
    let message = value
        .get("error")
        .and_then(Value::as_str)
        .or_else(|| value.get("message").and_then(Value::as_str))
        .map(str::trim)
        .filter(|message| !message.is_empty())?;
    Some(AgentErrorView {
        message: message.to_string(),
        code: None,
        stage: None,
        retry_class: None,
        delivery: AgentDeliveryStatusView::Unknown,
        recovery_action: None,
        diagnostic_ref: None,
    })
}
