impl TuiApp {
    async fn drain_fullscreen_events(&mut self, ui: &mut FullscreenUi<'_>) -> Result<bool> {
        let mut changed = false;
        changed |= self.drain_finished_auxiliary_agent_tasks(ui).await?;
        changed |= self.drain_finished_clipboard_copies(ui);
        changed |= self.drain_model_metadata_refresh(ui).await?;
        changed |= self.drain_model_catalog_fetches(ui).await?;
        changed |= ui.drain_file_search_results();

        let (had_pending, active_tool_frame_requested) =
            self.drain_available_fullscreen_stream_events(ui);
        changed |= had_pending;
        if had_pending {
            ui.follow_transcript_if_needed();
            ui.refresh_sidebar(self);
        }
        if active_tool_frame_requested {
            return Ok(true);
        }

        if ui
            .running
            .as_ref()
            .is_some_and(|running| running.task.is_finished())
        {
            let (had_pending, active_tool_frame_requested) =
                self.drain_available_fullscreen_stream_events(ui);
            changed |= had_pending;
            if had_pending {
                ui.follow_transcript_if_needed();
                ui.refresh_sidebar(self);
            }
            if active_tool_frame_requested {
                return Ok(true);
            }
        }

        if ui
            .running
            .as_ref()
            .is_some_and(|running| running.task.is_finished())
        {
            let mut running = ui.running.take().expect("checked running");
            let task = running.task;
            let completed = match task {
                RunningTask::Agent(task) => RunningCompletion::Agent(Box::new(task.await)),
                RunningTask::UserShell(task) => RunningCompletion::UserShell(task.await),
            };
            let mut pending = VecDeque::new();
            while let Ok(event) = running.rx.try_recv() {
                pending.push_back(event);
            }
            let (had_pending, _active_tool_frame_requested) =
                self.apply_pending_fullscreen_stream_events(ui, pending);
            changed = true;
            if had_pending {
                ui.follow_transcript_if_needed();
            }
            let mut restore_queued_after_interrupt = false;
            match completed {
                RunningCompletion::Agent(result) => match *result {
                    Ok(Ok(result)) => {
                        let interrupted =
                            ui.interrupt_requested && result.outcome == Outcome::Aborted;
                        restore_queued_after_interrupt |= interrupted;
                        self.last_context_snapshot = result.context_snapshot.clone();
                        ui.last_context_snapshot = result.context_snapshot.clone();
                        self.current_session = Some(result.session_id.clone());
                        self.refresh_current_session_title()?;
                        self.force_new_once = false;
                        if result.outcome != Outcome::Normal && !interrupted {
                            self.had_error = true;
                            ui.push_error(format!("turn ended: {}", result.outcome.as_str()));
                        }
                    }
                    Ok(Err(err)) => {
                        self.had_error = true;
                        ui.push_error(format!("error: {err:#}"));
                    }
                    Err(err) => {
                        self.had_error = true;
                        ui.push_error(format!("task failed: {err}"));
                    }
                },
                RunningCompletion::UserShell(result) => match result {
                    Ok(Ok(result)) => {
                        let interrupted =
                            ui.interrupt_requested && result.outcome == Outcome::Aborted;
                        restore_queued_after_interrupt |= interrupted;
                        if (result.outcome != Outcome::Normal || result.tool_failures > 0)
                            && !interrupted
                        {
                            self.had_error = true;
                        }
                    }
                    Ok(Err(err)) => {
                        self.had_error = true;
                        ui.push_error(format!("error: {err:#}"));
                    }
                    Err(err) => {
                        self.had_error = true;
                        ui.push_error(format!("task failed: {err}"));
                    }
                },
            }
            ui.finish_turn();
            ui.refresh_sidebar(self);
            if restore_queued_after_interrupt {
                ui.restore_queued_inputs_to_composer();
            } else {
                self.start_next_queued_input(ui)?;
            }
        } else if ui.turn_outcome.is_some() && ui.deferred_stream_events.is_empty() {
            self.finish_streamed_agent_turn(ui);
            changed = true;
        }
        Ok(changed)
    }

    fn drain_available_fullscreen_stream_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> (bool, bool) {
        let mut pending = std::mem::take(&mut ui.deferred_stream_events);
        if let Some(running) = &mut ui.running {
            while let Ok(event) = running.rx.try_recv() {
                pending.push_back(event);
            }
        }
        self.apply_pending_fullscreen_stream_events(ui, pending)
    }

