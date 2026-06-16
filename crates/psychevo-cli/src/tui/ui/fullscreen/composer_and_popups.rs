#[allow(unused_imports)]
pub(crate) use super::*;
impl<'a> FullscreenUi<'a> {
    pub(crate) fn reconcile_history_agent_rows(
        &mut self,
        edges: &[AgentEdgeRecord],
        catalog: Option<&AgentCatalog>,
    ) {
        if edges.is_empty() {
            return;
        }
        let mut used_edges = std::collections::BTreeSet::<usize>::new();
        for row in &mut self.transcript {
            if row.tool_name.as_deref() != Some("spawn_agent") || row.agent_target.is_some() {
                continue;
            }
            let row_was_active = active_tool_row(row);
            let Some((edge_index, edge)) = matching_agent_edge(row, edges, &used_edges) else {
                continue;
            };
            used_edges.insert(edge_index);
            row.agent_target = Some(edge.child_session_id.clone());
            if let Some(title) = agent_edge_title(edge, catalog) {
                row.title = title;
            }
            if row_was_active {
                row.text = agent_child_status_text(
                    "Running",
                    row.agent_child_tool_uses,
                    row.agent_child_latest_tokens,
                );
                row.full_text = None;
            }
        }
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
        self.scroll = self.max_transcript_scroll();
        self.auto_follow_transcript = true;
        self.pending_scroll_to_bottom = true;
    }

    pub(crate) fn status_running_elapsed(&self, current_session: Option<&str>) -> Option<Duration> {
        if !self.status_has_running(current_session) {
            return None;
        }
        #[cfg(test)]
        if let Some(elapsed) = self.running_elapsed_override {
            return Some(elapsed);
        }
        if let Some(session_id) = current_session
            && let Some(activity) = self.foreign_gateway_activities.get(session_id)
        {
            return Some(activity.started.elapsed());
        }
        self.visible_turn_started
            .or(self.turn_started)
            .map(|started| started.elapsed())
            .or(Some(Duration::default()))
    }

    pub(crate) fn bottom_panel_activity_elapsed(&self) -> Duration {
        #[cfg(test)]
        if let Some(elapsed) = self.running_elapsed_override {
            return elapsed;
        }
        self.motion_started.elapsed()
    }

    pub(crate) fn background_running_session_ids(
        &self,
        current_session: Option<&str>,
    ) -> BTreeSet<String> {
        let mut sessions = BTreeSet::new();
        if let Some(running) = &self.running
            && let Some(session_id) = running.session_id.as_deref()
            && Some(session_id) != current_session
        {
            sessions.insert(session_id.to_string());
        }
        for agent in &self.auxiliary_agent_tasks {
            if !agent.visible_live {
                continue;
            }
            if let Some(session_id) = agent.session_id.as_deref()
                && Some(session_id) != current_session
            {
                sessions.insert(session_id.to_string());
            }
            if let Some(session_id) = agent.child_session_id.as_deref()
                && Some(session_id) != current_session
            {
                sessions.insert(session_id.to_string());
            }
        }
        for shell in &self.auxiliary_shell_tasks {
            if let Some(session_id) = shell.session_id.as_deref()
                && Some(session_id) != current_session
            {
                sessions.insert(session_id.to_string());
            }
        }
        for session_id in self.foreign_gateway_activities.keys() {
            if Some(session_id.as_str()) != current_session {
                sessions.insert(session_id.clone());
            }
        }
        sessions
    }

    pub(crate) fn status_has_running(&self, current_session: Option<&str>) -> bool {
        self.local_status_has_running(current_session)
            || self.foreign_gateway_activity_matches_current_session(current_session)
    }

    pub(crate) fn local_status_has_running(&self, current_session: Option<&str>) -> bool {
        self.running.as_ref().is_some_and(|running| {
            current_session_matches(running.session_id.as_deref(), current_session)
        }) || self.auxiliary_agent_matches_current_session(current_session)
            || self.auxiliary_shell_matches_current_session(current_session)
    }

