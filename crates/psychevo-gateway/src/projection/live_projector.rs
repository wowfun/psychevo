use std::borrow::Cow;
use std::collections::BTreeMap;

#[cfg(test)]
use psychevo::application::RunStreamEvent;
use psychevo::application::TurnEvent;
use serde_json::{Value, json};

use psychevo_gateway_protocol::events_transcript::{
    GatewayEvent, TranscriptBlockKind, TranscriptBlockStatus,
};

use super::GatewayLiveProjector;
#[cfg(test)]
use super::gateway_event_from_run_stream;
use super::live_helpers::{
    DEFAULT_TEXT_ORDER, live_block, live_text_block_id, runtime_message_role,
};
use super::live_projector_state::force_event_thread_id;
use super::runtime_events::gateway_event_from_runtime_value;
use super::tool_helpers::tool_name_from_value;

impl GatewayLiveProjector {
    pub fn new(thread_id: Option<String>) -> Self {
        Self {
            thread_id,
            active_turn_id: None,
            assistant_segment: 0,
            stream_seq: 0,
            entries: BTreeMap::new(),
            tool_owners: BTreeMap::new(),
            tool_aliases: BTreeMap::new(),
            tool_positions: BTreeMap::new(),
            tool_args: BTreeMap::new(),
            write_previews: BTreeMap::new(),
            exec_sessions: BTreeMap::new(),
            child_projectors: BTreeMap::new(),
        }
    }

    #[cfg(test)]
    pub fn project(&mut self, turn_id: &str, event: &RunStreamEvent) -> Option<GatewayEvent> {
        if let RunStreamEvent::Scoped {
            session_id,
            turn_id: scoped_turn_id,
            event,
        } = event
        {
            return self.project_scoped(
                scoped_turn_id.as_deref().unwrap_or(turn_id),
                session_id,
                event,
            );
        }
        self.prepare_turn(turn_id);
        let mut event = match event {
            RunStreamEvent::AssistantTextDelta { text } => {
                self.project_assistant_text_delta(turn_id, text)?
            }
            RunStreamEvent::ReasoningDelta { text } => {
                self.project_reasoning_delta(turn_id, text)?
            }
            RunStreamEvent::ReasoningEnd => self.project_reasoning_end(turn_id)?,
            RunStreamEvent::Event(value) => self.project_runtime_event(turn_id, value)?,
            _ => gateway_event_from_run_stream(turn_id, event)?,
        };
        let turn_completed = matches!(event, GatewayEvent::TurnCompleted { .. });
        self.attach_thread_id(&mut event);
        if turn_completed {
            self.reset_turn_state();
        }
        Some(event)
    }

    /// Projects one typed Framework event without reconstructing the legacy
    /// runtime stream representation.
    pub fn project_turn_event(&mut self, turn_id: &str, event: &TurnEvent) -> Option<GatewayEvent> {
        if let TurnEvent::Scoped {
            thread_id,
            turn_id: scoped_turn_id,
            event,
        } = event
        {
            return self.project_scoped_turn_event(scoped_turn_id, thread_id, event);
        }
        if matches!(
            event,
            TurnEvent::Completed { .. } | TurnEvent::Failed { .. }
        ) {
            self.reset_turn_state();
            return None;
        }

        let mut projected = match event {
            TurnEvent::MessageDelta { text } => {
                self.prepare_turn(turn_id);
                self.project_assistant_text_delta(turn_id, text)?
            }
            TurnEvent::ReasoningDelta { text } => {
                self.prepare_turn(turn_id);
                self.project_reasoning_delta(turn_id, text)?
            }
            TurnEvent::ReasoningCompleted { .. } => {
                self.prepare_turn(turn_id);
                self.project_reasoning_end(turn_id)?
            }
            TurnEvent::Message {
                stage,
                message,
                usage,
                metadata,
                accounting,
            } => {
                self.prepare_turn(turn_id);
                let value = framework_message_event_value(
                    *stage,
                    message,
                    usage.as_ref(),
                    metadata.as_ref(),
                    accounting.as_ref(),
                );
                self.project_framework_runtime_event(turn_id, &value)?
            }
            TurnEvent::Tool { stage, data } => {
                self.prepare_turn(turn_id);
                let value = framework_runtime_event_value(
                    data,
                    match stage {
                        psychevo::ItemStage::Started => "tool_execution_start",
                        psychevo::ItemStage::Updated => "tool_execution_update",
                        psychevo::ItemStage::Completed => "tool_execution_end",
                    },
                );
                self.project_framework_runtime_event(turn_id, value.as_ref())?
            }
            TurnEvent::Warning { data } => {
                self.prepare_turn(turn_id);
                let value = framework_runtime_event_value(data, "warning");
                self.project_framework_runtime_event(turn_id, value.as_ref())?
            }
            TurnEvent::Runtime { data } => {
                self.prepare_turn(turn_id);
                self.project_framework_runtime_event(turn_id, data)?
            }
            TurnEvent::ActivityChanged { .. }
            | TurnEvent::Accepted { .. }
            | TurnEvent::Started { .. }
            | TurnEvent::InteractionRequested { .. }
            | TurnEvent::InteractionResolved { .. }
            | TurnEvent::ResyncRequired { .. } => return None,
            TurnEvent::Scoped { .. } | TurnEvent::Completed { .. } | TurnEvent::Failed { .. } => {
                unreachable!("scoped and terminal Framework events are handled above")
            }
        };
        self.attach_thread_id(&mut projected);
        Some(projected)
    }

