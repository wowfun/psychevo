use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use psychevo_agent_core::{AgentEvent, ControlHandle, EventSink, Message, Result as CoreResult};
use serde_json::{Value, json};

use crate::accounting::account_usage;
use crate::context_usage::ContextRecorder;
use crate::messages::{
    add_assistant_metadata, add_elapsed_ms_metadata, sanitize_message_for_output,
};
use crate::store::{ContextEvidenceInput, SqliteStore};
use crate::types::{
    MessageAccounting, ModelMetadata, PromptDisplayMetadata, RunStreamEvent, RunStreamSink,
    SelectedAgent, SmokeControl, TUI_DISPLAY_METADATA_KEY,
};

pub(crate) struct PersistenceSink {
    pub(crate) store: SqliteStore,
    pub(crate) session_id: String,
    pub(crate) prompt_snapshot: Option<String>,
    pub(crate) prompt_snapshot_written: Arc<Mutex<bool>>,
    pub(crate) prompt_context_evidence: Arc<Vec<ContextEvidenceInput>>,
    pub(crate) started: Instant,
    pub(crate) tool_elapsed_ms: Arc<Mutex<BTreeMap<String, u64>>>,
    pub(crate) control: SmokeControl,
    pub(crate) control_handle: Option<ControlHandle>,
    pub(crate) events: Option<Arc<Mutex<Vec<Value>>>>,
    pub(crate) stream_events: Option<RunStreamSink>,
    pub(crate) include_reasoning: bool,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) model_metadata: ModelMetadata,
    pub(crate) context_recorder: Option<ContextRecorder>,
    pub(crate) prompt_display: Option<PromptDisplayMetadata>,
    pub(crate) selected_agent: Option<SelectedAgent>,
    pub(crate) prompt_prefix_metadata: Option<Value>,
}

impl EventSink for PersistenceSink {
    fn emit(&self, event: AgentEvent) -> BoxFuture<'static, CoreResult<()>> {
        let store = self.store.clone();
        let session_id = self.session_id.clone();
        let prompt_snapshot = self.prompt_snapshot.clone();
        let prompt_snapshot_written = Arc::clone(&self.prompt_snapshot_written);
        let prompt_context_evidence = Arc::clone(&self.prompt_context_evidence);
        let control = self.control;
        let control_handle = self.control_handle.clone();
        let events = self.events.clone();
        let stream_events = self.stream_events.clone();
        let include_reasoning = self.include_reasoning;
        let reasoning_effort = self.reasoning_effort.clone();
        let model_metadata = self.model_metadata.clone();
        let context_recorder = self.context_recorder.clone();
        let prompt_display = self.prompt_display.clone();
        let selected_agent = self.selected_agent.clone();
        let prompt_prefix_metadata = self.prompt_prefix_metadata.clone();
        let started = self.started;
        let tool_elapsed_ms = Arc::clone(&self.tool_elapsed_ms);
        Box::pin(async move {
            let elapsed = started.elapsed();
            if let AgentEvent::ToolExecutionEnd {
                tool_call_id,
                elapsed_ms,
                ..
            } = &event
            {
                tool_elapsed_ms
                    .lock()
                    .expect("tool elapsed lock poisoned")
                    .insert(tool_call_id.clone(), *elapsed_ms);
            }
            let event = annotate_sink_event(
                event,
                elapsed,
                &tool_elapsed_ms,
                reasoning_effort.as_deref(),
                selected_agent.as_ref(),
            );
            if let AgentEvent::MessageEnd {
                message: Message::Assistant { .. },
                usage,
                ..
            } = &event
                && let Some(recorder) = &context_recorder
            {
                recorder.record_provider_usage(usage.as_ref());
            }
            let accounting = message_accounting_for_event(&event, &model_metadata);
            if let Some(events) = events
                && let Some(value) = project_agent_event_with_accounting(
                    &event,
                    include_reasoning,
                    accounting.as_ref(),
                )
            {
                events.lock().expect("event lock poisoned").push(value);
            }
            if let Some(stream_events) = stream_events
                && let Some(value) =
                    project_run_stream_event_with_accounting(&event, accounting.as_ref())
            {
                stream_events(value);
            }
            match event {
                AgentEvent::AgentStart => match control {
                    SmokeControl::None => {}
                    SmokeControl::StopAfterTurn => {
                        if let Some(handle) = control_handle {
                            handle.stop();
                        }
                    }
                    SmokeControl::AbortOnAgentStart => {
                        if let Some(handle) = control_handle {
                            handle.abort();
                        }
                    }
                },
                AgentEvent::MessageEnd {
                    message,
                    usage,
                    metadata,
                } => {
                    let should_attach_snapshot = if matches!(message, Message::User { .. }) {
                        let mut written = prompt_snapshot_written
                            .lock()
                            .expect("prompt snapshot lock poisoned");
                        if *written {
                            false
                        } else {
                            *written = true;
                            true
                        }
                    } else {
                        false
                    };
                    if should_attach_snapshot {
                        let (metadata, content_text_override) = prompt_user_metadata(
                            prompt_snapshot.clone(),
                            prompt_display.as_ref(),
                            prompt_prefix_metadata.clone(),
                        );
                        store
                            .append_message_with_undo_snapshot_metadata_and_context_evidence(
                                &session_id,
                                &message,
                                metadata,
                                content_text_override,
                                prompt_context_evidence.as_slice(),
                            )
                            .map(|_| ())
                            .map_err(|err| {
                                psychevo_agent_core::Error::EventSink(err.to_string())
                            })?;
                    } else {
                        store
                            .append_message_with_metrics_and_accounting(
                                &session_id,
                                &message,
                                usage,
                                metadata,
                                accounting,
                            )
                            .map_err(|err| {
                                psychevo_agent_core::Error::EventSink(err.to_string())
                            })?;
                    }
                }
                AgentEvent::AgentEnd {
                    outcome,
                    terminal_reason,
                    ..
                } => store
                    .finish_session(&session_id, outcome, terminal_reason)
                    .map_err(|err| psychevo_agent_core::Error::EventSink(err.to_string()))?,
                _ => {}
            }
            Ok(())
        })
    }
}

