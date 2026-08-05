use super::gateway_helpers::gateway_event_session_id;
use super::helpers::{
    buffer_session_live_event, flush_pending_unowned_agent_events,
    push_pending_unowned_agent_event, turn_ended_error_message, turn_event_is_clarify_request,
    turn_event_session_id,
};
use crate::tui::support_turn_event::{turn_event_is_run_start, turn_event_presentation_value};
use crate::tui::{
    AuxiliaryAgentTask, ContextSnapshot, ForeignGatewayLiveEvent, FullscreenUi, GatewayEvent,
    GatewayLiveSnapshotObservation, PresentedShellEvent, RunningCompletion, RunningTask,
    RunningTurn, RunningTurnEvents, ShellCommandOutcome, TuiApp, TuiLiveEvent, TurnEvent,
    TurnOutcome, Value, VecDeque, rebind_queued_input_session, short_session,
};
use anyhow::Result;

impl TuiApp {
    pub(crate) async fn drain_fullscreen_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let mut changed = self.drain_starting_turn(ui).await?;
        changed |= self.drain_finished_starting_turn_cleanups(ui).await;
        changed |= self.drain_side_delete_tasks(ui).await;
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
        changed |= self.maybe_reload_live_agent_session(ui).await?;
        changed |= self.drain_foreign_gateway_live_events(ui).await?;

