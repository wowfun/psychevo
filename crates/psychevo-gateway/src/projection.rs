use psychevo_runtime::{RunStreamEvent, RunWarning};
use serde_json::Value;

use crate::protocol::{
    GatewayEvent, GatewaySelectedSkill, TimelineItem, TimelineItemKind, TimelineItemStatus,
};

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
            }
        }
        Some("message_update") => {
            let role = value
                .get("message")
                .and_then(|message| message.get("role"))
                .and_then(Value::as_str);
            if role == Some("assistant") {
                GatewayEvent::ItemUpdated {
                    turn_id: turn_id.to_string(),
                    item: live_item(
                        turn_id,
                        "assistant",
                        TimelineItemKind::Assistant,
                        TimelineItemStatus::Running,
                        None,
                        message_text(value.get("message")),
                    ),
                }
            } else {
                GatewayEvent::DebugAvailable {
                    turn_id: turn_id.to_string(),
                }
            }
        }
        Some("message_end") => {
            let role = value
                .get("message")
                .and_then(|message| message.get("role"))
                .and_then(Value::as_str);
            match role {
                Some("assistant") => GatewayEvent::ItemCompleted {
                    turn_id: turn_id.to_string(),
                    item: live_item_with_metadata(
                        turn_id,
                        "assistant",
                        TimelineItemKind::Assistant,
                        TimelineItemStatus::Completed,
                        None,
                        message_text(value.get("message")),
                        Some(assistant_message_metadata(value)),
                    ),
                },
                Some("user") => GatewayEvent::ItemCompleted {
                    turn_id: turn_id.to_string(),
                    item: live_item(
                        turn_id,
                        "prompt",
                        TimelineItemKind::Prompt,
                        TimelineItemStatus::Completed,
                        None,
                        message_text(value.get("message")),
                    ),
                },
                _ => GatewayEvent::DebugAvailable {
                    turn_id: turn_id.to_string(),
                },
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
        Some("agent_session_start") => GatewayEvent::ItemUpdated {
            turn_id: turn_id.to_string(),
            item: live_item_with_metadata(
                turn_id,
                value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("agent"),
                TimelineItemKind::Agent,
                TimelineItemStatus::Running,
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
    .with_metadata(tool_value_metadata(value))
}

trait TimelineItemMetadataExt {
    fn with_metadata(self, metadata: Value) -> Self;
}

impl TimelineItemMetadataExt for TimelineItem {
    fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

fn live_item(
    turn_id: &str,
    id_suffix: &str,
    kind: TimelineItemKind,
    status: TimelineItemStatus,
    title: Option<String>,
    body: Option<String>,
) -> TimelineItem {
    live_item_with_metadata(turn_id, id_suffix, kind, status, title, body, None)
}

fn live_item_with_metadata(
    turn_id: &str,
    id_suffix: &str,
    kind: TimelineItemKind,
    status: TimelineItemStatus,
    title: Option<String>,
    body: Option<String>,
    metadata: Option<Value>,
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
        metadata,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn assistant_message_metadata(value: &Value) -> Value {
    let mut metadata = serde_json::json!({
        "usage": value.get("usage").cloned().unwrap_or(Value::Null),
        "metadata": value.get("metadata").cloned().unwrap_or(Value::Null),
        "accounting": value.get("accounting").cloned().unwrap_or(Value::Null),
    });
    if let Some(object) = metadata.as_object_mut()
        && let Some(message) = value.get("message")
    {
        for key in ["provider", "model", "finish_reason", "outcome"] {
            if let Some(field) = message.get(key).filter(|field| !field.is_null()) {
                object.insert(key.to_string(), field.clone());
            }
        }
    }
    metadata
}

fn tool_value_metadata(value: &Value) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("projection".to_string(), Value::String("tool".to_string()));
    for key in [
        "type",
        "tool_name",
        "tool_call_id",
        "outcome",
        "source",
        "display",
        "result",
        "metadata",
    ] {
        if let Some(field) = value.get(key) {
            object.insert(key.to_string(), field.clone());
        }
    }
    if let Some(args) = value.get("args").cloned().or_else(|| {
        value
            .get("arguments_json")
            .and_then(Value::as_str)
            .and_then(|raw| serde_json::from_str(raw).ok())
    }) {
        object.insert("args".to_string(), args);
    }
    if !object.contains_key("outcome") {
        object.insert("outcome".to_string(), Value::String("normal".to_string()));
    }
    Value::Object(object)
}

fn runtime_value_metadata(value: &Value) -> Value {
    let mut object = value.as_object().cloned().unwrap_or_default();
    object.insert(
        "projection".to_string(),
        Value::String("runtimeValue".to_string()),
    );
    Value::Object(object)
}

fn selected_skills_from_value(value: &Value) -> Vec<GatewaySelectedSkill> {
    value
        .get("selected_skills")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|skill| {
            Some(GatewaySelectedSkill {
                name: skill.get("name")?.as_str()?.to_string(),
                path: skill
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
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

fn message_text(message: Option<&Value>) -> Option<String> {
    let text = message?
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn run_start_projects_selected_skills() {
        let event = gateway_event_from_run_stream(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "run_start",
                "session_id": "thread-1",
                "selected_skills": [
                    {"name": "reviewer", "path": "/tmp/reviewer/SKILL.md"}
                ]
            })),
        );
        match event {
            GatewayEvent::TurnStarted {
                thread_id,
                turn_id,
                selected_skills,
            } => {
                assert_eq!(thread_id.as_deref(), Some("thread-1"));
                assert_eq!(turn_id, "turn-1");
                assert_eq!(selected_skills.len(), 1);
                assert_eq!(selected_skills[0].name, "reviewer");
                assert_eq!(selected_skills[0].path, "/tmp/reviewer/SKILL.md");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn assistant_message_end_projects_turn_meta_fields() {
        let event = gateway_event_from_run_stream(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "done"}],
                    "provider": "mock",
                    "model": "mock-model",
                    "finish_reason": "stop",
                    "outcome": "normal"
                },
                "usage": {"input_tokens": 12},
                "metadata": {"elapsed_ms": 2000},
                "accounting": {"estimated_cost_nanodollars": 10}
            })),
        );
        match event {
            GatewayEvent::ItemCompleted { item, .. } => {
                let metadata = item.metadata.expect("metadata");
                assert_eq!(metadata["provider"], "mock");
                assert_eq!(metadata["model"], "mock-model");
                assert_eq!(metadata["finish_reason"], "stop");
                assert_eq!(metadata["outcome"], "normal");
                assert_eq!(metadata["usage"]["input_tokens"], 12);
                assert_eq!(metadata["metadata"]["elapsed_ms"], 2000);
                assert_eq!(metadata["accounting"]["estimated_cost_nanodollars"], 10);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