    fn apply_pending_fullscreen_stream_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        mut pending: VecDeque<RunStreamEvent>,
    ) -> (bool, bool) {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            let active_tool_frame_requested = self.apply_fullscreen_stream_event(ui, event);
            if active_tool_frame_requested {
                ui.deferred_stream_events.extend(pending);
                return (true, true);
            }
        }
        (had_pending, false)
    }

    fn apply_fullscreen_stream_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        event: RunStreamEvent,
    ) -> bool {
        if let RunStreamEvent::Event(value) = &event {
            if value.get("type").and_then(Value::as_str) == Some("context_snapshot")
                && let Ok(snapshot) = serde_json::from_value::<ContextSnapshot>(value.clone())
            {
                self.last_context_snapshot = Some(snapshot.clone());
                ui.last_context_snapshot = Some(snapshot);
            }
            self.observe_fullscreen_value_event(value);
        }
        ui.apply_stream_event(event, self.thinking_visible, self.debug)
    }

    fn observe_fullscreen_value_event(&mut self, value: &Value) {
        if value.get("type").and_then(Value::as_str) != Some("run_start") {
            return;
        }
        let Some(session_id) = value.get("session_id").and_then(Value::as_str) else {
            return;
        };
        if self.current_session.as_deref() != Some(session_id) {
            self.current_session = Some(session_id.to_string());
            self.current_session_title = None;
        }
        self.force_new_once = false;
    }

    fn finish_streamed_agent_turn(&mut self, ui: &mut FullscreenUi<'_>) {
        let outcome = ui.turn_outcome.unwrap_or(Outcome::Normal);
        if let Some(running) = ui.running.take()
            && let RunningTask::Agent(task) = running.task
        {
            ui.auxiliary_agent_tasks.push(task);
        }
        let interrupted = ui.interrupt_requested && outcome == Outcome::Aborted;
        if outcome != Outcome::Normal && !interrupted {
            self.had_error = true;
            ui.push_error(format!("turn ended: {}", outcome.as_str()));
        }
        ui.finish_turn();
        ui.refresh_sidebar(self);
        if interrupted {
            ui.restore_queued_inputs_to_composer();
        } else if let Err(err) = self.start_next_queued_input(ui) {
            self.had_error = true;
            ui.push_error(format!("error: {err:#}"));
        }
    }

    async fn drain_finished_auxiliary_agent_tasks(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let mut changed = false;
        let mut pending = Vec::new();
        for task in std::mem::take(&mut ui.auxiliary_agent_tasks) {
            if task.is_finished() {
                if let Ok(Ok(result)) = task.await {
                    self.last_context_snapshot = result.context_snapshot.clone();
                    ui.last_context_snapshot = result.context_snapshot;
                }
                self.refresh_current_session_title()?;
                ui.refresh_sidebar(self);
                changed = true;
            } else {
                pending.push(task);
            }
        }
        ui.auxiliary_agent_tasks = pending;
        Ok(changed)
    }

    fn render_fullscreen(&self, frame: &mut Frame<'_>, ui: &mut FullscreenUi<'_>) {
        let area = frame.area();
        ui.clear_screen_lines();
        ui.set_thinking_visible(self.thinking_visible);
        let sidebar_visible = ui.sidebar_forced && area.width >= 100 && !ui.sidebar_hidden;
        ui.last_sidebar_visible = sidebar_visible;
        let horizontal = if sidebar_visible {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(40), Constraint::Length(42)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(40)])
                .split(area)
        };
        let main = horizontal[0];
        if ui.bottom_panel.is_some() {
            ui.last_slash_menu_areas.clear();
            ui.last_file_popup_areas.clear();
            ui.last_skill_popup_areas.clear();
            let panel_height = bottom_panel_height(main.height);
            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(panel_height),
                    Constraint::Length(1),
                ])
                .split(main);
            render_transcript(frame, vertical[0], ui);
            if let Some(panel) = &mut ui.bottom_panel {
                render_bottom_panel(frame, vertical[1], panel, &mut ui.last_bottom_panel_areas);
            }
            render_status(frame, vertical[2], self, ui);
            if sidebar_visible {
                render_sidebar(frame, horizontal[1], ui);
            }
            render_active_selection(frame, ui);
            return;
        }
        let composer_height = composer_height(&ui.textarea);
        let file_popup_height = ui.file_popup_height();
        let skill_popup_height = ui.skill_popup_height();
        let composer_text = textarea_text(&ui.textarea);
        let slash_items = if file_popup_height == 0 && skill_popup_height == 0 {
            if ui.slash_menu_dismissed(&composer_text) {
                Vec::new()
            } else {
                self.slash_menu_items(&composer_text)
            }
        } else {
            Vec::new()
        };
        ui.clamp_slash_menu_selection(slash_items.len());
        ui.last_bottom_panel_areas.clear();
        if file_popup_height == 0 {
            ui.last_file_popup_areas.clear();
        }
        if skill_popup_height == 0 {
            ui.last_skill_popup_areas.clear();
        }
        let slash_height = if slash_items.is_empty() {
            0
        } else {
            (slash_items.len() as u16 + 2).min(10)
        };
        let popup_height = file_popup_height.max(skill_popup_height).max(slash_height);
        let vertical = if popup_height == 0 {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(composer_height),
                    Constraint::Length(1),
                ])
                .split(main)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(popup_height),
                    Constraint::Length(composer_height),
                    Constraint::Length(1),
                ])
                .split(main)
        };
        if popup_height == 0 {
            render_transcript(frame, vertical[0], ui);
            render_composer(frame, vertical[1], ui);
            render_status(frame, vertical[2], self, ui);
        } else if file_popup_height > 0 {
            render_transcript(frame, vertical[0], ui);
            render_file_popup(frame, vertical[1], ui);
            render_composer(frame, vertical[2], ui);
            render_status(frame, vertical[3], self, ui);
        } else if skill_popup_height > 0 {
            render_transcript(frame, vertical[0], ui);
            render_skill_popup(frame, vertical[1], ui);
            render_composer(frame, vertical[2], ui);
            render_status(frame, vertical[3], self, ui);
        } else {
            render_transcript(frame, vertical[0], ui);
            render_slash_menu(
                frame,
                vertical[1],
                &slash_items,
                ui.slash_menu_selected,
                &mut ui.last_slash_menu_areas,
            );
            render_composer(frame, vertical[2], ui);
            render_status(frame, vertical[3], self, ui);
        }
        if sidebar_visible {
            render_sidebar(frame, horizontal[1], ui);
        }
        render_active_selection(frame, ui);
    }

}
