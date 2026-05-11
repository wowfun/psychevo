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
use crate::store::SqliteStore;
use crate::types::{MessageAccounting, ModelMetadata, RunStreamEvent, RunStreamSink, SmokeControl};

pub(crate) struct PersistenceSink {
    pub(crate) store: SqliteStore,
    pub(crate) session_id: String,
    pub(crate) prompt_snapshot: Option<String>,
    pub(crate) prompt_snapshot_written: Arc<Mutex<bool>>,
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
}

impl EventSink for PersistenceSink {
    fn emit(&self, event: AgentEvent) -> BoxFuture<'static, CoreResult<()>> {
        let store = self.store.clone();
        let session_id = self.session_id.clone();
        let prompt_snapshot = self.prompt_snapshot.clone();
        let prompt_snapshot_written = Arc::clone(&self.prompt_snapshot_written);
        let control = self.control;
        let control_handle = self.control_handle.clone();
        let events = self.events.clone();
        let stream_events = self.stream_events.clone();
        let include_reasoning = self.include_reasoning;
        let reasoning_effort = self.reasoning_effort.clone();
        let model_metadata = self.model_metadata.clone();
        let context_recorder = self.context_recorder.clone();
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
                        store
                            .append_message_with_undo_snapshot(
                                &session_id,
                                &message,
                                prompt_snapshot.clone(),
                            )
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
                AgentEvent::AgentEnd { outcome, .. } => store
                    .finish_session(&session_id, outcome)
                    .map_err(|err| psychevo_agent_core::Error::EventSink(err.to_string()))?,
                _ => {}
            }
            Ok(())
        })
    }
}

fn message_accounting_for_event(
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

fn annotate_sink_event(
    event: AgentEvent,
    elapsed: Duration,
    tool_elapsed_ms: &Arc<Mutex<BTreeMap<String, u64>>>,
    reasoning_effort: Option<&str>,
) -> AgentEvent {
    match event {
        AgentEvent::MessageEnd {
            message,
            usage,
            metadata,
        } => {
            let metadata = match &message {
                Message::Assistant { .. } => {
                    add_assistant_metadata(metadata, elapsed, reasoning_effort)
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

pub(crate) fn project_agent_event(event: &AgentEvent, include_reasoning: bool) -> Option<Value> {
    project_agent_event_with_accounting(event, include_reasoning, None)
}

fn project_agent_event_with_accounting(
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
        AgentEvent::AgentEnd { outcome, messages } => AgentEvent::AgentEnd {
            outcome: *outcome,
            messages: messages.iter().map(sanitize_message_for_output).collect(),
        },
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

fn project_run_stream_event_with_accounting(
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
        } => Some(RunStreamEvent::Event(json!({
            "type": "tool_call_pending",
            "tool_call_id": tool_call_id,
            "tool_name": tool_name,
            "arguments_json": arguments_json,
            "content_index": content_index,
            "call_index": call_index,
        }))),
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
