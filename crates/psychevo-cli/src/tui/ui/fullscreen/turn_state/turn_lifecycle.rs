#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) const TUI_LOCAL_TRANSCRIPT_SOURCE: &str = "tui.local";
pub(crate) const TUI_TURN_START_TRANSCRIPT_SOURCE: &str = "tui.turn_start";

pub(crate) fn turn_local_transcript_source(source: Option<&str>) -> bool {
    matches!(
        source,
        Some(TUI_LOCAL_TRANSCRIPT_SOURCE | TUI_TURN_START_TRANSCRIPT_SOURCE)
    )
}

pub(crate) fn turn_start_local_row(row: &TranscriptRow) -> bool {
    row.transcript_source.as_deref() == Some(TUI_TURN_START_TRANSCRIPT_SOURCE)
}

impl<'a> FullscreenUi<'a> {
    pub(crate) fn mark_optimistic_rows_from(&mut self, start_index: usize) {
        for row in self.transcript.iter_mut().skip(start_index) {
            row.transcript_source = Some("tui.optimistic".to_string());
        }
    }

    pub(crate) fn bind_unbound_optimistic_rows_to_turn(&mut self, turn_id: &str) {
        for row in &mut self.transcript {
            if row.transcript_source.as_deref() == Some("tui.optimistic")
                && row.transcript_turn_id.is_none()
            {
                row.transcript_turn_id = Some(turn_id.to_string());
            }
        }
    }

    pub(crate) fn bind_unbound_local_rows_to_turn(&mut self, turn_id: &str) {
        for row in &mut self.transcript {
            if turn_local_transcript_source(row.transcript_source.as_deref())
                && row.transcript_turn_id.is_none()
            {
                row.transcript_turn_id = Some(turn_id.to_string());
            }
        }
    }

    pub(crate) fn tag_active_turn_local_row(&self, row: &mut TranscriptRow) {
        if let Some(running) = self.running.as_ref() {
            row.transcript_source = Some(TUI_LOCAL_TRANSCRIPT_SOURCE.to_string());
            row.transcript_turn_id = running.turn_id.clone();
        }
    }

    pub(crate) fn tag_active_turn_start_row(&self, row: &mut TranscriptRow) {
        row.transcript_source = Some(TUI_TURN_START_TRANSCRIPT_SOURCE.to_string());
        if let Some(running) = self.running.as_ref() {
            row.transcript_turn_id = running.turn_id.clone();
        }
    }

    pub(crate) fn take_local_rows_for_turn(&mut self, turn_id: &str) -> Vec<TranscriptRow> {
        let mut rows = Vec::new();
        for index in (0..self.transcript.len()).rev() {
            let Some(row) = self.transcript.get(index) else {
                continue;
            };
            if row.transcript_turn_id.as_deref() == Some(turn_id)
                && turn_local_transcript_source(row.transcript_source.as_deref())
            {
                rows.push(row.clone());
                self.remove_transcript_row(index);
            }
        }
        rows.reverse();
        rows
    }

    pub(crate) fn bind_unbound_live_turn_meta_to_turn(&mut self, turn_id: &str) {
        let Some(index) = self.meta_row else {
            return;
        };
        let Some(row) = self.transcript.get_mut(index) else {
            return;
        };
        if row.kind == TranscriptKind::Meta
            && row.transcript_source.as_deref() == Some("runtime.stream")
            && row.transcript_turn_id.is_none()
        {
            row.transcript_turn_id = Some(turn_id.to_string());
        }
    }

    pub(crate) fn remove_live_overlay_for_turn(&mut self, turn_id: &str) {
        for index in (0..self.transcript.len()).rev() {
            let Some(row) = self.transcript.get(index) else {
                continue;
            };
            let same_turn = row.transcript_turn_id.as_deref() == Some(turn_id);
            let live_source = matches!(
                row.transcript_source.as_deref(),
                Some("runtime.stream" | "tui.optimistic")
            );
            if same_turn && live_source {
                self.remove_transcript_row(index);
            }
        }
    }

    pub(crate) fn tag_live_turn_meta_row(&mut self, index: usize) {
        let turn_id = self
            .running
            .as_ref()
            .and_then(|running| running.turn_id.as_deref())
            .map(str::to_string)
            .or_else(|| {
                self.transcript
                    .iter()
                    .rev()
                    .find(|row| row.kind != TranscriptKind::Meta)
                    .and_then(|row| row.transcript_turn_id.clone())
            });
        let Some(row) = self.transcript.get_mut(index) else {
            return;
        };
        row.transcript_turn_id = turn_id;
        row.transcript_source = Some("runtime.stream".to_string());
        row.transcript_entry_id = None;
        row.transcript_block_id = None;
        row.transcript_message_seq = None;
    }