    pub(crate) fn observe_foreign_gateway_activity(
        &mut self,
        session_id: &str,
        activity: &GatewayActivity,
    ) {
        if !activity.running {
            self.foreign_gateway_activities.remove(session_id);
            return;
        }
        self.observe_foreign_gateway_activity_values(
            session_id,
            activity.active_turn_id.clone(),
            activity.started_at_ms,
        );
    }

    pub(crate) fn observe_foreign_gateway_activity_values(
        &mut self,
        session_id: &str,
        active_turn_id: Option<String>,
        started_at_ms: Option<i64>,
    ) {
        let started = started_at_ms
            .and_then(instant_from_wall_timestamp_ms)
            .or_else(|| {
                self.foreign_gateway_activities
                    .get(session_id)
                    .map(|activity| activity.started)
            })
            .unwrap_or_else(Instant::now);
        self.foreign_gateway_activities.insert(
            session_id.to_string(),
            ForeignGatewayActivity {
                active_turn_id,
                started,
            },
        );
    }

    pub(crate) fn clear_foreign_gateway_activity(&mut self, session_id: &str) {
        self.foreign_gateway_activities.remove(session_id);
    }

    pub(crate) fn foreign_gateway_activity_matches_current_session(
        &self,
        current_session: Option<&str>,
    ) -> bool {
        let Some(session_id) = current_session else {
            return false;
        };
        self.foreign_gateway_activities.contains_key(session_id)
    }

    pub(crate) fn foreign_gateway_activity_started(&self, session_id: &str) -> Option<Instant> {
        self.foreign_gateway_activities
            .get(session_id)
            .map(|activity| activity.started)
    }

    pub(crate) fn foreign_gateway_activity_turn_id(&self, session_id: &str) -> Option<String> {
        self.foreign_gateway_activities
            .get(session_id)
            .and_then(|activity| activity.active_turn_id.clone())
    }

    pub(crate) fn mark_gateway_live_event_applied(&mut self, seq: i64) -> bool {
        self.applied_gateway_live_event_seqs.insert(seq)
    }

    pub(crate) fn auxiliary_agent_matches_current_session(
        &self,
        current_session: Option<&str>,
    ) -> bool {
        let Some(session_id) = current_session else {
            return false;
        };
        self.auxiliary_agent_tasks
            .iter()
            .any(|agent| auxiliary_agent_live_for_session(agent, session_id))
    }

    pub(crate) fn auxiliary_shell_matches_current_session(
        &self,
        current_session: Option<&str>,
    ) -> bool {
        let Some(session_id) = current_session else {
            return false;
        };
        self.auxiliary_shell_tasks
            .iter()
            .any(|shell| shell.session_id.as_deref() == Some(session_id))
    }

    pub(crate) fn request_interrupt(&mut self, current_session: Option<&str>) -> bool {
        let mut interrupted = false;
        if let Some(running) = &self.running
            && current_session_matches(running.session_id.as_deref(), current_session)
        {
            running.control.abort();
            interrupted = true;
        }
        for agent in &self.auxiliary_agent_tasks {
            if current_session
                .is_some_and(|session_id| auxiliary_agent_live_for_session(agent, session_id))
            {
                agent.control.abort();
                interrupted = true;
            }
        }
        for shell in &self.auxiliary_shell_tasks {
            if current_session
                .is_some_and(|session_id| shell.session_id.as_deref() == Some(session_id))
            {
                shell.control.abort();
                interrupted = true;
            }
        }
        if !interrupted {
            return false;
        }
        for shell in &self.auxiliary_shell_tasks {
            shell.control.abort();
        }
        self.pending_auxiliary_shell_commands.clear();
        self.interrupt_requested = true;
        true
    }

    pub(crate) fn enter_shell_mode(&mut self) {
        self.shell_mode = true;
        self.clear_slash_menu_dismissal();
    }

    pub(crate) fn exit_shell_mode(&mut self) {
        self.shell_mode = false;
        self.clear_slash_menu_dismissal();
    }

    pub(crate) fn clear_composer(&mut self) {
        self.textarea = new_textarea();
        self.shell_mode = false;
        self.composer_cursor_top_row = 0;
    }

