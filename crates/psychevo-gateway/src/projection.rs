use std::collections::BTreeMap;

use psychevo_runtime::{RunStreamEvent, RunWarning};
use serde_json::{Value, json};

use crate::protocol::{
    GatewayEvent, GatewaySelectedSkill, TranscriptBlock, TranscriptBlockKind,
    TranscriptBlockStatus, TranscriptEntry, TranscriptEntryRole,
};

#[derive(Debug, Default)]
pub struct GatewayLiveProjector {
    thread_id: Option<String>,
    assistant_segment: usize,
    stream_seq: u64,
    entries: BTreeMap<usize, LiveEntryState>,
    tool_owners: BTreeMap<String, usize>,
    tool_aliases: BTreeMap<String, String>,
    tool_args: BTreeMap<String, Value>,
    exec_sessions: BTreeMap<u64, LiveExecState>,
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

struct AssistantContentProjection<'a> {
    turn_id: &'a str,
    event_value: &'a Value,
    content_block: &'a Value,
    index: usize,
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

impl GatewayLiveProjector {
    pub fn new(thread_id: Option<String>) -> Self {
        Self {
            thread_id,
            assistant_segment: 0,
            stream_seq: 0,
            entries: BTreeMap::new(),
            tool_owners: BTreeMap::new(),
            tool_aliases: BTreeMap::new(),
            tool_args: BTreeMap::new(),
            exec_sessions: BTreeMap::new(),
        }
    }

    pub fn project(&mut self, turn_id: &str, event: &RunStreamEvent) -> Option<GatewayEvent> {
        let mut event = match event {
            RunStreamEvent::ReasoningDelta { text } => {
                self.project_reasoning_delta(turn_id, text)?
            }
            RunStreamEvent::ReasoningEnd => self.project_reasoning_end(turn_id)?,
            RunStreamEvent::Scoped { event, .. } => return self.project(turn_id, event),
            RunStreamEvent::Event(value) => self
                .project_runtime_value(turn_id, value)
                .or_else(|| gateway_event_from_runtime_value(turn_id, value))?,
            _ => gateway_event_from_run_stream(turn_id, event)?,
        };
        self.attach_thread_id(&mut event);
        Some(event)
    }

    fn project_runtime_value(&mut self, turn_id: &str, value: &Value) -> Option<GatewayEvent> {
        match value.get("type").and_then(Value::as_str) {
            Some("message_update")
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
            _ => None,
        }
    }

    fn project_tool_event(&mut self, turn_id: &str, value: &Value) -> Option<GatewayEvent> {
        let tool_name = tool_name_from_value(value);
        let raw_tool_call_id = tool_call_id_from_value(value, tool_name);
        let args = tool_args_from_value(value);
        if !raw_tool_call_id.is_empty()
            && let Some(args) = args.clone()
        {
            self.tool_args.insert(raw_tool_call_id.to_string(), args);
        }
        let tool_call_id = self.canonical_tool_call_id(raw_tool_call_id, tool_name, args.as_ref());
        if tool_call_id != raw_tool_call_id
            && let Some(args) = args.clone()
        {
            self.tool_args.insert(tool_call_id.clone(), args);
        }

        match (value.get("type").and_then(Value::as_str), tool_name) {
            (Some("tool_execution_end"), "exec_command")
                if exec_session_id_from_result_value(value).is_some()
                    && exec_result_running_value(value) =>
            {
                self.project_yielded_exec_update(turn_id, value, &tool_call_id)
            }
            (
                Some("tool_call_pending" | "tool_execution_start" | "tool_execution_update"),
                "write_stdin",
            ) => None,
            (Some("tool_execution_end"), "write_stdin") if !tool_event_failed(value) => {
                self.project_write_stdin_success(turn_id, &tool_call_id, value)
            }
            _ => Some(self.project_visible_tool_event(turn_id, value, &tool_call_id)),
        }
    }

    fn project_yielded_exec_update(
        &mut self,
        turn_id: &str,
        value: &Value,
        tool_call_id: &str,
    ) -> Option<GatewayEvent> {
        let session_id = exec_session_id_from_result_value(value).expect("checked session id");
        let segment = self.tool_owner_segment(tool_call_id);
        let mut metadata = tool_value_metadata(value);
        set_metadata_field(&mut metadata, "tool_call_id", json!(tool_call_id));
        if metadata.get("args").is_none_or(Value::is_null)
            && let Some(args) = self.tool_args.get(tool_call_id)
        {
            set_metadata_field(&mut metadata, "args", args.clone());
        }
        if metadata.get("args").is_none_or(Value::is_null)
            && let Some(args) = self
                .exec_sessions
                .get(&session_id)
                .and_then(|state| state.metadata.get("args"))
                .filter(|args| !args.is_null())
                .cloned()
        {
            set_metadata_field(&mut metadata, "args", args);
        }
        let output = tool_result_output_value(&metadata);
        let (tool_call_id, metadata) = {
            let state = self
                .exec_sessions
                .entry(session_id)
                .or_insert_with(|| LiveExecState {
                    tool_call_id: tool_call_id.to_string(),
                    segment,
                    metadata: metadata.clone(),
                    output: String::new(),
                });
            state.tool_call_id = tool_call_id.to_string();
            state.segment = segment;
            state.metadata = metadata;
            merge_output(&mut state.output, &output);
            set_metadata_result_field(&mut state.metadata, "session_id", json!(session_id));
            set_metadata_result_field(&mut state.metadata, "output", json!(state.output));
            (state.tool_call_id.clone(), state.metadata.clone())
        };
        Some(self.project_tool_block_from_metadata(LiveToolBlockUpdate {
            turn_id,
            segment,
            tool_call_id: &tool_call_id,
            tool_name: "exec_command",
            status: TranscriptBlockStatus::Running,
            body: result_body_from_metadata(&metadata),
            metadata,
            completed: false,
        }))
    }

    fn project_write_stdin_success(
        &mut self,
        turn_id: &str,
        tool_call_id: &str,
        value: &Value,
    ) -> Option<GatewayEvent> {
        let target_session_id = self
            .tool_args
            .get(tool_call_id)
            .and_then(exec_session_id_from_args_value)
            .or_else(|| exec_session_id_from_result_value(value));
        let session_id = target_session_id?;
        let state = self.exec_sessions.get_mut(&session_id)?;

        let (segment, root_tool_call_id, metadata, status) = {
            let output = tool_result_output_runtime(value);
            merge_output(&mut state.output, &output);
            set_metadata_result_field(&mut state.metadata, "session_id", json!(session_id));
            set_metadata_result_field(&mut state.metadata, "output", json!(state.output));
            if let Some(exit_code) = value
                .get("result")
                .and_then(|result| result.get("exit_code"))
                .filter(|exit_code| !exit_code.is_null())
            {
                set_metadata_result_field(&mut state.metadata, "exit_code", exit_code.clone());
            }
            if let Some(outcome) = value.get("outcome") {
                set_metadata_field(&mut state.metadata, "outcome", outcome.clone());
            }

            let status = if exec_result_completed_value(&state.metadata) {
                TranscriptBlockStatus::Completed
            } else {
                TranscriptBlockStatus::Running
            };
            (
                state.segment,
                state.tool_call_id.clone(),
                state.metadata.clone(),
                status,
            )
        };
        if status == TranscriptBlockStatus::Completed {
            self.exec_sessions.remove(&session_id);
        }
        Some(self.project_tool_block_from_metadata(LiveToolBlockUpdate {
            turn_id,
            segment,
            tool_call_id: &root_tool_call_id,
            tool_name: "exec_command",
            status,
            body: result_body_from_metadata(&metadata),
            metadata,
            completed: status == TranscriptBlockStatus::Completed,
        }))
    }

    fn project_reasoning_delta(&mut self, turn_id: &str, text: &str) -> Option<GatewayEvent> {
        if text.is_empty() {
            return None;
        }
        let segment = self.assistant_segment;
        let block_id = live_reasoning_block_id(turn_id, segment);
        let current = self
            .entries
            .get(&segment)
            .and_then(|state| state.blocks.get(&block_id))
            .and_then(|block| block.body.as_deref())
            .unwrap_or_default();
        let body = format!("{current}{text}");
        let block = live_block(
            block_id,
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Running,
            DEFAULT_REASONING_ORDER,
            Some("Thinking".to_string()),
            Some(body),
            Some(json!({
                "projection": "reasoning",
                "origin": "run_stream_reasoning",
                "liveOrder": DEFAULT_REASONING_ORDER,
            })),
        );
        self.upsert_block(segment, block);
        Some(self.emit_entry_event(turn_id, segment, false, false))
    }

