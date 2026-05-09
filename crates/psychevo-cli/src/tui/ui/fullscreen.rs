impl<'a> FullscreenUi<'a> {
    fn new(app: &TuiApp) -> Self {
        let mut ui = Self {
            textarea: new_textarea(),
            transcript: Vec::new(),
            assistant_row: None,
            reasoning_row: None,
            meta_row: None,
            tool_rows: BTreeMap::new(),
            streaming_tool_message_seq: 0,
            streaming_tool_message_open: false,
            history_tool_titles: BTreeMap::new(),
            turn_started: None,
            turn_provider: String::new(),
            turn_model: String::new(),
            turn_mode: app.current_mode.as_str().to_string(),
            turn_context_limit: None,
            turn_usage: None,
            turn_metadata: None,
            turn_failures: 0,
            turn_outcome: None,
            history_prompt_started_ms: None,
            thinking_visible: app.thinking_visible,
            running: None,
            auxiliary_agent_tasks: Vec::new(),
            running_started: None,
            #[cfg(test)]
            running_elapsed_override: None,
            interrupt_requested: false,
            scroll: 0,
            last_transcript_height: 0,
            last_transcript_width: 0,
            transcript_layout: TranscriptLayoutCache::default(),
            auto_follow_transcript: true,
            pending_scroll_to_bottom: false,
            focus: FocusMode::Composer,
            selected_row: None,
            last_entry_areas: Vec::new(),
            sidebar_forced: app.state.sidebar_visible,
            sidebar_hidden: !app.state.sidebar_visible,
            last_sidebar_visible: false,
            sidebar: SidebarSnapshot::default(),
            sidebar_tokens: None,
            sidebar_context_limit: None,
            history: Vec::new(),
            history_kinds: Vec::new(),
            history_index: None,
            history_draft: None,
            queued_inputs: VecDeque::new(),
            history_search: false,
            history_query: String::new(),
            slash_menu_selected: 0,
            slash_menu_dismissed_input: None,
            last_slash_menu_areas: Vec::new(),
            file_search: FileSearchState::new(),
            last_file_popup_areas: Vec::new(),
            skill_search: SkillSearchState::default(),
            last_skill_popup_areas: Vec::new(),
            last_bottom_panel_areas: Vec::new(),
            bottom_panel: None,
            screen_lines: Vec::new(),
            selection: SelectionState::default(),
            terminal_clear_requested: false,
            quit_requested: false,
        };
        ui.refresh_sidebar(app);
        ui
    }

    fn refresh_sidebar(&mut self, app: &TuiApp) {
        let git = git_snapshot(&app.workdir);
        self.sidebar = SidebarSnapshot {
            title: app.session_sidebar_title(),
            session: app
                .current_session
                .as_deref()
                .map(short_session)
                .unwrap_or("(none)")
                .to_string(),
            workdir: tail_compact_path(&app.workdir.display().to_string(), 30),
            branch: git.branch,
            tokens: self.sidebar_tokens,
            context_percent: self.context_percent(),
            message_count: visible_transcript_message_count(&self.transcript),
            tool_count: self
                .transcript
                .iter()
                .filter(|row| {
                    matches!(
                        row.kind,
                        TranscriptKind::Explored | TranscriptKind::Ran | TranscriptKind::Changed
                    )
                })
                .count(),
            changed_files: git.changed_files,
        };
    }

    fn clear_transcript(&mut self) {
        self.transcript.clear();
        self.assistant_row = None;
        self.reasoning_row = None;
        self.meta_row = None;
        self.tool_rows.clear();
        self.history_tool_titles.clear();
        self.scroll = 0;
        self.last_transcript_height = 0;
        self.last_transcript_width = 0;
        self.transcript_layout = TranscriptLayoutCache::default();
        self.auto_follow_transcript = true;
        self.selected_row = None;
        self.last_entry_areas.clear();
        self.selection = SelectionState::default();
        self.terminal_clear_requested = true;
        self.sidebar_tokens = None;
        self.sidebar_context_limit = None;
        self.history_prompt_started_ms = None;
    }

    fn take_terminal_clear_request(&mut self) -> bool {
        std::mem::take(&mut self.terminal_clear_requested)
    }

    fn set_thinking_visible(&mut self, visible: bool) {
        self.thinking_visible = visible;
        if self
            .selected_row
            .and_then(|index| self.transcript.get(index))
            .is_some_and(|row| !row_visible(row, self.thinking_visible))
        {
            self.selected_row = None;
            self.ensure_selection();
        }
        self.clamp_transcript_scroll();
    }

    fn scroll_transcript(&mut self, amount: isize) {
        if amount < 0 {
            self.scroll = self.scroll.saturating_sub(amount.unsigned_abs() as u16);
        } else {
            self.scroll = self.scroll.saturating_add(amount as u16);
        }
        self.pending_scroll_to_bottom = false;
        let max_scroll = if self.transcript_layout.width == self.last_transcript_width
            && self.transcript_layout.thinking_visible == self.thinking_visible
            && self.transcript_layout.rows.len() == self.transcript.len()
        {
            Some(self.transcript_layout.max_scroll(self.last_transcript_height))
        } else {
            None
        };
        if let Some(max_scroll) = max_scroll {
            self.scroll = self.scroll.min(max_scroll);
            self.auto_follow_transcript = amount > 0 && self.scroll >= max_scroll;
        } else {
            self.clamp_transcript_scroll();
            self.auto_follow_transcript = amount > 0 && self.is_transcript_at_bottom();
        }
    }

    fn clamp_transcript_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_transcript_scroll());
    }

    fn max_transcript_scroll(&self) -> u16 {
        let total = transcript_line_count(
            &self.transcript,
            self.last_transcript_width,
            self.thinking_visible,
        )
        .min(usize::from(u16::MAX)) as u16;
        total.saturating_sub(self.last_transcript_height)
    }

    fn is_transcript_at_bottom(&self) -> bool {
        self.scroll >= self.max_transcript_scroll()
    }

    fn follow_transcript_if_needed(&mut self) {
        if self.auto_follow_transcript {
            self.scroll_to_bottom();
        } else {
            self.clamp_transcript_scroll();
        }
    }

    fn resolve_transcript_scroll_for_render_with_total(&mut self, total_height: usize) {
        let max_scroll = {
            let total = total_height.min(usize::from(u16::MAX)) as u16;
            total.saturating_sub(self.last_transcript_height)
        };
        if std::mem::take(&mut self.pending_scroll_to_bottom) {
            self.scroll = max_scroll;
            self.auto_follow_transcript = true;
        } else if self.auto_follow_transcript {
            self.scroll = max_scroll;
        } else {
            self.scroll = self.scroll.min(max_scroll);
            if self.scroll >= max_scroll {
                self.auto_follow_transcript = true;
            }
        }
    }

    fn context_percent(&self) -> Option<f64> {
        let tokens = self.sidebar_tokens?;
        let limit = self.sidebar_context_limit?;
        (limit > 0).then_some((tokens as f64 / limit as f64) * 100.0)
    }

    fn sidebar_enabled(&self) -> bool {
        self.sidebar_forced && !self.sidebar_hidden
    }

    fn clear_screen_lines(&mut self) {
        self.screen_lines.clear();
    }

    #[cfg(test)]
    fn push_screen_line(&mut self, x: u16, y: u16, text: impl Into<String>) {
        let text = text.into();
        self.screen_lines.push(ScreenLine {
            region: SelectableRegion::Transcript,
            y,
            cells: screen_cells_from_text(x, &text),
        });
    }

    fn capture_selectable_rows(
        &mut self,
        buffer: &ratatui::buffer::Buffer,
        area: Rect,
        region: SelectableRegion,
    ) {
        let area = buffer.area().intersection(area);
        if area.is_empty() {
            return;
        }
        for y in area.y..area.y.saturating_add(area.height) {
            if let Some(line) = screen_line_from_buffer(buffer, area.x, y, area.width, region) {
                self.screen_lines.push(line);
            }
        }
    }

    fn selectable_hit(&self, column: u16, row: u16) -> bool {
        self.screen_lines.iter().any(|line| {
            line.y == row
                && line
                    .cells
                    .iter()
                    .any(|cell| column >= cell.x && column < cell.x.saturating_add(cell.width))
        })
    }

    fn selection_region_at(&self, column: u16, row: u16) -> Option<SelectableRegion> {
        self.screen_lines
            .iter()
            .find(|line| {
                line.y == row
                    && line
                        .cells
                        .iter()
                        .any(|cell| column >= cell.x && column < cell.x.saturating_add(cell.width))
            })
            .map(|line| line.region)
    }

    fn start_selection(&mut self, column: u16, row: u16) {
        self.selection.anchor = Some((column, row));
        self.selection.focus = Some((column, row));
        self.selection.region = self.selection_region_at(column, row);
    }

    fn update_selection(&mut self, column: u16, row: u16) {
        if self.selection.anchor.is_some() {
            self.selection.focus = Some((column, row));
        }
    }

    fn clear_selection(&mut self) {
        self.selection = SelectionState::default();
    }

    fn selected_text(&self) -> Option<String> {
        selected_text_from_lines(&self.screen_lines, &self.selection)
    }

    fn push_history_message(
        &mut self,
        message: &Value,
        usage: Option<&Value>,
        metadata: Option<&Value>,
    ) {
        match message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "user" => {
                if let Some(text) = user_text_from_message(message) {
                    self.push_user(text);
                }
                self.history_prompt_started_ms = message_timestamp_ms(message);
            }
            "assistant" => {
                for (tool_call_id, title) in history_tool_titles_from_message(message) {
                    self.history_tool_titles.insert(tool_call_id, title);
                }
                if let Some(reasoning) = assistant_reasoning_from_message(message) {
                    self.transcript.push(TranscriptRow::with_title(
                        TranscriptKind::Thinking,
                        "Thinking",
                        reasoning,
                    ));
                }
                let has_answer = if let Some(text) = assistant_text_from_message(message) {
                    self.transcript.push(TranscriptRow::with_title(
                        TranscriptKind::Answer,
                        "",
                        text,
                    ));
                    true
                } else {
                    false
                };
                if let Some(total) = usage.and_then(usage_total_tokens) {
                    self.sidebar_tokens = Some(total);
                }
                if has_answer
                    && let Some(meta) =
                        history_meta_text(message, usage, metadata, self.history_prompt_started_ms)
                {
                    self.transcript
                        .push(TranscriptRow::with_title(TranscriptKind::Meta, "", meta));
                }
                self.history_prompt_started_ms = None;
            }
            "tool_result" => self.push_history_tool_result(message, metadata),
            _ => {}
        }
    }

    fn push_history_tool_result(&mut self, message: &Value, metadata: Option<&Value>) {
        let tool = message
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let is_error = message
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let tool_call_id = message
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let result = serde_json::from_str::<Value>(content)
            .unwrap_or_else(|_| serde_json::json!({ "content": content }));
        let value = serde_json::json!({
            "tool_name": tool,
            "result": result,
            "outcome": if is_error { "failed" } else { "normal" }
        });
        let title = self
            .history_tool_titles
            .get(tool_call_id)
            .cloned()
            .unwrap_or_else(|| tool_title(tool, &value));
        let mut row = TranscriptRow::with_title(evidence_kind(tool), title, "");
        row.failed = is_error;
        row.tool_elapsed = metadata_elapsed_duration(metadata);
        let (collapsed, full) = tool_output_text(&value);
        row.text = if collapsed.is_empty() {
            format_tool_summary(&value)
        } else {
            collapsed
        };
        row.full_text = full;
        self.transcript.push(row);
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll = self.max_transcript_scroll();
        self.auto_follow_transcript = true;
        self.pending_scroll_to_bottom = true;
    }

    fn running_elapsed(&self) -> Option<Duration> {
        #[cfg(test)]
        if let Some(elapsed) = self.running_elapsed_override {
            return Some(elapsed);
        }
        self.running_started
            .or(self.turn_started)
            .map(|started| started.elapsed())
    }

    fn request_interrupt(&mut self) -> bool {
        let Some(running) = &self.running else {
            return false;
        };
        if !self.interrupt_requested {
            running.control.abort();
            self.interrupt_requested = true;
        }
        true
    }

    fn restore_queued_inputs_to_composer(&mut self) {
        if self.queued_inputs.is_empty() {
            return;
        }
        let mut parts = self
            .queued_inputs
            .drain(..)
            .map(queued_input_text)
            .collect::<Vec<_>>();
        let draft = textarea_text(&self.textarea);
        if !draft.is_empty() {
            parts.push(draft);
        }
        self.textarea = textarea_with_text(&parts.join("\n"));
        self.reset_history_navigation();
        self.clear_slash_menu_dismissal();
        self.close_file_popup();
        self.close_skill_popup();
    }

    fn complete_slash_command(&mut self) {
        let input = textarea_text(&self.textarea);
        if let Some(completed) = slash_completion(&input) {
            self.textarea = textarea_with_text(&completed);
            self.slash_menu_selected = 0;
            self.clear_slash_menu_dismissal();
        }
    }

    fn current_file_token(&self) -> Option<FileToken> {
        current_file_token(&self.textarea)
    }

    fn sync_file_popup(&mut self, root: &Path) {
        let token = self.current_file_token();
        self.file_search.sync(root, token.as_ref());
    }

    fn drain_file_search_results(&mut self) -> bool {
        self.file_search.drain_results()
    }

    fn file_popup_visible(&self) -> bool {
        self.file_search.popup.is_some()
    }

    fn file_popup_height(&self) -> u16 {
        self.file_search.height()
    }

    fn close_file_popup(&mut self) {
        self.file_search.close();
        self.last_file_popup_areas.clear();
    }

    fn dismiss_file_popup(&mut self) {
        let query = self.current_file_token().map(|token| token.query);
        self.file_search.dismiss(query);
        self.last_file_popup_areas.clear();
    }

    fn selected_file_path(&self) -> Option<String> {
        self.file_search.selected_path()
    }

    fn move_file_popup_selection(&mut self, direction: isize) {
        self.file_search.move_selection(direction);
    }

    fn set_file_popup_selection(&mut self, index: usize) {
        self.file_search.set_selection(index);
    }

    fn insert_selected_file_path(&mut self) {
        let Some(path) = self.selected_file_path() else {
            return;
        };
        if replace_current_file_token(&mut self.textarea, &path) {
            self.file_search.close();
            self.file_search.dismissed_query = None;
            self.last_file_popup_areas.clear();
        }
    }

    fn file_popup_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_file_popup_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    fn current_skill_token(&self) -> Option<SkillToken> {
        current_skill_token(&self.textarea)
    }

    fn sync_skill_popup(&mut self, matches: Vec<SkillSearchMatch>) {
        let token = self.current_skill_token();
        self.skill_search.sync(token.as_ref(), matches);
    }

    fn skill_popup_visible(&self) -> bool {
        self.skill_search.popup.is_some()
    }

    fn skill_popup_height(&self) -> u16 {
        self.skill_search.height()
    }

    fn close_skill_popup(&mut self) {
        self.skill_search.close();
        self.last_skill_popup_areas.clear();
    }

    fn dismiss_skill_popup(&mut self) {
        let query = self.current_skill_token().map(|token| token.query);
        self.skill_search.dismiss(query);
        self.last_skill_popup_areas.clear();
    }

    fn selected_skill_name(&self) -> Option<String> {
        self.skill_search.selected_name()
    }

    fn move_skill_popup_selection(&mut self, direction: isize) {
        self.skill_search.move_selection(direction);
    }

    fn set_skill_popup_selection(&mut self, index: usize) {
        self.skill_search.set_selection(index);
    }

    fn insert_selected_skill_marker(&mut self) {
        let Some(name) = self.selected_skill_name() else {
            return;
        };
        self.insert_skill_marker(&name);
    }

    fn insert_skill_marker(&mut self, name: &str) {
        if replace_current_skill_token(&mut self.textarea, name) {
            self.skill_search.close();
            self.skill_search.dismissed_query = None;
            self.last_skill_popup_areas.clear();
            self.clear_slash_menu_dismissal();
        } else {
            self.textarea = textarea_with_text(&format!("${name} "));
            self.close_skill_popup();
            self.slash_menu_selected = 0;
            self.clear_slash_menu_dismissal();
        }
    }

    fn skill_popup_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_skill_popup_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    fn clamp_slash_menu_selection(&mut self, len: usize) {
        if len == 0 {
            self.slash_menu_selected = 0;
            self.last_slash_menu_areas.clear();
            return;
        }
        self.slash_menu_selected = self.slash_menu_selected.min(len.saturating_sub(1));
    }

    fn move_slash_menu_selection(&mut self, direction: isize, len: usize) {
        if len == 0 {
            self.slash_menu_selected = 0;
            return;
        }
        let current = self.slash_menu_selected.min(len.saturating_sub(1)) as isize;
        let next = (current + direction).rem_euclid(len as isize) as usize;
        self.slash_menu_selected = next;
    }

    fn set_slash_menu_selection(&mut self, index: usize, len: usize) {
        self.slash_menu_selected = if len == 0 {
            0
        } else {
            index.min(len.saturating_sub(1))
        };
    }

    fn slash_menu_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_slash_menu_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    fn slash_menu_dismissed(&self, input: &str) -> bool {
        self.slash_menu_dismissed_input.as_deref() == Some(input)
    }

    fn dismiss_slash_menu(&mut self) {
        self.slash_menu_dismissed_input = Some(textarea_text(&self.textarea));
        self.slash_menu_selected = 0;
        self.last_slash_menu_areas.clear();
    }

    fn clear_slash_menu_dismissal(&mut self) {
        self.slash_menu_dismissed_input = None;
    }

    fn bottom_panel_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_bottom_panel_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    fn set_bottom_panel_notice(&mut self, text: impl Into<String>) {
        if let Some(panel) = &mut self.bottom_panel {
            match panel {
                BottomPanel::ProviderWizard(panel) => panel.notice = Some(text.into()),
                _ => panel.selection_mut().notice = Some(text.into()),
            }
        }
    }

    fn push_user(&mut self, text: String) {
        self.transcript
            .push(TranscriptRow::with_title(TranscriptKind::Prompt, "", text));
    }

    fn start_assistant(&mut self) {
        self.assistant_row = None;
        self.reasoning_row = None;
        self.meta_row = None;
        self.tool_rows.clear();
        self.streaming_tool_message_seq = 0;
        self.streaming_tool_message_open = false;
        self.turn_started = None;
        self.running_started = Some(Instant::now());
        self.interrupt_requested = false;
        self.turn_provider.clear();
        self.turn_model.clear();
        self.turn_context_limit = None;
        self.turn_usage = None;
        self.turn_metadata = None;
        self.turn_failures = 0;
        self.turn_outcome = None;
    }

    fn push_status(&mut self, text: impl Into<String>) {
        self.transcript
            .push(TranscriptRow::simple(TranscriptKind::Status, text));
    }

    fn push_error(&mut self, text: impl Into<String>) {
        self.transcript
            .push(TranscriptRow::simple(TranscriptKind::Error, text));
    }

    fn insert_transcript_row(&mut self, index: usize, row: TranscriptRow) -> usize {
        let index = index.min(self.transcript.len());
        self.transcript.insert(index, row);
        increment_row_index(&mut self.assistant_row, index);
        increment_row_index(&mut self.reasoning_row, index);
        increment_row_index(&mut self.meta_row, index);
        increment_row_index(&mut self.selected_row, index);
        for row_index in self.tool_rows.values_mut() {
            if *row_index >= index {
                *row_index += 1;
            }
        }
        index
    }

    fn insert_evidence_row(&mut self, row: TranscriptRow) -> usize {
        let index = self
            .assistant_row
            .or(self.meta_row)
            .unwrap_or(self.transcript.len());
        self.insert_transcript_row(index, row)
    }

    fn insert_answer_row(&mut self, row: TranscriptRow) -> usize {
        let index = self.meta_row.unwrap_or(self.transcript.len());
        self.insert_transcript_row(index, row)
    }

    fn apply_stream_event(&mut self, event: RunStreamEvent, _thinking_visible: bool, debug: bool) {
        match event {
            RunStreamEvent::ReasoningDelta { text } => {
                let idx = self.reasoning_row.unwrap_or_else(|| {
                    let idx = self.insert_evidence_row(TranscriptRow::with_title(
                        TranscriptKind::Thinking,
                        "Thinking",
                        String::new(),
                    ));
                    self.reasoning_row = Some(idx);
                    idx
                });
                self.transcript[idx].text.push_str(&text);
            }
            RunStreamEvent::ReasoningEnd => {
                self.reasoning_row = None;
            }
            RunStreamEvent::Event(value) => self.apply_value_event(&value, debug),
        }
    }

    fn apply_value_event(&mut self, value: &Value, debug: bool) {
        match value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "run_start" => {
                self.turn_started = Some(Instant::now());
                self.turn_provider = value
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("provider")
                    .to_string();
                self.turn_model = value
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("model")
                    .to_string();
                self.turn_mode = value
                    .get("mode")
                    .and_then(Value::as_str)
                    .unwrap_or("default")
                    .to_string();
                self.turn_context_limit = value.get("context_limit").and_then(Value::as_u64);
                self.sidebar_context_limit = self.turn_context_limit;
                if let Some(skills) = selected_skill_names_from_event(value)
                    && !skills.is_empty()
                {
                    self.push_status(format!("skill loaded: {}", skills.join(", ")));
                }
            }
            "message_update" | "message_end" => {
                self.apply_streaming_tool_calls(value);
                if let Some(text) =
                    assistant_text_from_event(value).filter(|text| !text.trim().is_empty())
                {
                    let idx = self.assistant_row.unwrap_or_else(|| {
                        let idx = self.insert_answer_row(TranscriptRow::with_title(
                            TranscriptKind::Answer,
                            "",
                            String::new(),
                        ));
                        self.assistant_row = Some(idx);
                        idx
                    });
                    self.transcript[idx].text = text;
                }
                if value.get("type").and_then(Value::as_str) == Some("message_end") {
                    self.turn_usage = value.get("usage").cloned();
                    self.sidebar_tokens = self.turn_usage.as_ref().and_then(usage_total_tokens);
                    self.turn_metadata = value.get("metadata").cloned();
                    self.update_turn_meta(debug);
                    if value
                        .get("message")
                        .and_then(|message| message.get("role"))
                        .and_then(Value::as_str)
                        == Some("assistant")
                    {
                        self.assistant_row = None;
                    }
                }
            }
            "tool_execution_start" => {
                let tool = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let tool_call_id = value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let id_key = (!tool_call_id.is_empty()).then(|| tool_id_key(&tool_call_id));
                let idx = id_key
                    .as_ref()
                    .and_then(|key| self.tool_rows.get(key))
                    .copied()
                    .unwrap_or_else(|| {
                        let mut row = TranscriptRow::with_title(
                            evidence_kind(tool),
                            active_tool_title(tool, value),
                            "running",
                        );
                        row.tool_call_id =
                            (!tool_call_id.is_empty()).then_some(tool_call_id.clone());
                        row.tool_started = Some(tool_started_instant(value));
                        self.insert_evidence_row(row)
                    });
                let row = &mut self.transcript[idx];
                row.kind = evidence_kind(tool);
                row.title = active_tool_title(tool, value);
                row.text = "running".to_string();
                row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.clone());
                if row.tool_started.is_none() {
                    row.tool_started = Some(tool_started_instant(value));
                }
                if let Some(id_key) = id_key {
                    self.tool_rows.insert(id_key, idx);
                }
            }
            "tool_execution_end" => {
                let user_shell = value.get("source").and_then(Value::as_str) == Some("user_shell");
                let tool = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let tool_call_id = value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let outcome = value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .unwrap_or("normal");
                let failed = outcome != "normal";
                if failed && !user_shell {
                    self.turn_failures += 1;
                }
                let idx = self
                    .tool_rows
                    .get(&tool_id_key(tool_call_id))
                    .copied()
                    .unwrap_or_else(|| {
                        self.insert_evidence_row(TranscriptRow::with_title(
                            evidence_kind(tool),
                            tool_title(tool, value),
                            String::new(),
                        ))
                    });
                let row = &mut self.transcript[idx];
                row.kind = evidence_kind(tool);
                row.title = tool_title_for_update(tool, value, &row.title);
                row.failed = failed;
                row.tool_elapsed = metadata_elapsed_duration(Some(value))
                    .or_else(|| row.tool_started.map(|started| started.elapsed()));
                row.tool_started = None;
                let (collapsed, full) = tool_output_text(value);
                row.text = if collapsed.is_empty() {
                    format_tool_summary(value)
                } else {
                    collapsed
                };
                row.full_text = full;
                if !user_shell {
                    self.update_turn_meta(debug);
                }
            }
            "agent_end" => {
                self.turn_outcome = outcome_from_value(value);
            }
            _ => {}
        }
    }

    fn apply_streaming_tool_calls(&mut self, value: &Value) {
        let Some(event_type) = assistant_message_stream_event_type(value) else {
            return;
        };
        if !self.streaming_tool_message_open {
            self.streaming_tool_message_seq = self.streaming_tool_message_seq.saturating_add(1);
            self.streaming_tool_message_open = true;
        }
        let message_scope = self.streaming_tool_message_seq;
        for mut call in streaming_tool_calls_from_event(value) {
            call.position_key = scoped_tool_position_key(message_scope, &call.position_key);
            self.upsert_streaming_tool_call(call);
        }
        if event_type == "message_end" {
            self.streaming_tool_message_open = false;
        }
    }

    fn upsert_streaming_tool_call(&mut self, call: StreamingToolCall) {
        let id_key = call.id.as_deref().map(tool_id_key);
        let idx = id_key
            .as_ref()
            .and_then(|key| self.tool_rows.get(key))
            .or_else(|| self.tool_rows.get(&call.position_key))
            .copied();
        let value = serde_json::json!({ "args": call.args });
        let idx = if let Some(idx) = idx {
            let row = &mut self.transcript[idx];
            row.kind = evidence_kind(&call.tool_name);
            row.title = active_tool_title(&call.tool_name, &value);
            if row.text.is_empty() {
                row.text = "preparing".to_string();
            }
            if call.id.is_some() {
                row.tool_call_id = call.id.clone();
            }
            if row.tool_started.is_none() {
                row.tool_started = Some(Instant::now());
            }
            idx
        } else {
            let mut row = TranscriptRow::with_title(
                evidence_kind(&call.tool_name),
                active_tool_title(&call.tool_name, &value),
                "preparing",
            );
            row.tool_call_id = call.id.clone();
            row.tool_started = Some(Instant::now());
            self.insert_evidence_row(row)
        };
        self.tool_rows.insert(call.position_key, idx);
        if let Some(id_key) = id_key {
            self.tool_rows.insert(id_key, idx);
        }
    }

    fn finish_turn(&mut self) {
        self.mark_unfinished_tools_interrupted();
        self.assistant_row = None;
        self.reasoning_row = None;
        self.meta_row = None;
        self.tool_rows.clear();
        self.streaming_tool_message_open = false;
        self.turn_outcome = None;
        self.running_started = None;
        self.interrupt_requested = false;
        self.focus = FocusMode::Composer;
    }

    fn mark_unfinished_tools_interrupted(&mut self) {
        let mut indices = self.tool_rows.values().copied().collect::<Vec<_>>();
        indices.sort_unstable();
        indices.dedup();
        for index in indices {
            let Some(row) = self.transcript.get_mut(index) else {
                continue;
            };
            let Some(started) = row.tool_started.take() else {
                continue;
            };
            row.tool_elapsed = Some(started.elapsed());
            row.failed = true;
            row.text = "interrupted".to_string();
        }
    }

    fn replace_session_history_prompts(&mut self, prompts: Vec<String>) {
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

    fn push_submitted_history(&mut self, submitted: String) {
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

    fn reset_history_navigation(&mut self) {
        self.history_index = None;
        self.history_draft = None;
    }

    fn can_recall_history_previous(&self) -> bool {
        !self.history.is_empty() && self.textarea.cursor().0 == 0
    }

    fn can_recall_history_next(&self) -> bool {
        self.history_index.is_some() && self.textarea.cursor().0 + 1 >= self.textarea.lines().len()
    }

    fn clear_history_navigation_for_edit(&mut self) {
        if self.history_index.is_some() {
            self.history_index = None;
            self.history_draft = None;
        }
    }

    fn recall_history(&mut self, direction: isize) {
        if self.history.is_empty() {
            return;
        }
        if self.history_index.is_none() && direction < 0 {
            self.history_draft = Some(textarea_text(&self.textarea));
        }
        let next = match self.history_index {
            None if direction < 0 => self.history.len().saturating_sub(1),
            None => return,
            Some(index) if direction < 0 => index.saturating_sub(1),
            Some(index) => {
                if index + 1 >= self.history.len() {
                    self.history_index = None;
                    self.textarea = match self.history_draft.take() {
                        Some(draft) if !draft.is_empty() => textarea_with_text(&draft),
                        _ => new_textarea(),
                    };
                    return;
                }
                index + 1
            }
        };
        self.history_index = Some(next);
        self.textarea = textarea_with_text(&self.history[next]);
    }

    fn update_turn_meta(&mut self, debug: bool) {
        if self.assistant_row.is_none() && self.turn_failures == 0 {
            return;
        }
        let meta = turn_meta_text(TurnMetaProjection {
            mode: &self.turn_mode,
            provider: &self.turn_provider,
            model: &self.turn_model,
            started: self.turn_started,
            usage: self.turn_usage.as_ref(),
            metadata: self.turn_metadata.as_ref(),
            failures: self.turn_failures,
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
        self.transcript[idx].text = meta;
    }

    fn ensure_selection(&mut self) {
        if self.selected_row.is_some_and(|idx| {
            self.transcript
                .get(idx)
                .is_some_and(|row| row_visible(row, self.thinking_visible))
        }) {
            return;
        }
        self.selected_row = self
            .transcript
            .iter()
            .position(|row| row_visible(row, self.thinking_visible) && row.is_expandable())
            .or_else(|| {
                self.transcript
                    .iter()
                    .rposition(|row| row_visible(row, self.thinking_visible))
            });
    }

    fn move_selection(&mut self, direction: isize) {
        if self.transcript.is_empty() {
            self.selected_row = None;
            return;
        }
        self.auto_follow_transcript = false;
        self.ensure_selection();
        let visible = self
            .transcript
            .iter()
            .enumerate()
            .filter_map(|(index, row)| row_visible(row, self.thinking_visible).then_some(index))
            .collect::<Vec<_>>();
        if visible.is_empty() {
            self.selected_row = None;
            return;
        }
        let current_position = self
            .selected_row
            .and_then(|current| visible.iter().position(|index| *index == current))
            .unwrap_or(0);
        let next_position = if direction < 0 {
            current_position.saturating_sub(direction.unsigned_abs())
        } else {
            current_position
                .saturating_add(direction as usize)
                .min(visible.len().saturating_sub(1))
        };
        self.selected_row = visible.get(next_position).copied();
    }

    fn toggle_selected(&mut self) {
        self.auto_follow_transcript = false;
        if let Some(index) = self.selected_row
            && let Some(row) = self.transcript.get_mut(index)
            && row_visible(row, self.thinking_visible)
            && row.is_expandable()
        {
            row.expanded = !row.expanded;
        }
    }
}

fn selected_skill_names_from_event(value: &Value) -> Option<Vec<String>> {
    value.get("selected_skills")?.as_array().map(|skills| {
        skills
            .iter()
            .filter_map(|skill| skill.get("name").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .collect()
    })
}