    pub(crate) fn select_composer_all(&mut self) -> bool {
        if textarea_text(&self.textarea).is_empty() {
            return false;
        }
        self.textarea.select_all();
        self.close_file_popup();
        self.close_agent_popup();
        self.close_skill_popup();
        self.dismiss_slash_menu();
        true
    }

    pub(crate) fn cancel_composer_selection(&mut self) -> bool {
        if !self.textarea.is_selecting() {
            return false;
        }
        self.textarea.cancel_selection();
        true
    }

    pub(crate) fn set_composer_text(&mut self, text: &str) {
        if let Some(shell) = parse_shell_escape_input(text) {
            self.shell_mode = true;
            self.textarea = if shell.command.is_empty() {
                new_textarea()
            } else {
                textarea_with_text(&shell.command)
            };
        } else {
            self.shell_mode = false;
            self.textarea = if text.is_empty() {
                new_textarea()
            } else {
                textarea_with_text(text)
            };
        }
        self.slash_menu_selected = 0;
        self.clear_slash_menu_dismissal();
        self.composer_cursor_top_row = 0;
    }

    pub(crate) fn composer_submission_text(&self) -> String {
        let text = textarea_text(&self.textarea);
        if self.shell_mode {
            format!("!{text}")
        } else {
            text
        }
    }

    pub(crate) fn next_pending_input_sequence(&mut self) -> u64 {
        self.pending_input_sequence = self.pending_input_sequence.saturating_add(1);
        self.pending_input_sequence
    }

    pub(crate) fn pending_input_entries(&self) -> Vec<PendingInputEntry> {
        let mut entries = Vec::new();
        for steer in &self.pending_steers {
            entries.push(PendingInputEntry {
                target: PendingInputRef::Steer(steer.id),
                kind: PendingInputKind::Steer,
                text: steer.display_prompt.clone(),
                images: steer.images.clone(),
                sequence: steer.sequence,
            });
        }
        for input in &self.queued_inputs {
            if let QueuedInput::Prompt {
                display_prompt,
                images,
                sequence,
                ..
            } = input
            {
                entries.push(PendingInputEntry {
                    target: PendingInputRef::Queue(*sequence),
                    kind: PendingInputKind::Queue,
                    text: display_prompt.clone(),
                    images: images.clone(),
                    sequence: *sequence,
                });
            }
        }
        entries.sort_by_key(|entry| entry.sequence);
        entries
    }

    pub(crate) fn pending_input_entry(&self, target: PendingInputRef) -> Option<PendingInputEntry> {
        self.pending_input_entries()
            .into_iter()
            .find(|entry| entry.target == target)
    }

    pub(crate) fn has_pending_input_preview(&self) -> bool {
        self.pending_input_edit.is_some() || !self.pending_input_entries().is_empty()
    }

    pub(crate) fn start_pending_input_edit(&mut self, target: PendingInputRef) -> bool {
        let Some(entry) = self.pending_input_entry(target) else {
            return false;
        };
        self.pending_input_edit = Some(PendingInputEdit {
            target,
            kind: entry.kind,
            textarea: textarea_with_text(&entry.text),
            images: entry.images,
            cursor_top_row: 0,
        });
        self.clear_slash_menu_dismissal();
        self.close_file_popup();
        self.close_agent_popup();
        self.close_skill_popup();
        true
    }

    pub(crate) fn cancel_pending_input_edit(&mut self) -> bool {
        self.pending_input_edit.take().is_some()
    }

    pub(crate) fn pending_input_edit_text(&self) -> Option<String> {
        self.pending_input_edit
            .as_ref()
            .map(|edit| textarea_text(&edit.textarea))
    }

    pub(crate) fn pending_input_action_hit(
        &self,
        column: u16,
        row: u16,
    ) -> Option<(PendingInputRef, PendingInputAction)> {
        self.last_pending_input_action_areas
            .iter()
            .find_map(|(target, action, area)| {
                rect_contains(*area, column, row).then_some((*target, *action))
            })
    }

