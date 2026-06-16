#[allow(unused_imports)]
pub(crate) use super::*;
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
            if row.transcript_source.as_deref() == Some("tui.local")
                && row.transcript_turn_id.is_none()
            {
                row.transcript_turn_id = Some(turn_id.to_string());
            }
        }
    }

    pub(crate) fn tag_active_turn_local_row(&self, row: &mut TranscriptRow) {
        if let Some(running) = self.running.as_ref() {
            row.transcript_source = Some("tui.local".to_string());
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
                && row.transcript_source.as_deref() == Some("tui.local")
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

    pub(crate) fn replace_session_history_prompts(&mut self, prompts: Vec<String>) {
        let process_commands = self
            .history
            .iter()
            .zip(self.history_kinds.iter())
            .filter_map(|(entry, kind)| {
                (*kind == ComposerHistoryKind::ProcessCommand).then_some(entry.clone())
            })
            .collect::<Vec<_>>();
        self.history = prompts;
        self.history_kinds = vec![ComposerHistoryKind::SessionPrompt; self.history.len()];
        for command in process_commands {
            self.history.push(command);
            self.history_kinds.push(ComposerHistoryKind::ProcessCommand);
        }
        self.reset_history_navigation();
    }

    pub(crate) fn push_submitted_history(&mut self, submitted: String) {
        let kind = if submitted
            .trim_start()
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, '/' | '!'))
        {
            ComposerHistoryKind::ProcessCommand
        } else {
            ComposerHistoryKind::SessionPrompt
        };
        self.history.push(submitted);
        self.history_kinds.push(kind);
        self.reset_history_navigation();
    }

    pub(crate) fn reset_history_navigation(&mut self) {
        self.history_index = None;
        self.history_draft = None;
    }

    pub(crate) fn can_recall_history_previous(&self) -> bool {
        !self.history.is_empty() && self.textarea.cursor().0 == 0
    }

    pub(crate) fn can_recall_history_next(&self) -> bool {
        self.history_index.is_some() && self.textarea.cursor().0 + 1 >= self.textarea.lines().len()
    }

    pub(crate) fn clear_history_navigation_for_edit(&mut self) {
        if self.history_index.is_some() {
            self.history_index = None;
            self.history_draft = None;
        }
    }

    pub(crate) fn recall_history(&mut self, direction: isize) {
        if self.history.is_empty() {
            return;
        }
        if self.history_index.is_none() && direction < 0 {
            self.history_draft = Some(self.composer_submission_text());
        }
        let next = match self.history_index {
            None if direction < 0 => self.history.len().saturating_sub(1),
            None => return,
            Some(index) if direction < 0 => index.saturating_sub(1),
            Some(index) => {
                if index + 1 >= self.history.len() {
                    self.history_index = None;
                    let draft = self.history_draft.take().unwrap_or_default();
                    self.set_composer_text(&draft);
                    return;
                }
                index + 1
            }
        };
        self.history_index = Some(next);
        let entry = self.history[next].clone();
        self.set_composer_text(&entry);
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

    pub(crate) fn ensure_selection(&mut self) {
        if self
            .selected_target
            .is_some_and(|target| self.target_visible(target))
        {
            return;
        }
        if let Some(index) = self.selected_row
            && let Some(row) = self.transcript.get(index)
            && self.target_visible(TranscriptHitTarget::Row(row.id))
        {
            self.selected_target = Some(TranscriptHitTarget::Row(row.id));
            return;
        }
        let targets = self.visible_transcript_targets();
        self.selected_target = targets
            .iter()
            .copied()
            .find(|target| self.target_toggleable(*target))
            .or_else(|| targets.last().copied());
        self.selected_row = self
            .selected_target
            .and_then(|target| self.target_row_index(target));
    }

    pub(crate) fn move_selection(&mut self, direction: isize) {
        self.auto_follow_transcript = false;
        self.ensure_selection();
        let visible = self.visible_transcript_targets();
        if visible.is_empty() {
            self.selected_row = None;
            self.selected_target = None;
            return;
        }
        let current_position = self
            .selected_target
            .and_then(|current| visible.iter().position(|target| *target == current))
            .unwrap_or(0);
        let next_position = if direction < 0 {
            current_position.saturating_sub(direction.unsigned_abs())
        } else {
            current_position
                .saturating_add(direction as usize)
                .min(visible.len().saturating_sub(1))
        };
        self.set_selected_target(visible.get(next_position).copied());
        self.scroll_selected_target_into_view();
    }

    pub(crate) fn scroll_selected_target_into_view(&mut self) {
        let Some(selected) = self.selected_target else {
            return;
        };
        if !self.transcript_layout_matches_viewport() && self.last_transcript_width > 0 {
            refresh_transcript_layout(self, self.last_transcript_width);
        }
        let Some(block) = self
            .transcript_layout
            .blocks
            .iter()
            .find(|block| block.target == selected)
        else {
            return;
        };
        if block.height == 0 || self.last_transcript_height == 0 {
            return;
        }
        let viewport_start = usize::from(self.scroll);
        let viewport_end = viewport_start.saturating_add(usize::from(self.last_transcript_height));
        let row_start = block.start;
        let row_end = block.start.saturating_add(block.height);
        if row_start < viewport_start {
            self.scroll = row_start.min(usize::from(u16::MAX)) as u16;
        } else if row_end > viewport_end {
            let next = row_end.saturating_sub(usize::from(self.last_transcript_height));
            self.scroll = next.min(usize::from(u16::MAX)) as u16;
        }
        self.clamp_transcript_scroll();
    }

    pub(crate) fn toggle_selected(&mut self) {
        self.auto_follow_transcript = false;
        if self.selected_target.is_none() {
            self.ensure_selection();
        }
        if let Some(target) = self.selected_target {
            self.toggle_target(target);
        }
    }

    pub(crate) fn visible_transcript_targets(&self) -> Vec<TranscriptHitTarget> {
        transcript_render_blocks(self)
            .iter()
            .map(|block| block.target)
            .collect()
    }

    pub(crate) fn target_visible(&self, target: TranscriptHitTarget) -> bool {
        self.visible_transcript_targets()
            .into_iter()
            .any(|visible| visible == target)
    }

    pub(crate) fn target_row_index(&self, target: TranscriptHitTarget) -> Option<usize> {
        match target {
            TranscriptHitTarget::Row(row_id) => {
                self.transcript.iter().position(|row| row.id == row_id)
            }
            TranscriptHitTarget::AgentOpen(row_id) => {
                self.transcript.iter().position(|row| row.id == row_id)
            }
        }
    }

    pub(crate) fn agent_target_for_target(&self, target: TranscriptHitTarget) -> Option<String> {
        match target {
            TranscriptHitTarget::AgentOpen(row_id) => self
                .transcript
                .iter()
                .find(|row| row.id == row_id)
                .and_then(|row| row.agent_target.clone()),
            TranscriptHitTarget::Row(_) => None,
        }
    }

    pub(crate) fn selected_agent_target(&self) -> Option<String> {
        let target = self.selected_target?;
        let index = self.target_row_index(target)?;
        self.transcript
            .get(index)
            .filter(|row| row.agent_target.is_some())
            .and_then(|row| row.agent_target.clone())
    }

    pub(crate) fn visible_agent_target(&self) -> Option<String> {
        self.visible_transcript_targets()
            .into_iter()
            .rev()
            .find_map(|target| {
                self.target_row_index(target)
                    .and_then(|index| self.transcript.get(index))
                    .and_then(|row| row.agent_target.clone())
            })
            .or_else(|| {
                self.transcript
                    .iter()
                    .rev()
                    .find_map(|row| row.agent_target.clone())
            })
    }

    pub(crate) fn ensure_agent_open_selection(&mut self) {
        if self.selected_agent_target().is_some() {
            return;
        }
        let target = self
            .visible_transcript_targets()
            .into_iter()
            .rev()
            .find(|target| {
                self.target_row_index(*target)
                    .and_then(|index| self.transcript.get(index))
                    .is_some_and(|row| row.agent_target.is_some())
            })
            .or_else(|| {
                self.transcript
                    .iter()
                    .rev()
                    .find(|row| row.agent_target.is_some())
                    .map(|row| TranscriptHitTarget::Row(row.id))
            });
        if target.is_some() {
            self.set_selected_target(target);
        } else {
            self.ensure_selection();
        }
    }

    pub(crate) fn set_selected_target(&mut self, target: Option<TranscriptHitTarget>) {
        self.selected_target = target;
        self.selected_row = target.and_then(|target| self.target_row_index(target));
    }

    pub(crate) fn target_toggleable(&self, target: TranscriptHitTarget) -> bool {
        match target {
            TranscriptHitTarget::Row(row_id) => self
                .transcript
                .iter()
                .find(|row| row.id == row_id)
                .is_some_and(TranscriptRow::is_expandable),
            TranscriptHitTarget::AgentOpen(_) => false,
        }
    }

    pub(crate) fn toggle_target(&mut self, target: TranscriptHitTarget) {
        match target {
            TranscriptHitTarget::Row(row_id) | TranscriptHitTarget::AgentOpen(row_id) => {
                if let Some(row) = self.transcript.iter_mut().find(|row| row.id == row_id)
                    && row_visible(row, self.thinking_visible)
                    && row.is_expandable()
                {
                    toggle_transcript_row_details(row);
                }
            }
        }
        self.set_selected_target(Some(target));
        self.clamp_transcript_scroll();
    }

    pub(crate) fn transcript_hit(&self, column: u16, row: u16) -> Option<TranscriptHitTarget> {
        let mut first_hit = None;
        for (target, area) in &self.last_entry_areas {
            if !rect_contains(*area, column, row) {
                continue;
            }
            if matches!(target, TranscriptHitTarget::AgentOpen(_)) {
                return Some(*target);
            }
            first_hit.get_or_insert(*target);
        }
        first_hit
    }
}

