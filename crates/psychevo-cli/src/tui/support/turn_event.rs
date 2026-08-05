use std::borrow::Cow;

use psychevo::{
    ItemStage, TurnEvent,
    application::{ClarifyRequestEvent, ClarifyResolvedEvent, ClarifyResolvedReason},
};
use serde_json::{Value, json};

pub(crate) fn turn_event_presentation_value(event: &TurnEvent) -> Option<Cow<'_, Value>> {
    match event {
        TurnEvent::Runtime { data } => Some(Cow::Borrowed(data)),
        TurnEvent::Message {
            stage,
            message,
            usage,
            metadata,
            accounting,
        } => {
            let event_type = match stage {
                ItemStage::Started => "message_start",
                ItemStage::Updated => "message_update",
                ItemStage::Completed => "message_end",
            };
            let mut value = json!({ "type": event_type, "message": message });
            let object = value
                .as_object_mut()
                .expect("typed message presentation is an object");
            if let Some(usage) = usage {
                object.insert("usage".to_string(), usage.clone());
            }
            if let Some(metadata) = metadata {
                object.insert("metadata".to_string(), metadata.clone());
            }
            if let Some(accounting) = accounting {
                object.insert("accounting".to_string(), accounting.clone());
            }
            Some(Cow::Owned(value))
        }
        TurnEvent::Tool { stage, data } => Some(runtime_value_with_type(
            data,
            match stage {
                ItemStage::Started => "tool_execution_start",
                ItemStage::Updated => "tool_execution_update",
                ItemStage::Completed => "tool_execution_end",
            },
        )),
        TurnEvent::Warning { data } => Some(runtime_value_with_type(data, "warning")),
        TurnEvent::Scoped { event, .. } => turn_event_presentation_value(event),
        TurnEvent::ActivityChanged { .. }
        | TurnEvent::Accepted { .. }
        | TurnEvent::Started { .. }
        | TurnEvent::MessageDelta { .. }
        | TurnEvent::ReasoningDelta { .. }
        | TurnEvent::ReasoningCompleted { .. }
        | TurnEvent::InteractionRequested { .. }
        | TurnEvent::InteractionResolved { .. }
        | TurnEvent::Completed { .. }
        | TurnEvent::Failed { .. }
        | TurnEvent::ResyncRequired { .. } => None,
    }
}

pub(crate) fn turn_event_is_run_start(event: &TurnEvent) -> bool {
    match event {
        TurnEvent::Runtime { data } => {
            data.get("type").and_then(Value::as_str) == Some("run_start")
        }
        TurnEvent::Scoped { event, .. } => turn_event_is_run_start(event),
        _ => false,
    }
}

fn runtime_value_with_type<'a>(data: &'a Value, fallback_type: &str) -> Cow<'a, Value> {
    if data.get("type").is_some() {
        return Cow::Borrowed(data);
    }
    let mut value = data.clone();
    if let Some(object) = value.as_object_mut() {
        object.insert("type".to_string(), Value::String(fallback_type.to_string()));
        Cow::Owned(value)
    } else {
        Cow::Owned(json!({ "type": fallback_type, "data": value }))
    }
}

pub(crate) fn turn_event_session_id(event: &TurnEvent) -> Option<&str> {
    match event {
        TurnEvent::Scoped { thread_id, .. }
        | TurnEvent::ActivityChanged { thread_id, .. }
        | TurnEvent::Started { thread_id, .. }
        | TurnEvent::Completed { thread_id, .. }
        | TurnEvent::Failed { thread_id, .. } => Some(thread_id),
        TurnEvent::Accepted { receipt, .. } => Some(&receipt.thread_id),
        TurnEvent::Runtime { data } => data.get("session_id").and_then(Value::as_str),
        TurnEvent::Message { message, .. } => message.get("session_id").and_then(Value::as_str),
        TurnEvent::MessageDelta { .. }
        | TurnEvent::ReasoningDelta { .. }
        | TurnEvent::ReasoningCompleted { .. }
        | TurnEvent::Tool { .. }
        | TurnEvent::InteractionRequested { .. }
        | TurnEvent::InteractionResolved { .. }
        | TurnEvent::Warning { .. }
        | TurnEvent::ResyncRequired { .. } => None,
    }
}

