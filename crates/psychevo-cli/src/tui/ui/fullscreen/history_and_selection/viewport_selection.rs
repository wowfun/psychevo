
impl<'a> FullscreenUi<'a> {
    pub(crate) fn new(app: &TuiApp) -> Self {
        let mut ui = Self {
            textarea: new_textarea(),
            cwd: app.cwd.clone(),
            transcript: Vec::new(),
            assistant_row: None,
            assistant_preamble_row: None,
            reasoning_row: None,
            meta_row: None,
            gateway_item_rows: BTreeMap::new(),
            tool_rows: BTreeMap::new(),
            write_preview_trackers: BTreeMap::new(),
            streaming_tool_message_seq: 0,
            streaming_tool_message_open: false,
            deferred_stream_events: VecDeque::new(),
            history_tool_titles: BTreeMap::new(),
            history_tool_args: BTreeMap::new(),
            live_tool_args: BTreeMap::new(),
            clarify_tool_args: BTreeMap::new(),
            exec_session_rows: BTreeMap::new(),
            exec_session_elapsed: BTreeMap::new(),
            shell_mode: false,
            turn_started: None,
            turn_provider: String::new(),
            turn_model: String::new(),
            turn_mode: app.current_mode.as_str().to_string(),
            turn_context_limit: None,
            turn_usage: None,
            turn_metadata: None,
            turn_accounting: None,
            turn_session_id: None,
            active_event_session_id: None,
            turn_failures: 0,
            turn_interrupted: false,
            turn_outcome: None,
            turn_terminal_message: None,
            turn_had_reasoning: false,
            turn_terminal_visible_answer: false,
            history_prompt_started_ms: None,
            loaded_session_message_count: 0,
            thinking_visible: app.thinking_visible,
            raw_visible: app.raw_visible,
            running: None,
            foreign_gateway_activities: BTreeMap::new(),
            applied_gateway_live_event_seqs: BTreeSet::new(),
            auxiliary_agent_tasks: Vec::new(),
            agent_child_event_backlog: BTreeMap::new(),
            session_live_event_backlog: BTreeMap::new(),
            auxiliary_shell_tasks: Vec::new(),
            pending_auxiliary_shell_commands: VecDeque::new(),
            approval_rx: None,
            pending_permission_approvals: VecDeque::new(),
            active_permission_approval: None,
            visible_turn_started: None,
            motion_started: Instant::now(),
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
            selected_target: None,
            last_transcript_area: None,
            last_composer_area: None,
            last_composer_input_area: None,
            composer_cursor_top_row: 0,
            last_status_area: None,
            last_bottom_panel_area: None,
            last_entry_areas: Vec::new(),
            mouse_down_target: None,
            mouse_dragged: false,
            composer_mouse_selecting: false,
            sidebar_forced: app.state.sidebar_visible,
            sidebar_hidden: !app.state.sidebar_visible,
            last_sidebar_visible: false,
            sidebar: SidebarSnapshot::default(),
            sidebar_tokens: None,
            sidebar_context_limit: None,
            last_context_snapshot: app.last_context_snapshot.clone(),
            sidebar_cost_nanodollars: None,
            session_usage_summary: None,
            history: Vec::new(),
            history_kinds: Vec::new(),
            history_index: None,
            history_draft: None,
            queued_inputs: VecDeque::new(),
            pending_steers: VecDeque::new(),
            pending_input_edit: None,
            history_message_edit: None,
            pending_input_sequence: 0,
            pending_images: Vec::new(),
            history_search: false,
            history_query: String::new(),
            slash_menu_selected: 0,
            slash_menu_dismissed_input: None,
            completion_popup_selected: 0,
            pending_leader_started: None,
            last_slash_menu_areas: Vec::new(),
            last_pending_input_action_areas: Vec::new(),
            last_pending_input_edit_area: None,
            file_search: FileSearchState::new(),
            last_file_popup_areas: Vec::new(),
            agent_search: AgentSearchState::default(),
            last_agent_popup_areas: Vec::new(),
            skill_search: SkillSearchState::default(),
            last_skill_popup_areas: Vec::new(),
            last_completion_popup_areas: Vec::new(),
            last_bottom_panel_areas: Vec::new(),
            bottom_panel: None,
            diff_overlay: None,
            last_diff_overlay_area: None,
            ephemeral_status: None,
            screen_lines: Vec::new(),
            selection: SelectionState::default(),
            terminal_clear_requested: false,
            quit_requested: false,
        };
        ui.refresh_sidebar(app);
        ui
    }

    pub(crate) fn drain_permission_approval_requests(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &mut self.approval_rx {
            while let Ok(request) = rx.try_recv() {
                self.pending_permission_approvals.push_back(request);
                changed = true;
            }
        }
        changed | self.open_next_permission_approval()
    }