    fn project_reasoning_end(&mut self, turn_id: &str) -> Option<GatewayEvent> {
        let segment = self.assistant_segment;
        let block_id = live_reasoning_block_id(turn_id, segment);
        let body = self
            .entries
            .get(&segment)
            .and_then(|state| state.blocks.get(&block_id))
            .and_then(|block| block.body.clone())
            .filter(|body| !body.trim().is_empty())?;
        let block = live_block(
            block_id,
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Completed,
            DEFAULT_REASONING_ORDER,
            Some("Thinking".to_string()),
            Some(body),
            Some(json!({
                "projection": "reasoning",
                "origin": "run_stream_reasoning",
                "liveOrder": DEFAULT_REASONING_ORDER,
            })),
        );
        self.upsert_block(segment, block);
        Some(self.emit_entry_event(turn_id, segment, false, false))
    }

    fn project_assistant_message_event(
        &mut self,
        turn_id: &str,
        value: &Value,
        status: TranscriptBlockStatus,
        completed: bool,
    ) -> Option<GatewayEvent> {
        let message = value.get("message")?;
        let segment = self.assistant_segment;
        let is_tool_call_turn = assistant_message_is_tool_call_turn(Some(message));
        let mut visible = false;
        let content = message.get("content").and_then(Value::as_array);
        if let Some(content) = content {
            if completed {
                visible = self.replace_assistant_content_blocks(
                    turn_id,
                    value,
                    content,
                    segment,
                    status,
                    is_tool_call_turn,
                );
            } else {
                for (index, content_block) in content.iter().enumerate() {
                    visible |= self.project_assistant_content_block(AssistantContentProjection {
                        turn_id,
                        event_value: value,
                        content_block,
                        index,
                        segment,
                        status,
                        is_tool_call_turn,
                    });
                }
            }
        }
        if !visible {
            return None;
        }
        Some(self.emit_entry_event(turn_id, segment, completed, completed))
    }

    fn project_assistant_content_block(
        &mut self,
        projection: AssistantContentProjection<'_>,
    ) -> bool {
        let segment = projection.segment;
        let Some(block) = self.build_assistant_content_block(projection) else {
            return false;
        };
        self.upsert_block(segment, block);
        true
    }

    fn replace_assistant_content_blocks(
        &mut self,
        turn_id: &str,
        event_value: &Value,
        content: &[Value],
        segment: usize,
        status: TranscriptBlockStatus,
        is_tool_call_turn: bool,
    ) -> bool {
        let mut blocks = BTreeMap::new();
        for (index, content_block) in content.iter().enumerate() {
            let Some(block) = self.build_assistant_content_block(AssistantContentProjection {
                turn_id,
                event_value,
                content_block,
                index,
                segment,
                status,
                is_tool_call_turn,
            }) else {
                continue;
            };
            blocks.insert(block.id.clone(), block);
        }
        if !blocks.is_empty()
            && !blocks
                .values()
                .any(|block| block.kind == TranscriptBlockKind::Reasoning)
            && let Some(reasoning) = self.preserved_run_stream_reasoning_block(segment)
        {
            blocks.insert(reasoning.id.clone(), reasoning);
        }
        if blocks.is_empty() {
            return false;
        }
        self.replace_blocks(segment, blocks);
        true
    }

