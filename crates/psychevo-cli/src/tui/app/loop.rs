impl TuiApp {
    async fn run(&mut self, initial_prompt: String) -> Result<ExitCode> {
        let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
        if interactive {
            self.run_fullscreen_loop(initial_prompt).await?;
        } else {
            if !initial_prompt.trim().is_empty() {
                self.handle_line(&initial_prompt).await?;
            }
            self.run_scripted_loop().await?;
        }

        Ok(if self.had_error {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        })
    }

    async fn run_fullscreen_loop(&mut self, initial_prompt: String) -> Result<()> {
        let mut stdout = io::stdout();
        let mut terminal_guard = FullscreenTerminalGuard::enter(&mut stdout)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        let result = self
            .run_fullscreen_loop_inner(&mut terminal, initial_prompt)
            .await;
        let restore_result = terminal_guard.restore();
        match (result, restore_result) {
            (Err(err), _) => Err(err),
            (Ok(()), Err(err)) => Err(err),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    async fn run_fullscreen_loop_inner(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        initial_prompt: String,
    ) -> Result<()> {
        let mut ui = FullscreenUi::new(self);
        self.load_current_session_history(&mut ui)?;
        let mut needs_draw = true;
        if !initial_prompt.trim().is_empty()
            && self
                .submit_fullscreen_text(&mut ui, initial_prompt, false)
                .await?
        {
            return Ok(());
        }
        loop {
            needs_draw |= self.drain_fullscreen_events(&mut ui).await?;
            if ui.take_terminal_clear_request() {
                terminal.clear()?;
                needs_draw = true;
            }
            if needs_draw {
                terminal.draw(|frame| self.render_fullscreen(frame, &mut ui))?;
                needs_draw = false;
            }
            if ui.quit_requested && ui.running.is_none() {
                break;
            }
            if event::poll(FULLSCREEN_EVENT_POLL_INTERVAL)? {
                let outcome = self
                    .handle_fullscreen_event_batch(&mut ui, event::read()?)
                    .await?;
                needs_draw |= outcome.needs_draw;
                if outcome.should_quit {
                    break;
                }
            } else if ui.running.is_some()
                || !ui.auxiliary_agent_tasks.is_empty()
                || !ui.auxiliary_shell_tasks.is_empty()
            {
                needs_draw = true;
            }
        }
        if let Some(running) = ui.running.take() {
            running.control.abort();
            match running.task {
                RunningTask::Agent(task) => {
                    let _ = task.await;
                }
                RunningTask::UserShell(task) => {
                    let _ = task.await;
                }
            }
        }
        self.model_catalog.abort_unfinished();
        Ok(())
    }

    async fn handle_fullscreen_event_batch(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        first: CrosstermEvent,
    ) -> Result<FullscreenEventOutcome> {
        let mut outcome = FullscreenEventOutcome::default();
        let mut event = first;
        for _ in 0..MAX_READY_EVENTS_PER_FRAME {
            let current = self.handle_fullscreen_event(ui, event).await?;
            outcome.needs_draw |= current.needs_draw;
            outcome.should_quit |= current.should_quit;
            if outcome.should_quit || !event::poll(Duration::ZERO)? {
                break;
            }
            event = event::read()?;
        }
        Ok(outcome)
    }

    async fn handle_fullscreen_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        event: CrosstermEvent,
    ) -> Result<FullscreenEventOutcome> {
        match event {
            CrosstermEvent::Key(key) => {
                if key.kind == KeyEventKind::Release {
                    return Ok(FullscreenEventOutcome::default());
                }
                Ok(FullscreenEventOutcome {
                    needs_draw: true,
                    should_quit: self.handle_fullscreen_key(ui, key).await?,
                })
            }
            CrosstermEvent::Mouse(mouse) => Ok(FullscreenEventOutcome {
                needs_draw: mouse_event_needs_redraw(mouse.kind),
                should_quit: self.handle_fullscreen_mouse(ui, mouse).await?,
            }),
            CrosstermEvent::Paste(pasted) => {
                let pasted = normalize_bracketed_paste_text(&pasted);
                ui.clear_history_navigation_for_edit();
                let source = pasted.trim();
                if !source.is_empty()
                    && !pasted.contains('\n')
                    && let Ok(image) = resolve_image_source(source, &self.workdir)
                {
                    let placeholder = ui.add_pending_image(image);
                    ui.textarea.insert_str(&placeholder);
                    ui.textarea.insert_str(" ");
                } else {
                    ui.textarea.insert_str(&pasted);
                }
                ui.absorb_shell_escape_prefix();
                ui.sync_pending_images_with_textarea();
                ui.clear_slash_menu_dismissal();
                ui.sync_file_popup(&self.workdir);
                self.sync_agent_popup(ui);
                self.sync_skill_popup(ui);
                Ok(FullscreenEventOutcome {
                    needs_draw: true,
                    should_quit: false,
                })
            }
            CrosstermEvent::Resize(_, _) => Ok(FullscreenEventOutcome {
                needs_draw: true,
                should_quit: false,
            }),
            _ => Ok(FullscreenEventOutcome::default()),
        }
    }

    async fn run_scripted_loop(&mut self) -> Result<()> {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if self.handle_line(&line).await? {
                break;
            }
        }
        Ok(())
    }

    async fn handle_fullscreen_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        if key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.copy_selected_text(ui)?
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
                    if let Some(target) = ui.selected_agent_target() {
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
                    if let Some(target) = ui.selected_agent_target() {
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
                    ui.sync_file_popup(&self.workdir);
                    self.sync_agent_popup(ui);
                    self.sync_skill_popup(ui);
                    return Ok(false);
                }
                KeyCode::Enter if ui.selected_file_path().is_some() => {
                    ui.insert_selected_file_path();
                    ui.sync_file_popup(&self.workdir);
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
        if let Some(should_quit) = self.handle_slash_shortcut_key(ui, key).await? {
            return Ok(should_quit);
        }
        let slash_input = textarea_text(&ui.textarea);
        let slash_count = if ui.shell_mode
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
                ui.ensure_selection();
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
                        if let Some(name) = slash_skill_name(&command) {
                            ui.insert_skill_marker(&name);
                            self.sync_agent_popup(ui);
                            self.sync_skill_popup(ui);
                            return Ok(false);
                        }
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
        ui.sync_file_popup(&self.workdir);
        self.sync_agent_popup(ui);
        self.sync_skill_popup(ui);
        Ok(false)
    }

    async fn handle_slash_shortcut_key(
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
                let should_quit = match parse_slash_command_with_config(
                    &command_line,
                    &self.slash_config,
                ) {
                    Ok(Some(command)) => {
                        self.handle_fullscreen_command_with_echo(ui, command, Some(command_line))
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

    fn slash_shortcuts_active(&self, ui: &FullscreenUi<'_>) -> bool {
        ui.focus == FocusMode::Composer
            && ui.bottom_panel.is_none()
            && !ui.history_search
            && !ui.shell_mode
            && ui.selection.anchor.is_none()
            && !ui.agent_popup_visible()
            && !ui.file_popup_visible()
            && !ui.skill_popup_visible()
            && textarea_text(&ui.textarea).trim().is_empty()
            && ui.pending_images.is_empty()
    }

    async fn handle_fullscreen_mouse(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        mouse: MouseEvent,
    ) -> Result<bool> {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.handle_fullscreen_mouse_wheel(ui, mouse.column, mouse.row, -3);
            }
            MouseEventKind::ScrollDown => {
                self.handle_fullscreen_mouse_wheel(ui, mouse.column, mouse.row, 3);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(index) = ui.bottom_panel_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    if let Some(panel) = &mut ui.bottom_panel {
                        panel.selection_mut().set_selected(index);
                    }
                    let selected = ui
                        .bottom_panel
                        .as_ref()
                        .and_then(BottomPanel::selected_value);
                    self.apply_bottom_panel_selection(ui, selected)?;
                } else if let Some(index) = ui.agent_popup_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    ui.set_agent_popup_selection(index);
                    ui.insert_selected_agent_marker();
                    self.sync_agent_popup(ui);
                    self.sync_skill_popup(ui);
                } else if let Some(index) = ui.file_popup_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    ui.set_file_popup_selection(index);
                    ui.insert_selected_file_path();
                    ui.sync_file_popup(&self.workdir);
                    self.sync_agent_popup(ui);
                    self.sync_skill_popup(ui);
                } else if let Some(index) = ui.skill_popup_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    ui.set_skill_popup_selection(index);
                    ui.insert_selected_skill_marker();
                    self.sync_agent_popup(ui);
                    self.sync_skill_popup(ui);
                } else if let Some(index) = ui.slash_menu_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    let line = textarea_text(&ui.textarea);
                    ui.set_slash_menu_selection(index, self.slash_menu_items(&line).len());
                    if let Some(command) = selected_slash_menu_command_with_items(
                        &line,
                        ui.slash_menu_selected,
                        &self.slash_items(),
                    ) {
                        if let Some(name) = slash_skill_name(&command) {
                            ui.insert_skill_marker(&name);
                            self.sync_skill_popup(ui);
                            return Ok(false);
                        }
                        let submitted = command;
                        ui.clear_composer();
                        ui.slash_menu_selected = 0;
                        ui.clear_slash_menu_dismissal();
                        ui.close_agent_popup();
                        ui.close_skill_popup();
                        ui.push_submitted_history(submitted.clone());
                    match parse_slash_command_with_config(&submitted, &self.slash_config) {
                            Ok(Some(command)) => {
                                return self
                                    .handle_fullscreen_command_with_echo(
                                        ui,
                                        command,
                                        Some(submitted),
                                    )
                                    .await;
                            }
                            Ok(None) => {}
                            Err(err) => {
                                ui.push_command_result(
                                    normalize_submitted_slash_echo(&submitted),
                                    None,
                                    format!("error: {err:#}"),
                                    true,
                                );
                                return Ok(false);
                            }
                        }
                    }
                } else if let Some(target) = ui.transcript_hit(mouse.column, mouse.row) {
                    ui.mouse_down_target = Some(target);
                    ui.mouse_dragged = false;
                    if ui.selectable_hit(mouse.column, mouse.row) {
                        ui.start_selection(mouse.column, mouse.row);
                    } else {
                        ui.clear_selection();
                    }
                } else if ui.selectable_hit(mouse.column, mouse.row) {
                    ui.mouse_down_target = None;
                    ui.mouse_dragged = false;
                    ui.start_selection(mouse.column, mouse.row);
                } else {
                    ui.mouse_down_target = None;
                    ui.mouse_dragged = false;
                    ui.clear_selection();
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                ui.mouse_dragged = true;
                ui.update_selection(mouse.column, mouse.row);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                ui.update_selection(mouse.column, mouse.row);
                let up_target = ui.transcript_hit(mouse.column, mouse.row);
                let click_target = (!ui.mouse_dragged
                    && ui.mouse_down_target.is_some()
                    && ui.mouse_down_target == up_target)
                    .then_some(ui.mouse_down_target)
                    .flatten();
                if !self.start_copy_selected_text(ui) {
                    if let Some(target) = click_target {
                        if let Some(agent_target) = ui.agent_target_for_target(target) {
                            self.open_agent_target_session(ui, &agent_target)?;
                        } else {
                            ui.toggle_target(target);
                        }
                    }
                    ui.clear_selection();
                }
                ui.mouse_down_target = None;
                ui.mouse_dragged = false;
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_fullscreen_mouse_wheel(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        column: u16,
        row: u16,
        amount: isize,
    ) {
        match ui.mouse_wheel_target(column, row) {
            Some(MouseWheelTarget::BottomPanel) => {
                if let Some(panel) = &mut ui.bottom_panel {
                    scroll_bottom_panel(panel, amount);
                }
            }
            Some(MouseWheelTarget::Transcript) => ui.scroll_transcript(amount),
            None => {}
        }
    }
}

const FULLSCREEN_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const MAX_READY_EVENTS_PER_FRAME: usize = 64;
const TUI_MOUSE_CAPTURE_ENABLE_ANSI: &str = concat!(
    "\x1b[?1000h",
    "\x1b[?1002h",
    "\x1b[?1015h",
    "\x1b[?1006h",
    "\x1b[?1007h"
);
const TUI_MOUSE_CAPTURE_DISABLE_ANSI: &str = concat!(
    "\x1b[?1007l",
    "\x1b[?1006l",
    "\x1b[?1015l",
    "\x1b[?1002l",
    "\x1b[?1000l"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnableTuiMouseCapture;

impl crossterm::Command for EnableTuiMouseCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str(TUI_MOUSE_CAPTURE_ENABLE_ANSI)
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "tried to execute EnableTuiMouseCapture using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisableTuiMouseCapture;

impl crossterm::Command for DisableTuiMouseCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str(TUI_MOUSE_CAPTURE_DISABLE_ANSI)
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "tried to execute DisableTuiMouseCapture using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug)]
struct FullscreenTerminalGuard {
    active: bool,
}

impl FullscreenTerminalGuard {
    fn enter(stdout: &mut io::Stdout) -> Result<Self> {
        enable_raw_mode()?;
        if let Err(err) = write_fullscreen_enter_commands(stdout) {
            let _ = restore_fullscreen_terminal_modes();
            return Err(err.into());
        }
        Ok(Self { active: true })
    }

    fn restore(&mut self) -> Result<()> {
        if self.active {
            restore_fullscreen_terminal_modes()?;
            self.active = false;
        }
        Ok(())
    }
}

impl Drop for FullscreenTerminalGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = restore_fullscreen_terminal_modes();
            self.active = false;
        }
    }
}

