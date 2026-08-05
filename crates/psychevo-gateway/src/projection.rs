use std::collections::BTreeMap;

use psychevo::{
    application::{RunStreamEvent, TurnEvent},
    tool_argument_display::WriteArgumentPreview,
    tool_argument_display::WriteArgumentPreviewTracker,
};
use serde_json::{Value, json};

use psychevo_gateway_protocol::events_transcript::{
    GatewayActionKind, GatewayEvent, TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus,
    TranscriptEntryRole,
};

mod live_helpers;
mod live_projector;
mod live_projector_agents;
mod live_projector_assistant;
mod live_projector_state;
mod live_projector_tools;
mod live_projector_write_preview;
mod runtime_events;
mod tool_helpers;

use runtime_events::{
    clarify_action, clarify_resolution_outcome, gateway_event_from_runtime_value,
};
use tool_helpers::{LiveEntryBuild, live_entry, set_metadata_field};

#[derive(Debug, Default)]
pub struct GatewayLiveProjector {
    thread_id: Option<String>,
    active_turn_id: Option<String>,
    assistant_segment: usize,
    stream_seq: u64,
    entries: BTreeMap<usize, LiveEntryState>,
    tool_owners: BTreeMap<String, usize>,
    tool_aliases: BTreeMap<String, String>,
    tool_positions: BTreeMap<String, String>,
    tool_args: BTreeMap<String, Value>,
    write_previews: BTreeMap<String, LiveWritePreviewState>,
    exec_sessions: BTreeMap<u64, LiveExecState>,
    child_projectors: BTreeMap<String, GatewayLiveProjector>,
}

#[derive(Debug, Clone)]
struct LiveEntryState {
    segment: usize,
    started: bool,
    created_at_ms: i64,
    updated_at_ms: i64,
    next_placeholder_order: i64,
    blocks: BTreeMap<String, TranscriptBlock>,
}

#[derive(Debug, Clone)]
struct LiveExecState {
    tool_call_id: String,
    segment: usize,
    metadata: Value,
    output: String,
}

#[derive(Debug, Default)]
struct LiveWritePreviewState {
    tracker: WriteArgumentPreviewTracker,
    preview: Option<WriteArgumentPreview>,
}

struct AssistantContentProjection<'a> {
    turn_id: &'a str,
    event_value: &'a Value,
    content_block: &'a Value,
    index: usize,
    text_ordinal: Option<usize>,
    segment: usize,
    status: TranscriptBlockStatus,
    is_tool_call_turn: bool,
}

struct LiveToolBlockUpdate<'a> {
    turn_id: &'a str,
    segment: usize,
    tool_call_id: &'a str,
    tool_name: &'a str,
    status: TranscriptBlockStatus,
    body: Option<String>,
    metadata: Value,
    completed: bool,
}

struct LiveToolBlockBuild<'a> {
    turn_id: &'a str,
    segment: usize,
    tool_call_id: &'a str,
    tool_name: &'a str,
    status: TranscriptBlockStatus,
    body: Option<String>,
    metadata: Value,
    order: Option<i64>,
}

pub fn gateway_event_from_run_stream(
    turn_id: &str,
    event: &RunStreamEvent,
) -> Option<GatewayEvent> {
    Some(match event {
        RunStreamEvent::AssistantTextDelta { text } => assistant_text_delta_event(turn_id, text),
        RunStreamEvent::ReasoningDelta { text } => reasoning_delta_event(turn_id, text),
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

/// Projects one typed Framework event without rebuilding a runtime stream
/// event. Lifecycle, activity, and interaction events remain owned by the
/// caller's Application lifecycle projection.
pub fn gateway_event_from_turn_event(turn_id: &str, event: &TurnEvent) -> Option<GatewayEvent> {
    match event {
        TurnEvent::Scoped { turn_id, event, .. } => gateway_event_from_turn_event(turn_id, event),
        TurnEvent::Runtime { data } => gateway_event_from_runtime_value(turn_id, data),
        TurnEvent::Message {
            stage,
            message,
            usage,
            metadata,
            accounting,
        } => {
            let value = live_projector::framework_message_event_value(
                *stage,
                message,
                usage.as_ref(),
                metadata.as_ref(),
                accounting.as_ref(),
            );
            gateway_event_from_runtime_value(turn_id, &value)
        }
        TurnEvent::Tool { stage, data } => {
            let value = live_projector::framework_runtime_event_value(
                data,
                match stage {
                    psychevo::ItemStage::Started => "tool_execution_start",
                    psychevo::ItemStage::Updated => "tool_execution_update",
                    psychevo::ItemStage::Completed => "tool_execution_end",
                },
            );
            gateway_event_from_runtime_value(turn_id, value.as_ref())
        }
        TurnEvent::Warning { data } => {
            let value = live_projector::framework_runtime_event_value(data, "warning");
            gateway_event_from_runtime_value(turn_id, value.as_ref())
        }
        TurnEvent::MessageDelta { text } => Some(assistant_text_delta_event(turn_id, text)),
        TurnEvent::ReasoningDelta { text } => Some(reasoning_delta_event(turn_id, text)),
        TurnEvent::ActivityChanged { .. }
        | TurnEvent::Accepted { .. }
        | TurnEvent::Started { .. }
        | TurnEvent::ReasoningCompleted { .. }
        | TurnEvent::InteractionRequested { .. }
        | TurnEvent::InteractionResolved { .. }
        | TurnEvent::Completed { .. }
        | TurnEvent::Failed { .. }
        | TurnEvent::ResyncRequired { .. } => None,
    }
}

fn assistant_text_delta_event(turn_id: &str, text: &str) -> GatewayEvent {
    GatewayEvent::EntryUpdated {
        turn_id: turn_id.to_string(),
        entry: live_entry(LiveEntryBuild {
            turn_id,
            id_suffix: "assistant",
            role: TranscriptEntryRole::Assistant,
            kind: TranscriptBlockKind::Text,
            status: TranscriptBlockStatus::Running,
            title: None,
            body: Some(text.to_string()),
            metadata: Some(json!({
                "projection": "assistant_text_delta",
                "origin": "run_stream",
            })),
        }),
    }
}

fn reasoning_delta_event(turn_id: &str, text: &str) -> GatewayEvent {
    GatewayEvent::EntryUpdated {
        turn_id: turn_id.to_string(),
        entry: live_entry(LiveEntryBuild {
            turn_id,
            id_suffix: "assistant:reasoning",
            role: TranscriptEntryRole::Assistant,
            kind: TranscriptBlockKind::Reasoning,
            status: TranscriptBlockStatus::Running,
            title: Some("Thinking".to_string()),
            body: Some(text.to_string()),
            metadata: Some(json!({
                "projection": "reasoning",
                "origin": "run_stream_reasoning",
            })),
        }),
    }
}

pub(crate) fn set_write_argument_preview(
    metadata: &mut Value,
    preview: &WriteArgumentPreview,
    phase: &str,
) {
    set_metadata_field(
        metadata,
        "write_argument_preview",
        write_argument_preview_metadata_value(preview, phase),
    );
}

pub(crate) fn write_argument_preview_metadata_value(
    preview: &WriteArgumentPreview,
    phase: &str,
) -> Value {
    let mut value = serde_json::to_value(preview).unwrap_or(Value::Null);
    set_metadata_field(&mut value, "phase", json!(phase));
    value
}

#[cfg(test)]
mod tests;