    #[cfg(test)]
    fn project_scoped(
        &mut self,
        turn_id: &str,
        session_id: &str,
        event: &RunStreamEvent,
    ) -> Option<GatewayEvent> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return None;
        }
        let nested_scoped = matches!(event, RunStreamEvent::Scoped { .. });
        let child = self
            .child_projectors
            .entry(session_id.to_string())
            .or_insert_with(|| GatewayLiveProjector::new(Some(session_id.to_string())));
        let mut projected = child.project(turn_id, event)?;
        if !nested_scoped {
            force_event_thread_id(&mut projected, session_id);
        }
        Some(projected)
    }

    fn project_scoped_turn_event(
        &mut self,
        turn_id: &str,
        thread_id: &str,
        event: &TurnEvent,
    ) -> Option<GatewayEvent> {
        let thread_id = thread_id.trim();
        if thread_id.is_empty() {
            return None;
        }
        let nested_scoped = matches!(event, TurnEvent::Scoped { .. });
        let child = self
            .child_projectors
            .entry(thread_id.to_string())
            .or_insert_with(|| GatewayLiveProjector::new(Some(thread_id.to_string())));
        let mut projected = child.project_turn_event(turn_id, event)?;
        if !nested_scoped {
            force_event_thread_id(&mut projected, thread_id);
        }
        Some(projected)
    }

    fn project_runtime_event(&mut self, turn_id: &str, value: &Value) -> Option<GatewayEvent> {
        match self.project_runtime_value(turn_id, value) {
            Some(event) => Some(event),
            None if suppress_stateless_fallback(value) => None,
            None => gateway_event_from_runtime_value(turn_id, value),
        }
    }

    fn project_framework_runtime_event(
        &mut self,
        turn_id: &str,
        value: &Value,
    ) -> Option<GatewayEvent> {
        let event = self.project_runtime_event(turn_id, value)?;
        if matches!(
            event,
            GatewayEvent::TurnStarted { .. }
                | GatewayEvent::TurnCompleted { .. }
                | GatewayEvent::ActionRequested { .. }
                | GatewayEvent::ActionUpdated { .. }
                | GatewayEvent::ActionResolved { .. }
                | GatewayEvent::ActionCancelled { .. }
        ) {
            return None;
        }
        Some(event)
    }

    fn project_runtime_value(&mut self, turn_id: &str, value: &Value) -> Option<GatewayEvent> {
        match value.get("type").and_then(Value::as_str) {
            Some("message_start" | "message_update")
                if runtime_message_role(value.get("message")) == Some("assistant") =>
            {
                self.project_assistant_message_event(
                    turn_id,
                    value,
                    TranscriptBlockStatus::Running,
                    false,
                )
            }
            Some("message_end")
                if runtime_message_role(value.get("message")) == Some("assistant") =>
            {
                let event = self.project_assistant_message_event(
                    turn_id,
                    value,
                    TranscriptBlockStatus::Completed,
                    true,
                );
                self.advance_assistant_segment();
                event
            }
            Some("agent_message") => {
                let text = value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToString::to_string)?;
                let segment = self.assistant_segment;
                let block = live_block(
                    live_text_block_id(turn_id, segment, 0),
                    TranscriptBlockKind::Text,
                    TranscriptBlockStatus::Completed,
                    DEFAULT_TEXT_ORDER,
                    None,
                    Some(text),
                    None,
                );
                self.upsert_block(segment, block);
                let event = self.emit_entry_event(turn_id, segment, true, true);
                self.advance_assistant_segment();
                Some(event)
            }
            Some(
                "tool_call_pending"
                | "tool_execution_start"
                | "tool_execution_update"
                | "tool_execution_end",
            ) => self.project_tool_event(turn_id, value),
            Some("agent_session_start") => self.project_agent_session_start(turn_id, value),
            Some("exec_session_output_delta" | "exec_session_finished") => {
                self.project_exec_session_event(turn_id, value)
            }
            Some("acp_peer_plan") => self.project_acp_peer_plan(turn_id, value),
            _ => None,
        }
    }

    fn project_acp_peer_plan(&mut self, turn_id: &str, value: &Value) -> Option<GatewayEvent> {
        let body = value
            .get("body")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|body| !body.is_empty())
            .map(ToString::to_string)?;
        let segment = self.assistant_segment;
        let block = live_block(
            format!("turn:{turn_id}:acp-peer-plan"),
            TranscriptBlockKind::Status,
            TranscriptBlockStatus::Running,
            DEFAULT_TEXT_ORDER + 10,
            Some("Plan".to_string()),
            Some(body),
            Some(json!({
                "projection": "acp_peer_plan",
                "origin": "acp_peer",
                "source": "acp_peer",
                "turnId": turn_id,
                "plan": value.get("plan").cloned().unwrap_or(Value::Null),
            })),
        );
        self.upsert_block(segment, block);
        Some(self.emit_entry_event(turn_id, segment, false, false))
    }
}

