impl GatewayLiveProjector {
    fn project_tool_event(&mut self, turn_id: &str, value: &Value) -> Option<GatewayEvent> {
        let tool_name = tool_name_from_value(value);
        let raw_tool_call_id = self.raw_tool_call_id_for_event(tool_name, value);
        let args = tool_args_from_value(value);
        if !raw_tool_call_id.is_empty()
            && let Some(args) = args.clone()
        {
            self.tool_args.insert(raw_tool_call_id.clone(), args);
        }
        let tool_call_id = if tool_name == "spawn_agent" {
            self.strict_agent_tool_call_id(turn_id, &raw_tool_call_id, value)
        } else {
            self.canonical_tool_call_id(&raw_tool_call_id, tool_name, args.as_ref())
        };
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

    fn project_exec_session_event(&mut self, turn_id: &str, value: &Value) -> Option<GatewayEvent> {
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
            Some("tool_execution_end")
                if background_running_agent_result_value(tool_name, value) =>
            {
                TranscriptBlockStatus::Running
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
        if metadata.get("args").is_none_or(Value::is_null)
            && let Some(args) = self.tool_args.get(tool_call_id)
        {
            set_metadata_field(&mut metadata, "args", args.clone());
        }
        if tool_name == "spawn_agent" {
            self.enrich_agent_metadata_from_existing(turn_id, segment, tool_call_id, &mut metadata);
            enrich_agent_metadata_from_fields(&mut metadata);
        }
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
        let candidates = args
            .map(|args| self.matching_open_tool_candidates(tool_name, args))
            .unwrap_or_default();
        let candidates = if candidates.len() == 1 {
            candidates
        } else if candidates.is_empty() {
            self.matching_open_tool_name_candidates(tool_name)
        } else {
            Vec::new()
        };
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

    fn raw_tool_call_id_for_event(&self, tool_name: &str, value: &Value) -> String {
        if tool_name != "spawn_agent" {
            return tool_call_id_from_value(value, tool_name).to_string();
        }
        if let Some(tool_call_id) = value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|tool_call_id| !tool_call_id.is_empty())
        {
            return tool_call_id.to_string();
        }
        tool_position_key(self.assistant_segment, value)
            .map(|position_key| format!("{tool_name}@{position_key}"))
            .unwrap_or_else(|| {
                format!(
                    "{tool_name}@event:{}",
                    self.stream_seq.saturating_add(1)
                )
            })
    }

    fn register_tool_content_identity(
        &mut self,
        turn_id: &str,
        segment: usize,
        raw_tool_call_id: &str,
        tool_name: &str,
        metadata: &mut Value,
    ) -> String {
        if tool_name != "spawn_agent" {
            self.tool_owners
                .insert(raw_tool_call_id.to_string(), segment);
            return raw_tool_call_id.to_string();
        }
        let tool_call_id =
            self.strict_agent_tool_call_id_for_segment(turn_id, segment, raw_tool_call_id, metadata);
        set_metadata_field(metadata, "tool_call_id", json!(tool_call_id.clone()));
        self.tool_owners.insert(tool_call_id.clone(), segment);
        tool_call_id
    }

    fn strict_agent_tool_call_id(
        &mut self,
        turn_id: &str,
        raw_tool_call_id: &str,
        value: &Value,
    ) -> String {
        let segment = self
            .tool_owners
            .get(raw_tool_call_id)
            .copied()
            .unwrap_or(self.assistant_segment);
        self.strict_agent_tool_call_id_for_segment(turn_id, segment, raw_tool_call_id, value)
    }

    fn strict_agent_tool_call_id_for_segment(
        &mut self,
        turn_id: &str,
        segment: usize,
        raw_tool_call_id: &str,
        value: &Value,
    ) -> String {
        if raw_tool_call_id.trim().is_empty() {
            return raw_tool_call_id.to_string();
        }
        if let Some(canonical) = self.tool_aliases.get(raw_tool_call_id) {
            return canonical.clone();
        }
        if self.tool_owners.contains_key(raw_tool_call_id) {
            return raw_tool_call_id.to_string();
        }
        let Some(position_key) = tool_position_key(segment, value) else {
            return raw_tool_call_id.to_string();
        };
        let Some(existing) = self.tool_positions.get(&position_key).cloned() else {
            self.tool_positions
                .insert(position_key, raw_tool_call_id.to_string());
            return raw_tool_call_id.to_string();
        };
        if existing != raw_tool_call_id {
            self.migrate_tool_identity(turn_id, segment, &existing, raw_tool_call_id);
            self.tool_aliases
                .insert(existing.clone(), raw_tool_call_id.to_string());
            self.tool_positions
                .insert(position_key, raw_tool_call_id.to_string());
        }
        raw_tool_call_id.to_string()
    }

    fn migrate_tool_identity(
        &mut self,
        turn_id: &str,
        segment: usize,
        old_tool_call_id: &str,
        new_tool_call_id: &str,
    ) {
        if old_tool_call_id == new_tool_call_id {
            return;
        }
        if let Some(owner) = self.tool_owners.remove(old_tool_call_id) {
            self.tool_owners.insert(new_tool_call_id.to_string(), owner);
        }
        if let Some(args) = self.tool_args.remove(old_tool_call_id) {
            self.tool_args.entry(new_tool_call_id.to_string()).or_insert(args);
        }
        let old_block_id = live_tool_block_id(turn_id, old_tool_call_id);
        let new_block_id = live_tool_block_id(turn_id, new_tool_call_id);
        let Some(state) = self.entries.get_mut(&segment) else {
            return;
        };
        let Some(mut old_block) = state.blocks.remove(&old_block_id) else {
            return;
        };
        old_block.id = new_block_id.clone();
        if let Some(metadata) = old_block.metadata.as_mut() {
            set_metadata_field(metadata, "tool_call_id", json!(new_tool_call_id));
        }
        let block = state
            .blocks
            .get(&new_block_id)
            .map(|existing| merge_live_block(existing, old_block.clone()))
            .unwrap_or(old_block);
        state.blocks.insert(new_block_id, block);
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

    fn matching_open_tool_name_candidates(&self, tool_name: &str) -> Vec<(String, usize)> {
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
                candidates.push((candidate_id.to_string(), *segment));
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
}
