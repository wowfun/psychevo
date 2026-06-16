impl TuiApp {
    pub(crate) fn apply_gateway_tool_block(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session: Option<&str>,
        entry_meta: GatewayTranscriptEntryMeta<'_>,
        block: &TranscriptBlock,
        value: Value,
    ) -> bool {
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

        let tool = value
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let tool_call_id = value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let key = if tool_call_id.is_empty() {
            format!("gateway:{}", block.id)
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

        let yielded_exec_running = tool == "exec_command"
            && exec_session_id_from_result(&value).is_some()
            && exec_result_running(&value);

        if matches!(
            block.status,
            TranscriptBlockStatus::Pending | TranscriptBlockStatus::Running
        ) && !yielded_exec_running
        {
            let background_agent_handoff = background_running_agent_result(tool, &value);
            if tool == "clarify" {
                return false;
            }
            if tool == "write_stdin" {
                remove_visible_write_stdin_row(ui, tool_call_id);
                return false;
            }
            let idx = gateway_block_row_index(ui, &block.id)
                .or_else(|| ui.tool_rows.get(&key).copied())
                .unwrap_or_else(|| {
                    let mut row = TranscriptRow::with_title(
                        evidence_kind_for_value(tool, &value),
                        active_tool_title(tool, &value),
                        if block.status == TranscriptBlockStatus::Pending {
                            "preparing"
                        } else {
                            "running"
                        },
                    );
                    row.tool_call_id =
                        (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                    row.tool_name = Some(tool.to_string());
                    row.tool_started = Some(tool_started_instant(&value));
                    let idx = ui.insert_evidence_row(row);
                    ui.tool_rows.insert(key.clone(), idx);
                    idx
                });
            record_gateway_block_row(ui, &block.id, idx);
            ui.tool_rows.insert(key.clone(), idx);
            ui.remove_turn_meta();
            let existing_agent_handoff = tool == "Agent"
                && value.get("type").and_then(Value::as_str) == Some("agent_session_start")
                && ui.transcript.get(idx).is_some_and(|row| {
                    row.agent_target.is_some()
                        && row.tool_started.is_none()
                        && !row.interrupted
                        && !row.failed
                });
            let agent_handoff_update = background_agent_handoff || existing_agent_handoff;
            let existing_handoff_text = ui
                .transcript
                .get(idx)
                .map(|row| (row.text.clone(), row.full_text.clone()));
            let row = &mut ui.transcript[idx];
            row.kind = evidence_kind_for_value(tool, &value);
            row.tool_name = Some(tool.to_string());
            row.title = if tool == "Agent" {
                block
                    .title
                    .as_deref()
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| tool_title_for_update(tool, &value, &row.title))
            } else if background_running_agent_result(tool, &value) {
                tool_title_for_update(tool, &value, &row.title)
            } else {
                active_tool_title(tool, &value)
            };
            let (handoff_text, handoff_full_text) = if background_agent_handoff {
                let (collapsed, full) = tool_output_text(&value);
                (
                    if collapsed.is_empty() {
                        format_tool_summary(&value)
                    } else {
                        collapsed
                    },
                    full,
                )
            } else if agent_handoff_update {
                existing_handoff_text.unwrap_or_else(|| ("Started in background".to_string(), None))
            } else {
                (String::new(), None)
            };
            row.text = if agent_handoff_update {
                handoff_text
            } else if block.status == TranscriptBlockStatus::Pending {
                "preparing".to_string()
            } else if tool == "Agent" {
                agent_child_status_text("Running", 0, None)
            } else {
                "running".to_string()
            };
            if agent_handoff_update {
                row.full_text = handoff_full_text;
            }
            row.failed = false;
            row.interrupted = false;
            row.user_shell = user_shell;
            row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
            if tool == "Agent"
                && let Some(agent_target) = agent_target_from_tool_event(&value)
            {
                row.agent_target = Some(agent_target);
            }
            if agent_handoff_update {
                row.tool_elapsed =
                    completed_live_tool_elapsed(row, Some(&value)).or(row.tool_elapsed);
                row.tool_started = None;
            } else if row.tool_started.is_none() {
                row.tool_started = Some(tool_started_instant(&value));
                row.tool_elapsed = None;
            }
            tag_gateway_transcript_row(ui, idx, entry_meta, block);
            if tool == "Agent" {
                ui.remove_duplicate_agent_placeholders_for_tool_value(idx, &value);
            }
            return true;
        }

        let outcome = value
            .get("outcome")
            .and_then(Value::as_str)
            .unwrap_or("normal");
        let interrupted =
            block.status == TranscriptBlockStatus::Cancelled || tool_event_interrupted(&value);
        let user_confirmed_interrupt = interrupted && ui.interrupt_requested;
        let clarify_no_answer = tool == "clarify" && clarify_no_answer_result(&value);
        let failed = block.status == TranscriptBlockStatus::Failed
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
            tag_gateway_transcript_row(ui, idx, entry_meta, block);
            ui.exec_session_rows.insert(session_id, idx);
            if !already_finished && !tool_call_id.is_empty() {
                ui.tool_rows.insert(key, idx);
            }
            return !already_finished;
        }
        if tool == "exec_command"
            && let Some(session_id) = exec_session_id_from_result(&value)
            && exec_result_completed(&value)
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
            {
                let row = &mut ui.transcript[idx];
                row.kind = evidence_kind_for_value(tool, &value);
                row.tool_name = Some(tool.to_string());
                row.title = tool_title_for_update(tool, &value, &row.title);
                row.failed = failed;
                row.interrupted = interrupted;
                row.user_shell = user_shell;
                row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
            }
            ui.prefix_exec_session_output_if_needed(idx, tool_result_output(&value));
            let elapsed = ui
                .transcript
                .get(idx)
                .and_then(|row| completed_live_tool_elapsed(row, Some(&value)));
            ui.finish_exec_session_row(session_id, idx, elapsed, interrupted, false);
            if let Some(row) = ui.transcript.get_mut(idx) {
                row.failed = failed;
            }
            tag_gateway_transcript_row(ui, idx, entry_meta, block);
            return false;
        }
        if outcome != "normal" && !user_shell && !interrupted && !clarify_no_answer {
            ui.turn_failures += 1;
        }
        if user_confirmed_interrupt {
            ui.turn_interrupted = true;
        }

        let idx = gateway_block_row_index(ui, &block.id)
            .or_else(|| ui.tool_rows.get(&key).copied())
            .unwrap_or_else(|| {
                let mut row = TranscriptRow::with_title(
                    evidence_kind_for_value(tool, &value),
                    tool_title(tool, &value),
                    String::new(),
                );
                row.tool_name = Some(tool.to_string());
                row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                ui.insert_evidence_row(row)
            });
        record_gateway_block_row(ui, &block.id, idx);
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
            if let Some(agent_target) = agent_target_from_tool_event(&value) {
                row.agent_target = Some(agent_target);
            }
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
        tag_gateway_transcript_row(ui, idx, entry_meta, block);
        if is_write_like_tool(tool) {
            ui.remove_orphan_provisional_tool_intents(tool, Some(idx));
        }
        if tool == "Agent" {
            ui.remove_duplicate_agent_placeholders_for_tool_value(idx, &value);
        }
        if block.status != TranscriptBlockStatus::Running
            && !background_running_agent_result(tool, &value)
        {
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
        self.clear_new_session_draft();
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
                        pending_unowned_live_events: Vec::new(),
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
}