    pub(crate) fn open_next_permission_approval(&mut self) -> bool {
        if self.active_permission_approval.is_some()
            || matches!(self.bottom_panel, Some(BottomPanel::PermissionApproval(_)))
        {
            return false;
        }
        let Some(request) = self.pending_permission_approvals.pop_front() else {
            return false;
        };
        let previous_panel = self.bottom_panel.take();
        self.active_permission_approval = Some(request.response);
        self.bottom_panel = Some(BottomPanel::PermissionApproval(
            PermissionApprovalPanel::new(request.session_id, request.request, previous_panel),
        ));
        true
    }

    pub(crate) fn resolve_permission_approval(
        &mut self,
        panel: PermissionApprovalPanel,
        decision: PermissionApprovalDecision,
    ) {
        if let Some(response) = self.active_permission_approval.take() {
            let _ = response.send(decision);
        }
        self.bottom_panel = panel.restore_panel();
        self.open_next_permission_approval();
    }

    pub(crate) fn discard_permission_approvals_for_abort(&mut self) {
        if let Some(BottomPanel::PermissionApproval(panel)) = self.bottom_panel.take() {
            self.bottom_panel = panel.restore_panel();
        }
        self.active_permission_approval.take();
        self.pending_permission_approvals.clear();
        if let Some(rx) = &mut self.approval_rx {
            while rx.try_recv().is_ok() {}
        }
    }

    pub(crate) fn refresh_sidebar(&mut self, app: &TuiApp) {
        let git = git_snapshot(&app.cwd);
        self.sidebar = SidebarSnapshot {
            title: app.session_sidebar_title(),
            session: app
                .current_session
                .as_deref()
                .map(short_session)
                .unwrap_or("(none)")
                .to_string(),
            branch: git.branch,
            changed_files: git.changed_files,
        };
    }

    pub(crate) fn clear_transcript(&mut self) {
        self.transcript.clear();
        self.assistant_row = None;
        self.assistant_preamble_row = None;
        self.reasoning_row = None;
        self.meta_row = None;
        self.gateway_item_rows.clear();
        self.tool_rows.clear();
        self.write_preview_trackers.clear();
        self.history_tool_titles.clear();
        self.history_tool_args.clear();
        self.live_tool_args.clear();
        self.clarify_tool_args.clear();
        self.exec_session_rows.clear();
        self.exec_session_elapsed.clear();
        self.pending_input_edit = None;
        self.last_pending_input_action_areas.clear();
        self.last_pending_input_edit_area = None;
        self.scroll = 0;
        self.last_transcript_height = 0;
        self.last_transcript_width = 0;
        self.transcript_layout = TranscriptLayoutCache::default();
        self.auto_follow_transcript = true;
        self.selected_row = None;
        self.selected_target = None;
        self.last_entry_areas.clear();
        self.mouse_down_target = None;
        self.mouse_dragged = false;
        self.composer_mouse_selecting = false;
        self.selection = SelectionState::default();
        self.terminal_clear_requested = true;
        self.sidebar_tokens = None;
        self.sidebar_context_limit = None;
        self.last_context_snapshot = None;
        self.sidebar_cost_nanodollars = None;
        self.session_usage_summary = None;
        self.history_prompt_started_ms = None;
        self.visible_turn_started = None;
        self.loaded_session_message_count = 0;
        self.turn_had_reasoning = false;
        self.pending_images.clear();
        self.ephemeral_status = None;
        self.diff_overlay = None;
        self.last_diff_overlay_area = None;
    }

    pub(crate) fn take_terminal_clear_request(&mut self) -> bool {
        std::mem::take(&mut self.terminal_clear_requested)
    }

    pub(crate) fn set_thinking_visible(&mut self, visible: bool) {
        self.thinking_visible = visible;
        if self
            .selected_target
            .is_some_and(|target| !self.target_visible(target))
        {
            self.selected_row = None;
            self.selected_target = None;
            self.ensure_selection();
        }
        self.clamp_transcript_scroll();
    }

    pub(crate) fn set_raw_visible(&mut self, visible: bool) {
        self.raw_visible = visible;
        self.clamp_transcript_scroll();
    }

    pub(crate) fn scroll_transcript(&mut self, amount: isize) {
        if amount < 0 {
            self.scroll = self.scroll.saturating_sub(amount.unsigned_abs() as u16);
        } else {
            self.scroll = self.scroll.saturating_add(amount as u16);
        }
        self.pending_scroll_to_bottom = false;
        let max_scroll = self.max_transcript_scroll();
        self.scroll = self.scroll.min(max_scroll);
        self.auto_follow_transcript = amount > 0 && self.scroll >= max_scroll;
    }