pub(crate) fn message_accounting_for_event(
    event: &AgentEvent,
    model_metadata: &ModelMetadata,
) -> Option<MessageAccounting> {
    match event {
        AgentEvent::MessageEnd {
            message: Message::Assistant { .. },
            usage,
            ..
        } => account_usage(usage.as_ref(), model_metadata),
        _ => None,
    }
}

pub(crate) fn annotate_sink_event(
    event: AgentEvent,
    elapsed: Duration,
    tool_elapsed_ms: &Arc<Mutex<BTreeMap<String, u64>>>,
    reasoning_effort: Option<&str>,
    selected_agent: Option<&SelectedAgent>,
) -> AgentEvent {
    match event {
        AgentEvent::MessageEnd {
            message,
            usage,
            metadata,
        } => {
            let metadata = match &message {
                Message::Assistant { .. } => {
                    let metadata = add_assistant_metadata(metadata, elapsed, reasoning_effort);
                    add_selected_agent_metadata(metadata, selected_agent)
                }
                Message::ToolResult { tool_call_id, .. } => {
                    let elapsed_ms = tool_elapsed_ms
                        .lock()
                        .expect("tool elapsed lock poisoned")
                        .remove(tool_call_id);
                    match elapsed_ms {
                        Some(elapsed_ms) => add_elapsed_ms_metadata(metadata, elapsed_ms),
                        None => metadata,
                    }
                }
                _ => metadata,
            };
            AgentEvent::MessageEnd {
                message,
                usage,
                metadata,
            }
        }
        other => other,
    }
}

pub(crate) fn add_selected_agent_metadata(
    metadata: Option<Value>,
    selected_agent: Option<&SelectedAgent>,
) -> Option<Value> {
    let Some(selected_agent) = selected_agent else {
        return metadata;
    };
    let mut object = match metadata {
        Some(Value::Object(object)) => object,
        Some(other) => {
            let mut object = serde_json::Map::new();
            object.insert("provider_metadata".to_string(), other);
            object
        }
        None => serde_json::Map::new(),
    };
    object.insert("selected_agent".to_string(), json!(selected_agent));
    Some(Value::Object(object))
}

pub(crate) fn prompt_user_metadata(
    snapshot: Option<String>,
    prompt_display: Option<&PromptDisplayMetadata>,
    prompt_prefix_metadata: Option<Value>,
) -> (Option<Value>, Option<String>) {
    let mut metadata = serde_json::Map::new();
    if let Some(prefix) = prompt_prefix_metadata {
        metadata.insert("prompt_prefix".to_string(), prefix);
    }
    if let Some(snapshot) = snapshot {
        metadata.insert(
            "undo".to_string(),
            json!({
                "pre_snapshot": snapshot
            }),
        );
    }
    let content_text_override = prompt_display.map(|display| display.content_text.clone());
    if let Some(display) = prompt_display
        && let Ok(value) = serde_json::to_value(display)
    {
        metadata.insert(TUI_DISPLAY_METADATA_KEY.to_string(), value);
    }
    (
        (!metadata.is_empty()).then_some(Value::Object(metadata)),
        content_text_override,
    )
}

