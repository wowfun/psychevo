use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use agent_client_protocol::schema::{
    ClientCapabilities, ContentBlock, ContentChunk, FileSystemCapabilities, Implementation,
    InitializeRequest, LoadSessionRequest, NewSessionResponse, PermissionOption,
    PermissionOptionKind, ProtocolVersion, ReadTextFileRequest, ReadTextFileResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, WriteTextFileRequest,
    WriteTextFileResponse,
};
use agent_client_protocol::util::MatchDispatch;
use agent_client_protocol::{
    Agent, ByteStreams, Client, ConnectionTo, SessionMessage, schema::v2 as acp_v2,
};
use futures::{FutureExt, StreamExt, channel::mpsc, channel::oneshot};
use psychevo_runtime::{
    AgentDefinition, AssistantBlock, Error, ImageInput, Message, Outcome,
    PermissionApprovalDecision, PermissionApprovalOutcome, PermissionApprovalRequest, RunResult,
    RunStreamEvent, RunStreamSink, SelectedAgent, ToolCallBlock, UserContentBlock,
};
use serde_json::{Map, Value, json};
use tokio::process::Command;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::{ACP_PEER_METADATA_KEY, BackendTurnRequest, ResolvedPeerTurn, gateway_now_ms};

#[derive(Debug)]
pub(crate) struct AcpPeerTurnResult {
    pub(crate) run: RunResult,
    pub(crate) native_session_id: String,
}

#[derive(Clone)]
struct AcpClientContext {
    workdir: PathBuf,
    fs_read: bool,
    fs_write: bool,
    approval_handler: Option<Arc<dyn psychevo_runtime::ApprovalHandler>>,
}

pub(crate) async fn run_acp_peer_turn(
    peer: ResolvedPeerTurn,
    request: BackendTurnRequest,
    _turn_id: String,
) -> psychevo_runtime::Result<AcpPeerTurnResult> {
    let options = request.options;
    let state = options.state.clone();
    let store = state.store();
    let (session_id, existing_native_id) = ensure_local_session(&peer, &options)?;
    let is_new_native_session = existing_native_id.is_none();
    let prompt = peer_prompt_text(
        &peer.agent,
        &options.prompt,
        &options.image_inputs,
        is_new_native_session,
    );
    let prompt_for_history = prompt_history_text(&options.prompt, &options.image_inputs);
    let acp_context = AcpPeerTurnContext {
        workdir: options.workdir.clone(),
        local_session_id: session_id.clone(),
        native_session_id: existing_native_id,
        prompt,
        peer_model: options.model.clone(),
        peer_reasoning_effort: options.reasoning_effort.clone(),
        stream: request.stream.clone(),
        approval_handler: options.approval_handler.clone(),
    };

    emit_runtime_event(
        &request.stream,
        json!({
            "type": "turn_started",
            "session_id": session_id.clone(),
            "source": "peer_agent",
            "agent_name": peer.agent.name.clone(),
            "backend_id": peer.backend.id.clone(),
        }),
    );
    store.append_message(
        &session_id,
        &Message::User {
            content: vec![UserContentBlock::text(prompt_for_history.clone())],
            timestamp_ms: gateway_now_ms(),
        },
    )?;

    let acp = run_acp_stdio_turn(&peer, &acp_context).await;
    let acp = match acp {
        Ok(acp) => acp,
        Err(err) => {
            emit_runtime_event(
                &request.stream,
                json!({
                    "type": "turn_complete",
                    "session_id": session_id.clone(),
                    "source": "peer_agent",
                    "outcome": "failed",
                    "error": err.to_string(),
                }),
            );
            store.append_message(
                &session_id,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: err.to_string(),
                    }],
                    timestamp_ms: gateway_now_ms(),
                    finish_reason: Some("error".to_string()),
                    outcome: Outcome::Failed,
                    model: Some(peer.agent.name.clone()),
                    provider: Some(format!("acp:{}", peer.backend.id)),
                },
            )?;
            return Err(err);
        }
    };

    store.set_session_metadata_field(
        &session_id,
        ACP_PEER_METADATA_KEY,
        Some(peer_session_metadata(
            &peer,
            Some(&acp.native_session_id),
            acp.usage_update.as_ref(),
        )),
    )?;
    if let Some(title) = acp.session_title.as_deref() {
        let _ = store.set_session_title(&session_id, title);
    }
    let assistant_content = acp.persisted_assistant_content();
    if !assistant_content.is_empty() {
        store.append_message(
            &session_id,
            &Message::Assistant {
                content: assistant_content,
                timestamp_ms: gateway_now_ms(),
                finish_reason: Some("end_turn".to_string()),
                outcome: Outcome::Normal,
                model: Some(peer.agent.name.clone()),
                provider: Some(format!("acp:{}", peer.backend.id)),
            },
        )?;
    }
    for message in acp.persisted_tool_result_messages() {
        store.append_message(&session_id, &message)?;
    }
    emit_runtime_event(
        &request.stream,
        json!({
            "type": "message_end",
            "session_id": session_id.clone(),
            "message": {
                "role": "assistant",
                "content": acp.final_message_content(),
            },
        }),
    );
    emit_runtime_event(
        &request.stream,
        json!({
            "type": "turn_complete",
            "session_id": session_id.clone(),
            "source": "peer_agent",
            "outcome": "normal",
        }),
    );

    let run = RunResult {
        session_id: session_id.clone(),
        outcome: Outcome::Normal,
        terminal_reason: None,
        final_answer: acp.final_answer,
        db_path: state.db_path().to_path_buf(),
        workdir: options.workdir,
        provider: format!("acp:{}", peer.backend.id),
        model: peer.agent.name.clone(),
        base_url: String::new(),
        api_key_env: None,
        reasoning_effort: options.reasoning_effort,
        context_limit: None,
        tool_failures: 0,
        selected_agent: Some(SelectedAgent {
            name: peer.agent.name.clone(),
            source: peer.agent.source.as_str().to_string(),
            path: peer.agent.file_path.clone(),
        }),
        selected_skills: Vec::new(),
        context_snapshot: None,
        events: acp.events,
        warnings: Vec::new(),
    };
    Ok(AcpPeerTurnResult {
        run,
        native_session_id: acp.native_session_id,
    })
}

struct AcpTurnOutput {
    native_session_id: String,
    final_answer: String,
    reasoning_text: String,
    final_content: Vec<Value>,
    session_title: Option<String>,
    tools: Vec<Value>,
    usage_update: Option<Value>,
    events: Vec<Value>,
}