    pub(crate) fn upsert_streaming_tool_call(&mut self, call: StreamingToolCall) -> bool {
        if call.tool_name == "clarify" {
            return false;
        }
        if call.tool_name == "write_stdin"
            && let Some(session_id) = exec_session_id_from_args(&call.args)
            && self.exec_session_rows.contains_key(&session_id)
        {
            self.remove_streaming_tool_call_row(
                &call.tool_name,
                call.id.as_deref().unwrap_or_default(),
                Some(&call.position_key),
            );
            return false;
        }
        let mut value = serde_json::json!({ "args": call.args });
        if let Some(display) = &call.display
            && let Some(object) = value.as_object_mut()
        {
            object.insert(
                "display".to_string(),
                serde_json::to_value(display).unwrap_or(Value::Null),
            );
        }
        let id_key = call.id.as_deref().map(tool_id_key);
        let intent_key =
            (call.tool_name != "spawn_agent").then(|| tool_intent_key(&call.tool_name));
        let stale_agent_index = (call.tool_name == "spawn_agent")
            .then(|| self.completed_agent_invocation_index(&value, call.id.as_deref()))
            .flatten();
        let idx = id_key
            .as_ref()
            .and_then(|key| self.tool_rows.get(key))
            .or_else(|| self.tool_rows.get(&call.position_key))
            .or_else(|| {
                intent_key
                    .as_ref()
                    .and_then(|intent_key| self.tool_rows.get(intent_key))
            })
            .copied()
            .or_else(|| {
                (call.tool_name != "spawn_agent").then(|| {
                    self.matching_agent_placeholder_index(
                        &call.tool_name,
                        &value,
                        call.id.as_deref().unwrap_or_default(),
                    )
                })?
            });
        let mut active_tool_frame_requested = false;
        let idx = if let Some(idx) = idx {
            if call.tool_name == "spawn_agent"
                && self
                    .transcript
                    .get(idx)
                    .is_some_and(completed_agent_invocation_row)
            {
                self.tool_rows.insert(call.position_key, idx);
                if let Some(id_key) = id_key {
                    self.tool_rows.insert(id_key, idx);
                }
                return false;
            }
            if let Some(intent_key) = &intent_key {
                self.tool_rows.remove(intent_key);
            }
            let row = &mut self.transcript[idx];
            row.kind = evidence_kind_for_value(&call.tool_name, &value);
            row.tool_name = Some(call.tool_name.clone());
            row.title = active_tool_title(&call.tool_name, &value);
            if row.text.is_empty() {
                row.text = "preparing".to_string();
            }
            if call.tool_name == "spawn_agent"
                && let Some(full_text) = running_agent_tool_full_text(&value)
            {
                row.full_text = Some(full_text);
            }
            if call.id.is_some() {
                row.tool_call_id = call.id.clone();
            }
            if row.tool_started.is_none() {
                row.tool_started = Some(Instant::now());
                active_tool_frame_requested = true;
            }
            idx
        } else if let Some(idx) = stale_agent_index {
            self.tool_rows.insert(call.position_key, idx);
            if let Some(id_key) = id_key {
                self.tool_rows.insert(id_key, idx);
            }
            return false;
        } else {
            let mut row = TranscriptRow::with_title(
                evidence_kind_for_value(&call.tool_name, &value),
                active_tool_title(&call.tool_name, &value),
                "preparing",
            );
            row.tool_name = Some(call.tool_name.clone());
            row.tool_call_id = call.id.clone();
            if call.tool_name == "spawn_agent" {
                row.full_text = running_agent_tool_full_text(&value);
            }
            row.tool_started = Some(Instant::now());
            active_tool_frame_requested = true;
            self.insert_evidence_row(row)
        };
        self.tool_rows.insert(call.position_key, idx);
        if let Some(id_key) = id_key {
            self.tool_rows.insert(id_key, idx);
        }
        self.remove_turn_meta();
        self.remove_orphan_provisional_tool_intents(&call.tool_name, Some(idx));
        active_tool_frame_requested
    }