pub(crate) fn project_agent_event(event: &AgentEvent, include_reasoning: bool) -> Option<Value> {
    project_agent_event_with_accounting(event, include_reasoning, None)
}

pub(crate) fn project_agent_event_with_accounting(
    event: &AgentEvent,
    include_reasoning: bool,
    accounting: Option<&MessageAccounting>,
) -> Option<Value> {
    let projected = match event {
        AgentEvent::ReasoningDelta { text } => {
            return include_reasoning.then(|| json!({ "type": "reasoning_delta", "text": text }));
        }
        AgentEvent::ReasoningEnd { text } => {
            return include_reasoning.then(|| json!({ "type": "reasoning_end", "text": text }));
        }
        AgentEvent::ToolCallPending { .. } => return None,
        AgentEvent::AgentEnd {
            outcome,
            messages,
            terminal_reason,
        } => {
            let mut value = json!({
                "type": "agent_end",
                "outcome": outcome.as_str(),
                "messages": messages
                    .iter()
                    .map(sanitize_message_for_output)
                    .collect::<Vec<_>>(),
            });
            if let Some(reason) = terminal_reason
                && let Some(object) = value.as_object_mut()
            {
                object.insert(
                    "terminal_reason".to_string(),
                    serde_json::to_value(reason).ok()?,
                );
                object.insert(
                    "terminal_message".to_string(),
                    Value::String(reason.message()),
                );
            }
            return Some(value);
        }
        AgentEvent::MessageStart { message } => AgentEvent::MessageStart {
            message: sanitize_message_for_output(message),
        },
        AgentEvent::MessageUpdate { message } => AgentEvent::MessageUpdate {
            message: sanitize_message_for_output(message),
        },
        AgentEvent::MessageEnd { message, .. } => {
            let mut value = json!({
                "type": "message_end",
                "message": sanitize_message_for_output(message),
            });
            if let Some(accounting) = accounting
                && let Some(object) = value.as_object_mut()
            {
                object.insert("accounting".to_string(), accounting.public_json());
            }
            return Some(value);
        }
        other => other.clone(),
    };
    serde_json::to_value(projected).ok()
}

#[cfg(test)]
pub(crate) fn project_run_stream_event(event: &AgentEvent) -> Option<RunStreamEvent> {
    project_run_stream_event_with_accounting(event, None)
}

pub(crate) fn project_run_stream_event_with_accounting(
    event: &AgentEvent,
    accounting: Option<&MessageAccounting>,
) -> Option<RunStreamEvent> {
    match event {
        AgentEvent::ReasoningDelta { text } => {
            Some(RunStreamEvent::ReasoningDelta { text: text.clone() })
        }
        AgentEvent::ReasoningEnd { .. } => Some(RunStreamEvent::ReasoningEnd),
        AgentEvent::ToolCallPending {
            tool_call_id,
            tool_name,
            arguments_json,
            content_index,
            call_index,
            display,
        } => {
            let mut value = json!({
                "type": "tool_call_pending",
                "tool_call_id": tool_call_id,
                "tool_name": tool_name,
                "arguments_json": arguments_json,
                "content_index": content_index,
                "call_index": call_index,
            });
            if let Some(display) = display
                && let Some(object) = value.as_object_mut()
            {
                object.insert("display".to_string(), serde_json::to_value(display).ok()?);
            }
            Some(RunStreamEvent::Event(value))
        }
        AgentEvent::MessageEnd {
            message,
            usage,
            metadata,
        } => {
            let mut value = json!({
                "type": "message_end",
                "message": sanitize_message_for_output(message),
            });
            if let Some(usage) = usage
                && let Some(object) = value.as_object_mut()
            {
                object.insert("usage".to_string(), usage.clone());
            }
            if let Some(metadata) = metadata
                && let Some(object) = value.as_object_mut()
            {
                object.insert("metadata".to_string(), metadata.clone());
            }
            if let Some(accounting) = accounting
                && let Some(object) = value.as_object_mut()
            {
                object.insert("accounting".to_string(), accounting.public_json());
            }
            Some(RunStreamEvent::Event(value))
        }
        _ => project_agent_event(event, false).map(RunStreamEvent::Event),
    }
}
