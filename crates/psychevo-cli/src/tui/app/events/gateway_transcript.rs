impl TuiApp {
    pub(crate) fn apply_gateway_event(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        event: GatewayEvent,
    ) -> bool {
        let event_session = gateway_event_session_id(&event).or(owner_session);
        if let Some(session_id) = event_session
            && (owner_session.is_some() && self.current_session.as_deref() != Some(session_id)
                || self
                    .current_session
                    .as_deref()
                    .is_some_and(|current| current != session_id))
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
                ui.bind_unbound_optimistic_rows_to_turn(&turn_id);
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
            GatewayEvent::TurnCompleted {
                outcome,
                turn_id,
                committed_entries,
                ..
            } => {
                if let Some(outcome) = outcome.as_deref().and_then(outcome_from_str) {
                    ui.turn_outcome = Some(outcome);
                    if ui.interrupt_requested && outcome == Outcome::Aborted {
                        ui.turn_interrupted = true;
                    }
                }
                self.apply_committed_turn_entries(ui, owner_session, &turn_id, committed_entries);
                ui.update_turn_meta(self.debug, true, true, true);
                false
            }
            GatewayEvent::EntryDelta { delta, .. } => {
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
            GatewayEvent::EntryStarted { entry, .. }
            | GatewayEvent::EntryUpdated { entry, .. }
            | GatewayEvent::EntryCompleted { entry, .. } => {
                self.apply_gateway_transcript_entry(ui, owner_session, entry)
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
                    .or_else(|| owner_session.map(GatewayThreadSelector::thread_id))
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
            GatewayEvent::ActivityChanged { .. } | GatewayEvent::TitleChanged { .. } => false,
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
        self.clear_new_session_draft();
    }

    pub(crate) fn apply_gateway_transcript_entry(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        entry: TranscriptEntry,
    ) -> bool {
        let mut active = false;
        let meta = GatewayTranscriptEntryMeta {
            role: entry.role,
            thread_id: &entry.thread_id,
            turn_id: entry.turn_id.as_deref(),
            entry_id: &entry.id,
            message_seq: entry.message_seq,
            source: &entry.source,
        };
        for block in entry.blocks {
            active |= self.apply_gateway_transcript_block(ui, owner_session, meta, block);
        }
        active
    }

    pub(crate) fn apply_committed_turn_entries(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        turn_id: &str,
        committed_entries: Vec<TranscriptEntry>,
    ) -> bool {
        let loaded_message_count = ui.loaded_session_message_count as i64;
        let mut max_message_seq = None::<i64>;
        let entries = committed_entries
            .into_iter()
            .filter(|entry| {
                entry
                    .message_seq
                    .is_none_or(|seq| seq > loaded_message_count)
            })
            .inspect(|entry| {
                if let Some(seq) = entry.message_seq {
                    max_message_seq = Some(max_message_seq.map_or(seq, |max| max.max(seq)));
                }
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return false;
        }

        ui.bind_unbound_optimistic_rows_to_turn(turn_id);
        ui.bind_unbound_local_rows_to_turn(turn_id);
        ui.bind_unbound_live_turn_meta_to_turn(turn_id);
        let local_rows = ui.take_local_rows_for_turn(turn_id);
        ui.remove_live_overlay_for_turn(turn_id);
        let mut active = false;
        for entry in entries {
            active |= self.apply_gateway_transcript_entry(ui, owner_session, entry);
        }
        ui.transcript.extend(local_rows);
        if let Some(max_seq) = max_message_seq {
            ui.loaded_session_message_count = ui
                .loaded_session_message_count
                .max(max_seq.try_into().unwrap_or(usize::MAX));
        }
        active
    }

    pub(crate) fn apply_gateway_transcript_block(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        entry_meta: GatewayTranscriptEntryMeta<'_>,
        block: TranscriptBlock,
    ) -> bool {
        if let Some(value) = gateway_block_runtime_value(&block)
            && value.get("type").and_then(Value::as_str) == Some("agent_session_start")
        {
            ui.apply_agent_session_start(&value);
            return false;
        }
        if let Some(value) = gateway_block_tool_value(&block) {
            return self.apply_gateway_tool_block(ui, owner_session, entry_meta, &block, value);
        }
        let event_session = (!entry_meta.thread_id.is_empty())
            .then_some(entry_meta.thread_id)
            .or(owner_session);
        if let Some(session_id) = event_session
            && self
                .current_session
                .as_deref()
                .is_some_and(|current| current != session_id)
        {
            return false;
        }
        match (entry_meta.role, block.kind) {
            (TranscriptEntryRole::User, TranscriptBlockKind::Text) => {
                let text = transcript_block_text(&block);
                if !text.trim().is_empty() {
                    let idx = gateway_block_row_index(ui, &block.id).unwrap_or_else(|| {
                        let idx = ui.insert_transcript_row(
                            ui.transcript.len(),
                            TranscriptRow::with_title(TranscriptKind::Prompt, "", String::new()),
                        );
                        record_gateway_block_row(ui, &block.id, idx);
                        idx
                    });
                    if let Some(row) = ui.transcript.get_mut(idx) {
                        row.kind = TranscriptKind::Prompt;
                        row.title.clear();
                        row.text = text;
                        row.full_text = None;
                    }
                    tag_gateway_transcript_row(ui, idx, entry_meta, &block);
                }
                false
            }
            (TranscriptEntryRole::Assistant, TranscriptBlockKind::Text) => {
                let live_item = block.source == "runtime.stream";
                let text = transcript_block_text(&block);
                if text.trim().is_empty() {
                    return false;
                }
                if let Some(idx) = ui.reasoning_row.take()
                    && gateway_block_row_index(ui, &block.id) != Some(idx)
                {
                    ui.finish_thinking_row(idx);
                }
                let idx = gateway_block_row_index(ui, &block.id).unwrap_or_else(|| {
                    let idx = ui.insert_answer_row(TranscriptRow::with_title(
                        TranscriptKind::Answer,
                        "",
                        String::new(),
                    ));
                    record_gateway_block_row(ui, &block.id, idx);
                    ui.assistant_row = Some(idx);
                    idx
                });
                clear_gateway_row_slots_for_index(ui, idx);
                ui.assistant_row = Some(idx);
                if let Some(row) = ui.transcript.get_mut(idx) {
                    row.kind = TranscriptKind::Answer;
                    row.title.clear();
                    row.text = text;
                    row.full_text = None;
                    row.expanded = false;
                    row.tool_started = None;
                    row.tool_elapsed = None;
                    row.tool_name = None;
                    row.tool_call_id = None;
                }
                tag_gateway_transcript_row(ui, idx, entry_meta, &block);
                ui.remove_turn_meta();
                apply_gateway_assistant_turn_metadata(ui, &block);
                if matches!(
                    block.status,
                    TranscriptBlockStatus::Completed
                        | TranscriptBlockStatus::Failed
                        | TranscriptBlockStatus::Cancelled
                ) {
                    if gateway_assistant_block_receives_meta(&block) {
                        ui.turn_terminal_visible_answer = true;
                        if live_item {
                            ui.update_turn_meta(self.debug, true, false, false);
                        } else {
                            push_gateway_completed_turn_meta(ui, self.debug, entry_meta);
                        }
                    }
                    ui.assistant_row = None;
                    return false;
                }
                block.status == TranscriptBlockStatus::Running
            }
            (_, TranscriptBlockKind::Reasoning) => {
                let text = transcript_block_text(&block);
                let existing_idx = gateway_block_row_index(ui, &block.id).or(ui.reasoning_row);
                if text.trim().is_empty() && existing_idx.is_none() {
                    return false;
                }
                let idx = existing_idx.unwrap_or_else(|| {
                    let mut row = TranscriptRow::with_title(
                        TranscriptKind::Thinking,
                        gateway_reasoning_title(&block),
                        String::new(),
                    );
                    if !matches!(
                        block.status,
                        TranscriptBlockStatus::Completed
                            | TranscriptBlockStatus::Failed
                            | TranscriptBlockStatus::Cancelled
                    ) {
                        row.tool_started = Some(Instant::now());
                    }
                    let idx = ui.insert_evidence_row(row);
                    record_gateway_block_row(ui, &block.id, idx);
                    ui.reasoning_row = Some(idx);
                    idx
                });
                clear_gateway_row_slots_for_index(ui, idx);
                if let Some(row) = ui.transcript.get_mut(idx) {
                    row.kind = TranscriptKind::Thinking;
                    row.title = gateway_reasoning_title(&block);
                    row.tool_name = None;
                    row.tool_call_id = None;
                    if !matches!(
                        block.status,
                        TranscriptBlockStatus::Completed
                            | TranscriptBlockStatus::Failed
                            | TranscriptBlockStatus::Cancelled
                    ) && row.tool_started.is_none()
                    {
                        row.tool_started = Some(Instant::now());
                    }
                }
                tag_gateway_transcript_row(ui, idx, entry_meta, &block);
                ui.reasoning_row = Some(idx);
                if !text.trim().is_empty() {
                    ui.transcript[idx].set_evidence_body_text(text);
                    ui.turn_had_reasoning = true;
                    ui.remove_turn_meta();
                }
                if block.status == TranscriptBlockStatus::Completed {
                    ui.finish_thinking_row(idx);
                    if ui.reasoning_row == Some(idx) {
                        ui.reasoning_row = None;
                    }
                    return false;
                }
                true
            }
            (_, TranscriptBlockKind::Status) => {
                let text = transcript_block_text(&block);
                if !text.trim().is_empty() {
                    ui.push_status(text);
                }
                false
            }
            _ => {
                let key = format!("gateway:{}", block.id);
                let kind = transcript_kind_for_block(block.kind);
                let tool_call_id = gateway_block_tool_call_id(&block).map(str::to_string);
                let idx = ui
                    .tool_rows
                    .get(&key)
                    .copied()
                    .or_else(|| {
                        tool_call_id
                            .as_deref()
                            .and_then(|id| ui.tool_rows.get(&tool_id_key(id)).copied())
                    })
                    .unwrap_or_else(|| {
                    let mut row = TranscriptRow::with_title(
                        kind,
                        transcript_block_title(&block),
                        transcript_block_running_text(&block),
                    );
                    row.tool_name = block.title.clone();
                    if matches!(
                        block.status,
                        TranscriptBlockStatus::Pending | TranscriptBlockStatus::Running
                    ) {
                        row.tool_started = Some(Instant::now());
                    }
                    let idx = ui.insert_evidence_row(row);
                    ui.tool_rows.insert(key.clone(), idx);
                    idx
                });
                ui.tool_rows.insert(key, idx);
                if let Some(tool_call_id) = tool_call_id.as_deref() {
                    ui.tool_rows.insert(tool_id_key(tool_call_id), idx);
                }
                let active = {
                    let row = &mut ui.transcript[idx];
                    row.kind = kind;
                    row.title = transcript_block_title(&block);
                    row.tool_name = block.title.clone();
                    row.tool_call_id = tool_call_id;
                    row.failed = block.status == TranscriptBlockStatus::Failed;
                    row.interrupted = block.status == TranscriptBlockStatus::Cancelled;
                    row.text = transcript_block_running_text(&block);
                    row.full_text = block.detail.clone().filter(|detail| detail != &row.text);
                    if matches!(
                        block.status,
                        TranscriptBlockStatus::Pending | TranscriptBlockStatus::Running
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
                };
                tag_gateway_transcript_row(ui, idx, entry_meta, &block);
                active
            }
        }
    }
}