    fn build_assistant_content_block(
        &mut self,
        projection: AssistantContentProjection<'_>,
    ) -> Option<TranscriptBlock> {
        let AssistantContentProjection {
            turn_id,
            event_value,
            content_block,
            index,
            segment,
            status,
            is_tool_call_turn,
        } = projection;
        match content_block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = content_block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToString::to_string)?;
                let order = content_block_order(content_block, index, index as i64);
                let mut metadata = if is_tool_call_turn {
                    assistant_phase_metadata(event_value)
                } else {
                    assistant_message_metadata(event_value)
                };
                set_metadata_field(&mut metadata, "content_array_index", json!(index));
                set_metadata_field(&mut metadata, "liveOrder", json!(order));
                Some(live_block(
                    live_text_block_id(turn_id, segment, index),
                    TranscriptBlockKind::Text,
                    status,
                    order,
                    None,
                    Some(text),
                    Some(metadata),
                ))
            }
            Some("reasoning") => {
                let text = content_block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToString::to_string)?;
                let order = content_block_order(content_block, index, DEFAULT_REASONING_ORDER);
                Some(live_block(
                    live_reasoning_block_id(turn_id, segment),
                    TranscriptBlockKind::Reasoning,
                    status,
                    order,
                    Some("Thinking".to_string()),
                    Some(text),
                    Some(json!({
                        "projection": "reasoning",
                        "content_array_index": index,
                        "liveOrder": order,
                    })),
                ))
            }
            Some("tool_call" | "tool_calls" | "tool_use") => {
                let (tool_call_id, tool_name, metadata) =
                    tool_message_block_metadata(content_block, index)?;
                if let Some(args) = metadata.get("args").cloned() {
                    self.tool_args.insert(tool_call_id.clone(), args);
                }
                if tool_name == "write_stdin" {
                    return None;
                }
                self.tool_owners.insert(tool_call_id.clone(), segment);
                let order = content_block_order(content_block, index, index as i64);
                Some(self.live_tool_block_from_metadata(LiveToolBlockBuild {
                    turn_id,
                    segment,
                    tool_call_id: &tool_call_id,
                    tool_name: &tool_name,
                    status: TranscriptBlockStatus::Pending,
                    body: None,
                    metadata,
                    order: Some(order),
                }))
            }
            _ => None,
        }
    }

    fn preserved_run_stream_reasoning_block(&self, segment: usize) -> Option<TranscriptBlock> {
        self.entries
            .get(&segment)?
            .blocks
            .values()
            .find(|block| {
                block.kind == TranscriptBlockKind::Reasoning
                    && block
                        .body
                        .as_deref()
                        .or(block.detail.as_deref())
                        .or(block.preview.as_deref())
                        .is_some_and(|body| !body.trim().is_empty())
                    && block
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("origin"))
                        .and_then(Value::as_str)
                        == Some("run_stream_reasoning")
            })
            .cloned()
            .map(|mut block| {
                block.status = TranscriptBlockStatus::Completed;
                block.updated_at_ms = crate::gateway_now_ms();
                block
            })
    }

    fn project_visible_tool_event(
        &mut self,
        turn_id: &str,
        value: &Value,
        tool_call_id: &str,
    ) -> GatewayEvent {
        let tool_name = tool_name_from_value(value);
        let status = match value.get("type").and_then(Value::as_str) {
            Some("tool_call_pending") => TranscriptBlockStatus::Pending,
            Some("tool_execution_start" | "tool_execution_update") => {
                TranscriptBlockStatus::Running
            }
            Some("tool_execution_end")
                if value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .is_some_and(|outcome| outcome != "normal") =>
            {
                TranscriptBlockStatus::Failed
            }
            Some("tool_execution_end") => TranscriptBlockStatus::Completed,
            _ => TranscriptBlockStatus::Info,
        };
        let body = match value.get("type").and_then(Value::as_str) {
            Some("tool_execution_update") => value.get("partial_result").and_then(json_preview),
            Some("tool_execution_end") => value.get("result").and_then(json_preview),
            _ => None,
        };
        let segment = self.tool_owner_segment(tool_call_id);
        let mut metadata = tool_value_metadata(value);
        set_metadata_field(&mut metadata, "tool_call_id", json!(tool_call_id));
        self.project_tool_block_from_metadata(LiveToolBlockUpdate {
            turn_id,
            segment,
            tool_call_id,
            tool_name,
            status,
            body,
            metadata,
            completed: matches!(
                status,
                TranscriptBlockStatus::Completed
                    | TranscriptBlockStatus::Failed
                    | TranscriptBlockStatus::Cancelled
            ),
        })
    }

    fn canonical_tool_call_id(
        &mut self,
        raw_tool_call_id: &str,
        tool_name: &str,
        args: Option<&Value>,
    ) -> String {
        if raw_tool_call_id.is_empty() || tool_name == "write_stdin" {
            return raw_tool_call_id.to_string();
        }
        if let Some(canonical) = self.tool_aliases.get(raw_tool_call_id) {
            return canonical.clone();
        }
        if self.tool_owners.contains_key(raw_tool_call_id) {
            return raw_tool_call_id.to_string();
        }
        let Some(args) = args else {
            return raw_tool_call_id.to_string();
        };
        let candidates = self.matching_open_tool_candidates(tool_name, args);
        if candidates.len() != 1 {
            return raw_tool_call_id.to_string();
        }
        let (canonical, segment) = candidates[0].clone();
        self.tool_aliases
            .insert(raw_tool_call_id.to_string(), canonical.clone());
        self.tool_owners
            .insert(raw_tool_call_id.to_string(), segment);
        canonical
    }

    fn matching_open_tool_candidates(&self, tool_name: &str, args: &Value) -> Vec<(String, usize)> {
        let mut candidates = Vec::new();
        for (segment, state) in &self.entries {
            for block in state.blocks.values() {
                if !matches!(
                    block.status,
                    TranscriptBlockStatus::Pending | TranscriptBlockStatus::Running
                ) {
                    continue;
                }
                let Some(metadata) = block.metadata.as_ref() else {
                    continue;
                };
                if metadata
                    .get("projection")
                    .and_then(Value::as_str)
                    .is_some_and(|projection| projection != "tool")
                {
                    continue;
                }
                if metadata.get("tool_name").and_then(Value::as_str) != Some(tool_name) {
                    continue;
                }
                let Some(candidate_id) = metadata.get("tool_call_id").and_then(Value::as_str)
                else {
                    continue;
                };
                let Some(candidate_args) =
                    metadata.get("args").or_else(|| metadata.get("arguments"))
                else {
                    continue;
                };
                if candidate_args == args {
                    candidates.push((candidate_id.to_string(), *segment));
                }
            }
        }
        candidates
    }

    fn project_tool_block_from_metadata(
        &mut self,
        update: LiveToolBlockUpdate<'_>,
    ) -> GatewayEvent {
        let turn_id = update.turn_id;
        let segment = update.segment;
        let completed = update.completed;
        let block = self.live_tool_block_from_metadata(LiveToolBlockBuild {
            turn_id: update.turn_id,
            segment: update.segment,
            tool_call_id: update.tool_call_id,
            tool_name: update.tool_name,
            status: update.status,
            body: update.body,
            metadata: update.metadata,
            order: None,
        });
        self.upsert_block(segment, block);
        self.emit_entry_event(turn_id, segment, completed, false)
    }

    fn live_tool_block_from_metadata(&mut self, build: LiveToolBlockBuild<'_>) -> TranscriptBlock {
        let order = build
            .order
            .unwrap_or_else(|| self.tool_block_order(build.segment, build.tool_call_id));
        let title = live_tool_title(build.tool_name, &build.metadata);
        live_block(
            live_tool_block_id(build.turn_id, build.tool_call_id),
            tool_kind(build.tool_name),
            build.status,
            order,
            Some(title),
            build.body,
            Some(build.metadata),
        )
    }

    fn tool_owner_segment(&mut self, tool_call_id: &str) -> usize {
        if let Some(segment) = self.tool_owners.get(tool_call_id).copied() {
            return segment;
        }
        let segment = self.assistant_segment;
        if !tool_call_id.is_empty() {
            self.tool_owners.insert(tool_call_id.to_string(), segment);
        }
        segment
    }

    fn tool_block_order(&mut self, segment: usize, tool_call_id: &str) -> i64 {
        if let Some(order) = self
            .entries
            .get(&segment)
            .and_then(|state| state.tool_block_order(tool_call_id))
        {
            return order;
        }
        let state = self.entry_state_mut(segment);
        let order = state.next_placeholder_order;
        state.next_placeholder_order += 1;
        order
    }

    fn upsert_block(&mut self, segment: usize, block: TranscriptBlock) {
        self.entry_state_mut(segment).upsert_block(block);
    }

    fn replace_blocks(&mut self, segment: usize, blocks: BTreeMap<String, TranscriptBlock>) {
        self.entry_state_mut(segment).replace_blocks(blocks);
    }

    fn entry_state_mut(&mut self, segment: usize) -> &mut LiveEntryState {
        self.entries
            .entry(segment)
            .or_insert_with(|| LiveEntryState::new(segment))
    }

    fn emit_entry_event(
        &mut self,
        turn_id: &str,
        segment: usize,
        completed: bool,
        authoritative_blocks: bool,
    ) -> GatewayEvent {
        self.stream_seq += 1;
        let stream_seq = self.stream_seq;
        let state = self.entry_state_mut(segment);
        let was_started = state.started;
        state.started = true;
        state.updated_at_ms = crate::gateway_now_ms();
        let entry = state.to_entry(turn_id, stream_seq, authoritative_blocks);
        if completed {
            GatewayEvent::EntryCompleted {
                turn_id: turn_id.to_string(),
                entry,
            }
        } else if !was_started {
            GatewayEvent::EntryStarted {
                turn_id: turn_id.to_string(),
                entry,
            }
        } else {
            GatewayEvent::EntryUpdated {
                turn_id: turn_id.to_string(),
                entry,
            }
        }
    }

    fn advance_assistant_segment(&mut self) {
        self.assistant_segment += 1;
    }

    fn attach_thread_id(&mut self, event: &mut GatewayEvent) {
        if self.thread_id.is_none()
            && let Some(thread_id) = event_thread_id(event)
        {
            self.thread_id = Some(thread_id);
        }
        let Some(thread_id) = self.thread_id.as_deref() else {
            return;
        };
        match event {
            GatewayEvent::EntryStarted { entry, .. }
            | GatewayEvent::EntryUpdated { entry, .. }
            | GatewayEvent::EntryCompleted { entry, .. } => {
                if entry.thread_id.is_empty() {
                    entry.thread_id = thread_id.to_string();
                }
            }
            _ => {}
        }
    }
}

const DEFAULT_REASONING_ORDER: i64 = 0;
const DEFAULT_TEXT_ORDER: i64 = 100;
const TOOL_PLACEHOLDER_ORDER: i64 = 1000;

impl LiveEntryState {
    fn new(segment: usize) -> Self {
        let now = crate::gateway_now_ms();
        Self {
            segment,
            started: false,
            created_at_ms: now,
            updated_at_ms: now,
            next_placeholder_order: TOOL_PLACEHOLDER_ORDER,
            blocks: BTreeMap::new(),
        }
    }

    fn upsert_block(&mut self, block: TranscriptBlock) {
        let block = self
            .blocks
            .get(&block.id)
            .map(|existing| merge_live_block(existing, block.clone()))
            .unwrap_or(block);
        self.updated_at_ms = block.updated_at_ms;
        self.blocks.insert(block.id.clone(), block);
    }

    fn replace_blocks(&mut self, blocks: BTreeMap<String, TranscriptBlock>) {
        let mut replaced = BTreeMap::new();
        for (id, block) in blocks {
            let block = self
                .blocks
                .get(&id)
                .map(|existing| merge_live_block(existing, block.clone()))
                .unwrap_or(block);
            replaced.insert(id, block);
        }
        self.blocks = replaced;
        self.updated_at_ms = crate::gateway_now_ms();
    }