pub(crate) fn turn_event_is_clarify_request(event: &TurnEvent) -> bool {
    match event {
        TurnEvent::InteractionRequested { kind, .. } => kind == "clarify",
        TurnEvent::Runtime { data } => {
            data.get("type").and_then(Value::as_str) == Some("action_requested")
                && data.get("kind").and_then(Value::as_str) == Some("clarify")
        }
        TurnEvent::Scoped { event, .. } => turn_event_is_clarify_request(event),
        _ => false,
    }
}

pub(crate) fn turn_event_ends_agent_child_backlog(event: &TurnEvent) -> bool {
    match event {
        TurnEvent::Message {
            stage: ItemStage::Completed,
            ..
        }
        | TurnEvent::Completed { .. }
        | TurnEvent::Failed { .. } => true,
        TurnEvent::Runtime { data } => matches!(
            data.get("type").and_then(Value::as_str),
            Some("message_end") | Some("run_end")
        ),
        TurnEvent::Scoped { event, .. } => turn_event_ends_agent_child_backlog(event),
        _ => false,
    }
}

pub(crate) fn turn_event_ends_session_backlog(event: &TurnEvent) -> bool {
    match event {
        TurnEvent::Message {
            stage: ItemStage::Completed,
            ..
        }
        | TurnEvent::Completed { .. }
        | TurnEvent::Failed { .. } => true,
        TurnEvent::Runtime { data } => matches!(
            data.get("type").and_then(Value::as_str),
            Some("message_end") | Some("agent_end") | Some("run_end")
        ),
        TurnEvent::Scoped { event, .. } => turn_event_ends_session_backlog(event),
        _ => false,
    }
}

pub(crate) fn turn_event_clarify_request(event: &TurnEvent) -> Option<ClarifyRequestEvent> {
    match event {
        TurnEvent::InteractionRequested { kind, payload, .. } if kind == "clarify" => {
            let request = payload
                .get("raw")
                .cloned()
                .unwrap_or_else(|| payload.clone());
            serde_json::from_value(request).ok()
        }
        TurnEvent::Runtime { data } => clarify_request_from_action_value(data),
        TurnEvent::Scoped { event, .. } => turn_event_clarify_request(event),
        _ => None,
    }
}

pub(crate) fn turn_event_clarify_resolution(event: &TurnEvent) -> Option<ClarifyResolvedEvent> {
    match event {
        TurnEvent::InteractionResolved {
            interaction_id,
            kind,
            reason,
        } if kind == "clarify" => Some(ClarifyResolvedEvent {
            call_id: interaction_id.clone(),
            reason: clarify_resolved_reason_from_str(reason),
        }),
        TurnEvent::Runtime { data } => clarify_resolved_from_action_value(data),
        TurnEvent::Scoped { event, .. } => turn_event_clarify_resolution(event),
        _ => None,
    }
}

fn clarify_request_from_action_value(value: &Value) -> Option<ClarifyRequestEvent> {
    if value.get("kind").and_then(Value::as_str) != Some("clarify") {
        return None;
    }
    let payload = value.get("payload")?.clone();
    let request = payload.get("raw").cloned().unwrap_or(payload);
    serde_json::from_value(request).ok()
}

fn clarify_resolved_from_action_value(value: &Value) -> Option<ClarifyResolvedEvent> {
    if value.get("kind").and_then(Value::as_str) != Some("clarify") {
        return None;
    }
    let call_id = value
        .get("action_id")
        .or_else(|| value.get("actionId"))
        .and_then(Value::as_str)?
        .to_string();
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .or_else(|| value.get("outcome").and_then(Value::as_str))
        .map(clarify_resolved_reason_from_str)
        .unwrap_or(ClarifyResolvedReason::TurnFinished);
    Some(ClarifyResolvedEvent { call_id, reason })
}

fn clarify_resolved_reason_from_str(value: &str) -> ClarifyResolvedReason {
    match value {
        "answered" | "accepted" => ClarifyResolvedReason::Answered,
        "cancelled" | "canceled" => ClarifyResolvedReason::Cancelled,
        "timed_out" | "timedOut" => ClarifyResolvedReason::TimedOut,
        _ => ClarifyResolvedReason::TurnFinished,
    }
}
