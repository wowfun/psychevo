impl TuiApp {
    pub(crate) async fn handle_fullscreen_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        if ui.diff_overlay.is_some() {
            self.handle_diff_overlay_key(ui, key);
            return Ok(false);
        }
        if key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.copy_selected_text(ui)?
        {
            return Ok(false);
        }
        if key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.handle_side_conversation_ctrl_c(ui)?
        {
            return Ok(false);
        }
        if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.copy_latest_answer_markdown(ui);
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Left => {
                    self.open_agent_parent_session(ui)?;
                    return Ok(false);
                }
                KeyCode::Char('p') | KeyCode::Char('P') => {
                    self.open_agent_parent_session(ui)?;
                    return Ok(false);
                }
                KeyCode::Up => {
                    self.open_agent_sibling_session(ui, -1)?;
                    return Ok(false);
                }
                KeyCode::Right => {
                    self.open_agent_sibling_session(ui, 1)?;
                    return Ok(false);
                }
                _ => {}
            }
        }
        if key.code == KeyCode::Esc && ui.selection.anchor.is_some() {
            ui.clear_selection();
            return Ok(false);
        }
        if matches!(ui.bottom_panel, Some(BottomPanel::AgentRunPrompt(_))) {
            return self.handle_agent_run_prompt_key(ui, key).await;
        }
        if ui.bottom_panel.is_some() {
            return self.handle_bottom_panel_key(ui, key);
        }
        if ui.history_search {
            return self.handle_history_search_key(ui, key);
        }
        if ui.focus == FocusMode::Transcript {
            match key.code {
                KeyCode::Esc => {
                    if !self.request_current_session_interrupt(ui) {
                        ui.focus = FocusMode::Composer;
                    }
                }
                KeyCode::Up => ui.move_selection(-1),
                KeyCode::Down => ui.move_selection(1),
                KeyCode::Enter => {
                    let selected_visible =
                        ui.selected_target.is_some_and(|target| ui.target_visible(target));
                    if !selected_visible {
                        ui.ensure_agent_open_selection();
                    } else {
                        ui.ensure_selection();
                    }
                    let agent_target = if selected_visible {
                        ui.selected_agent_target()
                    } else {
                        ui.selected_agent_target()
                            .or_else(|| ui.visible_agent_target())
                    };
                    if let Some(target) = agent_target {
                        self.open_agent_target_session(ui, &target)?;
                    } else {
                        ui.toggle_selected();
                    }
                }
                KeyCode::Char(' ') => {
                    if ui
                        .selected_target
                        .is_some_and(|target| ui.target_toggleable(target))
                    {
                        ui.toggle_selected();
                    }
                }
                KeyCode::Char('o') | KeyCode::Char('O') => {
                    if !ui.selected_target.is_some_and(|target| ui.target_visible(target)) {
                        ui.ensure_agent_open_selection();
                    } else {
                        ui.ensure_selection();
                    }
                    if let Some(target) = ui.selected_agent_target().or_else(|| ui.visible_agent_target()) {
                        self.open_agent_target_session(ui, &target)?;
                    }
                }
                KeyCode::PageUp => ui.scroll_transcript(-6),
                KeyCode::PageDown => ui.scroll_transcript(6),
                _ => {}
            }
            return Ok(false);
        }
        if ui.agent_popup_visible() {
            match key.code {
                KeyCode::Up => {
                    ui.move_agent_popup_selection(-1);
                    return Ok(false);
                }
                KeyCode::Down => {
                    ui.move_agent_popup_selection(1);
                    return Ok(false);
                }
                KeyCode::Home => {
                    ui.set_agent_popup_selection(0);
                    return Ok(false);
                }
                KeyCode::End => {
                    ui.set_agent_popup_selection(FILE_POPUP_MAX_ROWS.saturating_sub(1));
                    return Ok(false);
                }
                KeyCode::Esc => {
                    ui.dismiss_agent_popup();
                    return Ok(false);
                }
                KeyCode::Tab | KeyCode::Enter if ui.selected_agent_name().is_some() => {
                    ui.insert_selected_agent_marker();
                    self.sync_agent_popup(ui);
                    self.sync_skill_popup(ui);
                    return Ok(false);
                }
                _ => {}
            }
        }
        if ui.file_popup_visible() {
            match key.code {
                KeyCode::Up => {
                    ui.move_file_popup_selection(-1);
                    return Ok(false);
                }
                KeyCode::Down => {
                    ui.move_file_popup_selection(1);
                    return Ok(false);
                }
                KeyCode::Home => {
                    ui.set_file_popup_selection(0);
                    return Ok(false);
                }
                KeyCode::End => {
                    ui.set_file_popup_selection(FILE_POPUP_MAX_ROWS.saturating_sub(1));
                    return Ok(false);
                }
                KeyCode::Esc => {
                    ui.dismiss_file_popup();
                    return Ok(false);
                }
                KeyCode::Tab => {
                    ui.insert_selected_file_path();
                    ui.sync_file_popup(&self.cwd);
                    self.sync_agent_popup(ui);
                    self.sync_skill_popup(ui);
                    return Ok(false);
                }
                KeyCode::Enter if ui.selected_file_path().is_some() => {
                    ui.insert_selected_file_path();
                    ui.sync_file_popup(&self.cwd);
                    self.sync_agent_popup(ui);
                    self.sync_skill_popup(ui);
                    return Ok(false);
                }
                _ => {}
            }
        }
        if ui.skill_popup_visible() {
            match key.code {
                KeyCode::Up => {
                    ui.move_skill_popup_selection(-1);
                    return Ok(false);
                }
                KeyCode::Down => {
                    ui.move_skill_popup_selection(1);
                    return Ok(false);
                }
                KeyCode::Home => {
                    ui.set_skill_popup_selection(0);
                    return Ok(false);
                }
                KeyCode::End => {
                    ui.set_skill_popup_selection(FILE_POPUP_MAX_ROWS.saturating_sub(1));
                    return Ok(false);
                }
                KeyCode::Esc => {
                    ui.dismiss_skill_popup();
                    return Ok(false);
                }
                KeyCode::Tab | KeyCode::Enter if ui.selected_skill_name().is_some() => {
                    ui.insert_selected_skill_marker();
                    self.sync_agent_popup(ui);
                    self.sync_skill_popup(ui);
                    return Ok(false);
                }
                _ => {}
            }
        }
        if ui.pending_input_edit.is_some() {
            return self.handle_pending_input_edit_key(ui, key);
        }
        if let Some(should_quit) = self.handle_slash_shortcut_key(ui, key).await? {
            return Ok(should_quit);
        }
        let slash_input = textarea_text(&ui.textarea);
        let slash_count = if ui.shell_mode
            || ui.textarea.is_selecting()
            || ui.current_file_token().is_some()
            || ui.current_agent_token().is_some()
            || ui.current_skill_token().is_some()
            || ui.slash_menu_dismissed(&slash_input)
        {
            0
        } else {
            self.slash_menu_items(&slash_input).len()
        };
        if slash_count > 0 {
            match key.code {
                KeyCode::Up => {
                    ui.move_slash_menu_selection(-1, slash_count);
                    return Ok(false);
                }
                KeyCode::Down => {
                    ui.move_slash_menu_selection(1, slash_count);
                    return Ok(false);
                }
                KeyCode::Home => {
                    ui.set_slash_menu_selection(0, slash_count);
                    return Ok(false);
                }
                KeyCode::End => {
                    ui.set_slash_menu_selection(slash_count.saturating_sub(1), slash_count);
                    return Ok(false);
                }
                KeyCode::Esc => {
                    ui.dismiss_slash_menu();
                    return Ok(false);
                }
                _ => {}
            }
        }
        if key.code == KeyCode::Char('a') && key.modifiers.contains(KeyModifiers::CONTROL) {
            ui.select_composer_all();
            return Ok(false);
        }
        if key.code == KeyCode::Esc && ui.cancel_composer_selection() {
            return Ok(false);
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if ui.running.is_some() {
                    ui.push_status("press Ctrl+C again to quit after the running turn");
                    ui.quit_requested = true;
                    return Ok(false);
                }
                return Ok(true);
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(true);
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ui.close_file_popup();
                ui.close_agent_popup();
                ui.close_skill_popup();
                ui.history_search = true;
                ui.history_query.clear();
                ui.push_status("history search");
            }
            KeyCode::Char('?')
                if key.modifiers.is_empty()
                    && !ui.shell_mode
                    && textarea_text(&ui.textarea).trim().is_empty() =>
            {
                ui.bottom_panel = Some(BottomPanel::Help(self.help_panel()));
                ui.clear_slash_menu_dismissal();
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ui.focus = FocusMode::Transcript;
                ui.selected_target = None;
                ui.selected_row = None;
                ui.ensure_agent_open_selection();
                ui.push_status("transcript review");
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let visible = !ui.sidebar_enabled();
                self.set_sidebar_visible_no_print(visible)?;
                ui.sidebar_forced = visible;
                ui.sidebar_hidden = !visible;
                ui.refresh_sidebar(self);
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ui.textarea.insert_newline();
            }
            KeyCode::Enter if is_newline_key(key) => {
                ui.textarea.insert_newline();
            }
            KeyCode::Enter => {
                ui.sync_pending_images_with_textarea();
                let line = textarea_text(&ui.textarea);
                if !ui.shell_mode && line.trim().is_empty() && ui.pending_images.is_empty() {
                    return Ok(false);
                }
                let submitted = if ui.shell_mode {
                    ui.composer_submission_text()
                } else if parse_shell_escape_input(&line).is_some()
                    || should_submit_typed_slash(&line)
                    || self.slash_config.is_configured_alias_token(&line)
                {
                    line.clone()
                } else {
                    if let Some(command) = selected_slash_menu_command_with_items(
                        &line,
                        ui.slash_menu_selected,
                        &self.slash_items(),
                    ) {
                        command
                    } else {
                        line.clone()
                    }
                };
                ui.clear_composer();
                ui.slash_menu_selected = 0;
                ui.clear_slash_menu_dismissal();
                ui.close_file_popup();
                ui.close_agent_popup();
                ui.close_skill_popup();
                if self.submit_fullscreen_text(ui, submitted, true).await? {
                    return Ok(true);
                }
            }
            KeyCode::BackTab => {
                self.cycle_mode(ui)?;
            }
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.cycle_mode(ui)?;
            }
            KeyCode::Tab => {
                if !ui.shell_mode {
                    ui.complete_slash_command(&self.slash_items());
                }
            }
            KeyCode::Char('1')
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                ui.clear_history_navigation_for_edit();
                if textarea_text(&ui.textarea).trim().is_empty() {
                    ui.textarea = new_textarea();
                    ui.enter_shell_mode();
                } else {
                    ui.textarea.insert_char('!');
                    ui.absorb_shell_escape_prefix();
                }
                ui.clear_slash_menu_dismissal();
            }
            KeyCode::Esc => {
                if ui.shell_mode && textarea_text(&ui.textarea).trim().is_empty() {
                    ui.exit_shell_mode();
                    ui.textarea = new_textarea();
                    ui.clear_slash_menu_dismissal();
                    ui.close_file_popup();
                    ui.close_agent_popup();
                    ui.close_skill_popup();
                    return Ok(false);
                }
                if is_empty_shell_escape_input(&textarea_text(&ui.textarea)) {
                    ui.clear_composer();
                    ui.clear_slash_menu_dismissal();
                    ui.close_file_popup();
                    ui.close_agent_popup();
                    ui.close_skill_popup();
                    return Ok(false);
                }
                if self.request_current_session_interrupt(ui) {
                    return Ok(false);
                }
            }
            KeyCode::Backspace if ui.shell_mode && textarea_text(&ui.textarea).is_empty() => {
                ui.exit_shell_mode();
                ui.clear_slash_menu_dismissal();
                ui.close_file_popup();
                ui.close_agent_popup();
                ui.close_skill_popup();
                return Ok(false);
            }
            KeyCode::PageUp => ui.scroll_transcript(-6),
            KeyCode::PageDown => ui.scroll_transcript(6),
            KeyCode::Up if ui.can_recall_history_previous() => {
                ui.recall_history(-1);
            }
            KeyCode::Down if ui.can_recall_history_next() => {
                ui.recall_history(1);
            }
            _ => {
                ui.clear_history_navigation_for_edit();
                ui.textarea.input(key);
                ui.absorb_shell_escape_prefix();
                ui.clear_slash_menu_dismissal();
            }
        }
        ui.sync_pending_images_with_textarea();
        ui.sync_file_popup(&self.cwd);
        self.sync_agent_popup(ui);
        self.sync_skill_popup(ui);
        Ok(false)
    }

    pub(crate) fn handle_diff_overlay_key(&mut self, ui: &mut FullscreenUi<'_>, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            ui.diff_overlay = None;
            ui.last_diff_overlay_area = None;
            return;
        }
        let viewport_height = ui
            .last_diff_overlay_area
            .map(|area| area.height)
            .unwrap_or(12);
        let page = viewport_height.saturating_sub(4).max(1) as isize;
        let Some(overlay) = ui.diff_overlay.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Up => overlay.scroll_by(-1, viewport_height),
            KeyCode::Down => overlay.scroll_by(1, viewport_height),
            KeyCode::PageUp => overlay.scroll_by(-page, viewport_height),
            KeyCode::PageDown => overlay.scroll_by(page, viewport_height),
            KeyCode::Home => overlay.scroll_to_top(),
            KeyCode::End => overlay.scroll_to_bottom(viewport_height),
            _ => {}
        }
    }

    pub(crate) fn handle_pending_input_edit_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(edit) = ui.pending_input_edit.as_mut() {
                    edit.textarea.insert_newline();
                }
            }
            KeyCode::Enter if is_newline_key(key) => {
                if let Some(edit) = ui.pending_input_edit.as_mut() {
                    edit.textarea.insert_newline();
                }
            }
            KeyCode::Enter => {
                self.confirm_pending_input_edit(ui)?;
            }
            KeyCode::Esc => {
                ui.cancel_pending_input_edit();
            }
            _ => {
                if let Some(edit) = ui.pending_input_edit.as_mut() {
                    edit.textarea.input(key);
                }
            }
        }
        Ok(false)
    }

    pub(crate) async fn handle_slash_shortcut_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<Option<bool>> {
        if !self.slash_shortcuts_active(ui) {
            ui.pending_leader_started = None;
            return Ok(None);
        }
        let leader_pending = ui
            .pending_leader_started
            .take()
            .is_some_and(|started| started.elapsed() <= self.slash_config.leader_timeout());
        match self.slash_config.shortcut_for_key(&key, leader_pending) {
            Some(SlashShortcutMatch::LeaderPrefix) => {
                ui.pending_leader_started = Some(Instant::now());
                Ok(Some(false))
            }
            Some(SlashShortcutMatch::Command(command_line)) => {
                let should_quit =
                    match parse_slash_command_with_config(&command_line, &self.slash_config) {
                        Ok(Some(command)) => {
                            self.handle_fullscreen_command_with_echo(
                                ui,
                                command,
                                Some(command_line),
                            )
                            .await?
                        }
                        Ok(None) => false,
                        Err(err) => {
                            ui.push_command_result(
                                normalize_submitted_slash_echo(&command_line),
                                None,
                                format!("error: {err:#}"),
                                true,
                            );
                            false
                        }
                    };
                Ok(Some(should_quit))
            }
            None => Ok(None),
        }
    }

    pub(crate) fn slash_shortcuts_active(&self, ui: &FullscreenUi<'_>) -> bool {
        ui.focus == FocusMode::Composer
            && ui.bottom_panel.is_none()
            && ui.diff_overlay.is_none()
            && !ui.history_search
            && !ui.shell_mode
            && ui.pending_input_edit.is_none()
            && ui.selection.anchor.is_none()
            && !ui.textarea.is_selecting()
            && !ui.agent_popup_visible()
            && !ui.file_popup_visible()
            && !ui.skill_popup_visible()
            && textarea_text(&ui.textarea).trim().is_empty()
            && ui.pending_images.is_empty()
    }
}