        let (had_pending, active_tool_frame_requested) =
            self.drain_available_fullscreen_stream_events(ui);
        changed |= had_pending;
        if had_pending {
            ui.follow_transcript_if_needed();
            ui.refresh_sidebar(self);
        }
        changed |= self.reload_invalidated_turn_projection(ui).await?;
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
            changed |= self.reload_invalidated_turn_projection(ui).await?;
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
            let agent_completion = matches!(&running.task, RunningTask::Agent(_));
            if agent_completion {
                self.journey_profile.observe_turn_completion_received();
            }
            let owner_session = running.session_id.clone();
            let task = running.task;
            let completed = match task {
                RunningTask::Agent(task) => RunningCompletion::Agent(Box::new(task.await)),
                RunningTask::UserShell(task) => RunningCompletion::UserShell(task.await),
            };
            let mut pending = VecDeque::new();
            while let Some(event) = running.events.try_recv() {
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
            changed |= self.reload_invalidated_turn_projection(ui).await?;
            let mut restore_queued_after_interrupt = false;
            match completed {
                RunningCompletion::Agent(result) => match *result {
                    Ok(Ok(result)) => {
                        let interrupted =
                            ui.interrupt_requested && result.outcome == TurnOutcome::Interrupted;
                        if interrupted {
                            ui.turn_interrupted = true;
                        }
                        restore_queued_after_interrupt |= interrupted;
                        self.last_context_snapshot = result.context_snapshot.clone();
                        ui.last_context_snapshot = result.context_snapshot.clone();
                        ui.session_live_event_backlog.remove(&result.thread_id);
                        if self.current_session.as_deref() == Some(result.thread_id.as_str()) {
                            self.refresh_current_session_title().await?;
                            self.clear_new_session_draft();
                        }
                        if result.outcome != TurnOutcome::Completed && !interrupted {
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
                        let interrupted = ui.interrupt_requested
                            && result.outcome == ShellCommandOutcome::Interrupted;
                        if interrupted {
                            ui.turn_interrupted = true;
                        }
                        restore_queued_after_interrupt |= interrupted;
                        if let Some(session_id) = result.thread_id {
                            ui.session_live_event_backlog.remove(&session_id);
                            if self.current_session.as_deref() == Some(session_id.as_str()) {
                                self.refresh_current_session_title().await?;
                                self.clear_new_session_draft();
                            }
                        }
                        if (result.outcome != ShellCommandOutcome::Completed
                            || result.tool_failures > 0)
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
            if agent_completion {
                self.journey_profile.observe_turn_completion_applied();
            }
            ui.update_turn_meta(self.debug, true, true, true);
            ui.finish_turn();
            ui.refresh_sidebar(self);
            if restore_queued_after_interrupt {
                ui.restore_queued_inputs_to_composer();
            } else if !self.maybe_start_auto_compaction(ui).await? {
                self.start_next_queued_input(ui).await?;
            }
        } else if ui.turn_outcome.is_some() && ui.deferred_stream_events.is_empty() {
            self.finish_streamed_agent_turn(ui).await;
            changed = true;
        }
        Ok(changed)
    }

    async fn drain_finished_starting_turn_cleanups(&mut self, ui: &mut FullscreenUi<'_>) -> bool {
        let mut changed = false;
        while let Some(index) = ui
            .starting_turn_cleanups
            .iter()
            .position(|cleanup| cleanup.is_finished())
        {
            ui.starting_turn_cleanups.remove(index).join().await;
            changed = true;
        }
        changed
    }

    async fn drain_starting_turn(&mut self, ui: &mut FullscreenUi<'_>) -> Result<bool> {
        if !ui
            .starting_turn
            .as_ref()
            .is_some_and(|starting| starting.task.is_finished())
        {
            return Ok(false);
        }
        let starting = ui.starting_turn.take().expect("checked starting Turn");
        let queue_owner_id = starting.queue_owner_id.clone();
        match starting.task.await {
            Ok(Ok(started)) => {
                let session_id = started.handle.receipt().thread_id.clone();
                let turn_id = started.handle.receipt().turn_id.clone();
                for input in &mut ui.queued_inputs {
                    rebind_queued_input_session(input, &queue_owner_id, &session_id);
                }
                self.current_session = Some(session_id.clone());
                self.reset_live_agent_reload_poll();
                self.clear_new_session_draft();
                ui.bind_unbound_optimistic_rows_to_turn(&turn_id);
                ui.approval_rx = Some(started.approval_rx);
                let events = started.handle.events();
                let control = started.handle.clone();
                let task = tokio::spawn(async move { started.handle.wait().await });
                ui.running = Some(RunningTurn {
                    session_id: Some(session_id),
                    control: control.into(),
                    selector: None,
                    turn_id: Some(turn_id),
                    events: RunningTurnEvents::Turn(events),
                    task: RunningTask::Agent(task),
                });
            }
            Ok(Err(err)) => {
                self.had_error = true;
                ui.discard_unbound_optimistic_rows();
                ui.finish_turn();
                ui.restore_failed_turn_start_to_composer(
                    &queue_owner_id,
                    starting.display_prompt,
                    starting.images,
                );
                ui.push_error(format!("error: {err:#}"));
            }
            Err(err) => {
                self.had_error = true;
                ui.discard_unbound_optimistic_rows();
                ui.finish_turn();
                ui.restore_failed_turn_start_to_composer(
                    &queue_owner_id,
                    starting.display_prompt,
                    starting.images,
                );
                ui.push_error(format!("turn admission task failed: {err}"));
            }
        }
        ui.follow_transcript_if_needed();
        ui.refresh_sidebar(self);
        Ok(true)
    }

    pub(crate) async fn replay_foreign_gateway_live_events_for_session(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
    ) -> Result<bool> {
        let mut changed = false;
        let mut after_seq = 0;
        loop {
            let page = self
                .runtime
                .gateway()
                .poll_foreign_live_events(after_seq, Some(session_id), 500)
                .await?;
            after_seq = page.next_seq;
            for observation in page.events {
                changed |= self
                    .apply_foreign_gateway_live_event_observation(ui, observation, Some(session_id))
                    .await?;
            }
            if page.scanned_records == 0 {
                break;
            }
        }
        changed |= self
            .replay_foreign_gateway_live_snapshots_for_session(ui, session_id)
            .await?;
        Ok(changed)
    }

    pub(crate) async fn drain_foreign_gateway_live_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let page = self
            .runtime
            .gateway()
            .poll_foreign_live_events(self.last_gateway_live_event_seq, None, 100)
            .await?;
        self.last_gateway_live_event_seq = page.next_seq;
        let mut changed = false;
        for observation in page.events {
            changed |= self
                .apply_foreign_gateway_live_event_observation(ui, observation, None)
                .await?;
        }
        changed |= self.drain_foreign_gateway_live_snapshots(ui).await?;
        Ok(changed)
    }

    async fn replay_foreign_gateway_live_snapshots_for_session(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
    ) -> Result<bool> {
        let snapshots = self
            .runtime
            .gateway()
            .foreign_live_snapshots(Some(session_id), 1000)
            .await?;
        let mut changed = false;
        for snapshot in snapshots {
            changed |= self
                .apply_foreign_gateway_live_snapshot(ui, snapshot, Some(session_id))
                .await?;
        }
        Ok(changed)
    }

    async fn drain_foreign_gateway_live_snapshots(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let snapshots = self
            .runtime
            .gateway()
            .foreign_live_snapshots(None, 1000)
            .await?;
        let mut changed = false;
        for snapshot in snapshots {
            changed |= self
                .apply_foreign_gateway_live_snapshot(ui, snapshot, None)
                .await?;
        }
        Ok(changed)
    }

    async fn apply_foreign_gateway_live_snapshot(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        snapshot: GatewayLiveSnapshotObservation,
        expected_session: Option<&str>,
    ) -> Result<bool> {
        if self
            .gateway_live_snapshot_revisions
            .get(&snapshot.snapshot_key)
            .is_some_and(|revision| *revision >= snapshot.revision)
        {
            return Ok(false);
        }
        let Some(session_id) = snapshot.context.thread_id else {
            return Ok(false);
        };
        if expected_session.is_some_and(|expected| expected != session_id) {
            return Ok(false);
        }
        if expected_session.is_none()
            && self.current_session.as_deref() != Some(session_id.as_str())
        {
            return Ok(false);
        }
        self.gateway_live_snapshot_revisions
            .insert(snapshot.snapshot_key, snapshot.revision);
        self.apply_foreign_gateway_live_event(ui, &session_id, snapshot.event)
            .await
    }

    async fn apply_foreign_gateway_live_event_observation(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        observation: ForeignGatewayLiveEvent,
        expected_session: Option<&str>,
    ) -> Result<bool> {
        let Some(session_id) = observation.context.thread_id else {
            return Ok(false);
        };
        if expected_session.is_some_and(|expected| expected != session_id) {
            return Ok(false);
        }
        if expected_session.is_none()
            && self.current_session.as_deref() != Some(session_id.as_str())
        {
            return Ok(false);
        }
        if !ui.mark_gateway_live_event_applied(observation.seq) {
            return Ok(false);
        }
        self.apply_foreign_gateway_live_event(ui, &session_id, observation.event)
            .await
    }

    async fn apply_foreign_gateway_live_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
        event: GatewayEvent,
    ) -> Result<bool> {
        match event {
            GatewayEvent::ActivityChanged { activity, .. } => {
                if activity.running
                    && activity.owner_id.as_deref() != Some(self.runtime.gateway().owner_id())
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
                    self.refresh_current_session_title().await?;
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
            while let Some(event) = running.events.try_recv() {
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

    pub(crate) fn apply_pending_fullscreen_turn_events_without_frames(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        mut pending: VecDeque<TurnEvent>,
    ) -> bool {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            self.apply_fullscreen_turn_event(ui, event);
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
                TuiLiveEvent::Turn(event) => {
                    let run_started = turn_event_is_run_start(&event);
                    let event_session = turn_event_session_id(&event).map(str::to_string);
                    if agent.session_id.is_none() {
                        if let Some(session_id) = event_session.clone() {
                            agent.session_id = Some(session_id);
                            flush_pending_unowned_agent_events(ui, agent);
                        } else {
                            push_pending_unowned_agent_event(agent, event);
                            continue;
                        }
                    }
                    if run_started
                        && let Err(err) = self.start_pending_auxiliary_shells_for_agent(ui, agent)
                    {
                        self.had_error = true;
                        ui.push_error(format!("error: {err:#}"));
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
                    self.apply_auxiliary_agent_turn_event(ui, owner_session, event);
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
                TuiLiveEvent::Shell(_) => {
                    unreachable!("an auxiliary Agent task does not own typed Shell events")
                }
            }
        }
        had_pending
    }

    pub(crate) fn apply_pending_auxiliary_shell_events(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        mut pending: VecDeque<PresentedShellEvent>,
    ) -> (bool, bool) {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            let active_tool_frame_requested =
                self.apply_owned_fullscreen_shell_event(ui, owner_session, event);
            if active_tool_frame_requested {
                ui.deferred_stream_events
                    .extend(pending.into_iter().map(TuiLiveEvent::Shell));
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

    pub(crate) fn apply_auxiliary_agent_turn_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        event: TurnEvent,
    ) {
        match event {
            TurnEvent::Scoped {
                thread_id, event, ..
            } => {
                self.apply_scoped_fullscreen_turn_event(ui, &thread_id, *event);
            }
            other => {
                if owner_session.is_none() && turn_event_session_id(&other).is_none() {
                    return;
                }
                self.apply_owned_fullscreen_turn_event(ui, owner_session, other);
            }
        }
    }

    pub(crate) fn apply_fullscreen_live_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        event: TuiLiveEvent,
    ) -> bool {
        match event {
            TuiLiveEvent::Turn(event) => self.apply_fullscreen_turn_event(ui, event),
            TuiLiveEvent::Shell(event) => self.apply_fullscreen_shell_event(ui, event),
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
            TuiLiveEvent::Turn(event) => {
                self.apply_owned_fullscreen_turn_event(ui, owner_session, event)
            }
            TuiLiveEvent::Shell(event) => {
                self.apply_owned_fullscreen_shell_event(ui, owner_session, event)
            }
            TuiLiveEvent::Gateway(event) => self.apply_gateway_event(ui, owner_session, *event),
        }
    }

    pub(crate) fn apply_fullscreen_shell_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        event: PresentedShellEvent,
    ) -> bool {
        let event_session = event.thread_id().map(str::to_string);
        if let Some(session_id) = event_session.as_deref() {
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
        let previous = ui.active_event_session_id.clone();
        if let Some(session_id) = event_session {
            ui.active_event_session_id = Some(session_id);
        }
        let active_tool_frame_requested = ui.apply_shell_event(event);
        ui.active_event_session_id = previous;
        active_tool_frame_requested
    }

    pub(crate) fn apply_owned_fullscreen_shell_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        event: PresentedShellEvent,
    ) -> bool {
        let event_has_session = event.thread_id().is_some();
        let event_session = event
            .thread_id()
            .map(str::to_string)
            .or_else(|| owner_session.map(str::to_string));
        if let Some(session_id) = event_session.as_deref()
            && self.current_session.as_deref() != Some(session_id)
        {
            buffer_session_live_event(ui, session_id, event);
            return false;
        }
        if !event_has_session && let Some(session_id) = event_session.as_deref() {
            buffer_session_live_event(ui, session_id, event.clone());
        }
        let previous = ui.active_event_session_id.clone();
        if let Some(session_id) = event_session {
            ui.active_event_session_id = Some(session_id);
        }
        let active_tool_frame_requested = ui.apply_shell_event(event);
        ui.active_event_session_id = previous;
        active_tool_frame_requested
    }

    pub(crate) fn apply_owned_fullscreen_turn_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        event: TurnEvent,
    ) -> bool {
        if let TurnEvent::Scoped {
            thread_id, event, ..
        } = event
        {
            return self.apply_scoped_fullscreen_turn_event(ui, &thread_id, *event);
        }
        let event_has_session = turn_event_session_id(&event).is_some();
        let event_session = turn_event_session_id(&event)
            .map(str::to_string)
            .or_else(|| owner_session.map(str::to_string));
        if let Some(session_id) = event_session.as_deref()
            && self.current_session.as_deref() != Some(session_id)
        {
            if turn_event_is_clarify_request(&event) {
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
        let active_tool_frame_requested = self.apply_fullscreen_turn_event(ui, event);
        ui.active_event_session_id = previous;
        active_tool_frame_requested
    }

    pub(crate) fn apply_fullscreen_turn_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        event: TurnEvent,
    ) -> bool {
        if let TurnEvent::Scoped {
            thread_id, event, ..
        } = event
        {
            return self.apply_scoped_fullscreen_turn_event(ui, &thread_id, *event);
        }
        let event_session_id = turn_event_session_id(&event).map(str::to_string);
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
        let profile_event = self.journey_profile.observe_turn_event_received(&event);
        let run_started = turn_event_presentation_value(&event).is_some_and(|value| {
            let value = value.as_ref();
            if value.get("type").and_then(Value::as_str) == Some("context_snapshot")
                && let Ok(snapshot) = serde_json::from_value::<ContextSnapshot>(value.clone())
            {
                self.last_context_snapshot = Some(snapshot.clone());
                ui.last_context_snapshot = Some(snapshot);
            }
            self.observe_fullscreen_value_event(ui, value)
        });
        let active_tool_frame_requested = ui.apply_turn_event_for_session(
            event,
            self.thinking_visible,
            self.debug,
            event_session,
        );
        self.journey_profile
            .observe_turn_event_applied(profile_event);
        if run_started && let Err(err) = self.start_pending_auxiliary_shells(ui) {
            self.had_error = true;
            ui.push_error(format!("error: {err:#}"));
        }
        active_tool_frame_requested
    }
}