fn write_fullscreen_enter_commands(out: &mut impl Write) -> io::Result<()> {
    execute!(out, EnterAlternateScreen)?;
    execute!(
        out,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
        crossterm::cursor::MoveTo(0, 0)
    )?;
    execute!(out, EnableBracketedPaste)?;
    execute!(out, EnableTuiMouseCapture)?;
    Ok(())
}

fn write_fullscreen_exit_commands(out: &mut impl Write) -> io::Result<()> {
    let mut first_error = execute!(out, DisableBracketedPaste).err();
    if let Err(err) = execute!(out, DisableTuiMouseCapture) {
        first_error.get_or_insert(err);
    }
    if let Err(err) = execute!(out, LeaveAlternateScreen) {
        first_error.get_or_insert(err);
    }
    if let Err(err) = execute!(out, crossterm::cursor::Show) {
        first_error.get_or_insert(err);
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn restore_fullscreen_terminal_modes() -> io::Result<()> {
    let mut stdout = io::stdout();
    let mut first_error = write_fullscreen_exit_commands(&mut stdout).err();
    if let Err(err) = disable_raw_mode() {
        first_error.get_or_insert(err);
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FullscreenEventOutcome {
    needs_draw: bool,
    should_quit: bool,
}

fn mouse_event_needs_redraw(kind: MouseEventKind) -> bool {
    !matches!(kind, MouseEventKind::Moved)
}

fn scroll_bottom_panel(panel: &mut BottomPanel, amount: isize) {
    match panel {
        BottomPanel::Help(panel) => panel.scroll_by(amount),
        BottomPanel::Models(panel) if panel.tab == ModelTab::Info => panel.scroll_info_by(amount),
        BottomPanel::ProviderWizard(_) => {}
        _ => panel.selection_mut().move_selection(amount),
    }
}

fn normalize_bracketed_paste_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn slash_skill_name(command: &str) -> Option<String> {
    let name = command.strip_prefix("/skill:")?;
    let name = name.split_whitespace().next().unwrap_or_default();
    (!name.is_empty()).then(|| name.to_string())
}
