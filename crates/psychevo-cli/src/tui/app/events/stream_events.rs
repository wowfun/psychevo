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
        changed |= self.drain_foreign_gateway_live_events(ui)?;

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
            while let Ok(event) = running.events.try_recv() {
                pending.push_back(event);
            }
            let had_pending = self.apply_pending_owned_fullscreen_live_events(
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
                            self.clear_new_session_draft();
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
                                self.clear_new_session_draft();
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

    pub(crate) fn replay_foreign_gateway_live_events_for_session(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
    ) -> Result<bool> {
        let mut changed = false;
        let mut after_seq = 0;
        loop {
            let records = self
                .state_runtime
                .store()
                .list_gateway_live_events_after(after_seq, 500)?;
            if records.is_empty() {
                break;
            }
            for record in &records {
                after_seq = after_seq.max(record.seq);
            }
            for record in records {
                changed |= self.apply_foreign_gateway_live_event_record(
                    ui,
                    record,
                    Some(session_id),
                )?;
            }
        }
        Ok(changed)
    }

    pub(crate) fn drain_foreign_gateway_live_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let records = self
            .state_runtime
            .store()
            .list_gateway_live_events_after(self.last_gateway_live_event_seq, 100)?;
        let mut changed = false;
        for record in records {
            self.last_gateway_live_event_seq = self.last_gateway_live_event_seq.max(record.seq);
            changed |= self.apply_foreign_gateway_live_event_record(ui, record, None)?;
        }
        Ok(changed)
    }

    fn apply_foreign_gateway_live_event_record(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        record: GatewayLiveEventRecord,
        expected_session: Option<&str>,
    ) -> Result<bool> {
        if record.owner_id.as_deref() == Some(self.gateway.owner_id()) {
            return Ok(false);
        }
        let event = match serde_json::from_value::<GatewayEvent>(record.event.clone()) {
            Ok(event) => event,
            Err(_) => return Ok(false),
        };
        let Some(session_id) = self.gateway_live_event_session_id(&record, &event)? else {
            return Ok(false);
        };
        if expected_session.is_some_and(|expected| expected != session_id) {
            return Ok(false);
        }
        if expected_session.is_none() && self.current_session.as_deref() != Some(session_id.as_str())
        {
            return Ok(false);
        }
        if !ui.mark_gateway_live_event_applied(record.seq) {
            return Ok(false);
        }
        Ok(self.apply_foreign_gateway_live_event(ui, &session_id, event)?)
    }

    fn gateway_live_event_session_id(
        &self,
        record: &GatewayLiveEventRecord,
        event: &GatewayEvent,
    ) -> Result<Option<String>> {
        if let Some(thread_id) = record.thread_id.as_ref().filter(|value| !value.is_empty()) {
            return Ok(Some(thread_id.clone()));
        }
        if let Some(thread_id) = gateway_event_session_id(event) {
            return Ok(Some(thread_id.to_string()));
        }
        let Some(activity_id) = record.activity_id.as_deref() else {
            return Ok(None);
        };
        Ok(self
            .state_runtime
            .store()
            .gateway_activity(activity_id)?
            .and_then(|activity| activity.thread_id))
    }

    fn apply_foreign_gateway_live_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
        event: GatewayEvent,
    ) -> Result<bool> {
        match event {
            GatewayEvent::ActivityChanged { activity, .. } => {
                if activity.running && activity.owner_id.as_deref() != Some(self.gateway.owner_id())
                {
                    ui.observe_foreign_gateway_activity_values(
                        session_id,
                        activity.active_turn_id,
                        activity.started_at_ms,
                    );
                } else {
                    ui.clear_foreign_gateway_activity(session_id);
                }
                Ok(true)
            }
            GatewayEvent::TitleChanged {
                title,
                display_title,
                ..
            } => {
                if self.current_session.as_deref() == Some(session_id) {
                    self.current_session_title = title.or(display_title);
                }
                Ok(true)
            }
            GatewayEvent::TurnCompleted { .. } => {
                self.apply_gateway_event(ui, Some(session_id), event);
                ui.clear_foreign_gateway_activity(session_id);
                if !ui.local_status_has_running(Some(session_id)) {
                    ui.visible_turn_started = None;
                    ui.turn_session_id = None;
                }
                if self.current_session.as_deref() == Some(session_id) {
                    self.refresh_current_session_title()?;
                }
                Ok(true)
            }
            _ => Ok(self.apply_gateway_event(ui, Some(session_id), event)),
        }
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
            while let Ok(event) = running.events.try_recv() {
                pending.push_back(event);
            }
        }
        if owner_session.is_none() {
            self.apply_pending_fullscreen_live_events(ui, pending)
        } else {
            self.apply_pending_owned_fullscreen_live_events_with_frames(
                ui,
                owner_session.as_deref(),
                pending,
            )
        }
    }

    pub(crate) fn apply_pending_fullscreen_live_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        mut pending: VecDeque<TuiLiveEvent>,
    ) -> (bool, bool) {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            let active_tool_frame_requested = self.apply_fullscreen_live_event(ui, None, event);
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

    pub(crate) fn apply_pending_auxiliary_agent_live_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        agent: &mut AuxiliaryAgentTask,
        mut pending: VecDeque<TuiLiveEvent>,
    ) -> bool {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            match event {
                TuiLiveEvent::Runtime(event) => {
                    let event_session = stream_event_session_id(&event).map(str::to_string);
                    if agent.session_id.is_none() {
                        if let Some(session_id) = event_session.clone() {
                            agent.session_id = Some(session_id);
                            flush_pending_unowned_agent_events(ui, agent);
                        } else {
                            push_pending_unowned_agent_event(agent, event);
                            continue;
                        }
                    }
                    let owner_session = event_session.as_deref().or(agent.session_id.as_deref());
                    if !agent.visible_live
                        || owner_session
                            .is_none_or(|session| self.current_session.as_deref() != Some(session))
                    {
                        if let Some(session_id) = event_session.as_deref().or(owner_session) {
                            buffer_session_live_event(ui, session_id, event);
                        }
                        continue;
                    }
                    self.apply_auxiliary_agent_stream_event(ui, owner_session, event);
                }
                TuiLiveEvent::Gateway(event) => {
                    if agent.session_id.is_none()
                        && let Some(session_id) = gateway_event_session_id(&event)
                    {
                        agent.session_id = Some(session_id.to_string());
                        flush_pending_unowned_agent_events(ui, agent);
                    }
                    self.apply_gateway_event(ui, agent.session_id.as_deref(), *event);
                }
            }
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
                ui.deferred_stream_events
                    .extend(pending.into_iter().map(TuiLiveEvent::Runtime));
                return (true, true);
            }
        }
        (had_pending, false)
    }

    pub(crate) fn apply_pending_owned_fullscreen_live_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        mut pending: VecDeque<TuiLiveEvent>,
    ) -> bool {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            self.apply_owned_fullscreen_live_event(ui, owner_session, event);
        }
        had_pending
    }

    pub(crate) fn apply_pending_owned_fullscreen_stream_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        pending: VecDeque<RunStreamEvent>,
    ) -> bool {
        self.apply_pending_owned_fullscreen_live_events(
            ui,
            owner_session,
            pending.into_iter().map(TuiLiveEvent::Runtime).collect(),
        )
    }

    pub(crate) fn apply_pending_owned_fullscreen_live_events_with_frames(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        mut pending: VecDeque<TuiLiveEvent>,
    ) -> (bool, bool) {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            let active_tool_frame_requested =
                self.apply_owned_fullscreen_live_event(ui, owner_session, event);
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
                if owner_session.is_none() && stream_event_session_id(&other).is_none() {
                    return;
                }
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
        if owner_session.is_none() && stream_event_session_id(&event).is_none() {
            return false;
        }
        self.apply_owned_fullscreen_stream_event(ui, owner_session, event)
    }

    pub(crate) fn apply_fullscreen_live_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        event: TuiLiveEvent,
    ) -> bool {
        match event {
            TuiLiveEvent::Runtime(event) => self.apply_fullscreen_stream_event(ui, event),
            TuiLiveEvent::Gateway(event) => self.apply_gateway_event(ui, owner_session, *event),
        }
    }

    pub(crate) fn apply_owned_fullscreen_live_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        event: TuiLiveEvent,
    ) -> bool {
        match event {
            TuiLiveEvent::Runtime(event) => {
                self.apply_owned_fullscreen_stream_event(ui, owner_session, event)
            }
            TuiLiveEvent::Gateway(event) => self.apply_gateway_event(ui, owner_session, *event),
        }
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

}
