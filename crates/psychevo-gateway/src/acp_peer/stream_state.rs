struct AcpTurnOutput {
    native_session_id: String,
    final_answer: String,
    final_content: Vec<Value>,
    content_slots: Vec<AcpPeerContentSlot>,
    session_title: Option<String>,
    tools: BTreeMap<String, Value>,
    usage_update: Option<Value>,
    events: Vec<Value>,
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
                AcpPeerContentSlot::Text { text } if !text.trim().is_empty() => {
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
    Text { text: String },
    Tool { tool_call_id: String },
}

struct AcpPeerStreamState {
    stream: Option<RunStreamSink>,
    local_session_id: String,
    final_answer: String,
    reasoning_text: String,
    reasoning_open: bool,
    content_slots: Vec<AcpPeerContentSlot>,
    tool_slots: BTreeMap<String, usize>,
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
            content_slots: Vec::new(),
            tool_slots: BTreeMap::new(),
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
        self.append_text_slot(text);
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

    fn append_text_slot(&mut self, text: String) {
        match self.content_slots.last_mut() {
            Some(AcpPeerContentSlot::Text { text: existing }) => existing.push_str(&text),
            _ => self.content_slots.push(AcpPeerContentSlot::Text { text }),
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
        for slot in &self.content_slots {
            match slot {
                AcpPeerContentSlot::Reasoning { text } if !text.trim().is_empty() => {
                    content.push(json!({
                        "type": "reasoning",
                        "text": text,
                        "content_index": content.len(),
                    }));
                }
                AcpPeerContentSlot::Text { text } if !text.trim().is_empty() => {
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
