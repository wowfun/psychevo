use psychevo_runtime::{RunStreamEvent, RunWarning};
use serde_json::Value;

use crate::protocol::{GatewayEvent, TimelineItem, TimelineItemKind, TimelineItemStatus};

pub fn gateway_event_from_run_stream(turn_id: &str, event: &RunStreamEvent) -> GatewayEvent {
    match event {
        RunStreamEvent::ReasoningDelta { text } => GatewayEvent::ItemDelta {
            turn_id: turn_id.to_string(),
            item_id: None,
            delta: text.clone(),
        },
        RunStreamEvent::ReasoningEnd => GatewayEvent::ItemCompleted {
            turn_id: turn_id.to_string(),
            item: live_item(
                turn_id,
                "reasoning",
                TimelineItemKind::Reasoning,
                TimelineItemStatus::Completed,
                None,
                None,
            ),
        },
        RunStreamEvent::ClarifyRequest(request) => GatewayEvent::ClarifyRequested {
            request_id: request.call_id.clone(),
            raw: serde_json::to_value(request).unwrap_or(Value::Null),
        },
        RunStreamEvent::ClarifyResolved(resolved) => GatewayEvent::ClarifyResolved {
            request_id: resolved.call_id.clone(),
            reason: format!("{:?}", resolved.reason),
        },
        RunStreamEvent::Scoped { event, .. } => gateway_event_from_run_stream(turn_id, event),
        RunStreamEvent::Event(value) => gateway_event_from_runtime_value(turn_id, value),
    }
}

fn gateway_event_from_runtime_value(turn_id: &str, value: &Value) -> GatewayEvent {
    match value.get("type").and_then(Value::as_str) {
        Some("run_start") | Some("agent_start") | Some("task_started") | Some("turn_started") => {
            GatewayEvent::TurnStarted {
                thread_id: value
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                turn_id: turn_id.to_string(),
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
            }
        }
        Some("agent_message") => GatewayEvent::ItemCompleted {
            turn_id: turn_id.to_string(),
            item: live_item(
                turn_id,
                "assistant",
                TimelineItemKind::Assistant,
                TimelineItemStatus::Completed,
                None,
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            ),
        },
        Some("tool_call_pending") => GatewayEvent::ItemStarted {
            turn_id: turn_id.to_string(),
            item: live_tool_item(
                turn_id,
                value,
                TimelineItemStatus::Pending,
                value
                    .get("arguments_json")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            ),
        },
        Some("tool_execution_start") => GatewayEvent::ItemStarted {
            turn_id: turn_id.to_string(),
            item: live_tool_item(
                turn_id,
                value,
                TimelineItemStatus::Running,
                value.get("args").and_then(json_preview),
            ),
        },
        Some("tool_execution_update") => GatewayEvent::ItemUpdated {
            turn_id: turn_id.to_string(),
            item: live_tool_item(
                turn_id,
                value,
                TimelineItemStatus::Running,
                value.get("partial_result").and_then(json_preview),
            ),
        },
        Some("tool_execution_end") => GatewayEvent::ItemCompleted {
            turn_id: turn_id.to_string(),
            item: live_tool_item(
                turn_id,
                value,
                if value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .is_some_and(|outcome| outcome != "normal")
                {
                    TimelineItemStatus::Failed
                } else {
                    TimelineItemStatus::Completed
                },
                value.get("result").and_then(json_preview),
            ),
        },
        Some("user_message") => GatewayEvent::ItemCompleted {
            turn_id: turn_id.to_string(),
            item: live_item(
                turn_id,
                "prompt",
                TimelineItemKind::Prompt,
                TimelineItemStatus::Completed,
                None,
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
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
                reason: value
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            }
        }
        _ => GatewayEvent::DebugAvailable {
            turn_id: turn_id.to_string(),
        },
    }
}

fn live_tool_item(
    turn_id: &str,
    value: &Value,
    status: TimelineItemStatus,
    body: Option<String>,
) -> TimelineItem {
    let tool_name = value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let tool_call_id = value
        .get("tool_call_id")
        .and_then(Value::as_str)
        .unwrap_or(tool_name);
    live_item(
        turn_id,
        &format!("tool:{tool_call_id}"),
        tool_kind(tool_name),
        status,
        Some(tool_name.to_string()),
        body,
    )
}

fn live_item(
    turn_id: &str,
    id_suffix: &str,
    kind: TimelineItemKind,
    status: TimelineItemStatus,
    title: Option<String>,
    body: Option<String>,
) -> TimelineItem {
    let now = crate::gateway_now_ms();
    TimelineItem {
        id: format!("live:{turn_id}:{id_suffix}"),
        thread_id: String::new(),
        turn_id: Some(turn_id.to_string()),
        sequence: 0,
        kind,
        status,
        source: "runtime.stream".to_string(),
        title,
        preview: body.as_deref().map(|text| compact_text(text, 240)),
        detail: body.clone(),
        body,
        artifact_ids: Vec::new(),
        metadata: None,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn tool_kind(tool_name: &str) -> TimelineItemKind {
    match tool_name {
        "exec_command" | "write_stdin" => TimelineItemKind::Shell,
        "read" | "write" | "edit" | "apply_patch" => TimelineItemKind::File,
        "web_fetch" | "web_search" => TimelineItemKind::Web,
        "mcp" | "mcp_call" => TimelineItemKind::Mcp,
        "clarify" => TimelineItemKind::Clarify,
        _ => TimelineItemKind::Tool,
    }
}

fn json_preview(value: &Value) -> Option<String> {
    serde_json::to_string(value).ok()
}

fn compact_text(text: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= max_chars {
            output.push_str("...");
            return output;
        }
        output.push(ch);
    }
    output
}