pub(super) fn framework_message_event_value(
    stage: psychevo::ItemStage,
    message: &Value,
    usage: Option<&Value>,
    metadata: Option<&Value>,
    accounting: Option<&Value>,
) -> Value {
    let event_type = match stage {
        psychevo::ItemStage::Started => "message_start",
        psychevo::ItemStage::Updated => "message_update",
        psychevo::ItemStage::Completed => "message_end",
    };
    let mut value = json!({ "type": event_type, "message": message });
    let object = value
        .as_object_mut()
        .expect("Framework message projection must be an object");
    if let Some(usage) = usage {
        object.insert("usage".to_string(), usage.clone());
    }
    if let Some(metadata) = metadata {
        object.insert("metadata".to_string(), metadata.clone());
    }
    if let Some(accounting) = accounting {
        object.insert("accounting".to_string(), accounting.clone());
    }
    value
}

pub(super) fn framework_runtime_event_value<'a>(
    data: &'a Value,
    fallback_type: &str,
) -> Cow<'a, Value> {
    if data
        .as_object()
        .is_some_and(|object| object.contains_key("type"))
    {
        return Cow::Borrowed(data);
    }
    let mut value = data.clone();
    if let Some(object) = value.as_object_mut() {
        object.insert("type".to_string(), Value::String(fallback_type.to_string()));
        Cow::Owned(value)
    } else {
        Cow::Owned(json!({
            "type": fallback_type,
            "data": value,
        }))
    }
}

fn suppress_stateless_fallback(value: &Value) -> bool {
    matches!(
        value.get("type").and_then(Value::as_str),
        Some(
            "tool_call_pending"
                | "tool_execution_start"
                | "tool_execution_update"
                | "tool_execution_end"
        )
    ) && tool_name_from_value(value) == "write_stdin"
}
