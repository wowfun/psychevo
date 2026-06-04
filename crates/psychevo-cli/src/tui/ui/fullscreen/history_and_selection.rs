#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

impl<'a> FullscreenUi<'a> {
    pub(crate) fn new(app: &TuiApp) -> Self {
        let mut ui = Self {
            textarea: new_textarea(),
            workdir: app.workdir.clone(),
            transcript: Vec::new(),
            assistant_row: None,
            assistant_preamble_row: None,
            reasoning_row: None,
            meta_row: None,
            gateway_item_rows: BTreeMap::new(),
            tool_rows: BTreeMap::new(),
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
            history: Vec::new(),
            history_kinds: Vec::new(),
            history_index: None,
            history_draft: None,
            queued_inputs: VecDeque::new(),
            pending_steers: VecDeque::new(),
            pending_input_edit: None,
            pending_input_sequence: 0,
            pending_images: Vec::new(),
            history_search: false,
            history_query: String::new(),
            slash_menu_selected: 0,
            slash_menu_dismissed_input: None,
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
        let git = git_snapshot(&app.workdir);
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
        let Some((text_row, text_col)) =
            composer_cursor_from_point(&self.textarea, area, column, row)
        else {
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

    #[cfg(test)]
    pub(crate) fn push_history_message(
        &mut self,
        message: &Value,
        usage: Option<&Value>,
        metadata: Option<&Value>,
    ) {
        self.push_history_message_with_accounting(message, usage, metadata, None);
    }

    #[cfg(test)]
    pub(crate) fn push_history_message_with_accounting(
        &mut self,
        message: &Value,
        usage: Option<&Value>,
        metadata: Option<&Value>,
        accounting: Option<&Value>,
    ) {
        self.push_history_message_with_accounting_options(
            message, usage, metadata, accounting, false,
        );
    }

    pub(crate) fn push_history_message_with_accounting_options(
        &mut self,
        message: &Value,
        usage: Option<&Value>,
        metadata: Option<&Value>,
        accounting: Option<&Value>,
        suppress_terminal_meta: bool,
    ) {
        self.push_history_message_with_projection_options(
            message,
            usage,
            metadata,
            accounting,
            suppress_terminal_meta,
            None,
        );
    }

    pub(crate) fn push_history_message_with_projection_options(
        &mut self,
        message: &Value,
        usage: Option<&Value>,
        metadata: Option<&Value>,
        accounting: Option<&Value>,
        suppress_terminal_meta: bool,
        active_tool_call_ids: Option<&BTreeSet<String>>,
    ) {
        if btw_inherited_message(metadata) {
            return;
        }
        match message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "user" => {
                if let Some(display) = agent_notification_display(metadata) {
                    let mut row =
                        TranscriptRow::with_title(TranscriptKind::Status, "Agent", display);
                    row.agent_target = agent_notification_target(metadata);
                    self.transcript.push(row);
                } else if let Some(display) = user_shell_display_from_message(message, metadata) {
                    self.push_history_user_shell(display);
                } else if let Some(display) = user_display_from_message(message, metadata) {
                    self.push_user_with_attachment_meta(display.text, display.attachment_meta);
                    self.history_prompt_started_ms = message_timestamp_ms(message);
                }
            }
            "assistant" => {
                let tool_calls = history_tool_calls_from_message(message);
                let has_reasoning =
                    if let Some(reasoning) = assistant_reasoning_from_message(message) {
                        self.transcript.push(TranscriptRow::with_title(
                            TranscriptKind::Thinking,
                            "Thinking",
                            reasoning,
                        ));
                        true
                    } else {
                        false
                    };
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
                if let Some(tokens) = usage.and_then(usage_context_tokens) {
                    self.sidebar_tokens = Some(tokens);
                }
                self.add_sidebar_cost(accounting);
                let keep_tool_calls_active = assistant_message_keeps_tool_calls_active(message);
                let mut kept_any_tool_call_active = false;
                for call in tool_calls {
                    let keep_call_active = keep_tool_calls_active
                        && active_tool_call_ids.is_none_or(|ids| ids.contains(&call.id));
                    if keep_call_active {
                        kept_any_tool_call_active = true;
                        self.push_history_active_tool_call(message, call);
                    } else {
                        self.push_history_interrupted_tool_call(call, metadata);
                    }
                }
                if !suppress_terminal_meta
                    && ((has_answer && visible_answer_message_receives_meta(message))
                        || (has_reasoning && reasoning_only_message_receives_meta(message)))
                    && let Some(meta) = history_meta_text(
                        message,
                        usage,
                        metadata,
                        accounting,
                        self.history_prompt_started_ms,
                    )
                {
                    self.transcript
                        .push(TranscriptRow::with_title(TranscriptKind::Meta, "", meta));
                }
                if !kept_any_tool_call_active {
                    self.history_prompt_started_ms = None;
                }
            }
            "tool_result" => self.push_history_tool_result(message, metadata),
            _ => {}
        }
    }

    pub(crate) fn push_history_user_shell(&mut self, display: UserShellDisplay) {
        let value = serde_json::json!({
            "type": "tool_execution_end",
            "tool_name": "exec_command",
            "args": {"cmd": display.command},
            "result": display.result,
            "outcome": display.outcome,
            "source": "user_shell",
        });
        let (collapsed, full) = tool_output_text(&value);
        let mut row = TranscriptRow::with_title(
            TranscriptKind::Ran,
            tool_title("exec_command", &value),
            if collapsed.is_empty() {
                format_tool_summary(&value)
            } else {
                collapsed
            },
        );
        row.full_text = full;
        row.interrupted = tool_event_interrupted(&value);
        row.failed =
            value.get("outcome").and_then(Value::as_str) != Some("normal") && !row.interrupted;
        row.user_shell = true;
        self.transcript.push(row);
    }

    pub(crate) fn push_history_active_tool_call(&mut self, message: &Value, call: HistoryToolCall) {
        self.history_tool_titles
            .insert(call.id.clone(), call.completed_title.clone());
        self.history_tool_args
            .insert(call.id.clone(), call.args.clone());
        if call.name == "write_stdin"
            && let Some(session_id) = exec_session_id_from_args(&call.args)
            && self.exec_session_rows.contains_key(&session_id)
        {
            if let Some(chars) = write_stdin_non_empty_chars(&call.args) {
                self.push_exec_stdin_row(session_id, chars);
            }
            return;
        }
        let mut row =
            TranscriptRow::with_title(evidence_kind(&call.name), call.active_title, "preparing");
        row.tool_call_id = Some(call.id.clone());
        row.tool_name = Some(call.name.clone());
        row.tool_started = Some(history_tool_started_instant(message));
        let idx = self.transcript.len();
        self.transcript.push(row);
        self.tool_rows.insert(tool_id_key(&call.id), idx);
    }

    pub(crate) fn push_history_interrupted_tool_call(
        &mut self,
        call: HistoryToolCall,
        metadata: Option<&Value>,
    ) {
        self.history_tool_titles
            .insert(call.id.clone(), call.completed_title.clone());
        self.history_tool_args
            .insert(call.id.clone(), call.args.clone());
        let mut row = TranscriptRow::with_title(
            evidence_kind(&call.name),
            call.completed_title,
            "interrupted",
        );
        row.tool_call_id = Some(call.id);
        row.tool_name = Some(call.name);
        row.tool_elapsed = metadata_elapsed_duration(metadata);
        row.interrupted = true;
        self.transcript.push(row);
    }

    pub(crate) fn push_history_tool_result(&mut self, message: &Value, metadata: Option<&Value>) {
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
        let outcome = if is_error && result.get("error").and_then(Value::as_str) == Some("aborted")
        {
            "aborted"
        } else if is_error {
            "failed"
        } else {
            "normal"
        };
        let args = self
            .history_tool_args
            .get(tool_call_id)
            .cloned()
            .unwrap_or(Value::Null);
        let value = serde_json::json!({
            "tool_name": tool,
            "args": args,
            "result": result,
            "outcome": outcome
        });
        if self.apply_history_exec_session_result(tool, tool_call_id, &value, metadata, is_error) {
            return;
        }
        let interrupted = tool_event_interrupted(&value);
        let clarify_no_answer = tool == "clarify" && clarify_no_answer_result(&value);
        let title = if matches!(tool, "Agent" | "clarify") {
            tool_title(tool, &value)
        } else {
            self.history_tool_titles
                .get(tool_call_id)
                .cloned()
                .unwrap_or_else(|| tool_title(tool, &value))
        };
        let idx = self.tool_rows.get(&tool_id_key(tool_call_id)).copied();
        let mut row = idx
            .and_then(|idx| self.transcript.get(idx).cloned())
            .unwrap_or_else(|| TranscriptRow::with_title(evidence_kind(tool), title.clone(), ""));
        row.kind = evidence_kind(tool);
        row.title = title;
        row.tool_name = Some(tool.to_string());
        row.interrupted = interrupted;
        row.failed = is_error && !interrupted && !clarify_no_answer;
        row.tool_elapsed = metadata_elapsed_duration(metadata)
            .or_else(|| row.tool_started.map(|started| started.elapsed()));
        row.tool_started = None;
        if tool == "Agent" {
            row.agent_target = agent_target_from_tool_event(&value);
        }
        if interrupted {
            row.text = "interrupted".to_string();
            row.full_text = None;
        } else {
            let (collapsed, full) = tool_output_text(&value);
            row.text = if collapsed.is_empty() {
                format_tool_summary(&value)
            } else {
                collapsed
            };
            row.full_text = full;
        }
        if let Some(idx) = idx {
            self.transcript[idx] = row;
            self.tool_rows.retain(|_, row_index| *row_index != idx);
        } else {
            self.transcript.push(row);
        }
    }

    pub(crate) fn apply_history_exec_session_result(
        &mut self,
        tool: &str,
        tool_call_id: &str,
        value: &Value,
        metadata: Option<&Value>,
        is_error: bool,
    ) -> bool {
        if tool == "exec_command"
            && let Some(session_id) = exec_session_id_from_result(value)
            && exec_result_running(value)
        {
            let title = self
                .history_tool_titles
                .get(tool_call_id)
                .cloned()
                .unwrap_or_else(|| tool_title(tool, value));
            let idx = self
                .tool_rows
                .get(&tool_id_key(tool_call_id))
                .copied()
                .unwrap_or_else(|| {
                    let mut row = TranscriptRow::with_title(evidence_kind(tool), title.clone(), "");
                    row.tool_call_id =
                        (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                    row.tool_name = Some(tool.to_string());
                    self.insert_evidence_row(row)
                });
            let row = &mut self.transcript[idx];
            row.kind = evidence_kind(tool);
            row.title = title;
            row.tool_name = Some(tool.to_string());
            row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
            row.failed = false;
            row.interrupted = false;
            row.tool_elapsed = metadata_elapsed_duration(metadata);
            row.tool_started = None;
            set_exec_row_text(
                row,
                with_exec_history_running_marker(tool_result_output(value)),
            );
            self.exec_session_rows.insert(session_id, idx);
            self.exec_session_elapsed.insert(
                session_id,
                metadata_elapsed_duration(metadata).unwrap_or_default(),
            );
            self.tool_rows.retain(|_, row_index| *row_index != idx);
            return true;
        }

        if tool != "write_stdin" || is_error {
            return false;
        }
        let Some(args) = value.get("args") else {
            return false;
        };
        let Some(session_id) = exec_session_id_from_args(args) else {
            return false;
        };
        let Some(idx) = self.exec_session_rows.get(&session_id).copied() else {
            return false;
        };

        let output = tool_result_output(value);
        if !output.is_empty() {
            self.append_exec_session_output(idx, &output);
        }
        if let Some(elapsed) = metadata_elapsed_duration(metadata) {
            let total = self.exec_session_elapsed.entry(session_id).or_default();
            *total += elapsed;
        }

        if exec_result_completed(value) {
            let elapsed = self.exec_session_elapsed.remove(&session_id);
            self.finish_exec_session_row(session_id, idx, elapsed, false, false);
        } else if let Some(row) = self.transcript.get_mut(idx) {
            let full = exec_row_full_text_without_history_marker(row);
            set_exec_row_text(row, with_exec_history_running_marker(full));
        }
        true
    }

    pub(crate) fn append_exec_session_output(&mut self, idx: usize, output: &str) {
        if output.is_empty() {
            return;
        }
        let Some(row) = self.transcript.get_mut(idx) else {
            return;
        };
        let mut full = exec_row_full_text_without_history_marker(row);
        full.push_str(output);
        set_exec_row_text(row, full);
    }

    pub(crate) fn prefix_exec_session_output_if_needed(&mut self, idx: usize, output: String) {
        if output.is_empty() {
            return;
        }
        let Some(row) = self.transcript.get_mut(idx) else {
            return;
        };
        let full = exec_row_full_text_without_history_marker(row);
        if full.is_empty() || output.starts_with(&full) {
            set_exec_row_text(row, output);
        } else if !full.starts_with(&output) && !full.ends_with(&output) {
            set_exec_row_text(row, format!("{output}{full}"));
        }
    }

    pub(crate) fn finish_exec_session_row(
        &mut self,
        session_id: u64,
        idx: usize,
        elapsed: Option<Duration>,
        interrupted: bool,
        keep_session_mapping: bool,
    ) {
        let Some(row) = self.transcript.get_mut(idx) else {
            return;
        };
        row.tool_elapsed = elapsed.or_else(|| row.tool_started.map(|started| started.elapsed()));
        row.tool_started = None;
        row.title = completed_tool_title_from_active(row.kind, &row.title);
        row.interrupted = interrupted;
        row.failed = false;
        if interrupted {
            row.text = "interrupted".to_string();
            row.full_text = None;
        } else {
            let full = exec_row_full_text_without_history_marker(row);
            set_exec_row_text(row, full);
        }
        let tool_call_id = row.tool_call_id.clone();
        if !keep_session_mapping {
            self.exec_session_rows.remove(&session_id);
        }
        self.exec_session_elapsed.remove(&session_id);
        if let Some(tool_call_id) = tool_call_id {
            self.tool_rows.remove(&tool_id_key(&tool_call_id));
        }
    }

    pub(crate) fn push_exec_stdin_row(&mut self, session_id: u64, chars: &str) {
        let mut row = TranscriptRow::with_title(
            TranscriptKind::Ran,
            format!("stdin {session_id}"),
            bounded_stdin_display(chars),
        );
        row.tool_name = Some("write_stdin".to_string());
        row.tool_elapsed = Some(Duration::ZERO);
        self.insert_evidence_row(row);
    }
}
