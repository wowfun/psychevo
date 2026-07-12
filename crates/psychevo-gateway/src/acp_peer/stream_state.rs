struct AcpTurnOutput {
    native_session_id: String,
    final_answer: String,
    final_content: Vec<Value>,
    content_slots: Vec<AcpPeerContentSlot>,
    latest_plan: Option<AcpPeerPlanProjection>,
    session_title: Option<String>,
    tools: BTreeMap<String, Value>,
    usage_update: Option<Value>,
    events: Vec<Value>,
    session_snapshot: AcpSessionSnapshot,
}

#[derive(Debug, Clone, PartialEq)]
struct AcpPeerPlanProjection {
    body: String,
    update: Value,
}

impl AcpTurnOutput {
    fn persisted_assistant_content(&self) -> Vec<AssistantBlock> {
        let mut content = Vec::new();
        for slot in &self.content_slots {
            match slot {
                AcpPeerContentSlot::Reasoning { text } if !text.trim().is_empty() => {
                    content.push(AssistantBlock::Reasoning {
                        text: text.clone(),
                        provider_evidence: None,
                    });
                }
                AcpPeerContentSlot::Text { text, .. } if !text.trim().is_empty() => {
                    content.push(AssistantBlock::Text { text: text.clone() });
                }
                AcpPeerContentSlot::Tool { tool_call_id } => {
                    let Some(tool) = self.tools.get(tool_call_id) else {
                        continue;
                    };
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
                _ => {}
            }
        }
        content
    }

    fn persisted_assistant_message_ids(&self) -> Vec<String> {
        self.content_slots
            .iter()
            .filter_map(|slot| match slot {
                AcpPeerContentSlot::Text {
                    message_id: Some(message_id),
                    ..
                } => Some(message_id.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn final_message_content(&self) -> Vec<Value> {
        self.final_content.clone()
    }

    fn persisted_tool_result_messages(&self) -> Vec<Message> {
        self.content_slots
            .iter()
            .filter_map(|slot| match slot {
                AcpPeerContentSlot::Tool { tool_call_id } => self.tools.get(tool_call_id),
                _ => None,
            })
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

#[derive(Debug, Clone, PartialEq)]
enum AcpPeerContentSlot {
    Reasoning { text: String },
    Text {
        text: String,
        message_id: Option<String>,
    },
    Tool { tool_call_id: String },
}

const ACP_MAX_HISTORY_REPLAY_MESSAGES: usize = 256;
const ACP_MAX_HISTORY_REPLAY_MESSAGE_CHARS: usize = 262_144;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AcpHistoryReplayProjection {
    assistant_messages: Vec<AcpHistoryReplayMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AcpHistoryReplayMessage {
    message_id: String,
    text: String,
}

impl AcpHistoryReplayProjection {
    fn reduce_agent_message_chunk(&mut self, chunk: ContentChunk) {
        let Some(message_id) = chunk.message_id.as_ref().map(ToString::to_string) else {
            // Stable ACP permits a missing messageId for tolerant clients, but
            // a replay fact without stable identity cannot safely reconcile or
            // deduplicate durable product history.
            return;
        };
        let Some(text) = acp_content_chunk_text(chunk) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        if let Some(existing) = self
            .assistant_messages
            .iter_mut()
            .find(|message| message.message_id == message_id)
        {
            let remaining = ACP_MAX_HISTORY_REPLAY_MESSAGE_CHARS
                .saturating_sub(existing.text.chars().count());
            existing.text.extend(text.chars().take(remaining));
            return;
        }
        if self.assistant_messages.len() >= ACP_MAX_HISTORY_REPLAY_MESSAGES {
            return;
        }
        self.assistant_messages.push(AcpHistoryReplayMessage {
            message_id,
            text: text
                .chars()
                .take(ACP_MAX_HISTORY_REPLAY_MESSAGE_CHARS)
                .collect(),
        });
    }
}

struct AcpPeerStreamState {
    stream: Option<RunStreamSink>,
    local_session_id: String,
    final_answer: String,
    reasoning_text: String,
    reasoning_open: bool,
    content_slots: Vec<AcpPeerContentSlot>,
    latest_plan: Option<AcpPeerPlanProjection>,
    tool_slots: BTreeMap<String, usize>,
    session_title: Option<String>,
    tools: BTreeMap<String, AcpPeerToolState>,
    usage_update: Option<Value>,
    events: Vec<Value>,
    history_replay: AcpHistoryReplayProjection,
    prompt_active: bool,
}

impl AcpPeerStreamState {
    fn new(stream: Option<RunStreamSink>, local_session_id: String) -> Self {
        Self {
            stream,
            local_session_id,
            final_answer: String::new(),
            reasoning_text: String::new(),
            reasoning_open: false,
            content_slots: Vec::new(),
            latest_plan: None,
            tool_slots: BTreeMap::new(),
            session_title: None,
            tools: BTreeMap::new(),
            usage_update: None,
            events: Vec::new(),
            history_replay: AcpHistoryReplayProjection::default(),
            prompt_active: false,
        }
    }

    fn begin_prompt(&mut self) {
        self.prompt_active = true;
    }

    fn reduce_notification(
        &mut self,
        envelope: AcpPeerInboundNotification,
        origin: AcpFactOrigin,
        generation: u64,
        session_epoch: u64,
    ) {
        let sequence = envelope.sequence;
        let notification = match envelope.payload {
            AcpPeerInboundPayload::Session(notification) => notification,
            AcpPeerInboundPayload::Unknown { method, params } => {
                self.handle_unknown_notification(
                    method,
                    params,
                    origin,
                    generation,
                    session_epoch,
                    sequence,
                );
                return;
            }
            AcpPeerInboundPayload::Barrier => return,
        };
        self.record_notification(&notification, origin, generation, session_epoch, sequence);
        // Replay is reduced into a separate typed projection. It must be
        // durably committed to the prior turn before a new prompt is sent and
        // must never be mistaken for the new turn's assistant output.
        if origin == AcpFactOrigin::History {
            if let SessionUpdate::AgentMessageChunk(chunk) = notification.update {
                self.history_replay.reduce_agent_message_chunk(chunk);
            }
            return;
        }
        // Pre-prompt live facts remain observable, but only facts observed
        // after prompt dispatch may contribute to this turn's assistant result.
        if !self.prompt_active {
            return;
        }
        let update_value = acp_product_json(&notification.update).unwrap_or_else(|err| {
            json!({
                "sessionUpdate": "decode_error",
                "error": err.to_string(),
            })
        });

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

    fn handle_unknown_notification(
        &mut self,
        method: String,
        params: Value,
        origin: AcpFactOrigin,
        generation: u64,
        session_epoch: u64,
        sequence: u64,
    ) {
        let event = json!({
            "type": "acp_peer_unknown_notification",
            "session_id": self.local_session_id.clone(),
            "native_session_id": params.get("sessionId").and_then(Value::as_str),
            "method": method,
            "update_kind": params
                .get("update")
                .and_then(|update| update.get("sessionUpdate"))
                .and_then(Value::as_str),
            "origin": origin.as_str(),
            "process_generation": generation,
            "session_epoch": session_epoch,
            "notification_sequence": sequence,
        });
        self.events.push(event.clone());
        emit_runtime_event(&self.stream, event);
    }

    fn record_notification(
        &mut self,
        notification: &SessionNotification,
        origin: AcpFactOrigin,
        generation: u64,
        session_epoch: u64,
        sequence: u64,
    ) {
        let native_session_id = notification.session_id.to_string();
        let update_value = acp_product_json(&notification.update).unwrap_or_else(|err| {
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
            "origin": origin.as_str(),
            "process_generation": generation,
            "session_epoch": session_epoch,
            "notification_sequence": sequence,
            "update": update_value.clone(),
        });
        self.events.push(event.clone());
        emit_runtime_event(&self.stream, event);
    }

    fn handle_agent_message_chunk(&mut self, chunk: ContentChunk) {
        let message_id = chunk.message_id.as_ref().map(ToString::to_string);
        self.handle_agent_message_chunk_text(acp_content_chunk_text(chunk), message_id);
    }

    fn handle_agent_message_chunk_text(
        &mut self,
        text: Option<String>,
        message_id: Option<String>,
    ) {
        let Some(text) = text else {
            return;
        };
        if text.is_empty() {
            return;
        }
        self.final_answer.push_str(&text);
        self.append_text_slot(text, message_id);
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
        self.append_reasoning_slot(text.clone());
        if let Some(stream) = &self.stream {
            stream(RunStreamEvent::ReasoningDelta { text });
        }
    }

    fn handle_tool_call(&mut self, update_value: Value) {
        let tool_call_id = acp_tool_call_id(&update_value);
        self.ensure_tool_slot(&tool_call_id);
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
        self.ensure_tool_slot(&tool_call_id);
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

    fn append_text_slot(&mut self, text: String, message_id: Option<String>) {
        match self.content_slots.last_mut() {
            Some(AcpPeerContentSlot::Text {
                text: existing,
                message_id: existing_message_id,
            }) if *existing_message_id == message_id => existing.push_str(&text),
            _ => self.content_slots.push(AcpPeerContentSlot::Text {
                text,
                message_id,
            }),
        }
    }

    fn append_reasoning_slot(&mut self, text: String) {
        match self.content_slots.last_mut() {
            Some(AcpPeerContentSlot::Reasoning { text: existing }) => existing.push_str(&text),
            _ => self
                .content_slots
                .push(AcpPeerContentSlot::Reasoning { text }),
        }
    }

    fn ensure_tool_slot(&mut self, tool_call_id: &str) {
        if self.tool_slots.contains_key(tool_call_id) {
            return;
        }
        let slot_index = self.content_slots.len();
        self.content_slots.push(AcpPeerContentSlot::Tool {
            tool_call_id: tool_call_id.to_string(),
        });
        self.tool_slots.insert(tool_call_id.to_string(), slot_index);
    }

    fn handle_plan(&mut self, update_value: Value) {
        let body = acp_plan_body(&update_value);
        self.latest_plan = Some(AcpPeerPlanProjection {
            body: body.clone(),
            update: update_value.clone(),
        });
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

    fn handle_prompt_usage(&mut self, usage: Value) {
        let mut usage = usage;
        strip_acp_reserved_meta(&mut usage);
        self.handle_usage_update(json!({ "usage": usage, "source": "prompt_response" }));
    }

    fn handle_codex_prompt_quota(&mut self, quota: CodexPromptQuotaProjection) {
        let Ok(quota) = serde_json::to_value(quota) else {
            return;
        };
        let usage_update = self
            .usage_update
            .get_or_insert_with(|| json!({ "source": "prompt_response" }));
        if !usage_update.is_object() {
            *usage_update = json!({ "source": "prompt_response" });
        }
        if let Some(object) = usage_update.as_object_mut() {
            object.insert("codexPromptQuota".to_string(), quota.clone());
        }
        let event = json!({
            "type": "acp_peer_codex_prompt_quota",
            "session_id": self.local_session_id.clone(),
            "source": "codex_acp_pack",
            "quota": quota,
        });
        self.events.push(event.clone());
        emit_runtime_event(&self.stream, event);
    }

    fn handle_codex_prompt_quota_rejection(&mut self, rejection: CodexPromptQuotaRejection) {
        let event = json!({
            "type": "acp_peer_capability_metadata_rejected",
            "session_id": self.local_session_id.clone(),
            "pack": "codex",
            "field": "prompt_response._meta.quota",
            "reason": rejection.as_str(),
        });
        self.events.push(event.clone());
        emit_runtime_event(&self.stream, event);
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
        for slot in &self.content_slots {
            match slot {
                AcpPeerContentSlot::Reasoning { text } if !text.trim().is_empty() => {
                    content.push(json!({
                        "type": "reasoning",
                        "text": text,
                        "content_index": content.len(),
                    }));
                }
                AcpPeerContentSlot::Text { text, .. } if !text.trim().is_empty() => {
                    content.push(json!({
                        "type": "text",
                        "text": text,
                        "content_index": content.len(),
                    }));
                }
                _ => {}
            }
        }
        content
    }
}

fn acp_product_json(value: &impl serde::Serialize) -> Result<Value, serde_json::Error> {
    let mut value = serde_json::to_value(value)?;
    strip_acp_reserved_meta(&mut value);
    Ok(value)
}

fn strip_acp_reserved_meta(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("_meta");
            for value in object.values_mut() {
                strip_acp_reserved_meta(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_acp_reserved_meta(value);
            }
        }
        _ => {}
    }
}
