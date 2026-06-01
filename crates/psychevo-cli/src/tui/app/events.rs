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
        owner_session: Option<&str>,
        mut pending: VecDeque<TuiLiveEvent>,
    ) -> bool {
        let mut had_pending = false;
        while let Some(event) = pending.pop_front() {
            had_pending = true;
            match event {
                TuiLiveEvent::Runtime(event) => {
                    self.apply_auxiliary_agent_stream_event(ui, owner_session, event);
                }
                TuiLiveEvent::Gateway(event) => {
                    self.apply_gateway_event(ui, owner_session, *event);
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

    pub(crate) fn apply_gateway_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        event: GatewayEvent,
    ) -> bool {
        let event_session = gateway_event_session_id(&event).or(owner_session);
        if let Some(session_id) = event_session
            && self
                .current_session
                .as_deref()
                .is_some_and(|current| current != session_id)
        {
            if matches!(event, GatewayEvent::ClarifyRequested { .. }) {
                ui.push_status(format!(
                    "clarify pending in session {}",
                    short_session(session_id)
                ));
            }
            return false;
        }
        match event {
            GatewayEvent::TurnStarted {
                thread_id,
                turn_id,
                selected_skills,
            } => {
                if let Some(running) = ui.running.as_mut() {
                    running.turn_id = Some(turn_id);
                    if running.session_id.is_none() {
                        running.session_id = thread_id.clone();
                    }
                }
                if let Some(session_id) = thread_id {
                    self.observe_gateway_thread_started(ui, &session_id);
                }
                if !selected_skills.is_empty() {
                    let names = selected_skills
                        .into_iter()
                        .map(|skill| skill.name)
                        .filter(|name| !name.trim().is_empty())
                        .collect::<Vec<_>>();
                    if !names.is_empty() {
                        ui.push_status(format!("skill loaded: {}", names.join(", ")));
                    }
                }
                false
            }
            GatewayEvent::TurnQueued { queue_position, .. } => {
                ui.push_status(format!("turn queued: #{queue_position}"));
                false
            }
            GatewayEvent::TurnCompleted { outcome, .. } => {
                if let Some(outcome) = outcome.as_deref().and_then(outcome_from_str) {
                    ui.turn_outcome = Some(outcome);
                    if ui.interrupt_requested && outcome == Outcome::Aborted {
                        ui.turn_interrupted = true;
                    }
                }
                ui.update_turn_meta(self.debug, true, true, true);
                false
            }
            GatewayEvent::ItemDelta { delta, .. } => {
                if delta.trim().is_empty() {
                    return false;
                }
                ui.turn_had_reasoning = true;
                ui.remove_turn_meta();
                let idx = ui.reasoning_row.unwrap_or_else(|| {
                    let mut row = TranscriptRow::with_title(
                        TranscriptKind::Thinking,
                        "Thinking",
                        String::new(),
                    );
                    row.tool_started = Some(Instant::now());
                    let idx = ui.insert_evidence_row(row);
                    ui.reasoning_row = Some(idx);
                    idx
                });
                ui.append_thinking_text(idx, &delta);
                let reasoning = ui.thinking_full_text(idx);
                self.thinking_visible && ui.apply_visible_tool_intent(&reasoning)
            }
            GatewayEvent::ItemStarted { item, .. }
            | GatewayEvent::ItemUpdated { item, .. }
            | GatewayEvent::ItemCompleted { item, .. } => {
                self.apply_gateway_timeline_item(ui, owner_session, item)
            }
            GatewayEvent::PermissionRequested {
                request_id,
                tool_name,
                summary,
                reason,
                matched_rule,
                suggested_rule,
                allow_always,
                timeout_secs,
            } => {
                let request = PermissionApprovalRequest {
                    tool_call_id: request_id.clone(),
                    tool_name,
                    summary,
                    reason,
                    matched_rule,
                    suggested_rule,
                    allow_always,
                    timeout_secs,
                };
                let selector = ui
                    .running
                    .as_ref()
                    .and_then(|running| running.selector.clone())
                    .unwrap_or_else(|| {
                        GatewayThreadSelector::source(self.gateway_source().source_key())
                    });
                let (response, rx) = oneshot::channel();
                ui.pending_permission_approvals
                    .push_back(TuiApprovalRequest {
                        session_id: owner_session
                            .map(str::to_string)
                            .or_else(|| self.current_session.clone()),
                        request,
                        response,
                    });
                let gateway = self.gateway.clone();
                tokio::spawn(async move {
                    let decision = rx
                        .await
                        .unwrap_or_else(|_| PermissionApprovalDecision::deny());
                    gateway.submit_permission(selector, &request_id, decision);
                });
                ui.open_next_permission_approval()
            }
            GatewayEvent::PermissionResolved { .. } => false,
            GatewayEvent::ClarifyRequested { raw, .. } => {
                match serde_json::from_value::<ClarifyRequestEvent>(raw) {
                    Ok(request) => {
                        ui.open_clarify_panel(request);
                        true
                    }
                    Err(err) => {
                        ui.push_status(format!("clarify request could not be decoded: {err}"));
                        false
                    }
                }
            }
            GatewayEvent::ClarifyResolved { request_id, reason } => {
                let reason = match reason.as_str() {
                    "Answered" | "answered" => ClarifyResolvedReason::Answered,
                    "Cancelled" | "cancelled" | "canceled" => ClarifyResolvedReason::Cancelled,
                    "TimedOut" | "timed_out" => ClarifyResolvedReason::TimedOut,
                    "TurnFinished" | "turn_finished" => ClarifyResolvedReason::TurnFinished,
                    _ => ClarifyResolvedReason::TurnFinished,
                };
                ui.apply_clarify_resolved(ClarifyResolvedEvent {
                    call_id: request_id,
                    reason,
                });
                false
            }
            GatewayEvent::Warning {
                message,
                suggestion,
                ..
            } => {
                ui.push_status(format!("warning: {message}"));
                if let Some(suggestion) = suggestion {
                    ui.push_status(format!("suggestion: {suggestion}"));
                }
                false
            }
            GatewayEvent::DebugAvailable { .. } => false,
        }
    }

    pub(crate) fn observe_gateway_thread_started(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
    ) {
        if self.current_session.as_deref() != Some(session_id) {
            self.current_session = Some(session_id.to_string());
            self.reset_live_agent_reload_poll();
            self.current_session_title = None;
        }
        ui.turn_started.get_or_insert_with(Instant::now);
        ui.turn_session_id = Some(session_id.to_string());
        if ui.visible_turn_started.is_none() {
            ui.visible_turn_started = ui.turn_started;
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
    }

    pub(crate) fn apply_gateway_timeline_item(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        item: TimelineItem,
    ) -> bool {
        if let Some(value) = gateway_item_runtime_value(&item)
            && value.get("type").and_then(Value::as_str) == Some("agent_session_start")
        {
            ui.apply_agent_session_start(&value);
            return false;
        }
        if let Some(value) = gateway_item_tool_value(&item) {
            return self.apply_gateway_tool_item(ui, owner_session, &item, value);
        }
        let event_session = (!item.thread_id.is_empty())
            .then_some(item.thread_id.as_str())
            .or(owner_session);
        if let Some(session_id) = event_session
            && self
                .current_session
                .as_deref()
                .is_some_and(|current| current != session_id)
        {
            return false;
        }
        match item.kind {
            TimelineItemKind::Prompt => {
                let text = timeline_item_text(&item);
                if !text.trim().is_empty() {
                    ui.push_user_with_attachment_meta(text, None);
                }
                false
            }
            TimelineItemKind::Assistant => {
                let live_item = item.source == "runtime.stream";
                let text = timeline_item_text(&item);
                if text.trim().is_empty() {
                    return false;
                }
                if let Some(idx) = ui.reasoning_row.take() {
                    ui.finish_thinking_row(idx);
                }
                let idx = ui.assistant_row.unwrap_or_else(|| {
                    let idx = ui.insert_answer_row(TranscriptRow::with_title(
                        TranscriptKind::Answer,
                        "",
                        String::new(),
                    ));
                    ui.assistant_row = Some(idx);
                    idx
                });
                ui.transcript[idx].text = text;
                ui.remove_turn_meta();
                apply_gateway_assistant_turn_metadata(ui, &item);
                if matches!(
                    item.status,
                    TimelineItemStatus::Completed
                        | TimelineItemStatus::Failed
                        | TimelineItemStatus::Cancelled
                ) {
                    if gateway_assistant_item_receives_meta(&item) {
                        ui.turn_terminal_visible_answer = true;
                        if live_item {
                            ui.update_turn_meta(self.debug, true, false, false);
                        } else {
                            push_gateway_completed_turn_meta(ui, self.debug);
                        }
                    }
                    ui.assistant_row = None;
                    return false;
                }
                item.status == TimelineItemStatus::Running
            }
            TimelineItemKind::Reasoning => {
                let text = timeline_item_text(&item);
                let existing_idx = ui.reasoning_row;
                if text.trim().is_empty() && existing_idx.is_none() {
                    return false;
                }
                let idx = existing_idx.unwrap_or_else(|| {
                    let mut row = TranscriptRow::with_title(
                        TranscriptKind::Thinking,
                        "Thinking",
                        String::new(),
                    );
                    if !matches!(
                        item.status,
                        TimelineItemStatus::Completed
                            | TimelineItemStatus::Failed
                            | TimelineItemStatus::Cancelled
                    ) {
                        row.tool_started = Some(Instant::now());
                    }
                    let idx = ui.insert_evidence_row(row);
                    ui.reasoning_row = Some(idx);
                    idx
                });
                if !text.trim().is_empty() {
                    ui.transcript[idx].text = text;
                    ui.turn_had_reasoning = true;
                    ui.remove_turn_meta();
                }
                if item.status == TimelineItemStatus::Completed {
                    if let Some(idx) = ui.reasoning_row.take() {
                        ui.finish_thinking_row(idx);
                    }
                    return false;
                }
                true
            }
            TimelineItemKind::Status => {
                let text = timeline_item_text(&item);
                if !text.trim().is_empty() {
                    ui.push_status(text);
                }
                false
            }
            _ => {
                let key = format!("gateway:{}", item.id);
                let kind = transcript_kind_for_timeline(item.kind);
                let idx = ui.tool_rows.get(&key).copied().unwrap_or_else(|| {
                    let mut row = TranscriptRow::with_title(
                        kind,
                        timeline_item_title(&item),
                        timeline_item_running_text(&item),
                    );
                    row.tool_name = item.title.clone();
                    if matches!(
                        item.status,
                        TimelineItemStatus::Pending | TimelineItemStatus::Running
                    ) {
                        row.tool_started = Some(Instant::now());
                    }
                    let idx = ui.insert_evidence_row(row);
                    ui.tool_rows.insert(key.clone(), idx);
                    idx
                });
                let row = &mut ui.transcript[idx];
                row.kind = kind;
                row.title = timeline_item_title(&item);
                row.tool_name = item.title.clone();
                row.failed = item.status == TimelineItemStatus::Failed;
                row.interrupted = item.status == TimelineItemStatus::Cancelled;
                row.text = timeline_item_running_text(&item);
                row.full_text = item.detail.clone().filter(|detail| detail != &row.text);
                if matches!(
                    item.status,
                    TimelineItemStatus::Pending | TimelineItemStatus::Running
                ) {
                    if row.tool_started.is_none() {
                        row.tool_started = Some(Instant::now());
                    }
                    row.tool_elapsed = None;
                    ui.remove_turn_meta();
                    true
                } else {
                    if let Some(started) = row.tool_started.take() {
                        row.tool_elapsed = Some(started.elapsed());
                    }
                    false
                }
            }
        }
    }

    pub(crate) fn apply_gateway_tool_item(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        item: &TimelineItem,
        value: Value,
    ) -> bool {
        let event_session = (!item.thread_id.is_empty())
            .then_some(item.thread_id.as_str())
            .or(owner_session);
        if let Some(session_id) = event_session
            && self
                .current_session
                .as_deref()
                .is_some_and(|current| current != session_id)
        {
            return false;
        }

        let tool = value
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let tool_call_id = value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let key = if tool_call_id.is_empty() {
            format!("gateway:{}", item.id)
        } else {
            tool_id_key(tool_call_id)
        };
        let user_shell = value.get("source").and_then(Value::as_str) == Some("user_shell");
        if !tool_call_id.is_empty()
            && let Some(args) = value
                .get("args")
                .cloned()
                .or_else(|| value.get("arguments").cloned())
            && !args.is_null()
        {
            ui.live_tool_args.insert(tool_call_id.to_string(), args);
        }

        if matches!(
            item.status,
            TimelineItemStatus::Pending | TimelineItemStatus::Running
        ) {
            if tool == "clarify" {
                return false;
            }
            if tool == "write_stdin" {
                remove_visible_write_stdin_row(ui, tool_call_id);
                return false;
            }
            let idx = ui.tool_rows.get(&key).copied().unwrap_or_else(|| {
                let mut row = TranscriptRow::with_title(
                    evidence_kind_for_value(tool, &value),
                    active_tool_title(tool, &value),
                    if item.status == TimelineItemStatus::Pending {
                        "preparing"
                    } else {
                        "running"
                    },
                );
                row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                row.tool_name = Some(tool.to_string());
                row.tool_started = Some(tool_started_instant(&value));
                let idx = ui.insert_evidence_row(row);
                ui.tool_rows.insert(key.clone(), idx);
                idx
            });
            ui.remove_turn_meta();
            let row = &mut ui.transcript[idx];
            row.kind = evidence_kind_for_value(tool, &value);
            row.tool_name = Some(tool.to_string());
            row.title = active_tool_title(tool, &value);
            row.text = if item.status == TimelineItemStatus::Pending {
                "preparing".to_string()
            } else if tool == "Agent" {
                agent_child_status_text("Running", 0, None)
            } else {
                "running".to_string()
            };
            row.failed = false;
            row.interrupted = false;
            row.user_shell = user_shell;
            row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
            if row.tool_started.is_none() {
                row.tool_started = Some(tool_started_instant(&value));
            }
            row.tool_elapsed = None;
            return true;
        }

        let outcome = value
            .get("outcome")
            .and_then(Value::as_str)
            .unwrap_or("normal");
        let interrupted =
            item.status == TimelineItemStatus::Cancelled || tool_event_interrupted(&value);
        let user_confirmed_interrupt = interrupted && ui.interrupt_requested;
        let clarify_no_answer = tool == "clarify" && clarify_no_answer_result(&value);
        let failed = item.status == TimelineItemStatus::Failed
            || (outcome != "normal" && !interrupted && !clarify_no_answer);
        if tool == "write_stdin" {
            let cached_args = (!tool_call_id.is_empty())
                .then(|| ui.live_tool_args.remove(tool_call_id))
                .flatten()
                .or_else(|| value.get("args").cloned())
                .or_else(|| value.get("arguments").cloned());
            let target_session_id = cached_args
                .as_ref()
                .and_then(exec_session_id_from_args)
                .or_else(|| exec_session_id_from_result(&value));
            remove_visible_write_stdin_row(ui, tool_call_id);
            if let Some(session_id) = target_session_id
                && let Some(idx) = ui.exec_session_rows.get(&session_id).copied()
                && !failed
                && !interrupted
            {
                let output = tool_result_output(&value);
                if !output.is_empty() {
                    ui.append_exec_session_output(idx, &output);
                }
                if exec_result_completed(&value) {
                    let elapsed = ui
                        .transcript
                        .get(idx)
                        .and_then(|row| completed_live_tool_elapsed(row, Some(&value)));
                    ui.finish_exec_session_row(session_id, idx, elapsed, false, false);
                }
            }
            if !tool_call_id.is_empty() {
                ui.tool_rows.remove(&tool_id_key(tool_call_id));
            }
            return false;
        }
        if tool == "exec_command"
            && let Some(session_id) = exec_session_id_from_result(&value)
            && exec_result_running(&value)
        {
            let idx = ui
                .tool_rows
                .get(&key)
                .copied()
                .or_else(|| ui.exec_session_rows.get(&session_id).copied())
                .unwrap_or_else(|| {
                    let mut row = TranscriptRow::with_title(
                        evidence_kind_for_value(tool, &value),
                        tool_title(tool, &value),
                        String::new(),
                    );
                    row.tool_name = Some(tool.to_string());
                    row.tool_call_id =
                        (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                    ui.insert_evidence_row(row)
                });
            let already_finished = ui
                .transcript
                .get(idx)
                .is_some_and(|row| row.tool_started.is_none() && row.tool_elapsed.is_some());
            {
                let row = &mut ui.transcript[idx];
                row.kind = evidence_kind_for_value(tool, &value);
                row.tool_name = Some(tool.to_string());
                row.title = tool_title_for_update(tool, &value, &row.title);
                row.failed = false;
                row.interrupted = false;
                row.user_shell = user_shell;
                row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                if !already_finished && row.tool_started.is_none() {
                    row.tool_started = Some(tool_started_instant(&value));
                }
                if !already_finished {
                    row.tool_elapsed = None;
                }
            }
            ui.prefix_exec_session_output_if_needed(idx, tool_result_output(&value));
            ui.exec_session_rows.insert(session_id, idx);
            if !already_finished && !tool_call_id.is_empty() {
                ui.tool_rows.insert(key, idx);
            }
            return !already_finished;
        }
        if outcome != "normal" && !user_shell && !interrupted && !clarify_no_answer {
            ui.turn_failures += 1;
        }
        if user_confirmed_interrupt {
            ui.turn_interrupted = true;
        }

        let idx = ui.tool_rows.get(&key).copied().unwrap_or_else(|| {
            let mut row = TranscriptRow::with_title(
                evidence_kind_for_value(tool, &value),
                tool_title(tool, &value),
                String::new(),
            );
            row.tool_name = Some(tool.to_string());
            row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
            ui.insert_evidence_row(row)
        });
        ui.tool_rows.insert(key, idx);
        let row = &mut ui.transcript[idx];
        row.kind = evidence_kind_for_value(tool, &value);
        row.tool_name = Some(tool.to_string());
        row.title = tool_title_for_update(tool, &value, &row.title);
        row.failed = failed;
        row.interrupted = interrupted;
        row.user_shell = user_shell;
        row.tool_elapsed = completed_live_tool_elapsed(row, Some(&value));
        row.tool_started = None;
        row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
        if tool == "Agent" {
            row.agent_target = agent_target_from_tool_event(&value);
            if let Some(summary) = value
                .get("result")
                .and_then(|result| result.get("child_session"))
            {
                row.agent_child_tool_uses = summary
                    .get("tool_call_count")
                    .and_then(Value::as_i64)
                    .unwrap_or(row.agent_child_tool_uses)
                    .max(0);
                row.agent_child_latest_tokens =
                    agent_child_latest_tokens(summary).or(row.agent_child_latest_tokens);
            }
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
        if is_write_like_tool(tool) {
            ui.remove_orphan_provisional_tool_intents(tool, Some(idx));
        }
        if item.status != TimelineItemStatus::Running {
            ui.tool_rows.remove(&tool_id_key(tool_call_id));
        }
        false
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
                selector,
                turn_id,
                events,
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
                        events,
                        task,
                    });
                }
                RunningTask::UserShell(task) => {
                    ui.running = Some(RunningTurn {
                        session_id: owner_session,
                        control,
                        selector,
                        turn_id,
                        events,
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
            while let Ok(event) = agent.events.try_recv() {
                events.push_back(event);
            }
            let had_pending = self.apply_pending_auxiliary_agent_live_events(
                ui,
                agent.session_id.as_deref(),
                events,
            );
            changed |= had_pending;
            if had_pending {
                ui.follow_transcript_if_needed();
                ui.refresh_sidebar(self);
            }

            if agent.task.is_finished() {
                let mut events = VecDeque::new();
                while let Ok(event) = agent.events.try_recv() {
                    events.push_back(event);
                }
                let had_pending = self.apply_pending_auxiliary_agent_live_events(
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
                    BottomPanel::PermissionApproval(panel) => panel
                        .desired_height(main.width)
                        .min(main.height.saturating_sub(5).max(8)),
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

fn outcome_from_str(value: &str) -> Option<Outcome> {
    match value {
        "normal" => Some(Outcome::Normal),
        "stopped" => Some(Outcome::Stopped),
        "failed" => Some(Outcome::Failed),
        "aborted" => Some(Outcome::Aborted),
        _ => None,
    }
}

fn timeline_item_text(item: &TimelineItem) -> String {
    item.body
        .as_ref()
        .or(item.detail.as_ref())
        .or(item.preview.as_ref())
        .cloned()
        .unwrap_or_default()
}

fn timeline_item_title(item: &TimelineItem) -> String {
    item.title.clone().unwrap_or_else(|| match item.kind {
        TimelineItemKind::Shell => "exec_command".to_string(),
        TimelineItemKind::File => "file".to_string(),
        TimelineItemKind::Web => "web".to_string(),
        TimelineItemKind::Mcp => "mcp".to_string(),
        TimelineItemKind::Clarify => "clarify".to_string(),
        TimelineItemKind::Permission => "permission".to_string(),
        TimelineItemKind::Skill => "skill".to_string(),
        TimelineItemKind::Agent => "Agent".to_string(),
        TimelineItemKind::Mailbox => "mailbox".to_string(),
        TimelineItemKind::Diff => "diff".to_string(),
        TimelineItemKind::Artifact => "artifact".to_string(),
        TimelineItemKind::Tool => "tool".to_string(),
        TimelineItemKind::Status => "status".to_string(),
        TimelineItemKind::Prompt | TimelineItemKind::Assistant | TimelineItemKind::Reasoning => {
            String::new()
        }
    })
}

fn timeline_item_running_text(item: &TimelineItem) -> String {
    let text = timeline_item_text(item);
    if !text.trim().is_empty() {
        return text;
    }
    match item.status {
        TimelineItemStatus::Pending => "pending".to_string(),
        TimelineItemStatus::Running => "running".to_string(),
        TimelineItemStatus::Cancelled => "interrupted".to_string(),
        TimelineItemStatus::Failed => "failed".to_string(),
        TimelineItemStatus::NeedsInput => "needs input".to_string(),
        TimelineItemStatus::Info | TimelineItemStatus::Completed => String::new(),
    }
}

fn transcript_kind_for_timeline(kind: TimelineItemKind) -> TranscriptKind {
    match kind {
        TimelineItemKind::File | TimelineItemKind::Diff | TimelineItemKind::Artifact => {
            TranscriptKind::Updated
        }
        TimelineItemKind::Web | TimelineItemKind::Mcp => TranscriptKind::Explored,
        TimelineItemKind::Status => TranscriptKind::Status,
        _ => TranscriptKind::Ran,
    }
}

fn gateway_event_session_id(event: &GatewayEvent) -> Option<&str> {
    match event {
        GatewayEvent::TurnStarted { thread_id, .. }
        | GatewayEvent::TurnQueued { thread_id, .. }
        | GatewayEvent::TurnCompleted { thread_id, .. } => thread_id.as_deref(),
        GatewayEvent::ItemStarted { item, .. }
        | GatewayEvent::ItemUpdated { item, .. }
        | GatewayEvent::ItemCompleted { item, .. } => {
            (!item.thread_id.is_empty()).then_some(item.thread_id.as_str())
        }
        GatewayEvent::ItemDelta { .. }
        | GatewayEvent::PermissionRequested { .. }
        | GatewayEvent::PermissionResolved { .. }
        | GatewayEvent::ClarifyRequested { .. }
        | GatewayEvent::ClarifyResolved { .. }
        | GatewayEvent::Warning { .. }
        | GatewayEvent::DebugAvailable { .. } => None,
    }
}

fn gateway_item_tool_value(item: &TimelineItem) -> Option<Value> {
    let value = item.metadata.as_ref()?;
    (value.get("projection").and_then(Value::as_str) == Some("tool")).then(|| value.clone())
}

fn gateway_item_runtime_value(item: &TimelineItem) -> Option<Value> {
    let value = item.metadata.as_ref()?;
    (value.get("projection").and_then(Value::as_str) == Some("runtimeValue")).then(|| value.clone())
}

fn remove_visible_write_stdin_row(ui: &mut FullscreenUi<'_>, tool_call_id: &str) {
    if tool_call_id.is_empty() {
        return;
    }
    ui.remove_streaming_tool_call_row("write_stdin", tool_call_id, None);
    let key = tool_id_key(tool_call_id);
    if let Some(index) = ui.tool_rows.get(&key).copied()
        && ui
            .transcript
            .get(index)
            .is_some_and(|row| row.tool_name.as_deref() == Some("write_stdin"))
    {
        ui.remove_transcript_row(index);
    }
}

fn apply_gateway_assistant_turn_metadata(ui: &mut FullscreenUi<'_>, item: &TimelineItem) {
    let Some(metadata) = item.metadata.as_ref() else {
        return;
    };
    if let Some(usage) = non_null_metadata_field(metadata, "usage") {
        if let Some(tokens) = usage_context_tokens(&usage) {
            ui.sidebar_tokens = Some(tokens);
        }
        ui.turn_usage = Some(usage);
    }
    if let Some(turn_metadata) = gateway_assistant_turn_metadata(metadata) {
        ui.turn_metadata = Some(turn_metadata);
    }
    if let Some(accounting) = non_null_metadata_field(metadata, "accounting") {
        ui.add_sidebar_cost(Some(&accounting));
        ui.turn_accounting = Some(accounting);
    }
    if let Some(provider) = metadata_string_field(metadata, "provider") {
        ui.turn_provider = provider;
    }
    if let Some(model) = metadata_string_field(metadata, "model") {
        ui.turn_model = model;
    }
    if let Some(mode) = metadata_string_field(metadata, "mode") {
        ui.turn_mode = mode;
    }
}

fn push_gateway_completed_turn_meta(ui: &mut FullscreenUi<'_>, debug: bool) {
    let meta = turn_meta_text(TurnMetaProjection {
        mode: &ui.turn_mode,
        provider: &ui.turn_provider,
        model: &ui.turn_model,
        started: None,
        usage: ui.turn_usage.as_ref(),
        metadata: ui.turn_metadata.as_ref(),
        accounting: ui.turn_accounting.as_ref(),
        failures: ui.turn_failures,
        interrupted: ui.turn_interrupted,
        debug,
    });
    if !meta.is_empty() {
        ui.transcript
            .push(TranscriptRow::with_title(TranscriptKind::Meta, "", meta));
    }
    ui.finish_turn();
}

fn gateway_assistant_item_receives_meta(item: &TimelineItem) -> bool {
    if item.kind != TimelineItemKind::Assistant || item.status != TimelineItemStatus::Completed {
        return false;
    }
    let Some(metadata) = item.metadata.as_ref() else {
        return true;
    };
    if metadata_string_field(metadata, "finish_reason")
        .as_deref()
        .is_some_and(|finish_reason| matches!(finish_reason, "tool_calls" | "aborted"))
    {
        return false;
    }
    metadata_string_field(metadata, "outcome")
        .as_deref()
        .is_none_or(|outcome| outcome == "normal")
}

fn gateway_assistant_turn_metadata(metadata: &Value) -> Option<Value> {
    if let Some(value) = non_null_metadata_field(metadata, "metadata") {
        return Some(value);
    }
    let object = metadata.as_object()?;
    let mut projected = serde_json::Map::new();
    for (key, value) in object {
        if matches!(
            key.as_str(),
            "usage"
                | "accounting"
                | "provider"
                | "model"
                | "mode"
                | "finish_reason"
                | "outcome"
                | "message_session_seq"
                | "content_array_index"
        ) {
            continue;
        }
        if !value.is_null() {
            projected.insert(key.clone(), value.clone());
        }
    }
    (!projected.is_empty()).then_some(Value::Object(projected))
}

fn non_null_metadata_field(metadata: &Value, key: &str) -> Option<Value> {
    metadata.get(key).filter(|value| !value.is_null()).cloned()
}

fn metadata_string_field(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[path = "events/helpers.rs"]
pub(crate) mod helpers;
#[allow(unused_imports)]
pub use helpers::*;