    fn tool_block_order(&self, tool_call_id: &str) -> Option<i64> {
        self.blocks.values().find_map(|block| {
            block
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("tool_call_id"))
                .and_then(Value::as_str)
                .is_some_and(|candidate| candidate == tool_call_id)
                .then_some(block.order)
        })
    }

    fn to_entry(
        &self,
        turn_id: &str,
        stream_seq: u64,
        authoritative_blocks: bool,
    ) -> TranscriptEntry {
        let mut blocks = self.blocks.values().cloned().collect::<Vec<_>>();
        blocks.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.created_at_ms.cmp(&right.created_at_ms))
                .then_with(|| left.id.cmp(&right.id))
        });
        TranscriptEntry {
            id: live_assistant_entry_id(turn_id, self.segment),
            thread_id: String::new(),
            turn_id: Some(turn_id.to_string()),
            message_seq: None,
            role: TranscriptEntryRole::Assistant,
            status: aggregate_entry_status(&blocks),
            source: "runtime.stream".to_string(),
            blocks,
            metadata: Some(json!({
                "projection": "assistant_segment",
                "liveOrder": self.segment,
                "streamSeq": stream_seq,
                "authoritativeBlocks": authoritative_blocks,
            })),
            usage: None,
            accounting: None,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

fn merge_live_block(existing: &TranscriptBlock, next: TranscriptBlock) -> TranscriptBlock {
    TranscriptBlock {
        id: existing.id.clone(),
        kind: next.kind,
        status: merge_live_block_status(existing.status, next.status),
        order: next.order,
        source: if next.source.trim().is_empty() {
            existing.source.clone()
        } else {
            next.source
        },
        title: next.title.or_else(|| existing.title.clone()),
        body: next.body.or_else(|| existing.body.clone()),
        preview: next.preview.or_else(|| existing.preview.clone()),
        detail: next.detail.or_else(|| existing.detail.clone()),
        artifact_ids: if next.artifact_ids.is_empty() {
            existing.artifact_ids.clone()
        } else {
            next.artifact_ids
        },
        metadata: merge_json_metadata(existing.metadata.clone(), next.metadata),
        result: next.result.or_else(|| existing.result.clone()),
        created_at_ms: existing.created_at_ms,
        updated_at_ms: next.updated_at_ms,
    }
}

fn merge_live_block_status(
    existing: TranscriptBlockStatus,
    next: TranscriptBlockStatus,
) -> TranscriptBlockStatus {
    if next == TranscriptBlockStatus::Pending && existing != TranscriptBlockStatus::Pending {
        existing
    } else {
        next
    }
}

fn merge_json_metadata(left: Option<Value>, right: Option<Value>) -> Option<Value> {
    match (left, right) {
        (Some(Value::Object(mut left)), Some(Value::Object(right))) => {
            for (key, value) in right {
                left.insert(key, value);
            }
            Some(Value::Object(left))
        }
        (_, Some(right)) => Some(right),
        (Some(left), None) => Some(left),
        (None, None) => None,
    }
}

fn aggregate_entry_status(blocks: &[TranscriptBlock]) -> TranscriptBlockStatus {
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::Failed)
    {
        return TranscriptBlockStatus::Failed;
    }
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::Cancelled)
    {
        return TranscriptBlockStatus::Cancelled;
    }
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::NeedsInput)
    {
        return TranscriptBlockStatus::NeedsInput;
    }
    if blocks.iter().any(|block| {
        matches!(
            block.status,
            TranscriptBlockStatus::Running | TranscriptBlockStatus::Pending
        )
    }) {
        return TranscriptBlockStatus::Running;
    }
    TranscriptBlockStatus::Completed
}

