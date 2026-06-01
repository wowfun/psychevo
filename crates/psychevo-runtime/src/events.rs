use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use psychevo_agent_core::{
    AgentEvent, AssistantBlock, ControlHandle, EventSink, Message, Result as CoreResult,
};
use serde_json::{Value, json};

use crate::accounting::account_usage;
use crate::context_usage::ContextRecorder;
use crate::messages::{
    add_assistant_metadata, add_elapsed_ms_metadata, sanitize_message_for_output,
};
use crate::store::store_message_fields::user_content_text;
use crate::store::{
    ContextEvidenceInput, SqliteStore, TimelineDebugEventInput, TimelineItemInput,
    TimelineItemKind, TimelineItemStatus,
};
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
                        let timeline_metadata = metadata.clone();
                        store
                            .append_message_with_undo_snapshot_metadata_and_context_evidence(
                                &session_id,
                                &message,
                                metadata,
                                content_text_override.clone(),
                                prompt_context_evidence.as_slice(),
                            )
                            .and_then(|seq| {
                                persist_timeline_for_message(
                                    &store,
                                    &session_id,
                                    seq,
                                    &message,
                                    content_text_override,
                                    timeline_metadata,
                                    accounting.as_ref(),
                                )
                            })
                            .map_err(|err| {
                                psychevo_agent_core::Error::EventSink(err.to_string())
                            })?;
                    } else {
                        let timeline_metadata = metadata.clone();
                        store
                            .append_message_with_metrics_accounting_and_context_evidence(
                                crate::store::store_messages::AppendMessageParams {
                                    session_id: &session_id,
                                    message: &message,
                                    usage,
                                    metadata,
                                    accounting: accounting.clone(),
                                    context_evidence: &[],
                                    content_text_override: None,
                                },
                            )
                            .and_then(|seq| {
                                persist_timeline_for_message(
                                    &store,
                                    &session_id,
                                    seq,
                                    &message,
                                    None,
                                    timeline_metadata,
                                    accounting.as_ref(),
                                )
                            })
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
                AgentEvent::ToolExecutionStart {
                    tool_call_id,
                    tool_name,
                    args,
                    display,
                    ..
                } => {
                    persist_tool_timeline_item(
                        &store,
                        &session_id,
                        &tool_call_id,
                        &tool_name,
                        TimelineItemStatus::Running,
                        Some(&args),
                        None,
                        display
                            .as_ref()
                            .and_then(|display| serde_json::to_value(display).ok()),
                    )
                    .map_err(|err| psychevo_agent_core::Error::EventSink(err.to_string()))?;
                }
                AgentEvent::ToolExecutionUpdate {
                    tool_call_id,
                    tool_name,
                    partial_result,
                } => {
                    persist_tool_timeline_item(
                        &store,
                        &session_id,
                        &tool_call_id,
                        &tool_name,
                        TimelineItemStatus::Running,
                        None,
                        Some(&partial_result),
                        None,
                    )
                    .map_err(|err| psychevo_agent_core::Error::EventSink(err.to_string()))?;
                }
                AgentEvent::ToolExecutionEnd {
                    tool_call_id,
                    tool_name,
                    result,
                    outcome,
                    elapsed_ms,
                    display,
                } => {
                    let mut metadata = serde_json::Map::new();
                    metadata.insert("elapsed_ms".to_string(), json!(elapsed_ms));
                    metadata.insert("outcome".to_string(), json!(outcome.as_str()));
                    if let Some(display) =
                        display.and_then(|display| serde_json::to_value(display).ok())
                    {
                        metadata.insert("display".to_string(), display);
                    }
                    persist_tool_timeline_item(
                        &store,
                        &session_id,
                        &tool_call_id,
                        &tool_name,
                        if outcome.as_str() == "normal" {
                            TimelineItemStatus::Completed
                        } else {
                            TimelineItemStatus::Failed
                        },
                        None,
                        Some(&result),
                        Some(Value::Object(metadata)),
                    )
                    .map_err(|err| psychevo_agent_core::Error::EventSink(err.to_string()))?;
                }
                other => {
                    persist_debug_event(&store, &session_id, &other)
                        .map_err(|err| psychevo_agent_core::Error::EventSink(err.to_string()))?;
                }
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

fn persist_timeline_for_message(
    store: &SqliteStore,
    session_id: &str,
    message_seq: i64,
    message: &Message,
    content_text_override: Option<String>,
    metadata: Option<Value>,
    accounting: Option<&MessageAccounting>,
) -> crate::Result<()> {
    match message {
        Message::User { content, .. } => {
            let text = content_text_override.unwrap_or_else(|| user_content_text(content));
            store.upsert_timeline_item(TimelineItemInput {
                session_id: session_id.to_string(),
                item_id: format!("message:{message_seq}:prompt"),
                turn_id: Some(format!("message:{message_seq}")),
                kind: TimelineItemKind::Prompt,
                status: TimelineItemStatus::Completed,
                source: "runtime.message".to_string(),
                title: None,
                body_text: Some(text.clone()),
                preview_text: Some(compact_text(&text, 240)),
                detail_text: Some(text),
                artifact_ids: Vec::new(),
                metadata,
            })?;
        }
        Message::Assistant {
            content,
            outcome,
            model,
            provider,
            ..
        } => {
            let status = if outcome.as_str() == "normal" {
                TimelineItemStatus::Completed
            } else {
                TimelineItemStatus::Failed
            };
            let text = content
                .iter()
                .filter_map(|block| match block {
                    AssistantBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                let mut item_metadata = metadata_object(metadata.clone());
                if let Some(accounting) = accounting {
                    item_metadata.insert("accounting".to_string(), accounting.public_json());
                }
                if let Some(model) = model {
                    item_metadata.insert("model".to_string(), json!(model));
                }
                if let Some(provider) = provider {
                    item_metadata.insert("provider".to_string(), json!(provider));
                }
                store.upsert_timeline_item(TimelineItemInput {
                    session_id: session_id.to_string(),
                    item_id: format!("message:{message_seq}:assistant"),
                    turn_id: Some(format!("message:{message_seq}")),
                    kind: TimelineItemKind::Assistant,
                    status: status.clone(),
                    source: "runtime.message".to_string(),
                    title: None,
                    body_text: Some(text.clone()),
                    preview_text: Some(compact_text(&text, 240)),
                    detail_text: Some(text),
                    artifact_ids: Vec::new(),
                    metadata: (!item_metadata.is_empty()).then_some(Value::Object(item_metadata)),
                })?;
            }
            for (index, block) in content.iter().enumerate() {
                match block {
                    AssistantBlock::Reasoning {
                        text,
                        provider_evidence,
                    } if !text.is_empty() => {
                        let mut item_metadata = serde_json::Map::new();
                        if let Some(provider_evidence) = provider_evidence {
                            item_metadata
                                .insert("provider_evidence".to_string(), provider_evidence.clone());
                        }
                        store.upsert_timeline_item(TimelineItemInput {
                            session_id: session_id.to_string(),
                            item_id: format!("message:{message_seq}:reasoning:{index}"),
                            turn_id: Some(format!("message:{message_seq}")),
                            kind: TimelineItemKind::Reasoning,
                            status: status.clone(),
                            source: "runtime.message".to_string(),
                            title: Some("Reasoning".to_string()),
                            body_text: Some(text.clone()),
                            preview_text: Some(compact_text(text, 240)),
                            detail_text: Some(text.clone()),
                            artifact_ids: Vec::new(),
                            metadata: (!item_metadata.is_empty())
                                .then_some(Value::Object(item_metadata)),
                        })?;
                    }
                    AssistantBlock::ToolCall(call) => {
                        store.upsert_timeline_item(TimelineItemInput {
                            session_id: session_id.to_string(),
                            item_id: format!("tool:{}", call.id),
                            turn_id: Some(format!("message:{message_seq}")),
                            kind: tool_kind(&call.name),
                            status: TimelineItemStatus::Pending,
                            source: "runtime.tool_call".to_string(),
                            title: Some(call.name.clone()),
                            body_text: None,
                            preview_text: Some(compact_text(&call.arguments_json, 240)),
                            detail_text: Some(call.arguments_json.clone()),
                            artifact_ids: Vec::new(),
                            metadata: Some(json!({
                                "message_session_seq": message_seq,
                                "content_index": call.content_index,
                                "call_index": call.call_index,
                                "arguments": call.arguments,
                                "arguments_error": call.arguments_error,
                            })),
                        })?;
                    }
                    _ => {}
                }
            }
        }
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            ..
        } => {
            store.upsert_timeline_item(TimelineItemInput {
                session_id: session_id.to_string(),
                item_id: format!("tool:{tool_call_id}"),
                turn_id: Some(format!("message:{message_seq}")),
                kind: tool_kind(tool_name),
                status: if *is_error {
                    TimelineItemStatus::Failed
                } else {
                    TimelineItemStatus::Completed
                },
                source: "runtime.tool_result".to_string(),
                title: Some(tool_name.clone()),
                body_text: Some(content.clone()),
                preview_text: Some(compact_text(content, 240)),
                detail_text: Some(content.clone()),
                artifact_ids: Vec::new(),
                metadata: Some(json!({
                    "message_session_seq": message_seq,
                    "tool_call_id": tool_call_id,
                    "is_error": is_error,
                    "message_metadata": metadata,
                })),
            })?;
        }
    }
    Ok(())
}

fn persist_tool_timeline_item(
    store: &SqliteStore,
    session_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    status: TimelineItemStatus,
    args: Option<&Value>,
    result: Option<&Value>,
    metadata: Option<Value>,
) -> crate::Result<()> {
    let preview_value = result.or(args);
    let preview_text = preview_value
        .and_then(|value| serde_json::to_string(value).ok())
        .map(|text| compact_text(&text, 240));
    let detail_text = preview_value.and_then(|value| serde_json::to_string_pretty(value).ok());
    let mut item_metadata = metadata_object(metadata);
    if let Some(args) = args {
        item_metadata.insert("args".to_string(), args.clone());
    }
    if let Some(result) = result {
        item_metadata.insert("result".to_string(), result.clone());
    }
    store.upsert_timeline_item(TimelineItemInput {
        session_id: session_id.to_string(),
        item_id: format!("tool:{tool_call_id}"),
        turn_id: None,
        kind: tool_kind(tool_name),
        status,
        source: "runtime.tool_execution".to_string(),
        title: Some(tool_name.to_string()),
        body_text: detail_text.clone(),
        preview_text,
        detail_text,
        artifact_ids: Vec::new(),
        metadata: (!item_metadata.is_empty()).then_some(Value::Object(item_metadata)),
    })?;
    Ok(())
}

fn persist_debug_event(
    store: &SqliteStore,
    session_id: &str,
    event: &AgentEvent,
) -> crate::Result<()> {
    let Some(event_type) = debug_event_type(event) else {
        return Ok(());
    };
    let payload = serde_json::to_value(event).ok();
    store.append_timeline_debug_event(TimelineDebugEventInput {
        session_id: session_id.to_string(),
        turn_id: None,
        event_type: event_type.to_string(),
        source: "runtime.agent_event".to_string(),
        scope: None,
        status: Some("observed".to_string()),
        summary: Some(debug_event_summary(event)),
        payload,
    })?;
    Ok(())
}

fn debug_event_type(event: &AgentEvent) -> Option<&'static str> {
    match event {
        AgentEvent::AgentStart => Some("agent.start"),
        AgentEvent::TurnStart { .. } => Some("turn.start"),
        AgentEvent::TurnEnd { .. } => Some("turn.end"),
        AgentEvent::ReasoningDelta { .. } => Some("reasoning.delta"),
        AgentEvent::ReasoningEnd { .. } => Some("reasoning.end"),
        AgentEvent::ToolCallPending { .. } => Some("tool.pending"),
        AgentEvent::MessageStart { .. } => Some("message.start"),
        AgentEvent::MessageUpdate { .. } => Some("message.update"),
        AgentEvent::AgentEnd { .. }
        | AgentEvent::MessageEnd { .. }
        | AgentEvent::ToolExecutionStart { .. }
        | AgentEvent::ToolExecutionUpdate { .. }
        | AgentEvent::ToolExecutionEnd { .. } => None,
    }
}

fn debug_event_summary(event: &AgentEvent) -> String {
    match event {
        AgentEvent::TurnStart { turn_index } => format!("turn {turn_index} started"),
        AgentEvent::TurnEnd {
            turn_index,
            outcome,
        } => format!("turn {turn_index} ended with {}", outcome.as_str()),
        AgentEvent::ReasoningDelta { text } => compact_text(text, 80),
        AgentEvent::ReasoningEnd { text } => compact_text(text, 80),
        AgentEvent::ToolCallPending {
            tool_name,
            tool_call_id,
            ..
        } => format!("{tool_name} pending ({tool_call_id})"),
        AgentEvent::MessageStart { message } => format!("{} message started", message.role()),
        AgentEvent::MessageUpdate { message } => format!("{} message updated", message.role()),
        AgentEvent::AgentStart => "agent started".to_string(),
        _ => "event observed".to_string(),
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

fn metadata_object(metadata: Option<Value>) -> serde_json::Map<String, Value> {
    match metadata {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = serde_json::Map::new();
            object.insert("value".to_string(), value);
            object
        }
        None => serde_json::Map::new(),
    }
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
