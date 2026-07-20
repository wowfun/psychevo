impl<'a> FullscreenUi<'a> {
    #[cfg(test)]
    pub(crate) fn push_history_message(
        &mut self,
        message: &Value,
        usage: Option<&Value>,
        metadata: Option<&Value>,
    ) {
        self.push_history_message_with_accounting(message, usage, metadata, None);
    }

    #[cfg(test)]
    pub(crate) fn push_history_message_with_accounting(
        &mut self,
        message: &Value,
        usage: Option<&Value>,
        metadata: Option<&Value>,
        accounting: Option<&Value>,
    ) {
        self.push_history_message_with_accounting_options(
            message, usage, metadata, accounting, false,
        );
    }

    pub(crate) fn push_history_message_with_accounting_options(
        &mut self,
        message: &Value,
        usage: Option<&Value>,
        metadata: Option<&Value>,
        accounting: Option<&Value>,
        suppress_terminal_meta: bool,
    ) {
        self.push_history_message_with_projection_options(
            message,
            usage,
            metadata,
            accounting,
            suppress_terminal_meta,
            None,
        );
    }

    pub(crate) fn push_history_message_with_projection_options(
        &mut self,
        message: &Value,
        usage: Option<&Value>,
        metadata: Option<&Value>,
        accounting: Option<&Value>,
        suppress_terminal_meta: bool,
        active_tool_call_ids: Option<&BTreeSet<String>>,
    ) {
        if side_inherited_message(metadata) {
            return;
        }
        match message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "user" => {
                if let Some(display) = agent_notification_display(metadata) {
                    let mut row =
                        TranscriptRow::with_title(TranscriptKind::Status, "Agent", display);
                    row.agent_target = agent_notification_target(metadata);
                    self.transcript.push(row);
                } else if let Some(display) = user_shell_display_from_message(message, metadata) {
                    self.push_history_user_shell(display);
                } else if let Some(display) = user_display_from_message(message, metadata) {
                    self.push_user_with_attachment_meta(display.text, display.attachment_meta);
                    self.history_prompt_started_ms = message_timestamp_ms(message);
                }
            }
            "assistant" => {
                let tool_calls = history_tool_calls_from_message(message);
                let has_reasoning =
                    if let Some(reasoning) = assistant_reasoning_from_message(message) {
                        let mut row = TranscriptRow::with_title(
                            TranscriptKind::Thinking,
                            "Thinking",
                            reasoning,
                        );
                        row.collapse_thinking_details();
                        self.transcript.push(row);
                        true
                    } else {
                        false
                    };
                let has_answer = if let Some(text) = assistant_text_from_message(message) {
                    self.transcript.push(TranscriptRow::with_title(
                        TranscriptKind::Answer,
                        "",
                        text,
                    ));
                    true
                } else {
                    false
                };
                if let Some(tokens) = usage.and_then(usage_context_tokens) {
                    self.sidebar_tokens = Some(tokens);
                }
                self.add_sidebar_cost(accounting);
                let keep_tool_calls_active = assistant_message_keeps_tool_calls_active(message);
                let mut kept_any_tool_call_active = false;
                for call in tool_calls {
                    let keep_call_active = keep_tool_calls_active
                        && active_tool_call_ids.is_none_or(|ids| ids.contains(&call.id));
                    if keep_call_active {
                        kept_any_tool_call_active = true;
                        self.push_history_active_tool_call(message, call);
                    } else {
                        self.push_history_interrupted_tool_call(call, metadata);
                    }
                }
                if !suppress_terminal_meta
                    && ((has_answer && visible_answer_message_receives_meta(message))
                        || (has_reasoning && reasoning_only_message_receives_meta(message)))
                    && let Some(meta) = history_meta_text(
                        message,
                        usage,
                        metadata,
                        accounting,
                        self.history_prompt_started_ms,
                    )
                {
                    self.transcript
                        .push(TranscriptRow::with_title(TranscriptKind::Meta, "", meta));
                }
                if !kept_any_tool_call_active {
                    self.history_prompt_started_ms = None;
                }
            }
            "tool_result" => self.push_history_tool_result(message, metadata),
            _ => {}
        }
    }

    pub(crate) fn push_history_user_shell(&mut self, display: UserShellDisplay) {
        let value = serde_json::json!({
            "type": "tool_execution_end",
            "tool_name": "exec_command",
            "args": {"cmd": display.command},
            "result": display.result,
            "outcome": display.outcome,
            "source": "user_shell",
        });
        let (collapsed, full) = tool_output_text(&value);
        let mut row = TranscriptRow::with_title(
            TranscriptKind::Ran,
            tool_title("exec_command", &value),
            if collapsed.is_empty() {
                format_tool_summary(&value)
            } else {
                collapsed
            },
        );
        row.full_text = full;
        row.interrupted = tool_event_interrupted(&value);
        row.failed =
            value.get("outcome").and_then(Value::as_str) != Some("normal") && !row.interrupted;
        row.user_shell = true;
        self.transcript.push(row);
    }

    pub(crate) fn push_history_active_tool_call(&mut self, message: &Value, call: HistoryToolCall) {
        self.history_tool_titles
            .insert(call.id.clone(), call.completed_title.clone());
        self.history_tool_args
            .insert(call.id.clone(), call.args.clone());
        if call.name == "write_stdin"
            && let Some(session_id) = exec_session_id_from_args(&call.args)
            && self.exec_session_rows.contains_key(&session_id)
        {
            if let Some(chars) = write_stdin_non_empty_chars(&call.args) {
                self.push_exec_stdin_row(session_id, chars);
            }
            return;
        }
        let mut row =
            TranscriptRow::with_title(evidence_kind(&call.name), call.active_title, "preparing");
        let tool_value = serde_json::json!({
            "tool_name": call.name.clone(),
            "args": call.args.clone(),
            "result": {}
        });
        if call.name == "spawn_agent" {
            row.full_text = running_agent_tool_full_text(&tool_value);
        } else if call.name == "write"
            && let Some(preview) = write_argument_preview_from_args(&call.args)
        {
            row.set_write_argument_preview(preview, "writing", None);
        }
        row.tool_call_id = Some(call.id.clone());
        row.tool_name = Some(call.name.clone());
        row.tool_started = Some(history_tool_started_instant(message));
        let idx = self.transcript.len();
        self.transcript.push(row);
        self.tool_rows.insert(tool_id_key(&call.id), idx);
    }

    pub(crate) fn push_history_interrupted_tool_call(
        &mut self,
        call: HistoryToolCall,
        metadata: Option<&Value>,
    ) {
        self.history_tool_titles
            .insert(call.id.clone(), call.completed_title.clone());
        self.history_tool_args
            .insert(call.id.clone(), call.args.clone());
        let mut row = TranscriptRow::with_title(
            evidence_kind(&call.name),
            call.completed_title,
            "interrupted",
        );
        row.tool_call_id = Some(call.id);
        let tool_value = serde_json::json!({
            "tool_name": call.name.clone(),
            "args": call.args.clone(),
            "result": {}
        });
        if call.name == "spawn_agent" {
            row.full_text = running_agent_tool_full_text(&tool_value);
        } else if call.name == "write"
            && let Some(preview) = write_argument_preview_from_args(&call.args)
        {
            row.set_write_argument_preview(preview, "cancelled", Some("interrupted"));
        }
        row.tool_name = Some(call.name);
        row.tool_elapsed = metadata_elapsed_duration(metadata);
        row.interrupted = true;
        self.transcript.push(row);
    }

    pub(crate) fn push_history_tool_result(&mut self, message: &Value, metadata: Option<&Value>) {
        let tool = message
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let is_error = message
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let tool_call_id = message
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let result = decode_persisted_tool_result_for_display(tool, content);
        let outcome = if is_error && result.get("error").and_then(Value::as_str) == Some("aborted")
        {
            "aborted"
        } else if is_error {
            "failed"
        } else {
            "normal"
        };
        let args = self
            .history_tool_args
            .get(tool_call_id)
            .cloned()
            .unwrap_or(Value::Null);
        let value = serde_json::json!({
            "tool_name": tool,
            "args": args,
            "result": result,
            "outcome": outcome
        });
        if self.apply_history_exec_session_result(tool, tool_call_id, &value, metadata, is_error) {
            return;
        }
        let interrupted = tool_event_interrupted(&value);
        let clarify_no_answer = tool == "clarify" && clarify_no_answer_result(&value);
        let title = if matches!(tool, "spawn_agent" | "clarify") {
            tool_title(tool, &value)
        } else {
            self.history_tool_titles
                .get(tool_call_id)
                .cloned()
                .unwrap_or_else(|| tool_title(tool, &value))
        };
        let idx = self.tool_rows.get(&tool_id_key(tool_call_id)).copied();
        let mut row = idx
            .and_then(|idx| self.transcript.get(idx).cloned())
            .unwrap_or_else(|| TranscriptRow::with_title(evidence_kind(tool), title.clone(), ""));
        row.kind = evidence_kind(tool);
        row.title = title;
        row.tool_name = Some(tool.to_string());
        row.interrupted = interrupted;
        row.failed = is_error && !interrupted && !clarify_no_answer;
        row.tool_elapsed = metadata_elapsed_duration(metadata)
            .or_else(|| row.tool_started.map(|started| started.elapsed()));
        row.tool_started = None;
        if tool == "spawn_agent" {
            row.agent_target = agent_target_from_tool_event(&value);
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
        if tool == "write" && (is_error || interrupted) {
            if row.write_argument_preview.is_none()
                && let Some(preview) = write_argument_preview_from_args(
                    value.get("args").unwrap_or(&Value::Null),
                )
            {
                row.write_argument_preview = Some(preview);
            }
            let terminal_detail = row
                .full_text
                .clone()
                .unwrap_or_else(|| row.text.clone());
            let phase = if interrupted { "cancelled" } else { "failed" };
            row.refresh_write_argument_preview(phase, Some(&terminal_detail));
        }
        if let Some(idx) = idx {
            self.transcript[idx] = row;
            self.tool_rows.retain(|_, row_index| *row_index != idx);
        } else {
            self.transcript.push(row);
        }
    }

    pub(crate) fn apply_history_exec_session_result(
        &mut self,
        tool: &str,
        tool_call_id: &str,
        value: &Value,
        metadata: Option<&Value>,
        is_error: bool,
    ) -> bool {
        if tool == "exec_command"
            && let Some(session_id) = exec_session_id_from_result(value)
            && exec_result_running(value)
        {
            let title = self
                .history_tool_titles
                .get(tool_call_id)
                .cloned()
                .unwrap_or_else(|| tool_title(tool, value));
            let idx = self
                .tool_rows
                .get(&tool_id_key(tool_call_id))
                .copied()
                .unwrap_or_else(|| {
                    let mut row = TranscriptRow::with_title(evidence_kind(tool), title.clone(), "");
                    row.tool_call_id =
                        (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                    row.tool_name = Some(tool.to_string());
                    self.insert_evidence_row(row)
                });
            let row = &mut self.transcript[idx];
            row.kind = evidence_kind(tool);
            row.title = title;
            row.tool_name = Some(tool.to_string());
            row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
            row.failed = false;
            row.interrupted = false;
            row.tool_elapsed = metadata_elapsed_duration(metadata);
            row.tool_started = None;
            set_exec_row_text(
                row,
                with_exec_history_running_marker(tool_result_output(value)),
            );
            self.exec_session_rows.insert(session_id, idx);
            self.exec_session_elapsed.insert(
                session_id,
                metadata_elapsed_duration(metadata).unwrap_or_default(),
            );
            self.tool_rows.retain(|_, row_index| *row_index != idx);
            return true;
        }

        if tool != "write_stdin" || is_error {
            return false;
        }
        let Some(args) = value.get("args") else {
            return false;
        };
        let Some(session_id) = exec_session_id_from_args(args) else {
            return false;
        };
        let Some(idx) = self.exec_session_rows.get(&session_id).copied() else {
            return false;
        };

        let output = tool_result_output(value);
        if !output.is_empty() {
            self.append_exec_session_output(idx, &output);
        }
        if let Some(elapsed) = metadata_elapsed_duration(metadata) {
            let total = self.exec_session_elapsed.entry(session_id).or_default();
            *total += elapsed;
        }

        if exec_result_completed(value) {
            let elapsed = self.exec_session_elapsed.remove(&session_id);
            self.finish_exec_session_row(session_id, idx, elapsed, false, false);
        } else if let Some(row) = self.transcript.get_mut(idx) {
            let full = exec_row_full_text_without_history_marker(row);
            set_exec_row_text(row, with_exec_history_running_marker(full));
        }
        true
    }

    pub(crate) fn append_exec_session_output(&mut self, idx: usize, output: &str) {
        if output.is_empty() {
            return;
        }
        let Some(row) = self.transcript.get_mut(idx) else {
            return;
        };
        let mut full = exec_row_full_text_without_history_marker(row);
        full.push_str(output);
        set_exec_row_text(row, full);
    }

    pub(crate) fn prefix_exec_session_output_if_needed(&mut self, idx: usize, output: String) {
        if output.is_empty() {
            return;
        }
        let Some(row) = self.transcript.get_mut(idx) else {
            return;
        };
        let full = exec_row_full_text_without_history_marker(row);
        if full.is_empty() || output.starts_with(&full) {
            set_exec_row_text(row, output);
        } else if !full.starts_with(&output) && !full.ends_with(&output) {
            set_exec_row_text(row, format!("{output}{full}"));
        }
    }

    pub(crate) fn finish_exec_session_row(
        &mut self,
        session_id: u64,
        idx: usize,
        elapsed: Option<Duration>,
        interrupted: bool,
        keep_session_mapping: bool,
    ) {
        let Some(row) = self.transcript.get_mut(idx) else {
            return;
        };
        row.tool_elapsed = elapsed.or_else(|| row.tool_started.map(|started| started.elapsed()));
        row.tool_started = None;
        row.title = completed_tool_title_from_active(row.kind, &row.title);
        row.interrupted = interrupted;
        row.failed = false;
        if interrupted {
            row.text = "interrupted".to_string();
            row.full_text = None;
        } else {
            let full = exec_row_full_text_without_history_marker(row);
            set_exec_row_text(row, full);
        }
        let tool_call_id = row.tool_call_id.clone();
        if !keep_session_mapping {
            self.exec_session_rows.remove(&session_id);
        }
        self.exec_session_elapsed.remove(&session_id);
        if let Some(tool_call_id) = tool_call_id {
            self.tool_rows.remove(&tool_id_key(&tool_call_id));
        }
    }

    pub(crate) fn push_exec_stdin_row(&mut self, session_id: u64, chars: &str) {
        let mut row = TranscriptRow::with_title(
            TranscriptKind::Ran,
            format!("stdin {session_id}"),
            bounded_stdin_display(chars),
        );
        row.tool_name = Some("write_stdin".to_string());
        row.tool_elapsed = Some(Duration::ZERO);
        self.insert_evidence_row(row);
    }
}