pub(crate) fn auxiliary_agent_live_for_session(
    agent: &AuxiliaryAgentTask,
    session_id: &str,
) -> bool {
    if !agent.visible_live {
        return false;
    }
    agent.child_session_id.as_deref() == Some(session_id)
        || agent.session_id.as_deref() == Some(session_id)
}

pub(crate) fn current_session_matches(
    owner_session: Option<&str>,
    current_session: Option<&str>,
) -> bool {
    match owner_session {
        Some(owner_session) => current_session == Some(owner_session),
        None => true,
    }
}

fn completed_agent_invocation_row(row: &TranscriptRow) -> bool {
    row.tool_name.as_deref() == Some("spawn_agent")
        && !active_tool_row(row)
        && (row.tool_started.is_none()
            || row.tool_elapsed.is_some()
            || row.agent_target.is_some()
            || row.failed
            || row.interrupted)
}

pub(crate) fn apply_agent_child_value_preview(row: &mut TranscriptRow, value: &Value) -> bool {
    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "tool_execution_start" => {
            let tool = value
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            append_agent_child_live_line(
                &mut row.agent_child_live_text,
                active_tool_title(tool, value),
            );
            true
        }
        "tool_execution_end" => {
            row.agent_child_tool_uses = row.agent_child_tool_uses.saturating_add(1);
            let tool = value
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            append_agent_child_live_line(&mut row.agent_child_live_text, tool_title(tool, value));
            true
        }
        "message_end" => {
            if let Some(usage) = value.get("usage") {
                row.agent_child_latest_tokens =
                    usage_total_tokens(usage).or(row.agent_child_latest_tokens);
            }
            if let Some(text) =
                assistant_text_from_event(value).filter(|text| !text.trim().is_empty())
            {
                append_agent_child_live_line(
                    &mut row.agent_child_live_text,
                    format!("Response: {}", single_line_preview(&text, 160)),
                );
            }
            true
        }
        "agent_end" => true,
        _ => false,
    }
}