    pub(crate) fn clamp_transcript_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_transcript_scroll());
    }

    pub(crate) fn max_transcript_scroll(&self) -> u16 {
        if self.transcript_layout_matches_viewport() {
            return self
                .transcript_layout
                .max_scroll(self.last_transcript_height);
        }
        let total = transcript_total_height_for_ui(self, self.last_transcript_width)
            .min(usize::from(u16::MAX)) as u16;
        total.saturating_sub(self.last_transcript_height)
    }

    pub(crate) fn transcript_layout_matches_viewport(&self) -> bool {
        transcript_layout_matches_current(self, self.last_transcript_width)
    }

    pub(crate) fn follow_transcript_if_needed(&mut self) {
        if self.auto_follow_transcript {
            self.scroll_to_bottom();
        } else {
            self.clamp_transcript_scroll();
        }
    }

    pub(crate) fn resolve_transcript_scroll_for_render_with_total(&mut self, total_height: usize) {
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

    pub(crate) fn add_sidebar_cost(&mut self, accounting: Option<&Value>) {
        let Some(cost) = accounting
            .and_then(|value| value.get("estimated_cost_nanodollars"))
            .and_then(Value::as_i64)
        else {
            return;
        };
        self.sidebar_cost_nanodollars = Some(
            self.sidebar_cost_nanodollars
                .unwrap_or_default()
                .saturating_add(cost),
        );
    }

    pub(crate) fn sidebar_enabled(&self) -> bool {
        self.sidebar_forced && !self.sidebar_hidden
    }

    pub(crate) fn clear_screen_lines(&mut self) {
        self.screen_lines.clear();
    }

    #[cfg(test)]
    pub(crate) fn push_screen_line(&mut self, x: u16, y: u16, text: impl Into<String>) {
        let text = text.into();
        self.screen_lines.push(ScreenLine {
            region: SelectableRegion::Transcript,
            y,
            cells: screen_cells_from_text(x, &text),
        });
    }

    pub(crate) fn capture_selectable_rows(
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

    pub(crate) fn selectable_hit(&self, column: u16, row: u16) -> bool {
        self.screen_lines.iter().any(|line| {
            line.y == row
                && line
                    .cells
                    .iter()
                    .any(|cell| column >= cell.x && column < cell.x.saturating_add(cell.width))
        })
    }

    pub(crate) fn selection_region_at(&self, column: u16, row: u16) -> Option<SelectableRegion> {
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

    pub(crate) fn start_selection(&mut self, column: u16, row: u16) {
        self.selection.anchor = Some((column, row));
        self.selection.focus = Some((column, row));
        self.selection.region = self.selection_region_at(column, row);
    }

    pub(crate) fn update_selection(&mut self, column: u16, row: u16) {
        if self.selection.anchor.is_some() {
            self.selection.focus = Some((column, row));
        }
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selection = SelectionState::default();
    }

    pub(crate) fn composer_input_hit(&self, column: u16, row: u16) -> bool {
        self.last_composer_input_area
            .is_some_and(|area| rect_contains(area, column, row))
    }

    pub(crate) fn move_composer_cursor_to_point(&mut self, column: u16, row: u16) -> bool {
        let Some(area) = self.last_composer_input_area else {
            return false;
        };
        let Some((text_row, text_col)) = composer_cursor_from_point(
            &self.textarea,
            area,
            self.composer_cursor_top_row,
            column,
            row,
        ) else {
            return false;
        };
        self.textarea.move_cursor(CursorMove::Jump(
            text_row.min(u16::MAX as usize) as u16,
            text_col.min(u16::MAX as usize) as u16,
        ));
        true
    }

    pub(crate) fn start_composer_mouse_selection(&mut self, column: u16, row: u16) -> bool {
        if !self.composer_input_hit(column, row) {
            return false;
        }
        self.clear_selection();
        self.mouse_down_target = None;
        self.mouse_dragged = false;
        self.composer_mouse_selecting = true;
        self.move_composer_cursor_to_point(column, row);
        self.textarea.start_selection();
        self.close_file_popup();
        self.close_agent_popup();
        self.close_skill_popup();
        self.dismiss_slash_menu();
        true
    }

    pub(crate) fn update_composer_mouse_selection(&mut self, column: u16, row: u16) -> bool {
        if !self.composer_mouse_selecting {
            return false;
        }
        self.move_composer_cursor_to_point(column, row);
        true
    }

    pub(crate) fn finish_composer_mouse_selection(&mut self) -> bool {
        if !self.composer_mouse_selecting {
            return false;
        }
        self.composer_mouse_selecting = false;
        if !self.mouse_dragged
            || self
                .textarea
                .selection_range()
                .is_none_or(|(start, end)| start == end)
        {
            self.textarea.cancel_selection();
        }
        self.mouse_down_target = None;
        self.mouse_dragged = false;
        true
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        selected_text_from_lines(&self.screen_lines, &self.selection)
    }

    pub(crate) fn latest_visible_answer_markdown(&self) -> Option<String> {
        self.transcript
            .iter()
            .rev()
            .find(|row| {
                row.kind == TranscriptKind::Answer && row_visible(row, self.thinking_visible)
            })
            .and_then(|row| {
                let text = row.full_text.as_deref().unwrap_or(&row.text);
                (!text.trim().is_empty()).then(|| text.to_string())
            })
    }

}
