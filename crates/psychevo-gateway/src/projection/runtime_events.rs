use psychevo::RunWarning;
use psychevo::application::ClarifyResolvedReason;
use serde_json::{Value, json};

use psychevo_gateway_protocol::events_transcript::{
    GatewayActionKind, GatewayActionOutcome, GatewayEvent, PendingActionView, TranscriptBlockKind,
    TranscriptBlockStatus, TranscriptEntryRole,
};
use psychevo_gateway_protocol::source::{
    AgentDeliveryStatusView, AgentErrorView, GatewayTurn, GatewayTurnError, GatewayTurnStatus,
};

use super::live_helpers::runtime_message_role;
use super::tool_helpers::{
    LiveEntryBuild, assistant_message_is_tool_call_turn, assistant_message_metadata,
    assistant_phase_metadata, json_preview, live_entry, live_tool_entry, message_text,
    runtime_value_metadata, selected_skills_from_value, tool_event_failed, tool_name_from_value,
};

pub(super) fn gateway_event_from_runtime_value(
    turn_id: &str,
    value: &Value,
) -> Option<GatewayEvent> {
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
        Some(event_type @ ("message_start" | "message_update")) => {
            let message = value.get("message");
            if runtime_message_role(message) == Some("assistant") {
                let is_preamble = assistant_message_is_tool_call_turn(message);
                let text = message_text(message);
                if is_preamble && text.is_none() {
                    return None;
                }
                let entry = live_entry(LiveEntryBuild {
                    turn_id,
                    id_suffix: "assistant",
                    role: TranscriptEntryRole::Assistant,
                    kind: TranscriptBlockKind::Text,
                    status: TranscriptBlockStatus::Running,
                    title: None,
                    body: text,
                    metadata: Some(if is_preamble {
                        assistant_phase_metadata(value)
                    } else {
                        assistant_message_metadata(value)
                    }),
                });
                if event_type == "message_start" {
                    GatewayEvent::EntryStarted {
                        turn_id: turn_id.to_string(),
                        entry,
                    }
                } else {
                    GatewayEvent::EntryUpdated {
                        turn_id: turn_id.to_string(),
                        entry,
                    }
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
                            entry: live_entry(LiveEntryBuild {
                                turn_id,
                                id_suffix: "assistant",
                                role: TranscriptEntryRole::Assistant,
                                kind: TranscriptBlockKind::Text,
                                status: TranscriptBlockStatus::Completed,
                                title: None,
                                body: message_text(value.get("message")),
                                metadata: Some(if is_preamble {
                                    assistant_phase_metadata(value)
                                } else {
                                    assistant_message_metadata(value)
                                }),
                            }),
                        }
                    }
                }
                Some("user") => GatewayEvent::EntryCompleted {
                    turn_id: turn_id.to_string(),
                    entry: live_entry(LiveEntryBuild {
                        turn_id,
                        id_suffix: "prompt",
                        role: TranscriptEntryRole::User,
                        kind: TranscriptBlockKind::Text,
                        status: TranscriptBlockStatus::Completed,
                        title: None,
                        body: message_text(value.get("message")),
                        metadata: None,
                    }),
                },
                _ => return None,
            }
        }
        Some("agent_message") => GatewayEvent::EntryCompleted {
            turn_id: turn_id.to_string(),
            entry: live_entry(LiveEntryBuild {
                turn_id,
                id_suffix: "assistant",
                role: TranscriptEntryRole::Assistant,
                kind: TranscriptBlockKind::Text,
                status: TranscriptBlockStatus::Completed,
                title: None,
                body: value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                metadata: None,
            }),
        },
        Some("agent_session_start") => GatewayEvent::EntryUpdated {
            turn_id: turn_id.to_string(),
            entry: live_entry(LiveEntryBuild {
                turn_id,
                id_suffix: value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("agent"),
                role: TranscriptEntryRole::Assistant,
                kind: TranscriptBlockKind::Agent,
                status: TranscriptBlockStatus::Running,
                title: value
                    .get("agent_name")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                body: value
                    .get("agent_description")
                    .or_else(|| value.get("task_name"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                metadata: Some(runtime_value_metadata(value)),
            }),
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
            entry: live_entry(LiveEntryBuild {
                turn_id,
                id_suffix: "prompt",
                role: TranscriptEntryRole::User,
                kind: TranscriptBlockKind::Text,
                status: TranscriptBlockStatus::Completed,
                title: None,
                body: value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                metadata: None,
            }),
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
                action: permission_action(value, turn_id),
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

fn permission_action(value: &Value, turn_id: &str) -> PendingActionView {
    let action_id = value
        .get("call_id")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let tool_name = value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string();
    let summary = value
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let matched_rule = value
        .get("matched_rule")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let suggested_rule = value
        .get("suggested_rule")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let allow_always = value
        .get("allow_always")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let timeout_secs = value
        .get("timeout_secs")
        .and_then(Value::as_u64)
        .unwrap_or(0);
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
        thread_id: None,
        turn_id: Some(turn_id.to_string()),
        activity_id: None,
        source_key: None,
        owner_id: None,
        lease_expires_at_ms: None,
    }
}

pub(super) fn clarify_action(
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

pub(super) fn clarify_resolution_outcome(reason: &ClarifyResolvedReason) -> GatewayActionOutcome {
    match reason {
        ClarifyResolvedReason::Answered => GatewayActionOutcome::Accepted,
        ClarifyResolvedReason::Cancelled => GatewayActionOutcome::Cancelled,
        ClarifyResolvedReason::TimedOut => GatewayActionOutcome::TimedOut,
        ClarifyResolvedReason::TurnFinished => GatewayActionOutcome::Completed,
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