impl AcpTurnOutput {
    fn persisted_assistant_content(&self) -> Vec<AssistantBlock> {
        let mut content = Vec::new();
        if !self.reasoning_text.trim().is_empty() {
            content.push(AssistantBlock::Reasoning {
                text: self.reasoning_text.clone(),
                provider_evidence: None,
            });
        }
        if !self.final_answer.trim().is_empty() {
            content.push(AssistantBlock::Text {
                text: self.final_answer.clone(),
            });
        }
        for tool in &self.tools {
            let call_index = content
                .iter()
                .filter(|block| matches!(block, AssistantBlock::ToolCall(_)))
                .count();
            let content_index = content.len();
            content.push(AssistantBlock::ToolCall(acp_tool_call_block(
                tool,
                content_index,
                call_index,
            )));
        }
        content
    }

    fn final_message_content(&self) -> Vec<Value> {
        self.final_content.clone()
    }

    fn persisted_tool_result_messages(&self) -> Vec<Message> {
        self.tools
            .iter()
            .filter_map(|tool| {
                let status = tool
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("pending");
                if !matches!(status, "completed" | "failed") {
                    return None;
                }
                let content = acp_tool_output(tool).unwrap_or_default();
                if content.trim().is_empty() && status != "failed" {
                    return None;
                }
                let content =
                    serde_json::to_string_pretty(&acp_tool_result(tool)).unwrap_or(content);
                Some(Message::ToolResult {
                    tool_call_id: acp_tool_call_id(tool),
                    tool_name: acp_tool_runtime_name(tool),
                    content,
                    is_error: status == "failed",
                    timestamp_ms: gateway_now_ms(),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct AcpPeerToolState {
    value: Value,
    started: bool,
}

struct AcpPeerStreamState {
    stream: Option<RunStreamSink>,
    local_session_id: String,
    final_answer: String,
    reasoning_text: String,
    reasoning_open: bool,
    session_title: Option<String>,
    tools: BTreeMap<String, AcpPeerToolState>,
    usage_update: Option<Value>,
    events: Vec<Value>,
}

impl AcpPeerStreamState {
    fn new(stream: Option<RunStreamSink>, local_session_id: String) -> Self {
        Self {
            stream,
            local_session_id,
            final_answer: String::new(),
            reasoning_text: String::new(),
            reasoning_open: false,
            session_title: None,
            tools: BTreeMap::new(),
            usage_update: None,
            events: Vec::new(),
        }
    }

    fn handle_notification(&mut self, notification: SessionNotification) {
        let native_session_id = notification.session_id.to_string();
        let update_value = serde_json::to_value(&notification.update).unwrap_or_else(|err| {
            json!({
                "sessionUpdate": "decode_error",
                "error": err.to_string(),
            })
        });
        let update_kind = acp_update_kind(&update_value);
        let event = json!({
            "type": "acp_peer_session_update",
            "session_id": self.local_session_id.clone(),
            "native_session_id": native_session_id,
            "update_kind": update_kind,
            "update": update_value.clone(),
        });
        self.events.push(event.clone());
        emit_runtime_event(&self.stream, event);

        match notification.update {
            SessionUpdate::AgentMessageChunk(chunk) => self.handle_agent_message_chunk(chunk),
            SessionUpdate::AgentThoughtChunk(chunk) => self.handle_agent_thought_chunk(chunk),
            SessionUpdate::ToolCall(_tool_call) => self.handle_tool_call(update_value),
            SessionUpdate::ToolCallUpdate(_tool_call) => self.handle_tool_call_update(update_value),
            SessionUpdate::Plan(_plan) => self.handle_plan(update_value),
            SessionUpdate::SessionInfoUpdate(_) => self.handle_session_info_update(update_value),
            SessionUpdate::UsageUpdate(_) => self.handle_usage_update(update_value),
            SessionUpdate::UserMessageChunk(_)
            | SessionUpdate::AvailableCommandsUpdate(_)
            | SessionUpdate::CurrentModeUpdate(_)
            | SessionUpdate::ConfigOptionUpdate(_) => {}
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    fn handle_notification_v2(&mut self, notification: acp_v2::SessionNotification) {
        let native_session_id = notification.session_id.to_string();
        let update_value = serde_json::to_value(&notification.update).unwrap_or_else(|err| {
            json!({
                "sessionUpdate": "decode_error",
                "error": err.to_string(),
            })
        });
        let update_kind = acp_update_kind(&update_value);
        let event = json!({
            "type": "acp_peer_session_update",
            "session_id": self.local_session_id.clone(),
            "native_session_id": native_session_id,
            "protocol_version": "2",
            "update_kind": update_kind,
            "update": update_value.clone(),
        });
        self.events.push(event.clone());
        emit_runtime_event(&self.stream, event);

        match notification.update {
            acp_v2::SessionUpdate::AgentMessageChunk(chunk) => {
                self.handle_agent_message_chunk_text(acp_v2_content_chunk_text(chunk))
            }
            acp_v2::SessionUpdate::AgentThoughtChunk(chunk) => {
                self.handle_agent_thought_chunk_text(acp_v2_content_chunk_text(chunk))
            }
            acp_v2::SessionUpdate::ToolCall(_tool_call) => self.handle_tool_call(update_value),
            acp_v2::SessionUpdate::ToolCallUpdate(_tool_call) => {
                self.handle_tool_call_update(update_value)
            }
            acp_v2::SessionUpdate::PlanUpdate(_plan) => self.handle_plan(update_value),
            acp_v2::SessionUpdate::SessionInfoUpdate(_) => {
                self.handle_session_info_update(update_value)
            }
            acp_v2::SessionUpdate::UsageUpdate(_) => self.handle_usage_update(update_value),
            acp_v2::SessionUpdate::UserMessageChunk(_)
            | acp_v2::SessionUpdate::AvailableCommandsUpdate(_)
            | acp_v2::SessionUpdate::ConfigOptionUpdate(_) => {}
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    fn handle_agent_message_chunk(&mut self, chunk: ContentChunk) {
        self.handle_agent_message_chunk_text(acp_content_chunk_text(chunk));
    }

    fn handle_agent_message_chunk_text(&mut self, text: Option<String>) {
        let Some(text) = text else {
            return;
        };
        if text.is_empty() {
            return;
        }
        self.final_answer.push_str(&text);
        emit_runtime_event(
            &self.stream,
            json!({
                "type": "message_update",
                "session_id": self.local_session_id.clone(),
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": self.final_answer.clone()}],
                },
            }),
        );
    }

    fn handle_agent_thought_chunk(&mut self, chunk: ContentChunk) {
        self.handle_agent_thought_chunk_text(acp_content_chunk_text(chunk));
    }

    fn handle_agent_thought_chunk_text(&mut self, text: Option<String>) {
        let Some(text) = text else {
            return;
        };
        if text.is_empty() {
            return;
        }
        self.reasoning_text.push_str(&text);
        self.reasoning_open = true;
        if let Some(stream) = &self.stream {
            stream(RunStreamEvent::ReasoningDelta { text });
        }
    }

    fn handle_tool_call(&mut self, update_value: Value) {
        let tool_call_id = acp_tool_call_id(&update_value);
        let runtime_event = acp_tool_runtime_event(&self.local_session_id, &update_value, false);
        let started = acp_tool_started_after_event(&runtime_event);
        self.tools.insert(
            tool_call_id,
            AcpPeerToolState {
                value: update_value,
                started,
            },
        );
        emit_runtime_event(&self.stream, runtime_event);
    }

    fn handle_tool_call_update(&mut self, update_value: Value) {
        let tool_call_id = acp_tool_call_id(&update_value);
        let previous = self.tools.get(&tool_call_id).cloned();
        let merged = match previous.as_ref() {
            Some(previous) => acp_merge_tool_update(&previous.value, &update_value),
            None => update_value,
        };
        let was_started = previous.as_ref().is_some_and(|state| state.started);
        let runtime_event = acp_tool_runtime_event(&self.local_session_id, &merged, was_started);
        let started = was_started || acp_tool_started_after_event(&runtime_event);
        self.tools.insert(
            tool_call_id,
            AcpPeerToolState {
                value: merged,
                started,
            },
        );
        emit_runtime_event(&self.stream, runtime_event);
    }

    fn handle_plan(&mut self, update_value: Value) {
        let body = acp_plan_body(&update_value);
        emit_runtime_event(
            &self.stream,
            json!({
                "type": "acp_peer_plan",
                "session_id": self.local_session_id.clone(),
                "source": "acp_peer",
                "body": body,
                "plan": update_value,
            }),
        );
    }

    fn handle_session_info_update(&mut self, update_value: Value) {
        if let Some(title) = update_value
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty())
        {
            self.session_title = Some(title.to_string());
        }
    }

    fn handle_usage_update(&mut self, update_value: Value) {
        self.usage_update = Some(update_value.clone());
        emit_runtime_event(
            &self.stream,
            json!({
                "type": "acp_peer_usage_update",
                "session_id": self.local_session_id.clone(),
                "source": "acp_peer",
                "usage": update_value,
            }),
        );
    }

    fn finish(&mut self) {
        if self.reasoning_open {
            if let Some(stream) = &self.stream {
                stream(RunStreamEvent::ReasoningEnd);
            }
            self.reasoning_open = false;
        }
    }

    fn final_message_content(&self) -> Vec<Value> {
        let mut content = Vec::new();
        if !self.reasoning_text.trim().is_empty() {
            content.push(json!({
                "type": "reasoning",
                "text": self.reasoning_text.clone(),
                "content_index": content.len(),
            }));
        }
        if !self.final_answer.trim().is_empty() {
            content.push(json!({
                "type": "text",
                "text": self.final_answer.clone(),
                "content_index": content.len(),
            }));
        }
        content
    }
}

fn acp_update_kind(update: &Value) -> String {
    update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

fn acp_content_chunk_text(chunk: ContentChunk) -> Option<String> {
    match chunk.content {
        ContentBlock::Text(text) => Some(text.text),
        _ => None,
    }
}

fn acp_v2_content_chunk_text(chunk: acp_v2::ContentChunk) -> Option<String> {
    match chunk.content {
        acp_v2::ContentBlock::Text(text) => Some(text.text),
        _ => None,
    }
}

fn acp_tool_call_id(value: &Value) -> String {
    value
        .get("toolCallId")
        .or_else(|| value.get("tool_call_id"))
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string()
}

fn acp_merge_tool_update(existing: &Value, update: &Value) -> Value {
    let mut merged = existing.as_object().cloned().unwrap_or_default();
    if let Some(update) = update.as_object() {
        for (key, value) in update {
            if key == "sessionUpdate" || value.is_null() {
                continue;
            }
            merged.insert(key.clone(), value.clone());
        }
    }
    merged.insert(
        "sessionUpdate".to_string(),
        Value::String("tool_call".to_string()),
    );
    Value::Object(merged)
}

fn acp_tool_runtime_event(local_session_id: &str, tool: &Value, was_started: bool) -> Value {
    let status = tool
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("pending");
    let event_type = match status {
        "completed" | "failed" => "tool_execution_end",
        "in_progress" if was_started => "tool_execution_update",
        "in_progress" => "tool_execution_start",
        _ => "tool_call_pending",
    };
    let mut event = Map::new();
    event.insert("type".to_string(), json!(event_type));
    event.insert("session_id".to_string(), json!(local_session_id));
    event.insert("source".to_string(), json!("acp_peer"));
    event.insert("tool_call_id".to_string(), json!(acp_tool_call_id(tool)));
    event.insert("tool_name".to_string(), json!(acp_tool_runtime_name(tool)));
    if let Some(title) = acp_tool_title(tool) {
        event.insert("display".to_string(), json!(title));
    }
    if let Some(args) = acp_tool_args(tool) {
        event.insert("args".to_string(), args.clone());
        event.insert(
            "arguments_json".to_string(),
            json!(serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string())),
        );
    }
    match event_type {
        "tool_execution_update" => {
            event.insert("partial_result".to_string(), acp_tool_result(tool));
        }
        "tool_execution_end" => {
            event.insert("result".to_string(), acp_tool_result(tool));
            event.insert(
                "outcome".to_string(),
                json!(if status == "failed" {
                    "failed"
                } else {
                    "normal"
                }),
            );
        }
        _ => {}
    }
    event.insert(
        "metadata".to_string(),
        json!({
            "origin": "acp_peer",
            "acp_update": tool,
        }),
    );
    Value::Object(event)
}

fn acp_tool_started_after_event(event: &Value) -> bool {
    matches!(
        event.get("type").and_then(Value::as_str),
        Some("tool_execution_start" | "tool_execution_update" | "tool_execution_end")
    )
}

fn acp_tool_title(tool: &Value) -> Option<String> {
    tool.get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(ToString::to_string)
}

fn acp_tool_runtime_name(tool: &Value) -> String {
    match tool.get("kind").and_then(Value::as_str).unwrap_or("other") {
        "read" => "read".to_string(),
        "edit" | "delete" | "move" => "edit".to_string(),
        "execute" => "exec_command".to_string(),
        "fetch" => "web_fetch".to_string(),
        "search" => "search".to_string(),
        "think" => "task".to_string(),
        "switch_mode" => "mode".to_string(),
        _ => acp_tool_title(tool).unwrap_or_else(|| "tool".to_string()),
    }
}

fn acp_tool_args(tool: &Value) -> Option<Value> {
    tool.get("rawInput")
        .or_else(|| tool.get("raw_input"))
        .filter(|value| !value.is_null())
        .cloned()
}

fn acp_tool_call_block(tool: &Value, content_index: usize, call_index: usize) -> ToolCallBlock {
    let arguments = acp_tool_args(tool).unwrap_or_else(|| Value::Object(Map::new()));
    let arguments_json = serde_json::to_string(&arguments).unwrap_or_else(|_| "{}".to_string());
    ToolCallBlock {
        id: acp_tool_call_id(tool),
        name: acp_tool_runtime_name(tool),
        arguments,
        arguments_json,
        arguments_error: None,
        content_index,
        call_index,
    }
}

fn acp_tool_result(tool: &Value) -> Value {
    let mut result = Map::new();
    if let Some(title) = acp_tool_title(tool) {
        result.insert("display".to_string(), json!(title));
    }
    result.insert("source".to_string(), json!("acp_peer"));
    if let Some(output) = acp_tool_output(tool) {
        result.insert("output".to_string(), json!(output));
    }
    if let Some(raw_output) = tool
        .get("rawOutput")
        .or_else(|| tool.get("raw_output"))
        .filter(|value| !value.is_null())
    {
        result.insert("raw_output".to_string(), raw_output.clone());
    }
    if let Some(content) = tool.get("content").filter(|value| !value.is_null()) {
        result.insert("content".to_string(), content.clone());
    }
    if let Some(locations) = tool.get("locations").filter(|value| !value.is_null()) {
        result.insert("locations".to_string(), locations.clone());
    }
    Value::Object(result)
}

fn acp_tool_output(tool: &Value) -> Option<String> {
    if let Some(content) = tool.get("content").and_then(Value::as_array) {
        let text = content
            .iter()
            .filter_map(acp_tool_content_text)
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    let raw_output = tool
        .get("rawOutput")
        .or_else(|| tool.get("raw_output"))
        .filter(|value| !value.is_null())?;
    if let Some(output) = raw_output.as_str() {
        return Some(output.to_string());
    }
    if let Some(output) = raw_output.get("output").and_then(Value::as_str) {
        return Some(output.to_string());
    }
    serde_json::to_string(raw_output).ok()
}

fn acp_tool_content_text(content: &Value) -> Option<String> {
    match content.get("type").and_then(Value::as_str) {
        Some("content") => {
            let content = content.get("content")?;
            match content.get("type").and_then(Value::as_str) {
                Some("text") => content
                    .get("text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                Some("image") => Some("[image]".to_string()),
                Some("resource_link") => content
                    .get("uri")
                    .and_then(Value::as_str)
                    .map(|uri| format!("Resource: {uri}")),
                Some("resource") => Some("[resource]".to_string()),
                _ => None,
            }
        }
        Some("diff") => {
            let path = content
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("file");
            Some(format!("Diff: {path}"))
        }
        Some("terminal") => {
            let terminal_id = content
                .get("terminalId")
                .or_else(|| content.get("terminal_id"))
                .and_then(Value::as_str)
                .unwrap_or("terminal");
            Some(format!("Terminal: {terminal_id}"))
        }
        _ => None,
    }
}

fn acp_plan_body(plan: &Value) -> String {
    let entries = plan
        .get("entries")
        .and_then(Value::as_array)
        .or_else(|| plan.get("plan")?.get("entries")?.as_array());
    let Some(entries) = entries else {
        return serde_json::to_string_pretty(plan).unwrap_or_else(|_| "ACP plan".to_string());
    };
    if entries.is_empty() {
        return "No plan entries.".to_string();
    }
    entries
        .iter()
        .map(|entry| {
            let status = entry
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending");
            let marker = match status {
                "completed" => "x",
                "in_progress" => "~",
                _ => " ",
            };
            let content = entry
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("Untitled task");
            format!("- [{marker}] {content}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone)]
struct AcpPeerTurnContext {
    workdir: PathBuf,
    local_session_id: String,
    native_session_id: Option<String>,
    prompt: String,
    peer_model: Option<String>,
    peer_reasoning_effort: Option<String>,
    stream: Option<RunStreamSink>,
    approval_handler: Option<Arc<dyn psychevo_runtime::ApprovalHandler>>,
}

async fn run_acp_stdio_turn(
    peer: &ResolvedPeerTurn,
    context: &AcpPeerTurnContext,
) -> psychevo_runtime::Result<AcpTurnOutput> {
    match run_acp_stdio_turn_v2(peer, context).await {
        Ok(output) => return Ok(output),
        Err(err) if err.fallback_safe => {
            emit_runtime_event(
                &context.stream,
                json!({
                    "type": "acp_peer_protocol_fallback",
                    "session_id": context.local_session_id,
                    "source": "acp_peer",
                    "from": "2",
                    "to": "1",
                    "error": err.error.to_string(),
                }),
            );
        }
        Err(err) => return Err(err.error),
    }

    run_acp_stdio_turn_v1(peer, context).await
}

struct AcpProtocolAttemptError {
    fallback_safe: bool,
    error: Error,
}

struct AcpPeerConfigSelection<'a> {
    config_id: &'static str,
    category: acp_v2::SessionConfigOptionCategory,
    requested: &'a str,
}

async fn apply_acp_v2_config_option(
    cx: &ConnectionTo<Agent>,
    config_options: &mut Vec<acp_v2::SessionConfigOption>,
    native_session_id: &str,
    local_session_id: &str,
    stream: &Option<RunStreamSink>,
    selection: AcpPeerConfigSelection<'_>,
) {
    let Some(value) = acp_v2_matching_select_value(config_options, &selection) else {
        emit_runtime_event(
            stream,
            json!({
                "type": "acp_peer_config_option_unmatched",
                "session_id": local_session_id,
                "source": "acp_peer",
                "protocol_version": "2",
                "config_id": selection.config_id,
                "requested": selection.requested,
            }),
        );
        return;
    };
    match cx
        .send_request(acp_v2::SetSessionConfigOptionRequest::new(
            native_session_id.to_string(),
            selection.config_id,
            value.as_str(),
        ))
        .block_task()
        .await
    {
        Ok(response) => {
            *config_options = response.config_options;
            emit_runtime_event(
                stream,
                json!({
                    "type": "acp_peer_config_option_set",
                    "session_id": local_session_id,
                    "source": "acp_peer",
                    "protocol_version": "2",
                    "config_id": selection.config_id,
                    "value": value,
                }),
            );
        }
        Err(err) => emit_runtime_event(
            stream,
            json!({
                "type": "acp_peer_config_option_failed",
                "session_id": local_session_id,
                "source": "acp_peer",
                "protocol_version": "2",
                "config_id": selection.config_id,
                "requested": selection.requested,
                "error": err.to_string(),
            }),
        ),
    }
}

fn acp_v2_matching_select_value(
    config_options: &[acp_v2::SessionConfigOption],
    selection: &AcpPeerConfigSelection<'_>,
) -> Option<String> {
    config_options
        .iter()
        .filter(|option| option.id.to_string() == selection.config_id)
        .find_map(|option| acp_v2_select_value(option, selection.requested))
        .or_else(|| {
            config_options
                .iter()
                .filter(|option| option.category.as_ref() == Some(&selection.category))
                .find_map(|option| acp_v2_select_value(option, selection.requested))
        })
}

fn acp_v2_select_value(option: &acp_v2::SessionConfigOption, requested: &str) -> Option<String> {
    let acp_v2::SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    match &select.options {
        acp_v2::SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .find(|option| option.value.to_string() == requested)
            .map(|option| option.value.to_string()),
        acp_v2::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .find(|option| option.value.to_string() == requested)
            .map(|option| option.value.to_string()),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

async fn run_acp_stdio_turn_v2(
    peer: &ResolvedPeerTurn,
    turn: &AcpPeerTurnContext,
) -> Result<AcpTurnOutput, AcpProtocolAttemptError> {
    let command = peer
        .backend
        .command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or_else(|| AcpProtocolAttemptError {
            fallback_safe: false,
            error: Error::Message(format!(
                "agent backend `{}` is missing command",
                peer.backend.id
            )),
        })?;
    let cwd = backend_cwd(&peer.backend.cwd, &turn.workdir);
    let mut child = Command::new(command);
    child
        .args(&peer.backend.args)
        .envs(&peer.backend.env)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = child.spawn().map_err(|err| AcpProtocolAttemptError {
        fallback_safe: false,
        error: Error::Message(format!(
            "failed to spawn ACP backend `{}` ({command}): {err}",
            peer.backend.id
        )),
    })?;
    let stdin = child.stdin.take().ok_or_else(|| AcpProtocolAttemptError {
        fallback_safe: false,
        error: Error::Message(format!(
            "ACP backend `{}` did not provide stdin",
            peer.backend.id
        )),
    })?;
    let stdout = child.stdout.take().ok_or_else(|| AcpProtocolAttemptError {
        fallback_safe: false,
        error: Error::Message(format!(
            "ACP backend `{}` did not provide stdout",
            peer.backend.id
        )),
    })?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
    let client_context = Arc::new(AcpClientContext {
        workdir: turn.workdir.clone(),
        fs_read: peer_allows_fs_read(peer),
        fs_write: peer_allows_fs_write(peer),
        approval_handler: turn.approval_handler.clone(),
    });
    let workdir = turn.workdir.clone();
    let prompt_sent = Arc::new(AtomicBool::new(false));
    let prompt_sent_for_result = Arc::clone(&prompt_sent);
    let (notification_tx, notification_rx) = mpsc::unbounded::<acp_v2::SessionNotification>();

    emit_runtime_event(
        &turn.stream,
        json!({
            "type": "acp_peer_protocol_attempt",
            "session_id": turn.local_session_id,
            "source": "acp_peer",
            "protocol_version": "2",
        }),
    );

    let turn_stream = turn.stream.clone();
    let turn_local_session_id = turn.local_session_id.clone();
    let turn_native_session_id = turn.native_session_id.clone();
    let turn_prompt = turn.prompt.clone();
    let turn_peer_model = turn.peer_model.clone();
    let turn_peer_reasoning_effort = turn.peer_reasoning_effort.clone();

    let result = Client
        .v2()
        .name("psychevo-gateway-acp-peer")
        .on_receive_notification(
            {
                let notification_tx = notification_tx.clone();
                async move |notification: acp_v2::SessionNotification, _cx| {
                    let _ = notification_tx.unbounded_send(notification);
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let context = Arc::clone(&client_context);
                async move |request: acp_v2::RequestPermissionRequest, responder, _cx| {
                    let context = Arc::clone(&context);
                    responder.respond_with_result(request_permission_v2(context, request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async move |cx| {
            cx.send_request(
                acp_v2::InitializeRequest::new(ProtocolVersion::V2)
                    .capabilities(client_capabilities_v2())
                    .client_info(
                        acp_v2::Implementation::new(
                            "psychevo-gateway",
                            env!("CARGO_PKG_VERSION"),
                        )
                        .title("Psychevo Gateway"),
                    ),
            )
            .block_task()
            .await?;
            emit_runtime_event(
                &turn_stream,
                json!({
                    "type": "acp_peer_protocol_negotiated",
                    "session_id": turn_local_session_id,
                    "source": "acp_peer",
                    "protocol_version": "2",
                }),
            );

            let (native_session_id, mut config_options) = if let Some(native_session_id) = turn_native_session_id {
                let loaded = cx.send_request(acp_v2::LoadSessionRequest::new(
                    native_session_id.clone(),
                    &workdir,
                ))
                .block_task()
                .await?;
                (native_session_id, loaded.config_options.unwrap_or_default())
            } else {
                let created = cx.send_request(acp_v2::NewSessionRequest::new(&workdir))
                    .block_task()
                    .await?;
                (
                    created.session_id.to_string(),
                    created.config_options.unwrap_or_default(),
                )
            };

            if let Some(model) = turn_peer_model.as_deref() {
                apply_acp_v2_config_option(
                    &cx,
                    &mut config_options,
                    &native_session_id,
                    &turn_local_session_id,
                    &turn_stream,
                    AcpPeerConfigSelection {
                        config_id: "model",
                        category: acp_v2::SessionConfigOptionCategory::Model,
                        requested: model,
                    },
                )
                .await;
            }

            if let Some(effort) = turn_peer_reasoning_effort.as_deref() {
                apply_acp_v2_config_option(
                    &cx,
                    &mut config_options,
                    &native_session_id,
                    &turn_local_session_id,
                    &turn_stream,
                    AcpPeerConfigSelection {
                        config_id: "effort",
                        category: acp_v2::SessionConfigOptionCategory::ThoughtLevel,
                        requested: effort,
                    },
                )
                .await;
            }

            let prompt_request = acp_v2::PromptRequest::new(
                native_session_id.clone(),
                vec![acp_v2::ContentBlock::Text(acp_v2::TextContent::new(
                    turn_prompt,
                ))],
            );
            let mut state = AcpPeerStreamState::new(turn_stream, turn_local_session_id);
            let mut notification_rx = notification_rx.fuse();
            let (done_tx, done_rx) =
                oneshot::channel::<Result<acp_v2::PromptResponse, agent_client_protocol::Error>>();
            prompt_sent.store(true, Ordering::SeqCst);
            cx.spawn({
                let cx = cx.clone();
                async move {
                    let result = cx.send_request(prompt_request).block_task().await;
                    let _ = done_tx.send(result);
                    Ok(())
                }
            })?;
            let mut done_rx = done_rx.fuse();

            loop {
                futures::select! {
                    notification = notification_rx.next() => {
                        if let Some(notification) = notification {
                            if notification.session_id.to_string() == native_session_id {
                                state.handle_notification_v2(notification);
                            }
                        } else {
                            let response = done_rx
                                .await
                                .map_err(|_| agent_client_protocol::Error::internal_error().data("prompt response channel cancelled"))?;
                            response?;
                            break;
                        }
                    }
                    response = done_rx => {
                        let response = response
                            .map_err(|_| agent_client_protocol::Error::internal_error().data("prompt response channel cancelled"))?;
                        response?;
                        break;
                    }
                }
            }

            while let Some(notification) = notification_rx.next().now_or_never().flatten() {
                if notification.session_id.to_string() == native_session_id {
                    state.handle_notification_v2(notification);
                }
            }

            state.finish();
            let final_answer = state.final_answer.clone();
            let reasoning_text = state.reasoning_text.clone();
            let final_content = state.final_message_content();
            let session_title = state.session_title.clone();
            let usage_update = state.usage_update.clone();
            let tools = state
                .tools
                .values()
                .map(|state| state.value.clone())
                .collect();
            Ok(AcpTurnOutput {
                native_session_id,
                final_answer,
                reasoning_text,
                final_content,
                session_title,
                tools,
                usage_update,
                events: state.events,
            })
        })
        .await;

    let _ = child.kill().await;
    let _ = child.wait().await;

    result.map_err(|err| AcpProtocolAttemptError {
        fallback_safe: !prompt_sent_for_result.load(Ordering::SeqCst),
        error: Error::Message(format!("ACP peer `{}` v2 failed: {err}", peer.backend.id)),
    })
}

async fn run_acp_stdio_turn_v1(
    peer: &ResolvedPeerTurn,
    turn: &AcpPeerTurnContext,
) -> psychevo_runtime::Result<AcpTurnOutput> {
    let command = peer
        .backend
        .command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or_else(|| {
            Error::Message(format!(
                "agent backend `{}` is missing command",
                peer.backend.id
            ))
        })?;
    let cwd = backend_cwd(&peer.backend.cwd, &turn.workdir);
    let mut child = Command::new(command);
    child
        .args(&peer.backend.args)
        .envs(&peer.backend.env)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = child.spawn().map_err(|err| {
        Error::Message(format!(
            "failed to spawn ACP backend `{}` ({command}): {err}",
            peer.backend.id
        ))
    })?;
    let stdin = child.stdin.take().ok_or_else(|| {
        Error::Message(format!(
            "ACP backend `{}` did not provide stdin",
            peer.backend.id
        ))
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        Error::Message(format!(
            "ACP backend `{}` did not provide stdout",
            peer.backend.id
        ))
    })?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
    let context = Arc::new(AcpClientContext {
        workdir: turn.workdir.clone(),
        fs_read: peer_allows_fs_read(peer),
        fs_write: peer_allows_fs_write(peer),
        approval_handler: turn.approval_handler.clone(),
    });
    let workdir = turn.workdir.clone();
    let turn_stream = turn.stream.clone();
    let turn_local_session_id = turn.local_session_id.clone();
    let turn_native_session_id = turn.native_session_id.clone();
    let turn_prompt = turn.prompt.clone();

    let result = Client
        .builder()
        .name("psychevo-gateway-acp-peer")
        .on_receive_request(
            {
                let context = Arc::clone(&context);
                async move |request: ReadTextFileRequest, responder, _cx| {
                    let context = Arc::clone(&context);
                    responder.respond_with_result(read_text_file(context, request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let context = Arc::clone(&context);
                async move |request: WriteTextFileRequest, responder, _cx| {
                    let context = Arc::clone(&context);
                    responder.respond_with_result(write_text_file(context, request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let context = Arc::clone(&context);
                async move |request: RequestPermissionRequest, responder, _cx| {
                    let context = Arc::clone(&context);
                    responder.respond_with_result(request_permission(context, request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async move |cx| {
            let capabilities = client_capabilities(peer);
            cx.send_request(
                InitializeRequest::new(ProtocolVersion::V1)
                    .client_capabilities(capabilities)
                    .client_info(
                        Implementation::new("psychevo-gateway", env!("CARGO_PKG_VERSION"))
                            .title("Psychevo Gateway"),
                    ),
            )
            .block_task()
            .await?;
            emit_runtime_event(
                &turn_stream,
                json!({
                    "type": "acp_peer_protocol_negotiated",
                    "session_id": turn_local_session_id,
                    "source": "acp_peer",
                    "protocol_version": "1",
                }),
            );

            let mut session = if let Some(native_session_id) = turn_native_session_id {
                let loaded = cx
                    .send_request(LoadSessionRequest::new(native_session_id.clone(), &workdir))
                    .block_task()
                    .await?;
                cx.attach_session(
                    NewSessionResponse::new(native_session_id)
                        .modes(loaded.modes)
                        .meta(loaded.meta),
                    Vec::new(),
                )?
            } else {
                cx.build_session(&workdir)
                    .block_task()
                    .start_session()
                    .await?
            };
            session.send_prompt(turn_prompt)?;
            let mut state = AcpPeerStreamState::new(turn_stream, turn_local_session_id);
            loop {
                let update = session.read_update().await?;
                match update {
                    SessionMessage::SessionMessage(dispatch) => {
                        MatchDispatch::new(dispatch)
                            .if_notification(async |notif: SessionNotification| {
                                state.handle_notification(notif);
                                Ok(())
                            })
                            .await
                            .otherwise_ignore()?;
                    }
                    SessionMessage::StopReason(_stop_reason) => break,
                    _ => {}
                }
            }
            state.finish();
            let final_answer = state.final_answer.clone();
            let reasoning_text = state.reasoning_text.clone();
            let final_content = state.final_message_content();
            let session_title = state.session_title.clone();
            let usage_update = state.usage_update.clone();
            let tools = state
                .tools
                .values()
                .map(|state| state.value.clone())
                .collect();
            Ok(AcpTurnOutput {
                native_session_id: session.session_id().to_string(),
                final_answer,
                reasoning_text,
                final_content,
                session_title,
                tools,
                usage_update,
                events: state.events,
            })
        })
        .await
        .map_err(|err| Error::Message(format!("ACP peer `{}` failed: {err}", peer.backend.id)));

    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}

fn ensure_local_session(
    peer: &ResolvedPeerTurn,
    options: &psychevo_runtime::RunOptions,
) -> psychevo_runtime::Result<(String, Option<String>)> {
    let store = options.state.store();
    if let Some(session_id) = &options.session {
        store.resume_session(session_id)?;
        let native = store
            .session_metadata(session_id)?
            .and_then(|metadata| peer_native_session_id(&metadata, &peer.backend.id));
        return Ok((session_id.clone(), native));
    }
    let session_id = store.create_session_with_metadata(
        &options.workdir,
        "peer_agent",
        &peer.agent.name,
        &format!("acp:{}", peer.backend.id),
        Some(peer_root_metadata(peer, None)),
    )?;
    Ok((session_id, None))
}

fn peer_session_metadata(
    peer: &ResolvedPeerTurn,
    native_session_id: Option<&str>,
    usage_update: Option<&Value>,
) -> Value {
    let mut value = json!({
        "agentName": peer.agent.name.clone(),
        "backendId": peer.backend.id.clone(),
        "backendKind": peer.backend.kind.as_str(),
    });
    if let Some(native_session_id) = native_session_id
        && let Some(object) = value.as_object_mut()
    {
        object.insert(
            "nativeSessionId".to_string(),
            Value::String(native_session_id.to_string()),
        );
        object.insert(
            "nativeAlias".to_string(),
            Value::String(format!("acp:{}:{native_session_id}", peer.backend.id)),
        );
    }
    if let Some(usage_update) = usage_update
        && let Some(object) = value.as_object_mut()
    {
        object.insert("usageUpdate".to_string(), usage_update.clone());
    }
    value
}

fn peer_root_metadata(peer: &ResolvedPeerTurn, native_session_id: Option<&str>) -> Value {
    json!({
        ACP_PEER_METADATA_KEY: peer_session_metadata(peer, native_session_id, None),
    })
}

fn peer_native_session_id(metadata: &Value, backend_id: &str) -> Option<String> {
    let peer = metadata.get(ACP_PEER_METADATA_KEY)?;
    let stored_backend = peer.get("backendId").and_then(Value::as_str)?;
    if stored_backend != backend_id {
        return None;
    }
    peer.get("nativeSessionId")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn emit_runtime_event(stream: &Option<psychevo_runtime::RunStreamSink>, value: Value) {
    if let Some(stream) = stream {
        stream(RunStreamEvent::Event(value));
    }
}

fn peer_prompt_text(
    agent: &AgentDefinition,
    prompt: &str,
    images: &[ImageInput],
    include_instructions: bool,
) -> String {
    let mut parts = Vec::new();
    if include_instructions && !agent.instructions.trim().is_empty() {
        parts.push(agent.instructions.trim().to_string());
    }
    parts.push(prompt.to_string());
    for image in images {
        match image {
            ImageInput::ImageUrl(url) => parts.push(format!("[image: {url}]")),
            ImageInput::LocalPath(path) => parts.push(format!(
                "[local image omitted for ACP peer: {}]",
                path.display()
            )),
        }
    }
    parts.join("\n\n")
}

fn prompt_history_text(prompt: &str, images: &[ImageInput]) -> String {
    let mut parts = vec![prompt.to_string()];
    for image in images {
        match image {
            ImageInput::ImageUrl(url) => parts.push(format!("[image: {url}]")),
            ImageInput::LocalPath(path) => parts.push(format!("[local image: {}]", path.display())),
        }
    }
    parts.join("\n\n")
}

fn client_capabilities(peer: &ResolvedPeerTurn) -> ClientCapabilities {
    ClientCapabilities::new()
        .fs(FileSystemCapabilities::new()
            .read_text_file(peer_allows_fs_read(peer))
            .write_text_file(peer_allows_fs_write(peer)))
        .terminal(false)
}

fn client_capabilities_v2() -> acp_v2::ClientCapabilities {
    acp_v2::ClientCapabilities::new()
}

fn peer_allows_fs_read(peer: &ResolvedPeerTurn) -> bool {
    peer.backend.client_capabilities.contains("fs.read")
        && agent_allows_any_tool(&peer.agent, &["read"])
}

fn peer_allows_fs_write(peer: &ResolvedPeerTurn) -> bool {
    peer.backend.client_capabilities.contains("fs.write")
        && agent_allows_any_tool(&peer.agent, &["write", "edit"])
}

fn agent_allows_any_tool(agent: &AgentDefinition, tools: &[&str]) -> bool {
    let allowed = agent
        .tool_policy
        .allowed
        .as_ref()
        .is_none_or(|allowed| tools.iter().any(|tool| allowed.contains(*tool)));
    let denied = tools
        .iter()
        .all(|tool| agent.tool_policy.denied.contains(*tool));
    allowed && !denied
}

async fn read_text_file(
    context: Arc<AcpClientContext>,
    request: ReadTextFileRequest,
) -> Result<ReadTextFileResponse, agent_client_protocol::Error> {
    let content =
        read_text_file_content(context, &request.path, request.line, request.limit).await?;
    Ok(ReadTextFileResponse::new(content))
}

async fn read_text_file_content(
    context: Arc<AcpClientContext>,
    path: &Path,
    line: Option<u32>,
    limit: Option<u32>,
) -> Result<String, agent_client_protocol::Error> {
    if !context.fs_read {
        return Err(agent_client_protocol::Error::invalid_request().data("fs.read is not allowed"));
    }
    let path = guarded_existing_path(&context.workdir, path)?;
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(acp_internal_error)?;
    Ok(apply_line_window(text, line, limit))
}

async fn write_text_file(
    context: Arc<AcpClientContext>,
    request: WriteTextFileRequest,
) -> Result<WriteTextFileResponse, agent_client_protocol::Error> {
    write_text_file_content(context, &request.path, request.content).await?;
    Ok(WriteTextFileResponse::new())
}

async fn write_text_file_content(
    context: Arc<AcpClientContext>,
    path: &Path,
    content: String,
) -> Result<(), agent_client_protocol::Error> {
    if !context.fs_write {
        return Err(agent_client_protocol::Error::invalid_request().data("fs.write is not allowed"));
    }
    let decision = if let Some(handler) = &context.approval_handler {
        handler
            .request_permission(PermissionApprovalRequest {
                tool_call_id: format!("acp-write-{}", uuid::Uuid::now_v7()),
                tool_name: "fs/write_text_file".to_string(),
                summary: format!("Write {}", path.display()),
                reason: "ACP peer requested a file write".to_string(),
                matched_rule: None,
                suggested_rule: None,
                allow_always: false,
                timeout_secs: handler.timeout_secs(),
            })
            .await
    } else {
        PermissionApprovalDecision::deny()
    };
    if matches!(decision.outcome, PermissionApprovalOutcome::Deny) {
        return Err(agent_client_protocol::Error::invalid_request().data("permission denied"));
    }
    let path = guarded_writable_path(&context.workdir, path)?;
    tokio::fs::write(&path, content)
        .await
        .map_err(acp_internal_error)?;
    Ok(())
}

async fn request_permission(
    context: Arc<AcpClientContext>,
    request: RequestPermissionRequest,
) -> Result<RequestPermissionResponse, agent_client_protocol::Error> {
    let decision = if let Some(handler) = &context.approval_handler {
        handler
            .request_permission(PermissionApprovalRequest {
                tool_call_id: request.tool_call.tool_call_id.to_string(),
                tool_name: request
                    .tool_call
                    .fields
                    .title
                    .clone()
                    .unwrap_or_else(|| "ACP tool".to_string()),
                summary: request
                    .tool_call
                    .fields
                    .title
                    .clone()
                    .unwrap_or_else(|| "ACP peer requested permission".to_string()),
                reason: "ACP peer requested permission".to_string(),
                matched_rule: None,
                suggested_rule: None,
                allow_always: request
                    .options
                    .iter()
                    .any(|option| option.kind == PermissionOptionKind::AllowAlways),
                timeout_secs: handler.timeout_secs(),
            })
            .await
    } else {
        PermissionApprovalDecision::deny()
    };
    let Some(option_id) = permission_option_id(&request.options, decision.outcome) else {
        return Ok(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ));
    };
    Ok(RequestPermissionResponse::new(
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
    ))
}

async fn request_permission_v2(
    context: Arc<AcpClientContext>,
    request: acp_v2::RequestPermissionRequest,
) -> Result<acp_v2::RequestPermissionResponse, agent_client_protocol::Error> {
    let title = request
        .tool_call
        .fields
        .title
        .clone()
        .unwrap_or_else(|| "ACP tool".to_string());
    let decision = if let Some(handler) = &context.approval_handler {
        handler
            .request_permission(PermissionApprovalRequest {
                tool_call_id: request.tool_call.tool_call_id.to_string(),
                tool_name: title.clone(),
                summary: title,
                reason: "ACP peer requested permission".to_string(),
                matched_rule: None,
                suggested_rule: None,
                allow_always: request
                    .options
                    .iter()
                    .any(|option| option.kind == acp_v2::PermissionOptionKind::AllowAlways),
                timeout_secs: handler.timeout_secs(),
            })
            .await
    } else {
        PermissionApprovalDecision::deny()
    };
    let Some(option_id) = permission_option_id_v2(&request.options, decision.outcome) else {
        return Ok(acp_v2::RequestPermissionResponse::new(
            acp_v2::RequestPermissionOutcome::Cancelled,
        ));
    };
    Ok(acp_v2::RequestPermissionResponse::new(
        acp_v2::RequestPermissionOutcome::Selected(acp_v2::SelectedPermissionOutcome::new(
            option_id,
        )),
    ))
}

fn permission_option_id(
    options: &[PermissionOption],
    outcome: PermissionApprovalOutcome,
) -> Option<String> {
    let preferred = match outcome {
        PermissionApprovalOutcome::AllowAlways => PermissionOptionKind::AllowAlways,
        PermissionApprovalOutcome::AllowOnce | PermissionApprovalOutcome::AllowSession => {
            PermissionOptionKind::AllowOnce
        }
        PermissionApprovalOutcome::Deny => PermissionOptionKind::RejectOnce,
    };
    options
        .iter()
        .find(|option| option.kind == preferred)
        .or_else(|| {
            options.iter().find(|option| {
                matches!(
                    (outcome, option.kind),
                    (
                        PermissionApprovalOutcome::AllowOnce
                            | PermissionApprovalOutcome::AllowSession
                            | PermissionApprovalOutcome::AllowAlways,
                        PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
                    ) | (
                        PermissionApprovalOutcome::Deny,
                        PermissionOptionKind::RejectOnce | PermissionOptionKind::RejectAlways
                    )
                )
            })
        })
        .map(|option| option.option_id.to_string())
}

fn permission_option_id_v2(
    options: &[acp_v2::PermissionOption],
    outcome: PermissionApprovalOutcome,
) -> Option<String> {
    let preferred = match outcome {
        PermissionApprovalOutcome::AllowAlways => acp_v2::PermissionOptionKind::AllowAlways,
        PermissionApprovalOutcome::AllowOnce | PermissionApprovalOutcome::AllowSession => {
            acp_v2::PermissionOptionKind::AllowOnce
        }
        PermissionApprovalOutcome::Deny => acp_v2::PermissionOptionKind::RejectOnce,
    };
    options
        .iter()
        .find(|option| option.kind == preferred)
        .or_else(|| {
            options.iter().find(|option| {
                matches!(
                    (outcome, &option.kind),
                    (
                        PermissionApprovalOutcome::AllowOnce
                            | PermissionApprovalOutcome::AllowSession
                            | PermissionApprovalOutcome::AllowAlways,
                        acp_v2::PermissionOptionKind::AllowOnce
                            | acp_v2::PermissionOptionKind::AllowAlways
                    ) | (
                        PermissionApprovalOutcome::Deny,
                        acp_v2::PermissionOptionKind::RejectOnce
                            | acp_v2::PermissionOptionKind::RejectAlways
                    )
                )
            })
        })
        .map(|option| option.option_id.to_string())
}

fn guarded_existing_path(
    workdir: &Path,
    path: &Path,
) -> Result<PathBuf, agent_client_protocol::Error> {
    let path = path
        .canonicalize()
        .map_err(|err| agent_client_protocol::Error::invalid_request().data(err.to_string()))?;
    let workdir = workdir
        .canonicalize()
        .map_err(|err| agent_client_protocol::Error::internal_error().data(err.to_string()))?;
    if !path.starts_with(&workdir) {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("path is outside the ACP peer workspace"));
    }
    Ok(path)
}

fn guarded_writable_path(
    workdir: &Path,
    path: &Path,
) -> Result<PathBuf, agent_client_protocol::Error> {
    if !path.is_absolute() {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("fs/write_text_file path must be absolute"));
    }
    let parent = path.parent().ok_or_else(|| {
        agent_client_protocol::Error::invalid_request()
            .data("fs/write_text_file path has no parent")
    })?;
    let parent = parent
        .canonicalize()
        .map_err(|err| agent_client_protocol::Error::invalid_request().data(err.to_string()))?;
    let workdir = workdir
        .canonicalize()
        .map_err(|err| agent_client_protocol::Error::internal_error().data(err.to_string()))?;
    if !parent.starts_with(&workdir) {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("path is outside the ACP peer workspace"));
    }
    Ok(path.to_path_buf())
}

fn apply_line_window(text: String, line: Option<u32>, limit: Option<u32>) -> String {
    if line.is_none() && limit.is_none() {
        return text;
    }
    let start = line.unwrap_or(1).saturating_sub(1) as usize;
    let limit = limit.unwrap_or(u32::MAX) as usize;
    text.lines()
        .skip(start)
        .take(limit)
        .collect::<Vec<_>>()
        .join("\n")
}

fn backend_cwd(value: &str, workdir: &Path) -> PathBuf {
    let value = value.trim();
    if value.is_empty() || value == "invocation" {
        return workdir.to_path_buf();
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        workdir.join(path)
    }
}

fn acp_internal_error(err: impl std::fmt::Display) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(err.to_string())
}
