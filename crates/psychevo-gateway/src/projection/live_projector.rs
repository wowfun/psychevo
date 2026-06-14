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
            format!("live:{turn_id}:assistant:{segment}:acp-peer-plan"),
            TranscriptBlockKind::Status,
            TranscriptBlockStatus::Running,
            DEFAULT_TEXT_ORDER + 10,
            Some("Plan".to_string()),
            Some(body),
            Some(json!({
                "projection": "acp_peer_plan",
                "origin": "acp_peer",
                "source": "acp_peer",
                "plan": value.get("plan").cloned().unwrap_or(Value::Null),
            })),
        );
        self.upsert_block(segment, block);
        Some(self.emit_entry_event(turn_id, segment, false, false))
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

    fn project_exec_session_event(
        &mut self,
        turn_id: &str,
        value: &Value,
    ) -> Option<GatewayEvent> {
        let session_id = value.get("session_id").and_then(Value::as_u64)?;
        let event_type = value.get("type").and_then(Value::as_str);
        let completed = event_type == Some("exec_session_finished");
        let (segment, root_tool_call_id, metadata, status) = {
            let state = self.exec_sessions.get_mut(&session_id)?;
            if let Some(output) = value.get("output").and_then(Value::as_str) {
                merge_output(&mut state.output, output);
                set_metadata_result_field(&mut state.metadata, "output", json!(state.output));
            }
            if completed {
                if let Some(exit_code) = value.get("exit_code") {
                    set_metadata_result_field(&mut state.metadata, "exit_code", exit_code.clone());
                }
                if let Some(elapsed_ms) = value.get("elapsed_ms") {
                    set_metadata_field(&mut state.metadata, "elapsed_ms", elapsed_ms.clone());
                }
                if value
                    .get("interrupted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    set_metadata_field(&mut state.metadata, "outcome", json!("cancelled"));
                }
            }
            let status = if completed
                && value
                    .get("interrupted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                TranscriptBlockStatus::Cancelled
            } else if completed {
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
        if completed {
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
            completed,
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
        for block in self.preserved_acp_peer_blocks(segment) {
            blocks.entry(block.id.clone()).or_insert(block);
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

    fn preserved_acp_peer_blocks(&self, segment: usize) -> Vec<TranscriptBlock> {
        self.entries
            .get(&segment)
            .map(|state| {
                state
                    .blocks
                    .values()
                    .filter(|block| {
                        block.metadata.as_ref().is_some_and(|metadata| {
                            metadata
                                .get("source")
                                .and_then(Value::as_str)
                                .or_else(|| metadata.get("origin").and_then(Value::as_str))
                                == Some("acp_peer")
                                || metadata
                                    .get("metadata")
                                    .and_then(|metadata| metadata.get("origin"))
                                    .and_then(Value::as_str)
                                    == Some("acp_peer")
                        })
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
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