fn live_block(
    id: String,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    order: i64,
    title: Option<String>,
    body: Option<String>,
    metadata: Option<Value>,
) -> TranscriptBlock {
    let now = crate::gateway_now_ms();
    TranscriptBlock {
        id,
        kind,
        status,
        order,
        source: "runtime.stream".to_string(),
        title,
        preview: body.as_deref().map(|text| compact_text(text, 240)),
        detail: body.clone(),
        body,
        artifact_ids: Vec::new(),
        metadata,
        result: None,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn live_assistant_entry_id(turn_id: &str, segment: usize) -> String {
    format!("live:{turn_id}:assistant:{segment}")
}

fn live_reasoning_block_id(turn_id: &str, segment: usize) -> String {
    format!("live:{turn_id}:assistant:{segment}:reasoning")
}

fn live_text_block_id(turn_id: &str, segment: usize, index: usize) -> String {
    format!("live:{turn_id}:assistant:{segment}:text:{index}")
}

fn live_tool_block_id(turn_id: &str, tool_call_id: &str) -> String {
    format!("live:{turn_id}:tool:{tool_call_id}")
}

fn content_block_order(block: &Value, _index: usize, fallback: i64) -> i64 {
    block
        .get("content_index")
        .or_else(|| block.get("content_array_index"))
        .and_then(Value::as_i64)
        .unwrap_or(fallback)
}

fn tool_message_block_metadata(block: &Value, index: usize) -> Option<(String, String, Value)> {
    let tool_name = block
        .get("name")
        .or_else(|| block.get("tool_name"))
        .or_else(|| {
            block
                .get("function")
                .and_then(|function| function.get("name"))
        })
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string();
    let tool_call_id = block
        .get("id")
        .or_else(|| block.get("tool_call_id"))
        .or_else(|| block.get("call_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("{tool_name}:{index}"));
    if tool_call_id.trim().is_empty() {
        return None;
    }
    let arguments_json = block
        .get("arguments_json")
        .or_else(|| {
            block
                .get("function")
                .and_then(|function| function.get("arguments"))
        })
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let args = block
        .get("arguments")
        .or_else(|| block.get("args"))
        .cloned()
        .or_else(|| {
            arguments_json
                .as_deref()
                .and_then(|raw| serde_json::from_str(raw).ok())
        })
        .unwrap_or(Value::Null);
    let order = content_block_order(block, index, index as i64);
    let mut metadata = json!({
        "projection": "tool",
        "tool_name": tool_name.clone(),
        "tool_call_id": tool_call_id.clone(),
        "content_array_index": index,
        "liveOrder": order,
        "arguments": args.clone(),
        "args": args,
        "outcome": "normal",
    });
    if let Some(arguments_json) = arguments_json {
        set_metadata_field(&mut metadata, "arguments_json", json!(arguments_json));
    }
    for key in ["content_index", "call_index", "arguments_error"] {
        if let Some(value) = block.get(key) {
            set_metadata_field(&mut metadata, key, value.clone());
        }
    }
    Some((tool_call_id, tool_name, metadata))
}

fn event_thread_id(event: &GatewayEvent) -> Option<String> {
    match event {
        GatewayEvent::TurnStarted {
            thread_id: Some(thread_id),
            ..
        }
        | GatewayEvent::TurnQueued {
            thread_id: Some(thread_id),
            ..
        }
        | GatewayEvent::TurnCompleted {
            thread_id: Some(thread_id),
            ..
        } => Some(thread_id.clone()),
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. }
            if !entry.thread_id.is_empty() =>
        {
            Some(entry.thread_id.clone())
        }
        _ => None,
    }
}

fn runtime_message_role(message: Option<&Value>) -> Option<&str> {
    message
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
}

pub fn gateway_event_from_run_stream(
    turn_id: &str,
    event: &RunStreamEvent,
) -> Option<GatewayEvent> {
    Some(match event {
        RunStreamEvent::ReasoningDelta { text } => GatewayEvent::EntryDelta {
            turn_id: turn_id.to_string(),
            entry_id: None,
            block_id: None,
            delta: text.clone(),
        },
        RunStreamEvent::ClarifyRequest(request) => GatewayEvent::ClarifyRequested {
            request_id: request.call_id.clone(),
            raw: serde_json::to_value(request).unwrap_or(Value::Null),
        },
        RunStreamEvent::ClarifyResolved(resolved) => GatewayEvent::ClarifyResolved {
            request_id: resolved.call_id.clone(),
            reason: format!("{:?}", resolved.reason),
        },
        RunStreamEvent::Scoped { event, .. } => {
            return gateway_event_from_run_stream(turn_id, event);
        }
        RunStreamEvent::Event(value) => return gateway_event_from_runtime_value(turn_id, value),
        RunStreamEvent::ReasoningEnd => return None,
    })
}

fn gateway_event_from_runtime_value(turn_id: &str, value: &Value) -> Option<GatewayEvent> {
    Some(match value.get("type").and_then(Value::as_str) {
        Some("run_start") | Some("agent_start") | Some("task_started") | Some("turn_started") => {
            GatewayEvent::TurnStarted {
                thread_id: value
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                turn_id: turn_id.to_string(),
                selected_skills: selected_skills_from_value(value),
            }
        }
        Some("task_complete") | Some("turn_complete") | Some("agent_end") => {
            GatewayEvent::TurnCompleted {
                thread_id: value
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                turn_id: turn_id.to_string(),
                outcome: value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                committed_entries: Vec::new(),
            }
        }
        Some("message_update") => {
            let message = value.get("message");
            if runtime_message_role(message) == Some("assistant") {
                let is_preamble = assistant_message_is_tool_call_turn(message);
                let text = message_text(message);
                if is_preamble && text.is_none() {
                    return None;
                }
                GatewayEvent::EntryUpdated {
                    turn_id: turn_id.to_string(),
                    entry: live_entry(
                        turn_id,
                        "assistant",
                        TranscriptEntryRole::Assistant,
                        TranscriptBlockKind::Text,
                        TranscriptBlockStatus::Running,
                        None,
                        text,
                        Some(if is_preamble {
                            assistant_phase_metadata(value)
                        } else {
                            assistant_message_metadata(value)
                        }),
                    ),
                }
            } else {
                return None;
            }
        }
        Some("message_end") => {
            let message = value.get("message");
            match runtime_message_role(message) {
                Some("assistant") => {
                    let is_preamble = assistant_message_is_tool_call_turn(message);
                    if is_preamble && message_text(message).is_none() {
                        return None;
                    } else {
                        GatewayEvent::EntryCompleted {
                            turn_id: turn_id.to_string(),
                            entry: live_entry(
                                turn_id,
                                "assistant",
                                TranscriptEntryRole::Assistant,
                                TranscriptBlockKind::Text,
                                TranscriptBlockStatus::Completed,
                                None,
                                message_text(value.get("message")),
                                Some(if is_preamble {
                                    assistant_phase_metadata(value)
                                } else {
                                    assistant_message_metadata(value)
                                }),
                            ),
                        }
                    }
                }
                Some("user") => GatewayEvent::EntryCompleted {
                    turn_id: turn_id.to_string(),
                    entry: live_entry(
                        turn_id,
                        "prompt",
                        TranscriptEntryRole::User,
                        TranscriptBlockKind::Text,
                        TranscriptBlockStatus::Completed,
                        None,
                        message_text(value.get("message")),
                        None,
                    ),
                },
                _ => return None,
            }
        }
        Some("agent_message") => GatewayEvent::EntryCompleted {
            turn_id: turn_id.to_string(),
            entry: live_entry(
                turn_id,
                "assistant",
                TranscriptEntryRole::Assistant,
                TranscriptBlockKind::Text,
                TranscriptBlockStatus::Completed,
                None,
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                None,
            ),
        },
        Some("agent_session_start") => GatewayEvent::EntryUpdated {
            turn_id: turn_id.to_string(),
            entry: live_entry(
                turn_id,
                value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("agent"),
                TranscriptEntryRole::Assistant,
                TranscriptBlockKind::Agent,
                TranscriptBlockStatus::Running,
                value
                    .get("agent_name")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                value
                    .get("agent_description")
                    .or_else(|| value.get("task_name"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                Some(runtime_value_metadata(value)),
            ),
        },
        Some("tool_call_pending" | "tool_execution_start" | "tool_execution_update")
            if tool_name_from_value(value) == "write_stdin" =>
        {
            return None;
        }
        Some("tool_execution_end")
            if tool_name_from_value(value) == "write_stdin" && !tool_event_failed(value) =>
        {
            return None;
        }
        Some("tool_call_pending") => GatewayEvent::EntryStarted {
            turn_id: turn_id.to_string(),
            entry: live_tool_entry(
                turn_id,
                value,
                TranscriptBlockStatus::Pending,
                value
                    .get("arguments_json")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            ),
        },
        Some("tool_execution_start") => GatewayEvent::EntryStarted {
            turn_id: turn_id.to_string(),
            entry: live_tool_entry(
                turn_id,
                value,
                TranscriptBlockStatus::Running,
                value.get("args").and_then(json_preview),
            ),
        },
        Some("tool_execution_update") => GatewayEvent::EntryUpdated {
            turn_id: turn_id.to_string(),
            entry: live_tool_entry(
                turn_id,
                value,
                TranscriptBlockStatus::Running,
                value.get("partial_result").and_then(json_preview),
            ),
        },
        Some("tool_execution_end") => GatewayEvent::EntryCompleted {
            turn_id: turn_id.to_string(),
            entry: live_tool_entry(
                turn_id,
                value,
                if value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .is_some_and(|outcome| outcome != "normal")
                {
                    TranscriptBlockStatus::Failed
                } else {
                    TranscriptBlockStatus::Completed
                },
                value.get("result").and_then(json_preview),
            ),
        },
        Some("user_message") => GatewayEvent::EntryCompleted {
            turn_id: turn_id.to_string(),
            entry: live_entry(
                turn_id,
                "prompt",
                TranscriptEntryRole::User,
                TranscriptBlockKind::Text,
                TranscriptBlockStatus::Completed,
                None,
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                None,
            ),
        },
        Some("warning") => serde_json::from_value::<RunWarning>(value.clone())
            .map(|warning| GatewayEvent::Warning {
                kind: warning.kind,
                message: warning.message,
                source_path: warning.source_path.map(|path| path.display().to_string()),
                suggestion: warning.suggestion,
            })
            .unwrap_or_else(|_| GatewayEvent::Warning {
                kind: "runtime_warning".to_string(),
                message: "runtime warning could not be decoded".to_string(),
                source_path: None,
                suggestion: None,
            }),
        Some("exec_approval_request") | Some("apply_patch_approval_request") => {
            GatewayEvent::PermissionRequested {
                request_id: value
                    .get("call_id")
                    .or_else(|| value.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                tool_name: value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string(),
                summary: value
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                reason: value
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                matched_rule: value
                    .get("matched_rule")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                suggested_rule: value
                    .get("suggested_rule")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                allow_always: value
                    .get("allow_always")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                timeout_secs: value
                    .get("timeout_secs")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            }
        }
        _ => return None,
    })
}

fn live_tool_entry(
    turn_id: &str,
    value: &Value,
    status: TranscriptBlockStatus,
    body: Option<String>,
) -> TranscriptEntry {
    let tool_name = value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let tool_call_id = value
        .get("tool_call_id")
        .and_then(Value::as_str)
        .unwrap_or(tool_name);
    let metadata = tool_value_metadata(value);
    let title = live_tool_title(tool_name, &metadata);
    live_entry(
        turn_id,
        &format!("tool:{tool_call_id}"),
        TranscriptEntryRole::Assistant,
        tool_kind(tool_name),
        status,
        Some(title),
        body,
        Some(metadata),
    )
}

fn live_tool_title(tool_name: &str, metadata: &Value) -> String {
    if tool_name == "exec_command"
        && let Some(command) = metadata
            .get("args")
            .and_then(|args| args.get("cmd"))
            .and_then(Value::as_str)
            .and_then(first_shell_command_line)
    {
        return format!("exec_command {command}");
    }
    tool_name.to_string()
}

fn first_shell_command_line(text: &str) -> Option<&str> {
    let mut first_non_empty = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        first_non_empty.get_or_insert(line);
        if !line.starts_with('#') {
            return Some(line);
        }
    }
    first_non_empty
}

#[allow(clippy::too_many_arguments)]
fn live_entry(
    turn_id: &str,
    id_suffix: &str,
    role: TranscriptEntryRole,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    title: Option<String>,
    body: Option<String>,
    metadata: Option<Value>,
) -> TranscriptEntry {
    let now = crate::gateway_now_ms();
    let id = format!("live:{turn_id}:{id_suffix}");
    TranscriptEntry {
        id: id.clone(),
        thread_id: String::new(),
        turn_id: Some(turn_id.to_string()),
        message_seq: None,
        role,
        status,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id,
            kind,
            status,
            order: 0,
            source: "runtime.stream".to_string(),
            title,
            preview: body.as_deref().map(|text| compact_text(text, 240)),
            detail: body.clone(),
            body,
            artifact_ids: Vec::new(),
            metadata,
            result: None,
            created_at_ms: now,
            updated_at_ms: now,
        }],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn assistant_message_metadata(value: &Value) -> Value {
    let mut metadata = serde_json::json!({
        "usage": value.get("usage").cloned().unwrap_or(Value::Null),
        "metadata": value.get("metadata").cloned().unwrap_or(Value::Null),
        "accounting": value.get("accounting").cloned().unwrap_or(Value::Null),
    });
    if let Some(object) = metadata.as_object_mut()
        && let Some(message) = value.get("message")
    {
        for key in ["provider", "model", "finish_reason", "outcome"] {
            if let Some(field) = message.get(key).filter(|field| !field.is_null()) {
                object.insert(key.to_string(), field.clone());
            }
        }
    }
    metadata
}

fn assistant_phase_metadata(value: &Value) -> Value {
    let mut metadata = assistant_message_metadata(value);
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            "projection".to_string(),
            Value::String("assistant_phase".to_string()),
        );
    }
    metadata
}

fn assistant_message_is_tool_call_turn(message: Option<&Value>) -> bool {
    let Some(message) = message else {
        return false;
    };
    if message
        .get("finish_reason")
        .and_then(Value::as_str)
        .is_some_and(|finish_reason| finish_reason == "tool_calls")
    {
        return true;
    }
    if message
        .get("tool_calls")
        .is_some_and(|value| !value.is_null())
    {
        return true;
    }
    message
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|blocks| {
            blocks.iter().any(|block| {
                matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("tool_call" | "tool_calls" | "tool_use")
                )
            })
        })
}

fn tool_value_metadata(value: &Value) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("projection".to_string(), Value::String("tool".to_string()));
    for key in [
        "type",
        "tool_name",
        "tool_call_id",
        "outcome",
        "source",
        "display",
        "result",
        "metadata",
    ] {
        if let Some(field) = value.get(key) {
            object.insert(key.to_string(), field.clone());
        }
    }
    if let Some(args) = value.get("args").cloned().or_else(|| {
        value
            .get("arguments_json")
            .and_then(Value::as_str)
            .and_then(|raw| serde_json::from_str(raw).ok())
    }) {
        object.insert("args".to_string(), args);
    }
    if !object.contains_key("outcome") {
        object.insert("outcome".to_string(), Value::String("normal".to_string()));
    }
    Value::Object(object)
}