    pub(crate) fn absorb_shell_escape_prefix(&mut self) -> bool {
        if self.shell_mode {
            return false;
        }
        let text = textarea_text(&self.textarea);
        let Some(shell) = parse_shell_escape_input(&text) else {
            return false;
        };
        self.shell_mode = true;
        self.textarea = if shell.command.is_empty() {
            new_textarea()
        } else {
            textarea_with_text(&shell.command)
        };
        self.slash_menu_selected = 0;
        self.clear_slash_menu_dismissal();
        self.close_agent_popup();
        self.close_skill_popup();
        true
    }

    pub(crate) fn restore_queued_inputs_to_composer(&mut self) {
        if self.queued_inputs.is_empty() && self.pending_steers.is_empty() {
            return;
        }
        let mut restored = Vec::new();
        let steers = self.pending_steers.drain(..).collect::<Vec<_>>();
        for steer in steers {
            restored.push((steer.sequence, steer.display_prompt, steer.images));
        }
        for input in self.queued_inputs.drain(..) {
            let sequence = queued_input_sequence(&input);
            let images = match &input {
                QueuedInput::Prompt { images, .. } => images.clone(),
                QueuedInput::Shell { .. } | QueuedInput::Compact { .. } => Vec::new(),
            };
            restored.push((sequence, queued_input_text(input), images));
        }
        restored.sort_by_key(|(sequence, _, _)| *sequence);
        let mut parts = Vec::new();
        for (_, text, images) in restored {
            self.pending_images.extend(images);
            parts.push(text);
        }
        let draft = self.composer_submission_text();
        if !draft.is_empty() && draft != "!" {
            parts.push(draft);
        }
        self.set_composer_text(&parts.join("\n"));
        self.reset_history_navigation();
        self.clear_slash_menu_dismissal();
        self.close_file_popup();
        self.close_agent_popup();
        self.close_skill_popup();
        self.pending_input_edit = None;
    }

    pub(crate) fn add_pending_image(&mut self, image: ImageInput) -> String {
        self.sync_pending_images_with_textarea();
        let text = textarea_text(&self.textarea);
        let placeholder = next_image_placeholder(&self.pending_images, &text);
        self.pending_images.push(PendingImageAttachment {
            placeholder: placeholder.clone(),
            image,
        });
        placeholder
    }

    pub(crate) fn sync_pending_images_with_textarea(&mut self) {
        let text = textarea_text(&self.textarea);
        self.pending_images
            .retain(|attachment| text.contains(&attachment.placeholder));
    }

    pub(crate) fn take_submitted_images(&mut self, prompt: &str) -> Vec<PendingImageAttachment> {
        let mut indexed = self
            .pending_images
            .iter()
            .filter_map(|attachment| {
                prompt
                    .find(&attachment.placeholder)
                    .map(|index| (index, attachment.clone()))
            })
            .collect::<Vec<_>>();
        indexed.sort_by_key(|(index, _)| *index);
        self.pending_images.clear();
        indexed
            .into_iter()
            .map(|(_, attachment)| attachment)
            .collect()
    }

    pub(crate) fn complete_slash_command(&mut self, items: &[SlashMenuItem]) {
        let input = textarea_text(&self.textarea);
        if let Some(completed) = slash_completion_with_items(&input, items) {
            self.textarea = textarea_with_text(&completed);
            self.slash_menu_selected = 0;
            self.clear_slash_menu_dismissal();
        }
    }

    pub(crate) fn current_file_token(&self) -> Option<FileToken> {
        current_file_token(&self.textarea)
    }

    pub(crate) fn sync_file_popup(&mut self, root: &Path) {
        let token = self.current_file_token();
        self.file_search.sync(root, token.as_ref());
    }

    pub(crate) fn drain_file_search_results(&mut self) -> bool {
        self.file_search.drain_results()
    }

    pub(crate) fn file_popup_visible(&self) -> bool {
        self.file_search.popup.is_some()
    }

    pub(crate) fn file_popup_height(&self) -> u16 {
        self.file_search.height()
    }

    pub(crate) fn close_file_popup(&mut self) {
        self.file_search.close();
        self.last_file_popup_areas.clear();
    }

