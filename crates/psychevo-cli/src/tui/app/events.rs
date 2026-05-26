#[allow(unused_imports)]
pub(crate) use super::*;
impl TuiApp {
    pub(crate) async fn drain_fullscreen_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let mut changed = false;
        let (agent_changed, active_tool_frame_requested) =
            self.drain_finished_auxiliary_agent_tasks(ui).await?;
        changed |= agent_changed;
        if active_tool_frame_requested {
            return Ok(true);
        }
        let (shell_changed, active_tool_frame_requested) =
            self.drain_auxiliary_shell_tasks(ui).await?;
        changed |= shell_changed;
        if active_tool_frame_requested {
            return Ok(true);
        }
        changed |= self.drain_finished_clipboard_copies(ui);
        changed |= self.drain_side_cleanup_task(ui).await?;
        changed |= self.drain_compaction_task(ui).await?;
        changed |= self.drain_diff_task(ui).await?;
        changed |= self.drain_model_metadata_refresh(ui).await?;
        changed |= self.drain_model_catalog_fetches(ui).await?;
        changed |= ui.drain_file_search_results();
        changed |= ui.drain_permission_approval_requests();
        changed |= self.maybe_reload_live_agent_session(ui)?;

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
            let owner_session = running.session_id.clone();
            let task = running.task;
            let completed = match task {
                RunningTask::Agent(task) => RunningCompletion::Agent(Box::new(task.await)),
                RunningTask::UserShell(task) => RunningCompletion::UserShell(task.await),
            };
            let mut pending = VecDeque::new();
            while let Ok(event) = running.rx.try_recv() {
                pending.push_back(event);
            }
            let had_pending = self.apply_pending_owned_fullscreen_stream_events(
                ui,
                owner_session.as_deref(),
                pending,
            );
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
                        if interrupted {
                            ui.turn_interrupted = true;
                        }
                        restore_queued_after_interrupt |= interrupted;
                        self.last_context_snapshot = result.context_snapshot.clone();
                        ui.last_context_snapshot = result.context_snapshot.clone();
                        ui.session_live_event_backlog.remove(&result.session_id);
                        if self.current_session.as_deref() == Some(result.session_id.as_str()) {
                            self.refresh_current_session_title()?;
                            self.force_new_once = false;
                        }
                        if result.outcome != Outcome::Normal && !interrupted {
                            self.had_error = true;
                            ui.push_error(turn_ended_error_message(
                                result.outcome,
                                result.terminal_reason,
                            ));
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
                        if interrupted {
                            ui.turn_interrupted = true;
                        }
                        restore_queued_after_interrupt |= interrupted;
                        if let Some(session_id) = result.session_id {
                            ui.session_live_event_backlog.remove(&session_id);
                            if self.current_session.as_deref() == Some(session_id.as_str()) {
                                self.refresh_current_session_title()?;
                                self.force_new_once = false;
                            }
                        }
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
            ui.update_turn_meta(self.debug, true, true, true);
            ui.finish_turn();
            ui.refresh_sidebar(self);
            if restore_queued_after_interrupt {
                ui.restore_queued_inputs_to_composer();
            } else if !self.maybe_start_auto_compaction(ui)? {
                self.start_next_queued_input(ui)?;
            }
        } else if ui.turn_outcome.is_some() && ui.deferred_stream_events.is_empty() {
            self.finish_streamed_agent_turn(ui);
            changed = true;
        }
        Ok(changed)
    }

    pub(crate) fn drain_available_fullscreen_stream_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> (bool, bool) {
        let mut pending = std::mem::take(&mut ui.deferred_stream_events);
        let owner_session = ui
            .running
            .as_ref()
            .and_then(|running| running.session_id.clone());
        if let Some(running) = &mut ui.running {
            while let Ok(event) = running.rx.try_recv() {
                pending.push_back(event);
            }
        }
        if owner_session.is_none() {
            self.apply_pending_fullscreen_stream_events(ui, pending)
        } else {
            self.apply_pending_owned_fullscreen_stream_events_with_frames(
                ui,
                owner_session.as_deref(),
                pending,
            )
        }
    }

    pub(crate) fn apply_pending_fullscreen_stream_events(
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

    pub(crate) fn apply_pending_fullscreen_stream_events_without_frames(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        mut pending: VecDeque<RunStreamEvent>,
    ) -> bool {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            self.apply_fullscreen_stream_event(ui, event);
        }
        had_pending
    }

    pub(crate) fn apply_pending_auxiliary_agent_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        mut pending: VecDeque<RunStreamEvent>,
    ) -> bool {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            self.apply_auxiliary_agent_stream_event(ui, owner_session, event);
        }
        had_pending
    }

    pub(crate) fn apply_pending_auxiliary_shell_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        mut pending: VecDeque<RunStreamEvent>,
    ) -> (bool, bool) {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            let active_tool_frame_requested =
                self.apply_auxiliary_shell_stream_event(ui, owner_session, event);
            if active_tool_frame_requested {
                ui.deferred_stream_events.extend(pending);
                return (true, true);
            }
        }
        (had_pending, false)
    }

    pub(crate) fn apply_pending_owned_fullscreen_stream_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        mut pending: VecDeque<RunStreamEvent>,
    ) -> bool {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            self.apply_owned_fullscreen_stream_event(ui, owner_session, event);
        }
        had_pending
    }

    pub(crate) fn apply_pending_owned_fullscreen_stream_events_with_frames(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        mut pending: VecDeque<RunStreamEvent>,
    ) -> (bool, bool) {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            let active_tool_frame_requested =
                self.apply_owned_fullscreen_stream_event(ui, owner_session, event);
            if active_tool_frame_requested {
                ui.deferred_stream_events.extend(pending);
                return (true, true);
            }
        }
        (had_pending, false)
    }

    pub(crate) fn apply_auxiliary_agent_stream_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        event: RunStreamEvent,
    ) {
        match event {
            RunStreamEvent::Scoped { session_id, event } => {
                self.apply_scoped_fullscreen_stream_event(ui, &session_id, *event);
            }
            other => {
                self.apply_owned_fullscreen_stream_event(ui, owner_session, other);
            }
        }
    }

    pub(crate) fn apply_auxiliary_shell_stream_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        event: RunStreamEvent,
    ) -> bool {
        self.apply_owned_fullscreen_stream_event(ui, owner_session, event)
    }

    pub(crate) fn apply_owned_fullscreen_stream_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        event: RunStreamEvent,
    ) -> bool {
        if let RunStreamEvent::Scoped { session_id, event } = event {
            return self.apply_scoped_fullscreen_stream_event(ui, &session_id, *event);
        }
        let event_has_session = stream_event_session_id(&event).is_some();
        let event_session = stream_event_session_id(&event)
            .map(str::to_string)
            .or_else(|| owner_session.map(str::to_string));
        if let Some(session_id) = event_session.as_deref()
            && self.current_session.as_deref() != Some(session_id)
        {
            if matches!(event, RunStreamEvent::ClarifyRequest(_)) {
                ui.push_status(format!(
                    "clarify pending in session {}",
                    short_session(session_id)
                ));
            }
            buffer_session_live_event(ui, session_id, event);
            return false;
        }
        if !event_has_session && let Some(session_id) = event_session.as_deref() {
            buffer_session_live_event(ui, session_id, event.clone());
        }
        let previous = ui.active_event_session_id.clone();
        if let Some(session_id) = event_session.as_deref() {
            ui.active_event_session_id = Some(session_id.to_string());
        }
        let active_tool_frame_requested = self.apply_fullscreen_stream_event(ui, event);
        ui.active_event_session_id = previous;
        active_tool_frame_requested
    }

    pub(crate) fn apply_fullscreen_stream_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        event: RunStreamEvent,
    ) -> bool {
        if let RunStreamEvent::Scoped { session_id, event } = event {
            return self.apply_scoped_fullscreen_stream_event(ui, &session_id, *event);
        }
        let event_session_id = stream_event_session_id(&event).map(str::to_string);
        if let Some(session_id) = event_session_id.as_deref() {
            let running_owner_missing = ui
                .running
                .as_ref()
                .is_some_and(|running| running.session_id.is_none());
            if let Some(running) = ui.running.as_mut()
                && running.session_id.is_none()
            {
                running.session_id = Some(session_id.to_string());
            }
            if running_owner_missing && self.current_session.is_none() {
                self.current_session = Some(session_id.to_string());
                self.reset_live_agent_reload_poll();
                self.current_session_title = None;
            }
            if self
                .current_session
                .as_deref()
                .is_some_and(|current| current != session_id)
                && !running_owner_missing
            {
                buffer_session_live_event(ui, session_id, event);
                return false;
            }
            if self.current_session.as_deref() == Some(session_id) {
                buffer_session_live_event(ui, session_id, event.clone());
            }
        }
        let event_session = event_session_id.as_deref();
        if let RunStreamEvent::Event(value) = &event {
            if value.get("type").and_then(Value::as_str) == Some("context_snapshot")
                && let Ok(snapshot) = serde_json::from_value::<ContextSnapshot>(value.clone())
            {
                self.last_context_snapshot = Some(snapshot.clone());
                ui.last_context_snapshot = Some(snapshot);
            }
            let run_started = self.observe_fullscreen_value_event(ui, value);
            let active_tool_frame_requested = ui.apply_stream_event_for_session(
                event,
                self.thinking_visible,
                self.debug,
                event_session,
            );
            if run_started && let Err(err) = self.start_pending_auxiliary_shells(ui) {
                self.had_error = true;
                ui.push_error(format!("error: {err:#}"));
            }
            return active_tool_frame_requested;
        }
        ui.apply_stream_event_for_session(event, self.thinking_visible, self.debug, event_session)
    }

    pub(crate) fn apply_scoped_fullscreen_stream_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
        event: RunStreamEvent,
    ) -> bool {
        if self.current_session.as_deref() == Some(session_id) {
            buffer_session_live_event(ui, session_id, event.clone());
            return ui.apply_stream_event_for_session(
                event,
                self.thinking_visible,
                self.debug,
                Some(session_id),
            );
        }
        if session_live_event_ends_backlog(&event) {
            ui.session_live_event_backlog.remove(session_id);
        } else {
            if matches!(event, RunStreamEvent::ClarifyRequest(_)) {
                ui.push_status(format!(
                    "clarify pending in session {}",
                    short_session(session_id)
                ));
            }
            buffer_session_live_event(ui, session_id, event.clone());
        }
        if agent_child_event_ends_live_backlog(&event) {
            ui.agent_child_event_backlog.remove(session_id);
        } else {
            let backlog = ui
                .agent_child_event_backlog
                .entry(session_id.to_string())
                .or_default();
            backlog.push(event.clone());
            const MAX_AGENT_CHILD_BACKLOG_EVENTS: usize = 200;
            if backlog.len() > MAX_AGENT_CHILD_BACKLOG_EVENTS {
                let drain = backlog.len() - MAX_AGENT_CHILD_BACKLOG_EVENTS;
                backlog.drain(0..drain);
            }
        }
        ui.apply_agent_child_preview_event(session_id, &event);
        false
    }

    pub(crate) fn observe_fullscreen_value_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        value: &Value,
    ) -> bool {
        if value.get("type").and_then(Value::as_str) != Some("run_start") {
            return false;
        }
        let Some(session_id) = value.get("session_id").and_then(Value::as_str) else {
            return false;
        };
        if self.current_session.as_deref() != Some(session_id) {
            self.current_session = Some(session_id.to_string());
            self.reset_live_agent_reload_poll();
            self.current_session_title = None;
        }
        if let Some(running) = ui.running.as_mut()
            && running.session_id.is_none()
        {
            running.session_id = Some(session_id.to_string());
        }
        if let Err(err) = self.persist_main_agent_selection_for_session(session_id) {
            self.had_error = true;
            ui.push_error(format!(
                "error: failed to persist main agent selection: {err:#}"
            ));
        }
        self.force_new_once = false;
        true
    }

    pub(crate) fn finish_streamed_agent_turn(&mut self, ui: &mut FullscreenUi<'_>) {
        let outcome = ui.turn_outcome.unwrap_or(Outcome::Normal);
        let terminal_message = ui.turn_terminal_message.take();
        if let Some(running) = ui.running.take() {
            let owner_session = self.current_session.clone();
            let RunningTurn {
                session_id,
                control,
                rx,
                task,
            } = running;
            let owner_session = session_id.or(owner_session);
            match task {
                RunningTask::Agent(task) => {
                    ui.auxiliary_agent_tasks.push(AuxiliaryAgentTask {
                        session_id: owner_session,
                        child_session_id: None,
                        visible_live: false,
                        control,
                        rx,
                        task,
                    });
                }
                RunningTask::UserShell(task) => {
                    ui.running = Some(RunningTurn {
                        session_id: owner_session,
                        control,
                        rx,
                        task: RunningTask::UserShell(task),
                    });
                }
            }
        }
        let interrupted = ui.interrupt_requested && outcome == Outcome::Aborted;
        if interrupted {
            ui.turn_interrupted = true;
        }
        if outcome != Outcome::Normal && !interrupted {
            self.had_error = true;
            ui.push_error(turn_ended_error_text(outcome, terminal_message.as_deref()));
        }
        ui.update_turn_meta(self.debug, true, true, true);
        ui.finish_turn();
        ui.refresh_sidebar(self);
        if interrupted {
            ui.restore_queued_inputs_to_composer();
        } else if let Err(err) = self.maybe_start_auto_compaction(ui).and_then(|started| {
            if started {
                Ok(())
            } else {
                self.start_next_queued_input(ui)
            }
        }) {
            self.had_error = true;
            ui.push_error(format!("error: {err:#}"));
        }
    }

    pub(crate) async fn drain_compaction_task(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let Some(task) = self.compaction_task.as_ref() else {
            return Ok(false);
        };
        if !task.task.is_finished() {
            return Ok(false);
        }
        let task = self.compaction_task.take().expect("checked task");
        let command_echo = task.command_echo;
        let manual = task.manual;
        let session_id = task.session_id;
        match task.task.await {
            Ok(Ok(result)) => {
                self.last_context_snapshot = None;
                ui.last_context_snapshot = None;
                if command_echo.is_some() || manual {
                    ui.push_command_result(
                        command_echo.unwrap_or_else(|| "/compact".to_string()),
                        Some("Context Compaction"),
                        format_compaction_result(&result, true),
                        !result.compacted && result.message.starts_with("error:"),
                    );
                } else if result.compacted {
                    ui.set_ephemeral_status(format!(
                        "context compacted: {} -> {} tokens",
                        result
                            .tokens_before
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "?".to_string()),
                        result
                            .tokens_after
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "?".to_string())
                    ));
                } else {
                    ui.clear_ephemeral_status();
                }
                if self.current_session.as_deref() == Some(session_id.as_str()) {
                    self.refresh_current_session_title()?;
                }
            }
            Ok(Err(err)) => {
                self.had_error = true;
                if let Some(command_echo) = command_echo {
                    ui.push_command_result(command_echo, None, format!("error: {err}"), true);
                } else {
                    ui.set_ephemeral_error(format!("compaction failed: {err}"));
                }
            }
            Err(err) if err.is_cancelled() => {}
            Err(err) => {
                self.had_error = true;
                ui.set_ephemeral_error(format!("compaction failed: {err}"));
            }
        }
        ui.refresh_sidebar(self);
        self.start_next_queued_input(ui)?;
        Ok(true)
    }

    pub(crate) async fn drain_diff_task(&mut self, ui: &mut FullscreenUi<'_>) -> Result<bool> {
        let Some(task) = self.diff_task.as_ref() else {
            return Ok(false);
        };
        if !task.task.is_finished() {
            return Ok(false);
        }
        let task = self.diff_task.take().expect("checked task");
        match task.task.await {
            Ok(Ok(diff)) => {
                ui.diff_overlay = Some(diff_overlay_from_workspace_diff(&diff));
            }
            Ok(Err(err)) => {
                ui.diff_overlay = Some(DiffOverlay::error(err));
            }
            Err(err) if err.is_cancelled() => {}
            Err(err) => {
                ui.diff_overlay = Some(DiffOverlay::error(err.to_string()));
            }
        }
        Ok(true)
    }

    pub(crate) fn maybe_start_auto_compaction(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        if self.in_btw_side()
            || self.current_session.is_none()
            || ui.running.is_some()
            || self.compaction_task.is_some()
        {
            return Ok(false);
        }
        let Some(snapshot) = ui
            .last_context_snapshot
            .as_ref()
            .or(self.last_context_snapshot.as_ref())
            .cloned()
        else {
            return Ok(false);
        };
        let session = self.current_session.clone().expect("checked session");
        let options = AutoCompactionCheckOptions {
            state: self.state_runtime.clone(),
            workdir: self.workdir.clone(),
            session,
            config_path: self.config_path.clone(),
            model: self.current_model.clone(),
            reasoning_effort: self.current_variant.clone(),
            inherited_env: Some(self.env_map.clone()),
        };
        if !auto_compaction_due_for_snapshot(&options, &snapshot)? {
            return Ok(false);
        }
        self.start_compaction_task(
            ui,
            None,
            None,
            false,
            CompactionReason::AutoThreshold,
            false,
        )?;
        Ok(true)
    }

    pub(crate) async fn drain_finished_auxiliary_agent_tasks(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<(bool, bool)> {
        let mut changed = false;
        let mut pending = Vec::new();
        for mut agent in std::mem::take(&mut ui.auxiliary_agent_tasks) {
            let mut events = VecDeque::new();
            while let Ok(event) = agent.rx.try_recv() {
                events.push_back(event);
            }
            let had_pending =
                self.apply_pending_auxiliary_agent_events(ui, agent.session_id.as_deref(), events);
            changed |= had_pending;
            if had_pending {
                ui.follow_transcript_if_needed();
                ui.refresh_sidebar(self);
            }

            if agent.task.is_finished() {
                let mut events = VecDeque::new();
                while let Ok(event) = agent.rx.try_recv() {
                    events.push_back(event);
                }
                let had_pending = self.apply_pending_auxiliary_agent_events(
                    ui,
                    agent.session_id.as_deref(),
                    events,
                );
                if had_pending {
                    ui.follow_transcript_if_needed();
                }
                if let Ok(Ok(result)) = agent.task.await {
                    self.last_context_snapshot = result.context_snapshot.clone();
                    ui.last_context_snapshot = result.context_snapshot;
                    ui.session_live_event_backlog.remove(&result.session_id);
                }
                self.refresh_current_session_title()?;
                ui.refresh_sidebar(self);
                changed = true;
            } else {
                pending.push(agent);
            }
        }
        ui.auxiliary_agent_tasks = pending;
        Ok((changed, false))
    }

    pub(crate) async fn drain_auxiliary_shell_tasks(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<(bool, bool)> {
        let mut changed = false;
        let mut pending_tasks = Vec::new();
        let mut tasks = std::mem::take(&mut ui.auxiliary_shell_tasks).into_iter();
        while let Some(mut shell) = tasks.next() {
            let mut pending = VecDeque::new();
            while let Ok(event) = shell.rx.try_recv() {
                pending.push_back(event);
            }
            let (had_pending, active_tool_frame_requested) =
                self.apply_pending_auxiliary_shell_events(ui, shell.session_id.as_deref(), pending);
            changed |= had_pending;
            if had_pending {
                ui.follow_transcript_if_needed();
                ui.refresh_sidebar(self);
            }
            if active_tool_frame_requested {
                pending_tasks.push(shell);
                pending_tasks.extend(tasks);
                ui.auxiliary_shell_tasks = pending_tasks;
                return Ok((true, true));
            }

            if shell.task.is_finished() {
                let mut pending = VecDeque::new();
                while let Ok(event) = shell.rx.try_recv() {
                    pending.push_back(event);
                }
                let (had_pending, active_tool_frame_requested) = self
                    .apply_pending_auxiliary_shell_events(ui, shell.session_id.as_deref(), pending);
                if had_pending {
                    ui.follow_transcript_if_needed();
                }
                if active_tool_frame_requested {
                    pending_tasks.push(shell);
                    pending_tasks.extend(tasks);
                    ui.auxiliary_shell_tasks = pending_tasks;
                    return Ok((true, true));
                }

                match shell.task.await {
                    Ok(Ok(result)) => {
                        let interrupted =
                            ui.interrupt_requested && result.outcome == Outcome::Aborted;
                        if let Some(session_id) = result.session_id {
                            ui.session_live_event_backlog.remove(&session_id);
                            if self.current_session.as_deref() == Some(session_id.as_str()) {
                                self.refresh_current_session_title()?;
                                self.force_new_once = false;
                            }
                        }
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
                }
                ui.refresh_sidebar(self);
                changed = true;
            } else {
                pending_tasks.push(shell);
            }
        }
        ui.auxiliary_shell_tasks = pending_tasks;
        Ok((changed, false))
    }

    pub(crate) fn render_fullscreen(&self, frame: &mut Frame<'_>, ui: &mut FullscreenUi<'_>) {
        let area = frame.area();
        ui.clear_screen_lines();
        ui.set_thinking_visible(self.thinking_visible);
        ui.set_raw_visible(self.raw_visible);
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
        let session_identity = self.session_identity_label();
        if ui.bottom_panel.is_some() {
            ui.last_slash_menu_areas.clear();
            ui.last_pending_input_action_areas.clear();
            ui.last_pending_input_edit_area = None;
            ui.last_file_popup_areas.clear();
            ui.last_agent_popup_areas.clear();
            ui.last_skill_popup_areas.clear();
            let panel_height = ui
                .bottom_panel
                .as_ref()
                .map(|panel| match panel {
                    BottomPanel::Clarify(panel) => {
                        panel.desired_height().min(bottom_panel_height(main.height))
                    }
                    BottomPanel::PermissionApproval(panel) => {
                        panel.desired_height().min(bottom_panel_height(main.height))
                    }
                    _ => bottom_panel_height(main.height),
                })
                .unwrap_or_else(|| bottom_panel_height(main.height));
            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(panel_height),
                    Constraint::Length(1),
                ])
                .split(main);
            ui.set_render_areas(vertical[0], None, vertical[2], Some(vertical[1]));
            render_transcript(frame, vertical[0], ui, None);
            let running_session_ids =
                ui.background_running_session_ids(self.current_session.as_deref());
            let activity_elapsed = ui.bottom_panel_activity_elapsed();
            if let Some(panel) = &mut ui.bottom_panel {
                if let BottomPanel::Sessions(selection) = panel {
                    selection.running_session_ids = running_session_ids;
                }
                render_bottom_panel(
                    frame,
                    vertical[1],
                    panel,
                    &mut ui.last_bottom_panel_areas,
                    activity_elapsed,
                );
            }
            render_status(frame, vertical[2], self, ui);
            if sidebar_visible {
                render_sidebar(frame, horizontal[1], ui);
            }
            render_active_selection(frame, ui);
            if let Some(overlay) = ui.diff_overlay.as_mut() {
                render_diff_overlay(frame, area, overlay, &mut ui.last_diff_overlay_area);
            } else {
                ui.last_diff_overlay_area = None;
            }
            return;
        }
        let composer_height = composer_height(&ui.textarea);
        let pending_preview_height = pending_input_preview_height(ui, main.width);
        let file_popup_height = ui.file_popup_height();
        let agent_popup_height = ui.agent_popup_height();
        let skill_popup_height = ui.skill_popup_height();
        let composer_text = textarea_text(&ui.textarea);
        let slash_items = if !ui.textarea.is_selecting()
            && file_popup_height == 0
            && agent_popup_height == 0
            && skill_popup_height == 0
        {
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
        if agent_popup_height == 0 {
            ui.last_agent_popup_areas.clear();
        }
        if skill_popup_height == 0 {
            ui.last_skill_popup_areas.clear();
        }
        let slash_height = if slash_items.is_empty() {
            0
        } else {
            (slash_items.len() as u16).min(FILE_POPUP_MAX_ROWS as u16)
        };
        let popup_height = agent_popup_height
            .max(file_popup_height)
            .max(skill_popup_height)
            .max(slash_height);
        let vertical = if popup_height == 0 {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(pending_preview_height),
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
                    Constraint::Length(pending_preview_height),
                    Constraint::Length(composer_height),
                    Constraint::Length(1),
                ])
                .split(main)
        };
        if popup_height == 0 {
            ui.set_render_areas(vertical[0], Some(vertical[2]), vertical[3], None);
            render_transcript(frame, vertical[0], ui, session_identity.as_deref());
            render_pending_input_preview(frame, vertical[1], ui);
            render_composer(frame, vertical[2], ui);
            render_status(frame, vertical[3], self, ui);
        } else if agent_popup_height > 0 {
            ui.set_render_areas(vertical[0], Some(vertical[3]), vertical[4], None);
            render_transcript(frame, vertical[0], ui, session_identity.as_deref());
            render_agent_popup(frame, vertical[1], ui);
            render_pending_input_preview(frame, vertical[2], ui);
            render_composer(frame, vertical[3], ui);
            render_status(frame, vertical[4], self, ui);
        } else if file_popup_height > 0 {
            ui.set_render_areas(vertical[0], Some(vertical[3]), vertical[4], None);
            render_transcript(frame, vertical[0], ui, session_identity.as_deref());
            render_file_popup(frame, vertical[1], ui);
            render_pending_input_preview(frame, vertical[2], ui);
            render_composer(frame, vertical[3], ui);
            render_status(frame, vertical[4], self, ui);
        } else if skill_popup_height > 0 {
            ui.set_render_areas(vertical[0], Some(vertical[3]), vertical[4], None);
            render_transcript(frame, vertical[0], ui, session_identity.as_deref());
            render_skill_popup(frame, vertical[1], ui);
            render_pending_input_preview(frame, vertical[2], ui);
            render_composer(frame, vertical[3], ui);
            render_status(frame, vertical[4], self, ui);
        } else {
            ui.set_render_areas(vertical[0], Some(vertical[3]), vertical[4], None);
            render_transcript(frame, vertical[0], ui, session_identity.as_deref());
            render_slash_menu(
                frame,
                vertical[1],
                &slash_items,
                ui.slash_menu_selected,
                &mut ui.last_slash_menu_areas,
            );
            render_pending_input_preview(frame, vertical[2], ui);
            render_composer(frame, vertical[3], ui);
            render_status(frame, vertical[4], self, ui);
        }
        if sidebar_visible {
            render_sidebar(frame, horizontal[1], ui);
        }
        render_active_selection(frame, ui);
        if let Some(overlay) = ui.diff_overlay.as_mut() {
            render_diff_overlay(frame, area, overlay, &mut ui.last_diff_overlay_area);
        } else {
            ui.last_diff_overlay_area = None;
        }
    }
}

#[path = "events/helpers.rs"]
pub(crate) mod helpers;
#[allow(unused_imports)]
pub use helpers::*;