    pub(crate) fn remove_streaming_tool_call_row(
        &mut self,
        tool_name: &str,
        tool_call_id: &str,
        position_key: Option<&str>,
    ) {
        let mut keys = Vec::new();
        if !tool_call_id.is_empty() {
            keys.push(tool_id_key(tool_call_id));
        }
        if let Some(position_key) = position_key {
            keys.push(position_key.to_string());
        }
        if tool_name != "spawn_agent" {
            keys.push(tool_intent_key(tool_name));
        }

        let mut index = None;
        for key in &keys {
            if let Some(row_index) = self.tool_rows.remove(key) {
                index.get_or_insert(row_index);
            }
        }
        let Some(index) = index else {
            return;
        };
        let Some(row) = self.transcript.get(index) else {
            return;
        };
        if row.tool_name.as_deref() == Some(tool_name)
            && row.tool_started.is_some()
            && row.tool_elapsed.is_none()
        {
            self.remove_transcript_row(index);
        }
    }

    pub(crate) fn finish_turn(&mut self) {
        self.mark_unfinished_tools_interrupted();
        if let Some(idx) = self.reasoning_row {
            self.finish_thinking_row(idx);
        }
        self.assistant_row = None;
        self.assistant_preamble_row = None;
        self.reasoning_row = None;
        self.meta_row = None;
        self.gateway_item_rows.clear();
        self.tool_rows.clear();
        self.live_tool_args.clear();
        self.streaming_tool_message_open = false;
        self.deferred_stream_events.clear();
        self.turn_started = None;
        self.turn_provider.clear();
        self.turn_model.clear();
        self.turn_context_limit = None;
        self.turn_usage = None;
        self.turn_metadata = None;
        self.turn_accounting = None;
        self.turn_failures = 0;
        self.turn_outcome = None;
        self.turn_terminal_message = None;
        self.turn_interrupted = false;
        self.turn_session_id = None;
        self.active_event_session_id = None;
        self.turn_had_reasoning = false;
        self.turn_terminal_visible_answer = false;
        self.visible_turn_started = None;
        self.interrupt_requested = false;
        self.focus = FocusMode::Composer;
    }