    pub(crate) fn dismiss_file_popup(&mut self) {
        let query = self.current_file_token().map(|token| token.query);
        self.file_search.dismiss(query);
        self.last_file_popup_areas.clear();
    }

    pub(crate) fn selected_file_path(&self) -> Option<String> {
        self.file_search.selected_path()
    }

    pub(crate) fn move_file_popup_selection(&mut self, direction: isize) {
        self.file_search.move_selection(direction);
    }

    pub(crate) fn set_file_popup_selection(&mut self, index: usize) {
        self.file_search.set_selection(index);
    }

    pub(crate) fn insert_selected_file_path(&mut self) {
        let Some(path) = self.selected_file_path() else {
            return;
        };
        if replace_current_file_token(&mut self.textarea, &path) {
            self.file_search.close();
            self.file_search.dismissed_query = None;
            self.last_file_popup_areas.clear();
        }
    }

    pub(crate) fn file_popup_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_file_popup_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn current_skill_token(&self) -> Option<SkillToken> {
        current_skill_token(&self.textarea)
    }

    pub(crate) fn current_agent_token(&self) -> Option<AgentToken> {
        current_agent_token(&self.textarea)
    }

    pub(crate) fn sync_agent_popup(&mut self, matches: Vec<AgentSearchMatch>) {
        let token = self.current_agent_token();
        self.agent_search.sync(token.as_ref(), matches);
    }

    pub(crate) fn agent_popup_visible(&self) -> bool {
        self.agent_search.popup.is_some()
    }

    pub(crate) fn agent_popup_height(&self) -> u16 {
        self.agent_search.height()
    }

    pub(crate) fn close_agent_popup(&mut self) {
        self.agent_search.close();
        self.last_agent_popup_areas.clear();
    }

    pub(crate) fn dismiss_agent_popup(&mut self) {
        let query = self.current_agent_token().map(|token| token.query);
        self.agent_search.dismiss(query);
        self.last_agent_popup_areas.clear();
    }

    pub(crate) fn selected_agent_name(&self) -> Option<String> {
        self.agent_search.selected_name()
    }

    pub(crate) fn move_agent_popup_selection(&mut self, direction: isize) {
        self.agent_search.move_selection(direction);
    }

    pub(crate) fn set_agent_popup_selection(&mut self, index: usize) {
        self.agent_search.set_selection(index);
    }

    pub(crate) fn insert_selected_agent_marker(&mut self) {
        let Some(name) = self.selected_agent_name() else {
            return;
        };
        if replace_current_agent_token(&mut self.textarea, &name) {
            self.agent_search.close();
            self.agent_search.dismissed_query = None;
            self.last_agent_popup_areas.clear();
            self.clear_slash_menu_dismissal();
        }
    }

    pub(crate) fn agent_popup_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_agent_popup_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn sync_skill_popup(&mut self, matches: Vec<SkillSearchMatch>) {
        let token = self.current_skill_token();
        self.skill_search.sync(token.as_ref(), matches);
    }

    pub(crate) fn skill_popup_visible(&self) -> bool {
        self.skill_search.popup.is_some()
    }

    pub(crate) fn skill_popup_height(&self) -> u16 {
        self.skill_search.height()
    }

    pub(crate) fn close_skill_popup(&mut self) {
        self.skill_search.close();
        self.last_skill_popup_areas.clear();
    }

    pub(crate) fn dismiss_skill_popup(&mut self) {
        let query = self.current_skill_token().map(|token| token.query);
        self.skill_search.dismiss(query);
        self.last_skill_popup_areas.clear();
    }

    pub(crate) fn selected_skill_name(&self) -> Option<String> {
        self.skill_search.selected_name()
    }

    pub(crate) fn move_skill_popup_selection(&mut self, direction: isize) {
        self.skill_search.move_selection(direction);
    }

    pub(crate) fn set_skill_popup_selection(&mut self, index: usize) {
        self.skill_search.set_selection(index);
    }

    pub(crate) fn insert_selected_skill_marker(&mut self) {
        let Some(name) = self.selected_skill_name() else {
            return;
        };
        self.insert_skill_marker(&name);
    }

