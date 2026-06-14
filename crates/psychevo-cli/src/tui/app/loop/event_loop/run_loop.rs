impl TuiApp {
    pub(crate) async fn run(&mut self, initial_prompt: String) -> Result<ExitCode> {
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

    pub(crate) async fn run_fullscreen_loop(&mut self, initial_prompt: String) -> Result<()> {
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

    pub(crate) async fn run_fullscreen_loop_inner(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        initial_prompt: String,
    ) -> Result<()> {
        let mut ui = FullscreenUi::new(self);
        self.load_current_session_history(&mut ui)?;
        let mut needs_draw = true;
        let mut next_passive_redraw = schedule_next_passive_redraw(Instant::now());
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
                next_passive_redraw = schedule_next_passive_redraw(Instant::now());
            }
            if ui.quit_requested && ui.running.is_none() && self.compaction_task.is_none() {
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
            } else if fullscreen_has_passive_motion(&ui)
                && passive_redraw_due(Instant::now(), &mut next_passive_redraw)
            {
                needs_draw = true;
            }
        }
        if let Some(running) = ui.running.take() {
            if let Some(selector) = running.selector.clone() {
                self.gateway.interrupt_turn(selector);
            }
            running.control.abort();
            ui.discard_permission_approvals_for_abort();
            match running.task {
                RunningTask::Agent(task) => {
                    let _ = task.await;
                }
                RunningTask::UserShell(task) => {
                    let _ = task.await;
                }
            }
        } else {
            ui.discard_permission_approvals_for_abort();
        }
        if let Some(task) = self.diff_task.take() {
            task.task.abort();
            let _ = task.task.await;
        }
        self.model_catalog.abort_unfinished();
        Ok(())
    }

    pub(crate) async fn handle_fullscreen_event_batch(
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

    pub(crate) async fn handle_fullscreen_event(
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
                    && let Ok(ImageInput::LocalPath(path)) =
                        resolve_image_source(source, &self.workdir)
                {
                    let placeholder = ui.add_pending_image(ImageInput::LocalPath(path));
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

    pub(crate) async fn run_scripted_loop(&mut self) -> Result<()> {
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
        if let Some(task) = self.compaction_task.take() {
            task.task.abort();
            let _ = task.task.await;
        }
        if let Some(task) = self.diff_task.take() {
            task.task.abort();
            let _ = task.task.await;
        }
        Ok(())
    }

}
