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
        persisted_assistant_content(&self.content_slots, &self.tools)
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
        persisted_tool_result_messages(&self.content_slots, &self.tools)
    }
}

fn persisted_assistant_content(
    content_slots: &[AcpPeerContentSlot],
    tools: &BTreeMap<String, Value>,
) -> Vec<AssistantBlock> {
    let mut content = Vec::new();
    for slot in content_slots {
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
                let Some(tool) = tools.get(tool_call_id) else {
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

fn persisted_tool_result_messages(
    content_slots: &[AcpPeerContentSlot],
    tools: &BTreeMap<String, Value>,
) -> Vec<Message> {
    content_slots
        .iter()
        .filter_map(|slot| match slot {
            AcpPeerContentSlot::Tool { tool_call_id } => tools.get(tool_call_id),
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
            let content = serde_json::to_string_pretty(&acp_tool_result(tool)).unwrap_or(content);
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

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AcpHistoryReplayProjection {
    entries: Vec<AcpHistoryReplayEntry>,
    lossy: bool,
    active_assistant_index: Option<usize>,
    tool_entry_indices: BTreeMap<String, usize>,
    plan_entry_index: Option<usize>,
    anonymous_fact_sequence: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum AcpHistoryReplayEntry {
    User {
        replay_id: String,
        delivery_message_id: Option<String>,
        text: String,
    },
    Assistant {
        replay_id: String,
        delivery_message_id: Option<String>,
        content_slots: Vec<AcpPeerContentSlot>,
        tools: BTreeMap<String, Value>,
        plan: Option<AcpPeerPlanProjection>,
    },
}

#[derive(Debug)]
pub(crate) struct AcpSessionLoadOutput {
    pub(crate) snapshot: AcpSessionSnapshot,
    pub(crate) replay: AcpHistoryReplayProjection,
}

impl AcpHistoryReplayProjection {
    fn is_complete(&self) -> bool {
        !self.lossy
    }

    fn mark_partial(&mut self) {
        self.lossy = true;
    }

    fn next_anonymous_replay_id(&mut self, role: &str) -> String {
        self.anonymous_fact_sequence = self.anonymous_fact_sequence.saturating_add(1);
        format!("anonymous:{role}:{}", self.anonymous_fact_sequence)
    }

    fn reduce_update(&mut self, update: SessionUpdate, update_value: Value) {
        match update {
            SessionUpdate::UserMessageChunk(chunk) => self.reduce_user_message_chunk(chunk),
            SessionUpdate::AgentMessageChunk(chunk) => {
                self.reduce_assistant_chunk(chunk, false)
            }
            SessionUpdate::AgentThoughtChunk(chunk) => self.reduce_assistant_chunk(chunk, true),
            SessionUpdate::ToolCall(_) | SessionUpdate::ToolCallUpdate(_) => {
                self.reduce_tool(update_value)
            }
            SessionUpdate::Plan(_) => self.reduce_plan(update_value),
            SessionUpdate::AvailableCommandsUpdate(_)
            | SessionUpdate::CurrentModeUpdate(_)
            | SessionUpdate::ConfigOptionUpdate(_)
            | SessionUpdate::SessionInfoUpdate(_)
            | SessionUpdate::UsageUpdate(_) => {}
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    fn reduce_user_message_chunk(&mut self, chunk: ContentChunk) {
        self.active_assistant_index = None;
        let delivery_message_id = replay_message_id(&chunk);
        if delivery_message_id.is_none() {
            // Stable ACP permits a missing messageId for tolerant clients. Keep
            // projectable display content under an internal replay identity,
            // but never promote that identity to delivery evidence.
            self.mark_partial();
        }
        let Some(text) = acp_content_chunk_text(chunk) else {
            self.mark_partial();
            return;
        };
        if text.is_empty() {
            return;
        }
        if let Some(AcpHistoryReplayEntry::User {
            delivery_message_id: existing_message_id,
            text: existing,
            ..
        }) = self.entries.last_mut()
            && delivery_message_id.is_some()
            && existing_message_id == &delivery_message_id
        {
            let remaining = ACP_MAX_HISTORY_REPLAY_MESSAGE_CHARS
                .saturating_sub(existing.chars().count());
            if text.chars().count() > remaining {
                self.lossy = true;
            }
            existing.extend(text.chars().take(remaining));
            return;
        }
        if self.entries.len() >= ACP_MAX_HISTORY_REPLAY_MESSAGES {
            self.mark_partial();
            return;
        }
        if text.chars().count() > ACP_MAX_HISTORY_REPLAY_MESSAGE_CHARS {
            self.mark_partial();
        }
        let replay_id = delivery_message_id
            .clone()
            .unwrap_or_else(|| self.next_anonymous_replay_id("user"));
        self.entries.push(AcpHistoryReplayEntry::User {
            replay_id,
            delivery_message_id,
            text: text
                .chars()
                .take(ACP_MAX_HISTORY_REPLAY_MESSAGE_CHARS)
                .collect(),
        });
    }

    fn reduce_assistant_chunk(&mut self, chunk: ContentChunk, reasoning: bool) {
        let delivery_message_id = replay_message_id(&chunk);
        if delivery_message_id.is_none() {
            self.mark_partial();
        }
        let Some(text) = acp_content_chunk_text(chunk) else {
            self.active_assistant_index = None;
            self.mark_partial();
            return;
        };
        if text.is_empty() {
            return;
        }
        if let Some(active_index) = self.active_assistant_index
            && let Some(AcpHistoryReplayEntry::Assistant {
                delivery_message_id: Some(active_message_id),
                content_slots,
                ..
            }) = self.entries.get_mut(active_index)
            && delivery_message_id.as_deref() == Some(active_message_id.as_str())
        {
            if !append_bounded_replay_slot(content_slots, text, reasoning) {
                self.lossy = true;
            }
            return;
        }
        if self.entries.len() >= ACP_MAX_HISTORY_REPLAY_MESSAGES {
            self.mark_partial();
            return;
        }
        let mut content_slots = Vec::new();
        if !append_bounded_replay_slot(&mut content_slots, text, reasoning) {
            self.mark_partial();
        }
        let replay_id = delivery_message_id
            .clone()
            .unwrap_or_else(|| self.next_anonymous_replay_id("assistant"));
        self.entries.push(AcpHistoryReplayEntry::Assistant {
            replay_id,
            delivery_message_id,
            content_slots,
            tools: BTreeMap::new(),
            plan: None,
        });
        self.active_assistant_index = Some(self.entries.len() - 1);
    }

    fn reduce_tool(&mut self, update: Value) {
        let tool_call_id = acp_tool_call_id(&update);
        if let Some(entry_index) = self.tool_entry_indices.get(&tool_call_id).copied()
            && let Some(AcpHistoryReplayEntry::Assistant {
                content_slots,
                tools,
                ..
            }) = self.entries.get_mut(entry_index)
        {
            if !content_slots.iter().any(|slot| {
                matches!(slot, AcpPeerContentSlot::Tool { tool_call_id: existing } if existing == &tool_call_id)
            }) {
                content_slots.push(AcpPeerContentSlot::Tool {
                    tool_call_id: tool_call_id.clone(),
                });
            }
            let merged = tools
                .get(&tool_call_id)
                .map(|existing| acp_merge_tool_update(existing, &update))
                .unwrap_or(update);
            tools.insert(tool_call_id, merged);
            return;
        }
        let entry_index = match self.active_assistant_index {
            Some(index) if matches!(self.entries.get(index), Some(AcpHistoryReplayEntry::Assistant { plan: None, .. })) => index,
            _ => {
                if self.entries.len() >= ACP_MAX_HISTORY_REPLAY_MESSAGES {
                    self.mark_partial();
                    return;
                }
                self.entries.push(AcpHistoryReplayEntry::Assistant {
                    replay_id: format!("tool:{tool_call_id}"),
                    delivery_message_id: None,
                    content_slots: Vec::new(),
                    tools: BTreeMap::new(),
                    plan: None,
                });
                self.entries.len() - 1
            }
        };
        let Some(AcpHistoryReplayEntry::Assistant {
            content_slots,
            tools,
            ..
        }) = self.entries.get_mut(entry_index)
        else {
            return;
        };
        content_slots.push(AcpPeerContentSlot::Tool {
            tool_call_id: tool_call_id.clone(),
        });
        tools.insert(tool_call_id.clone(), update);
        self.tool_entry_indices.insert(tool_call_id, entry_index);
    }

    fn reduce_plan(&mut self, update: Value) {
        let projection = AcpPeerPlanProjection {
            body: acp_plan_body(&update),
            update,
        };
        if let Some(entry_index) = self.plan_entry_index
            && let Some(AcpHistoryReplayEntry::Assistant { plan, .. }) =
                self.entries.get_mut(entry_index)
        {
            *plan = Some(projection);
            return;
        }
        if self.entries.len() >= ACP_MAX_HISTORY_REPLAY_MESSAGES {
            self.mark_partial();
            return;
        }
        self.entries.push(AcpHistoryReplayEntry::Assistant {
            replay_id: "plan:legacy-v1".to_string(),
            delivery_message_id: None,
            content_slots: Vec::new(),
            tools: BTreeMap::new(),
            plan: Some(projection),
        });
        self.plan_entry_index = Some(self.entries.len() - 1);
    }
}

fn replay_message_id(chunk: &ContentChunk) -> Option<String> {
    chunk
        .message_id
        .as_ref()
        .map(ToString::to_string)
        .filter(|message_id| !message_id.trim().is_empty())
}

fn replay_entry_identity(entry: &AcpHistoryReplayEntry) -> &str {
    match entry {
        AcpHistoryReplayEntry::User { replay_id, .. }
        | AcpHistoryReplayEntry::Assistant { replay_id, .. } => replay_id,
    }
}

fn replay_entry_delivery_message_ids(entry: &AcpHistoryReplayEntry) -> Vec<String> {
    match entry {
        AcpHistoryReplayEntry::User {
            delivery_message_id: Some(message_id),
            ..
        } => vec![message_id.clone()],
        AcpHistoryReplayEntry::User {
            delivery_message_id: None,
            ..
        } => Vec::new(),
        AcpHistoryReplayEntry::Assistant {
            delivery_message_id: Some(message_id),
            ..
        } => vec![message_id.clone()],
        AcpHistoryReplayEntry::Assistant {
            delivery_message_id: None,
            ..
        } => Vec::new(),
    }
}

fn append_bounded_replay_slot(
    content_slots: &mut Vec<AcpPeerContentSlot>,
    text: String,
    reasoning: bool,
) -> bool {
    let used = content_slots
        .iter()
        .map(|slot| match slot {
            AcpPeerContentSlot::Reasoning { text }
            | AcpPeerContentSlot::Text { text, .. } => text.chars().count(),
            AcpPeerContentSlot::Tool { .. } => 0,
        })
        .sum::<usize>();
    let remaining = ACP_MAX_HISTORY_REPLAY_MESSAGE_CHARS.saturating_sub(used);
    let lossless = text.chars().count() <= remaining;
    let bounded = text
        .chars()
        .take(remaining)
        .collect::<String>();
    if bounded.is_empty() {
        return lossless;
    }
    match (reasoning, content_slots.last_mut()) {
        (true, Some(AcpPeerContentSlot::Reasoning { text })) => text.push_str(&bounded),
        (
            false,
            Some(AcpPeerContentSlot::Text {
                text,
                message_id: _,
            }),
        ) => text.push_str(&bounded),
        (true, _) => content_slots.push(AcpPeerContentSlot::Reasoning { text: bounded }),
        (false, _) => content_slots.push(AcpPeerContentSlot::Text {
            text: bounded,
            message_id: None,
        }),
    }
    lossless
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
            let update_value = acp_product_json(&notification.update).unwrap_or_else(|err| {
                json!({
                    "sessionUpdate": "decode_error",
                    "error": err.to_string(),
                })
            });
            self.history_replay
                .reduce_update(notification.update, update_value);
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