    pub(crate) fn insert_skill_marker(&mut self, name: &str) {
        if replace_current_skill_token(&mut self.textarea, name) {
            self.skill_search.close();
            self.skill_search.dismissed_query = None;
            self.last_skill_popup_areas.clear();
            self.clear_slash_menu_dismissal();
        } else {
            self.set_composer_text(&format!("${name} "));
            self.close_skill_popup();
            self.slash_menu_selected = 0;
            self.clear_slash_menu_dismissal();
        }
    }

    pub(crate) fn skill_popup_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_skill_popup_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn clamp_slash_menu_selection(&mut self, len: usize) {
        if len == 0 {
            self.slash_menu_selected = 0;
            self.last_slash_menu_areas.clear();
            return;
        }
        self.slash_menu_selected = self.slash_menu_selected.min(len.saturating_sub(1));
    }

    pub(crate) fn move_slash_menu_selection(&mut self, direction: isize, len: usize) {
        if len == 0 {
            self.slash_menu_selected = 0;
            return;
        }
        let current = self.slash_menu_selected.min(len.saturating_sub(1)) as isize;
        let next = (current + direction).rem_euclid(len as isize) as usize;
        self.slash_menu_selected = next;
    }

    pub(crate) fn set_slash_menu_selection(&mut self, index: usize, len: usize) {
        self.slash_menu_selected = if len == 0 {
            0
        } else {
            index.min(len.saturating_sub(1))
        };
    }

    pub(crate) fn slash_menu_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_slash_menu_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn slash_menu_dismissed(&self, input: &str) -> bool {
        self.slash_menu_dismissed_input.as_deref() == Some(input)
    }

    pub(crate) fn dismiss_slash_menu(&mut self) {
        self.slash_menu_dismissed_input = Some(textarea_text(&self.textarea));
        self.slash_menu_selected = 0;
        self.last_slash_menu_areas.clear();
    }

    pub(crate) fn clear_slash_menu_dismissal(&mut self) {
        self.slash_menu_dismissed_input = None;
    }

    pub(crate) fn bottom_panel_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_bottom_panel_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn set_render_areas(
        &mut self,
        transcript: Rect,
        composer: Option<Rect>,
        status: Rect,
        bottom_panel: Option<Rect>,
    ) {
        self.last_transcript_area = Some(transcript);
        self.last_composer_area = composer;
        self.last_composer_input_area = None;
        self.last_status_area = Some(status);
        self.last_bottom_panel_area = bottom_panel;
    }

    pub(crate) fn set_composer_input_area(&mut self, area: Option<Rect>) {
        self.last_composer_input_area = area;
    }

    pub(crate) fn composer_terminal_cursor_position(&mut self, area: Rect) -> Option<(u16, u16)> {
        composer_terminal_cursor_position(&self.textarea, area, &mut self.composer_cursor_top_row)
    }

    pub(crate) fn mouse_wheel_target(&self, column: u16, row: u16) -> Option<MouseWheelTarget> {
        if self
            .last_bottom_panel_area
            .is_some_and(|area| rect_contains(area, column, row))
        {
            return Some(MouseWheelTarget::BottomPanel);
        }
        if self
            .last_transcript_area
            .is_some_and(|area| rect_contains(area, column, row))
        {
            return Some(MouseWheelTarget::Transcript);
        }
        if self
            .last_composer_area
            .is_some_and(|area| rect_contains(area, column, row))
            || self
                .last_status_area
                .is_some_and(|area| rect_contains(area, column, row))
        {
            return None;
        }
        None
    }