pub(crate) fn append_agent_child_live_line(buffer: &mut String, line: impl AsRef<str>) {
    let line = line.as_ref().trim();
    if line.is_empty() {
        return;
    }
    if !buffer.is_empty() {
        buffer.push('\n');
    }
    buffer.push_str(line);
}

pub(crate) fn append_agent_child_live_fragment(
    buffer: &mut String,
    label: &str,
    fragment: &str,
) -> bool {
    if fragment.trim().is_empty() {
        return false;
    }
    let prefix = format!("{label}: ");
    let last_line_start = buffer.rfind('\n').map(|index| index + 1).unwrap_or(0);
    if buffer
        .get(last_line_start..)
        .is_some_and(|line| line.starts_with(&prefix))
    {
        buffer.push_str(fragment);
        return true;
    }
    append_agent_child_live_line(buffer, format!("{prefix}{}", fragment.trim_start()));
    true
}

pub(crate) fn refresh_agent_child_preview(row: &mut TranscriptRow) {
    let status = if active_tool_row(row) {
        "Running"
    } else if row.interrupted {
        "Interrupted"
    } else if row.failed {
        "Failed"
    } else {
        "Done"
    };
    let status = agent_child_status_text(
        status,
        row.agent_child_tool_uses,
        row.agent_child_latest_tokens,
    );
    if row.agent_child_live_text.trim().is_empty() {
        row.text = status;
        row.full_text = None;
        return;
    }
    let full = format!("{status}\n{}", row.agent_child_live_text);
    row.set_evidence_body_text(full);
}

pub(crate) fn agent_child_status_text(status: &str, tool_uses: i64, tokens: Option<u64>) -> String {
    let token_suffix = tokens
        .map(|tokens| format!(" · {} tokens", format_compact_count(tokens)))
        .unwrap_or_default();
    format!(
        "{status} ({} {}{})",
        tool_uses,
        pluralize(tool_uses, "tool use"),
        token_suffix
    )
}

pub(crate) fn exec_session_id_from_args(args: &Value) -> Option<u64> {
    args.get("session_id").and_then(Value::as_u64)
}

pub(crate) fn exec_session_id_from_result(value: &Value) -> Option<u64> {
    value
        .get("result")
        .and_then(|result| result.get("session_id"))
        .and_then(Value::as_u64)
}

pub(crate) fn exec_result_running(value: &Value) -> bool {
    exec_session_id_from_result(value).is_some()
        && value
            .get("result")
            .and_then(|result| result.get("exit_code"))
            .is_none_or(Value::is_null)
}

pub(crate) fn exec_result_completed(value: &Value) -> bool {
    value
        .get("result")
        .and_then(|result| result.get("exit_code"))
        .is_some_and(|exit_code| !exit_code.is_null())
}

pub(crate) fn tool_result_output(value: &Value) -> String {
    value
        .get("result")
        .and_then(|result| result.get("output"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn write_stdin_non_empty_chars(args: &Value) -> Option<&str> {
    args.get("chars")
        .and_then(Value::as_str)
        .filter(|chars| !chars.is_empty())
}

pub(crate) fn bounded_stdin_display(chars: &str) -> String {
    const MAX_CHARS: usize = 4096;
    if chars.chars().count() <= MAX_CHARS {
        return chars.to_string();
    }
    let mut output = chars.chars().take(MAX_CHARS).collect::<String>();
    output.push_str("\n... truncated");
    output
}