    pub(crate) fn mark_unfinished_tools_interrupted(&mut self) {
        let mut indices = self.tool_rows.values().copied().collect::<Vec<_>>();
        indices.sort_unstable();
        indices.dedup();
        for index in indices {
            let Some(row) = self.transcript.get_mut(index) else {
                continue;
            };
            if row.tool_name.as_deref() == Some("exec_command")
                && row
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|id| self.tool_rows.contains_key(&tool_id_key(id)))
            {
                continue;
            }
            let Some(started) = row.tool_started.take() else {
                continue;
            };
            row.tool_elapsed = Some(started.elapsed());
            if row.tool_name.as_deref() == Some("spawn_agent") {
                row.failed = false;
                row.interrupted = false;
                if row.agent_target.is_none()
                    && matches!(row.text.as_str(), "preparing" | "running" | "")
                {
                    row.text = "not completed".to_string();
                }
                continue;
            }
            row.title = completed_tool_title_from_active(row.kind, &row.title);
            row.failed = false;
            row.interrupted = true;
            row.text = "interrupted".to_string();
        }
    }

    pub(crate) fn update_turn_meta(
        &mut self,
        debug: bool,
        allow_visible_answer: bool,
        allow_reasoning_only: bool,
        allow_failure_summary: bool,
    ) {
        if self.has_active_tool_rows() {
            self.remove_turn_meta();
            return;
        }
        let last_visible_kind = self.last_non_meta_transcript_kind();
        let terminal_visible_answer = allow_visible_answer
            && (self.assistant_row.is_some() || self.turn_terminal_visible_answer)
            && last_visible_kind == Some(TranscriptKind::Answer);
        let terminal_reasoning_only = allow_reasoning_only
            && self.turn_had_reasoning
            && last_visible_kind == Some(TranscriptKind::Thinking);
        let terminal_failure_summary =
            allow_failure_summary && (self.turn_failures > 0 || self.turn_interrupted);
        if !(terminal_visible_answer || terminal_reasoning_only || terminal_failure_summary) {
            return;
        }
        let running_session = self
            .active_event_session_id
            .as_deref()
            .or(self.turn_session_id.as_deref());
        if self.running.is_some()
            || running_session.is_some_and(|session_id| self.status_has_running(Some(session_id)))
        {
            self.remove_turn_meta();
            return;
        }
        let meta = turn_meta_text(TurnMetaProjection {
            mode: &self.turn_mode,
            provider: &self.turn_provider,
            model: &self.turn_model,
            started: self.turn_started,
            usage: self.turn_usage.as_ref(),
            metadata: self.turn_metadata.as_ref(),
            accounting: self.turn_accounting.as_ref(),
            failures: self.turn_failures,
            interrupted: self.turn_interrupted,
            debug,
        });
        if meta.is_empty() {
            return;
        }
        let idx = self.meta_row.unwrap_or_else(|| {
            let idx = self.transcript.len();
            self.transcript.push(TranscriptRow::with_title(
                TranscriptKind::Meta,
                "",
                String::new(),
            ));
            self.meta_row = Some(idx);
            idx
        });
        self.tag_live_turn_meta_row(idx);
        self.transcript[idx].text = meta;
    }

    pub(crate) fn last_non_meta_transcript_kind(&self) -> Option<TranscriptKind> {
        self.transcript
            .iter()
            .rev()
            .find(|row| row.kind != TranscriptKind::Meta)
            .map(|row| row.kind)
    }

    pub(crate) fn remove_turn_meta(&mut self) {
        if let Some(index) = self.meta_row {
            self.remove_transcript_row(index);
        }
    }

    pub(crate) fn has_active_tool_rows(&self) -> bool {
        self.transcript.iter().any(active_tool_row)
    }

    pub(crate) fn remove_orphan_provisional_tool_intents(
        &mut self,
        tool: &str,
        keep_index: Option<usize>,
    ) {
        let kind = evidence_kind(tool);
        let fallback_title = active_tool_title(tool, &serde_json::json!({ "args": Value::Null }));
        let mut indices = self
            .transcript
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                (Some(index) != keep_index
                    && row.kind == kind
                    && row.tool_name.as_deref() == Some(tool)
                    && row.title == fallback_title
                    && row.tool_call_id.is_none()
                    && active_tool_row(row))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        indices.sort_unstable_by(|a, b| b.cmp(a));
        for index in indices {
            self.remove_transcript_row(index);
        }
    }

    pub(crate) fn completed_agent_invocation_index(
        &self,
        _value: &Value,
        tool_call_id: Option<&str>,
    ) -> Option<usize> {
        let tool_call_id = tool_call_id
            .map(str::trim)
            .filter(|tool_call_id| !tool_call_id.is_empty())?;
        self.transcript
            .iter()
            .enumerate()
            .find(|(_, row)| {
                completed_agent_invocation_row(row)
                    && row.tool_call_id.as_deref() == Some(tool_call_id)
            })
            .map(|(index, _)| index)
    }

    pub(crate) fn matching_agent_placeholder_index(
        &self,
        tool: &str,
        _value: &Value,
        tool_call_id: &str,
    ) -> Option<usize> {
        if tool != "spawn_agent" {
            return None;
        }
        let tool_call_id = tool_call_id.trim();
        if tool_call_id.is_empty() {
            return None;
        }
        self.transcript
            .iter()
            .enumerate()
            .find(|(_, row)| {
                row.tool_name.as_deref() == Some("spawn_agent")
                    && active_tool_row(row)
                    && row.tool_call_id.as_deref() == Some(tool_call_id)
            })
            .map(|(index, _)| index)
    }

    pub(crate) fn remove_duplicate_agent_placeholders(&mut self, keep_index: usize, value: &Value) {
        let tool_call_id = value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut indices = self
            .transcript
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                (index != keep_index
                    && row.tool_name.as_deref() == Some("spawn_agent")
                    && row.agent_target.is_none()
                    && active_tool_row(row)
                    && !tool_call_id.is_empty()
                    && row.tool_call_id.as_deref() == Some(tool_call_id))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        indices.sort_unstable_by(|a, b| b.cmp(a));
        for index in indices {
            self.remove_transcript_row(index);
        }
    }

    pub(crate) fn remove_duplicate_agent_placeholders_for_tool_value(
        &mut self,
        keep_index: usize,
        value: &Value,
    ) {
        let incoming_tool_call_id = value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let incoming_target = agent_target_from_tool_event(value);
        let mut indices = self
            .transcript
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let duplicate_child_target = incoming_target
                    .as_deref()
                    .is_some_and(|target| row.agent_target.as_deref() == Some(target));
                let duplicate_tool_call_id = incoming_tool_call_id
                    .is_some_and(|tool_call_id| row.tool_call_id.as_deref() == Some(tool_call_id));
                (index != keep_index
                    && row.tool_name.as_deref() == Some("spawn_agent")
                    && (duplicate_child_target || duplicate_tool_call_id))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        indices.sort_unstable_by(|a, b| b.cmp(a));
        for index in indices {
            self.remove_transcript_row(index);
        }
    }


}