    pub(crate) fn set_bottom_panel_notice(&mut self, text: impl Into<String>) {
        if let Some(panel) = &mut self.bottom_panel {
            match panel {
                BottomPanel::ProviderWizard(panel) => panel.notice = Some(text.into()),
                BottomPanel::AgentRunPrompt(panel) => panel.notice = Some(text.into()),
                BottomPanel::AgentEditor(panel) => panel.notice = Some(text.into()),
                _ => panel.selection_mut().notice = Some(text.into()),
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn push_user(&mut self, text: String) {
        self.push_user_with_images(text, &[]);
    }

    pub(crate) fn push_user_with_attachment_meta(
        &mut self,
        text: String,
        attachment_meta: Option<String>,
    ) {
        self.transcript
            .push(TranscriptRow::with_title(TranscriptKind::Prompt, "", text));
        if let Some(meta) = attachment_meta {
            self.transcript
                .push(TranscriptRow::simple(TranscriptKind::Meta, meta));
        }
    }

    pub(crate) fn push_user_with_images(
        &mut self,
        text: String,
        images: &[PendingImageAttachment],
    ) {
        self.push_user_with_attachment_meta(text, attachment_metadata_text(images, &self.workdir));
    }

    pub(crate) fn commit_pending_steer_from_event(&mut self, value: &Value) -> bool {
        let Some(message) = value.get("message") else {
            return false;
        };
        if message.get("role").and_then(Value::as_str) != Some("user") {
            return false;
        }
        let Some(pending_id) = pending_input_id_from_message_end(value) else {
            return false;
        };
        if let Some(index) = self
            .pending_steers
            .iter()
            .position(|input| input.id.as_u64() == pending_id)
        {
            let input = self
                .pending_steers
                .remove(index)
                .expect("pending steer index");
            self.push_user_with_images(input.display_prompt, &input.images);
        } else {
            self.push_history_message_with_accounting_options(
                message,
                value.get("usage"),
                value.get("metadata"),
                value.get("accounting"),
                false,
            );
        }
        self.history_prompt_started_ms = message_timestamp_ms(message);
        true
    }

    pub(crate) fn start_assistant(&mut self) {
        self.assistant_row = None;
        self.assistant_preamble_row = None;
        self.reasoning_row = None;
        self.meta_row = None;
        self.gateway_item_rows.clear();
        self.tool_rows.clear();
        self.live_tool_args.clear();
        self.streaming_tool_message_seq = 0;
        self.streaming_tool_message_open = false;
        self.turn_started = None;
        self.visible_turn_started = Some(Instant::now());
        self.interrupt_requested = false;
        self.turn_provider.clear();
        self.turn_model.clear();
        self.turn_context_limit = None;
        self.turn_usage = None;
        self.turn_metadata = None;
        self.turn_accounting = None;
        self.turn_session_id = None;
        self.active_event_session_id = None;
        self.turn_failures = 0;
        self.turn_interrupted = false;
        self.turn_outcome = None;
        self.turn_terminal_message = None;
        self.turn_had_reasoning = false;
        self.turn_terminal_visible_answer = false;
    }

    pub(crate) fn push_status(&mut self, text: impl Into<String>) {
        let mut row = TranscriptRow::simple(TranscriptKind::Status, text);
        self.tag_active_turn_local_row(&mut row);
        self.transcript.push(row);
    }

    pub(crate) fn set_ephemeral_status(&mut self, text: impl Into<String>) {
        self.ephemeral_status = Some(UiEphemeralStatus {
            text: text.into(),
            failed: false,
        });
    }

    pub(crate) fn clear_ephemeral_status(&mut self) {
        self.ephemeral_status = None;
    }

    pub(crate) fn set_ephemeral_error(&mut self, text: impl Into<String>) {
        self.ephemeral_status = Some(UiEphemeralStatus {
            text: text.into(),
            failed: true,
        });
    }

    pub(crate) fn push_command_result(
        &mut self,
        command: impl Into<String>,
        title: Option<&str>,
        text: impl Into<String>,
        failed: bool,
    ) {
        let mut body = String::new();
        if let Some(title) = title
            && !title.trim().is_empty()
        {
            body.push_str(title.trim());
        }
        let text = text.into();
        if !text.is_empty() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&text);
        }
        let mut row = TranscriptRow::with_title(TranscriptKind::Command, command, body);
        row.failed = failed;
        self.tag_active_turn_local_row(&mut row);
        self.transcript.push(row);
    }

    pub(crate) fn push_error(&mut self, text: impl Into<String>) {
        self.transcript
            .push(TranscriptRow::simple(TranscriptKind::Error, text));
    }
}