fn tool_name_from_value(value: &Value) -> &str {
    value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("tool")
}

fn tool_call_id_from_value<'a>(value: &'a Value, fallback: &'a str) -> &'a str {
    value
        .get("tool_call_id")
        .and_then(Value::as_str)
        .unwrap_or(fallback)
}

fn tool_args_from_value(value: &Value) -> Option<Value> {
    value.get("args").cloned().or_else(|| {
        value
            .get("arguments_json")
            .and_then(Value::as_str)
            .and_then(|raw| serde_json::from_str(raw).ok())
    })
}

fn exec_session_id_from_args_value(value: &Value) -> Option<u64> {
    value.get("session_id").and_then(Value::as_u64)
}

fn exec_session_id_from_result_value(value: &Value) -> Option<u64> {
    value
        .get("result")
        .and_then(|result| result.get("session_id"))
        .and_then(Value::as_u64)
}

fn exec_result_running_value(value: &Value) -> bool {
    exec_session_id_from_result_value(value).is_some()
        && value
            .get("result")
            .and_then(|result| result.get("exit_code"))
            .is_none_or(Value::is_null)
}

fn exec_result_completed_value(value: &Value) -> bool {
    value
        .get("result")
        .and_then(|result| result.get("exit_code"))
        .is_some_and(|exit_code| !exit_code.is_null())
}

fn tool_event_failed(value: &Value) -> bool {
    value
        .get("outcome")
        .and_then(Value::as_str)
        .is_some_and(|outcome| outcome != "normal")
}

fn tool_result_output_runtime(value: &Value) -> String {
    value
        .get("result")
        .and_then(|result| result.get("output"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn tool_result_output_value(value: &Value) -> String {
    value
        .get("result")
        .and_then(|result| result.get("output"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn merge_output(existing: &mut String, next: &str) {
    if next.is_empty() || existing.ends_with(next) {
        return;
    }
    if next.starts_with(existing.as_str()) {
        *existing = next.to_string();
    } else {
        existing.push_str(next);
    }
}

fn set_metadata_field(metadata: &mut Value, key: &str, value: Value) {
    if let Some(object) = metadata.as_object_mut() {
        object.insert(key.to_string(), value);
    }
}

fn set_metadata_result_field(metadata: &mut Value, key: &str, value: Value) {
    let Some(object) = metadata.as_object_mut() else {
        return;
    };
    let result = object
        .entry("result".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !result.is_object() {
        *result = Value::Object(serde_json::Map::new());
    }
    if let Some(result) = result.as_object_mut() {
        result.insert(key.to_string(), value);
    }
}

fn result_body_from_metadata(metadata: &Value) -> Option<String> {
    metadata
        .get("result")
        .and_then(|result| result.get("output"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            metadata
                .get("result")
                .and_then(|result| serde_json::to_string(result).ok())
        })
}

fn runtime_value_metadata(value: &Value) -> Value {
    let mut object = value.as_object().cloned().unwrap_or_default();
    object.insert(
        "projection".to_string(),
        Value::String("runtimeValue".to_string()),
    );
    Value::Object(object)
}

fn selected_skills_from_value(value: &Value) -> Vec<GatewaySelectedSkill> {
    value
        .get("selected_skills")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|skill| {
            Some(GatewaySelectedSkill {
                name: skill.get("name")?.as_str()?.to_string(),
                path: skill
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}

fn tool_kind(tool_name: &str) -> TranscriptBlockKind {
    match tool_name {
        "exec_command" | "write_stdin" => TranscriptBlockKind::Shell,
        "read" | "write" | "edit" | "apply_patch" => TranscriptBlockKind::File,
        "web_fetch" | "web_search" => TranscriptBlockKind::Web,
        "mcp" | "mcp_call" => TranscriptBlockKind::Mcp,
        "clarify" => TranscriptBlockKind::Clarify,
        _ => TranscriptBlockKind::ToolCall,
    }
}

fn json_preview(value: &Value) -> Option<String> {
    serde_json::to_string(value).ok()
}

fn message_text(message: Option<&Value>) -> Option<String> {
    let text = message?
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn run_start_projects_selected_skills() {
        let event = gateway_event_from_run_stream(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "run_start",
                "session_id": "thread-1",
                "selected_skills": [
                    {"name": "reviewer", "path": "/tmp/reviewer/SKILL.md"}
                ]
            })),
        );
        match event.expect("run_start should project a Gateway event") {
            GatewayEvent::TurnStarted {
                thread_id,
                turn_id,
                selected_skills,
            } => {
                assert_eq!(thread_id.as_deref(), Some("thread-1"));
                assert_eq!(turn_id, "turn-1");
                assert_eq!(selected_skills.len(), 1);
                assert_eq!(selected_skills[0].name, "reviewer");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn live_projector_streams_reasoning_before_completion() {
        let mut projector = GatewayLiveProjector::default();
        let started = projector
            .project(
                "turn-1",
                &RunStreamEvent::ReasoningDelta {
                    text: "first".to_string(),
                },
            )
            .expect("started");
        let completed = projector
            .project("turn-1", &RunStreamEvent::ReasoningEnd)
            .expect("completed");

        match started {
            GatewayEvent::EntryStarted { entry, .. } => {
                assert_eq!(entry.id, "live:turn-1:assistant:0");
                assert_eq!(entry.blocks[0].body.as_deref(), Some("first"));
                assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Running);
            }
            other => panic!("unexpected started event: {other:?}"),
        }
        match completed {
            GatewayEvent::EntryUpdated { entry, .. } => {
                assert_eq!(entry.id, "live:turn-1:assistant:0");
                assert_eq!(entry.blocks[0].body.as_deref(), Some("first"));
                assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Completed);
            }
            other => panic!("unexpected completed event: {other:?}"),
        }
    }

    #[test]
    fn assistant_tool_call_text_projects_as_assistant_phase_text() {
        let mut projector = GatewayLiveProjector::default();
        let event = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "message_end",
                    "message": {
                        "role": "assistant",
                        "content": [
                            {"type": "text", "text": "I will write the file now."},
                            {
                                "type": "tool_call",
                                "id": "call-write",
                                "name": "write",
                                "arguments": {"path": "out.md"},
                                "arguments_json": "{\"path\":\"out.md\"}",
                                "content_index": 1,
                                "call_index": 0
                            }
                        ],
                        "provider": "mock",
                        "model": "mock-model",
                        "finish_reason": "tool_calls",
                        "outcome": "normal"
                    }
                })),
            )
            .expect("projected");
        match event {
            GatewayEvent::EntryCompleted { entry, .. } => {
                assert_eq!(entry.id, "live:turn-1:assistant:0");
                assert_eq!(entry.blocks.len(), 2);
                assert_eq!(entry.blocks[0].kind, TranscriptBlockKind::Text);
                assert_eq!(entry.blocks[0].order, 0);
                assert_eq!(entry.blocks[0].title, None);
                assert_eq!(
                    entry.blocks[0].body.as_deref(),
                    Some("I will write the file now.")
                );
                let metadata = entry.blocks[0].metadata.as_ref().expect("metadata");
                assert_eq!(metadata["projection"], "assistant_phase");
                assert_eq!(entry.blocks[1].kind, TranscriptBlockKind::File);
                assert_eq!(entry.blocks[1].order, 1);
                assert_eq!(
                    entry.blocks[1].metadata.as_ref().expect("tool metadata")["tool_call_id"],
                    "call-write"
                );
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn assistant_tool_call_without_text_projects_tool_without_empty_assistant_phase() {
        let mut projector = GatewayLiveProjector::default();
        let event = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "message_end",
                    "message": {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "tool_call",
                                "id": "call-write",
                                "name": "write",
                                "arguments": {"path": "out.md"},
                                "arguments_json": "{\"path\":\"out.md\"}",
                                "content_index": 0,
                                "call_index": 0
                            }
                        ],
                        "finish_reason": "tool_calls",
                        "outcome": "normal"
                    }
                })),
            )
            .expect("projected");
        match event {
            GatewayEvent::EntryCompleted { entry, .. } => {
                assert_eq!(entry.blocks.len(), 1);
                assert_eq!(entry.blocks[0].kind, TranscriptBlockKind::File);
                assert_eq!(
                    entry.blocks[0].metadata.as_ref().unwrap()["tool_call_id"],
                    "call-write"
                );
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn live_projector_reorders_tool_pending_after_reasoning_and_phase_text() {
        let mut projector = GatewayLiveProjector::default();
        let _ = projector.project(
            "turn-1",
            &RunStreamEvent::ReasoningDelta {
                text: "The user wants the X daily report.".to_string(),
            },
        );
        let _ = projector.project("turn-1", &RunStreamEvent::ReasoningEnd);
        let _ = projector.project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_call_pending",
                "tool_name": "exec_command",
                "tool_call_id": "call_fetch",
                "args": {"cmd": "python fetch.py"},
                "outcome": "normal"
            })),
        );

        let completed_message = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "message_end",
                    "message": {
                        "role": "assistant",
                        "content": [
                            {"type": "reasoning", "text": "The user wants the X daily report.", "content_index": 0},
                            {"type": "text", "text": "好的，开始执行 X 日报流程。先运行 `fetch.py` 抓取今日推文数据。", "content_index": 1},
                            {
                                "type": "tool_call",
                                "id": "call_fetch",
                                "name": "exec_command",
                                "arguments": {"cmd": "python fetch.py"},
                                "arguments_json": "{\"cmd\":\"python fetch.py\"}",
                                "content_index": 2,
                                "call_index": 0
                            }
                        ],
                        "finish_reason": "tool_calls",
                        "outcome": "normal"
                    }
                })),
            )
            .expect("completed message");

        let entry = gateway_entry(&completed_message);
        assert_eq!(entry.id, "live:turn-1:assistant:0");
        let metadata = entry.metadata.as_ref().expect("entry metadata");
        assert_eq!(metadata["projection"], "assistant_segment");
        assert_eq!(metadata["liveOrder"], 0);
        assert_eq!(metadata["authoritativeBlocks"], true);
        assert!(metadata["streamSeq"].as_u64().is_some());
        assert_eq!(
            entry
                .blocks
                .iter()
                .map(|block| (block.kind, block.order, block.body.as_deref().unwrap_or("")))
                .collect::<Vec<_>>(),
            vec![
                (
                    TranscriptBlockKind::Reasoning,
                    0,
                    "The user wants the X daily report."
                ),
                (
                    TranscriptBlockKind::Text,
                    1,
                    "好的，开始执行 X 日报流程。先运行 `fetch.py` 抓取今日推文数据。"
                ),
                (TranscriptBlockKind::Shell, 2, ""),
            ]
        );
        assert_eq!(
            entry.blocks[1].metadata.as_ref().unwrap()["projection"],
            "assistant_phase"
        );

        let running_tool = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "tool_execution_end",
                    "tool_name": "exec_command",
                    "tool_call_id": "call_fetch",
                    "result": {"session_id": 9, "exit_code": null, "output": "[x-fetch] running\n"},
                    "outcome": "normal"
                })),
            )
            .expect("running tool");
        let entry = gateway_entry(&running_tool);
        assert_eq!(entry.id, "live:turn-1:assistant:0");
        assert_eq!(
            entry.metadata.as_ref().unwrap()["authoritativeBlocks"],
            false
        );
        assert_eq!(entry.blocks[2].kind, TranscriptBlockKind::Shell);
        assert_eq!(entry.blocks[2].order, 2);
        assert_eq!(entry.blocks[2].status, TranscriptBlockStatus::Running);
        assert_eq!(entry.blocks[2].body.as_deref(), Some("[x-fetch] running\n"));
        assert_eq!(
            entry.blocks[2].title.as_deref(),
            Some("exec_command python fetch.py")
        );
    }

    #[test]
    fn live_projector_aliases_runtime_tool_id_to_matching_authoritative_tool_call() {
        let mut projector = GatewayLiveProjector::default();
        let completed_message = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "message_end",
                    "message": {
                        "role": "assistant",
                        "content": [
                            {"type": "text", "text": "先运行 fetch.py。", "content_index": 0},
                            {
                                "type": "tool_call",
                                "id": "model_call_fetch",
                                "name": "exec_command",
                                "arguments": {"cmd": "python fetch.py"},
                                "arguments_json": "{\"cmd\":\"python fetch.py\"}",
                                "content_index": 1,
                                "call_index": 0
                            }
                        ],
                        "finish_reason": "tool_calls",
                        "outcome": "normal"
                    }
                })),
            )
            .expect("completed message");
        assert_eq!(gateway_entry(&completed_message).blocks.len(), 2);

        let running_tool = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "tool_execution_end",
                    "tool_name": "exec_command",
                    "tool_call_id": "runtime_call_fetch",
                    "args": {"cmd": "python fetch.py"},
                    "result": {"session_id": 9, "exit_code": null, "output": "[x-fetch] running\n"},
                    "outcome": "normal"
                })),
            )
            .expect("running tool");

        let entry = gateway_entry(&running_tool);
        assert_eq!(entry.id, "live:turn-1:assistant:0");
        assert_eq!(
            entry
                .blocks
                .iter()
                .map(|block| block.kind)
                .collect::<Vec<_>>(),
            vec![TranscriptBlockKind::Text, TranscriptBlockKind::Shell]
        );
        let block = &entry.blocks[1];
        assert_eq!(block.id, "live:turn-1:tool:model_call_fetch");
        assert_eq!(
            block.metadata.as_ref().unwrap()["tool_call_id"],
            "model_call_fetch"
        );
        assert_eq!(block.status, TranscriptBlockStatus::Running);
        assert_eq!(block.body.as_deref(), Some("[x-fetch] running\n"));
    }

    #[test]
    fn live_projector_authoritative_message_end_preserves_runtime_reasoning() {
        let mut projector = GatewayLiveProjector::default();
        let _ = projector.project(
            "turn-1",
            &RunStreamEvent::ReasoningDelta {
                text: "This runtime stream is real reasoning.".to_string(),
            },
        );
        let _ = projector.project("turn-1", &RunStreamEvent::ReasoningEnd);
        let _ = projector.project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_call_pending",
                "tool_name": "exec_command",
                "tool_call_id": "call_fetch",
                "args": {"cmd": "python fetch.py"},
                "outcome": "normal"
            })),
        );

        let completed = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "message_end",
                    "message": {
                        "role": "assistant",
                        "content": [
                            {"type": "text", "text": "好的，开始执行 X 日报流程。", "content_index": 0},
                            {
                                "type": "tool_call",
                                "id": "call_fetch",
                                "name": "exec_command",
                                "arguments": {"cmd": "python fetch.py"},
                                "arguments_json": "{\"cmd\":\"python fetch.py\"}",
                                "content_index": 1,
                                "call_index": 0
                            }
                        ],
                        "finish_reason": "tool_calls",
                        "outcome": "normal"
                    }
                })),
            )
            .expect("completed message");

        let entry = gateway_entry(&completed);
        assert_eq!(entry.id, "live:turn-1:assistant:0");
        assert_eq!(
            entry.metadata.as_ref().unwrap()["authoritativeBlocks"],
            true
        );
        assert_eq!(
            entry
                .blocks
                .iter()
                .map(|block| block.kind)
                .collect::<Vec<_>>(),
            vec![
                TranscriptBlockKind::Reasoning,
                TranscriptBlockKind::Text,
                TranscriptBlockKind::Shell
            ]
        );
        assert_eq!(
            entry.blocks[0].body.as_deref(),
            Some("This runtime stream is real reasoning.")
        );
        assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Completed);
        assert_eq!(
            entry.blocks[0].metadata.as_ref().unwrap()["origin"],
            "run_stream_reasoning"
        );
        assert_eq!(
            entry.blocks[1].body.as_deref(),
            Some("好的，开始执行 X 日报流程。")
        );
        assert_eq!(entry.blocks[2].order, 1);
        assert_eq!(entry.blocks[2].status, TranscriptBlockStatus::Pending);
        assert_eq!(
            entry.blocks[2].title.as_deref(),
            Some("exec_command python fetch.py")
        );
    }

    #[test]
    fn live_projector_merges_write_stdin_polls_into_yielded_exec_command() {
        let mut projector = GatewayLiveProjector::default();
        let pending = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "tool_call_pending",
                    "tool_name": "exec_command",
                    "tool_call_id": "call_exec",
                    "args": {"cmd": "python fetch.py"},
                    "outcome": "normal"
                })),
            )
            .expect("pending exec");
        match pending {
            GatewayEvent::EntryStarted { entry, .. } => {
                let block = &entry.blocks[0];
                assert_eq!(block.title.as_deref(), Some("exec_command python fetch.py"));
                assert_eq!(block.status, TranscriptBlockStatus::Pending);
                let metadata = block.metadata.as_ref().expect("metadata");
                assert_eq!(metadata["args"]["cmd"], "python fetch.py");
            }
            other => panic!("unexpected pending event: {other:?}"),
        }

        let yielded = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "tool_execution_end",
                    "tool_name": "exec_command",
                    "tool_call_id": "call_exec",
                    "result": {"session_id": 7, "exit_code": null, "output": "first\n"},
                    "outcome": "normal"
                })),
            )
            .expect("yielded exec");
        assert_exec_event(
            &yielded,
            "call_exec",
            TranscriptBlockStatus::Running,
            "first\n",
            Some("exec_command python fetch.py"),
            None,
        );

        let hidden_poll = projector.project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_call_pending",
                "tool_name": "write_stdin",
                "tool_call_id": "call_poll",
                "args": {"session_id": 7, "yield_time_ms": 60000},
                "outcome": "normal"
            })),
        );
        assert!(hidden_poll.is_none());

        let polled = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "tool_execution_end",
                    "tool_name": "write_stdin",
                    "tool_call_id": "call_poll",
                    "result": {"session_id": null, "exit_code": null, "output": "second\n"},
                    "outcome": "normal"
                })),
            )
            .expect("poll result");
        assert_exec_event(
            &polled,
            "call_exec",
            TranscriptBlockStatus::Running,
            "first\nsecond\n",
            Some("exec_command python fetch.py"),
            None,
        );

        let _ = projector.project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_call_pending",
                "tool_name": "write_stdin",
                "tool_call_id": "call_done",
                "args": {"session_id": 7, "yield_time_ms": 60000},
                "outcome": "normal"
            })),
        );
        let completed = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "tool_execution_end",
                    "tool_name": "write_stdin",
                    "tool_call_id": "call_done",
                    "result": {"session_id": null, "exit_code": 0, "output": "third\n"},
                    "outcome": "normal"
                })),
            )
            .expect("completion");
        assert_exec_event(
            &completed,
            "call_exec",
            TranscriptBlockStatus::Completed,
            "first\nsecond\nthird\n",
            Some("exec_command python fetch.py"),
            Some(0),
        );
    }

    #[test]
    fn live_projector_hides_unmatched_successful_write_stdin_but_keeps_failed_one() {
        let mut projector = GatewayLiveProjector::default();
        let success = projector.project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "write_stdin",
                "tool_call_id": "call_poll",
                "result": {"session_id": null, "exit_code": null, "output": "late\n"},
                "outcome": "normal"
            })),
        );
        assert!(success.is_none());

        let failed = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "tool_execution_end",
                    "tool_name": "write_stdin",
                    "tool_call_id": "call_poll",
                    "result": {"session_id": null, "exit_code": 1, "output": "error\n"},
                    "outcome": "failed"
                })),
            )
            .expect("failed");
        match failed {
            GatewayEvent::EntryCompleted { entry, .. } => {
                assert_eq!(entry.id, "live:turn-1:assistant:0");
                assert_eq!(entry.blocks[0].title.as_deref(), Some("write_stdin"));
                assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Failed);
            }
            other => panic!("unexpected failed event: {other:?}"),
        }
    }

    #[test]
    fn live_projector_hides_assistant_write_stdin_tool_call_block() {
        let mut projector = GatewayLiveProjector::default();
        let hidden = projector.project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool_call",
                            "id": "call_poll",
                            "name": "write_stdin",
                            "arguments": {"session_id": 7, "yield_time_ms": 60000},
                            "arguments_json": "{\"session_id\":7,\"yield_time_ms\":60000}",
                            "content_index": 0,
                            "call_index": 0
                        }
                    ],
                    "finish_reason": "tool_calls",
                    "outcome": "normal"
                }
            })),
        );

        assert!(hidden.is_none());
    }

    #[test]
    fn live_projector_hidden_assistant_message_end_closes_segment() {
        let mut projector = GatewayLiveProjector::default();
        let _ = projector.project(
            "turn-1",
            &RunStreamEvent::ReasoningDelta {
                text: "The command is still running.".to_string(),
            },
        );
        let _ = projector.project("turn-1", &RunStreamEvent::ReasoningEnd);
        let hidden = projector.project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool_call",
                            "id": "call_poll",
                            "name": "write_stdin",
                            "arguments": {"session_id": 7, "yield_time_ms": 60000},
                            "arguments_json": "{\"session_id\":7,\"yield_time_ms\":60000}",
                            "content_index": 0,
                            "call_index": 0
                        }
                    ],
                    "finish_reason": "tool_calls",
                    "outcome": "normal"
                }
            })),
        );
        assert!(hidden.is_none());

        let next = projector
            .project(
                "turn-1",
                &RunStreamEvent::ReasoningDelta {
                    text: "fetch.py completed.".to_string(),
                },
            )
            .expect("next reasoning");
        match next {
            GatewayEvent::EntryStarted { entry, .. } => {
                assert_eq!(entry.id, "live:turn-1:assistant:1");
                assert_eq!(entry.blocks[0].body.as_deref(), Some("fetch.py completed."));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    fn gateway_entry(event: &GatewayEvent) -> &TranscriptEntry {
        match event {
            GatewayEvent::EntryStarted { entry, .. }
            | GatewayEvent::EntryUpdated { entry, .. }
            | GatewayEvent::EntryCompleted { entry, .. } => entry,
            other => panic!("unexpected event: {other:?}"),
        }
    }

    fn assert_exec_event(
        event: &GatewayEvent,
        expected_tool_call_id: &str,
        expected_status: TranscriptBlockStatus,
        expected_output: &str,
        expected_title: Option<&str>,
        expected_exit_code: Option<i64>,
    ) {
        let entry = gateway_entry(event);
        assert_eq!(entry.id, "live:turn-1:assistant:0");
        let block = entry
            .blocks
            .iter()
            .find(|block| {
                block
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("tool_call_id"))
                    .and_then(Value::as_str)
                    == Some(expected_tool_call_id)
            })
            .expect("tool block");
        assert_eq!(block.title.as_deref(), expected_title);
        assert_eq!(block.status, expected_status);
        let metadata = block.metadata.as_ref().expect("metadata");
        assert_eq!(metadata["tool_name"], "exec_command");
        assert_eq!(metadata["tool_call_id"], expected_tool_call_id);
        assert_eq!(metadata["args"]["cmd"], "python fetch.py");
        assert_eq!(metadata["result"]["output"], expected_output);
        match expected_exit_code {
            Some(exit_code) => assert_eq!(metadata["result"]["exit_code"], exit_code),
            None => assert!(
                metadata["result"]
                    .get("exit_code")
                    .is_none_or(Value::is_null),
                "{metadata:?}"
            ),
        }
    }
}
