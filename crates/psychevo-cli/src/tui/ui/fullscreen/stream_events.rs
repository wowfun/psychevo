#[allow(unused_imports)]
pub(crate) use super::*;
impl<'a> FullscreenUi<'a> {
    pub(crate) fn apply_stream_event(
        &mut self,
        event: RunStreamEvent,
        thinking_visible: bool,
        debug: bool,
    ) -> bool {
        match event {
            RunStreamEvent::ReasoningDelta { text } => {
                if !text.trim().is_empty() {
                    self.turn_had_reasoning = true;
                    self.remove_turn_meta();
                }
                let idx = self.reasoning_row.unwrap_or_else(|| {
                    let mut row = TranscriptRow::with_title(
                        TranscriptKind::Thinking,
                        "Thinking",
                        String::new(),
                    );
                    row.tool_started = Some(Instant::now());
                    let idx = self.insert_evidence_row(row);
                    self.reasoning_row = Some(idx);
                    idx
                });
                self.append_thinking_text(idx, &text);
                let reasoning = self.thinking_full_text(idx);
                thinking_visible && self.apply_visible_tool_intent(&reasoning)
            }
            RunStreamEvent::ReasoningEnd => {
                if let Some(idx) = self.reasoning_row.take() {
                    self.finish_thinking_row(idx);
                }
                false
            }
            RunStreamEvent::ClarifyRequest(request) => {
                self.open_clarify_panel(request);
                true
            }
            RunStreamEvent::ClarifyResolved(event) => {
                self.apply_clarify_resolved(event);
                false
            }
            RunStreamEvent::Event(value) => self.apply_value_event(value.as_value(), debug),
            RunStreamEvent::Scoped { event, .. } => {
                self.apply_stream_event(*event, thinking_visible, debug)
            }
        }
    }

    pub(crate) fn apply_stream_event_for_session(
        &mut self,
        event: RunStreamEvent,
        thinking_visible: bool,
        debug: bool,
        session_id: Option<&str>,
    ) -> bool {
        let previous = self.active_event_session_id.clone();
        if let Some(session_id) = session_id {
            self.active_event_session_id = Some(session_id.to_string());
        }
        let result = self.apply_stream_event(event, thinking_visible, debug);
        self.active_event_session_id = previous;
        result
    }

    pub(crate) fn apply_value_event(&mut self, value: &Value, debug: bool) -> bool {
        match value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "run_start" => {
                let now = Instant::now();
                self.turn_started = Some(now);
                self.turn_session_id = value
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if self.visible_turn_started.is_none() {
                    self.visible_turn_started = Some(now);
                }
                self.turn_provider = value
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("provider")
                    .to_string();
                self.turn_model = value
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("model")
                    .to_string();
                self.turn_mode = value
                    .get("mode")
                    .and_then(Value::as_str)
                    .unwrap_or("default")
                    .to_string();
                self.turn_context_limit = value.get("context_limit").and_then(Value::as_u64);
                self.sidebar_context_limit = self.turn_context_limit;
                if let Some(skills) = selected_skill_names_from_event(value)
                    && !skills.is_empty()
                {
                    self.push_turn_start_status(format!("skill loaded: {}", skills.join(", ")));
                }
                false
            }
            "warning" => {
                if let Some(message) = value.get("message").and_then(Value::as_str) {
                    self.push_status(format!("warning: {message}"));
                }
                if let Some(suggestion) = value.get("suggestion").and_then(Value::as_str) {
                    self.push_status(format!("suggestion: {suggestion}"));
                }
                false
            }
            "action_requested" => {
                if let Some(request) = clarify_request_from_action_value(value) {
                    self.open_clarify_panel(request);
                    return true;
                }
                false
            }
            "action_resolved" | "action_cancelled" => {
                if let Some(event) = clarify_resolved_from_action_value(value) {
                    self.apply_clarify_resolved(event);
                }
                false
            }
            "message_update" | "message_end" => {
                let event_type = value.get("type").and_then(Value::as_str);
                if event_type == Some("message_end") && self.commit_pending_steer_from_event(value)
                {
                    return false;
                }
                let mut active_tool_frame_requested = false;
                let message = value.get("message");
                let is_tool_call_preamble = message.is_some_and(assistant_message_has_tool_calls)
                    || message
                        .and_then(|message| message.get("finish_reason"))
                        .and_then(Value::as_str)
                        == Some("tool_calls");
                if let Some(text) =
                    assistant_text_from_event(value).filter(|text| !text.trim().is_empty())
                {
                    if is_tool_call_preamble {
                        self.apply_assistant_preamble_text(
                            text.clone(),
                            event_type == Some("message_end"),
                        );
                    } else {
                        if let Some(idx) = self.reasoning_row.take() {
                            self.finish_thinking_row(idx);
                        }
                        let idx = self.assistant_row.unwrap_or_else(|| {
                            let idx = self.insert_answer_row(TranscriptRow::with_title(
                                TranscriptKind::Answer,
                                "",
                                String::new(),
                            ));
                            self.assistant_row = Some(idx);
                            idx
                        });
                        self.transcript[idx].text = text.clone();
                        self.remove_turn_meta();
                        if event_type == Some("message_update") {
                            active_tool_frame_requested |= self.apply_visible_tool_intent(&text);
                        }
                    }
                }
                active_tool_frame_requested |= self.apply_streaming_tool_calls(value);
                if event_type == Some("message_end") {
                    let matched_tools = streaming_tool_calls_from_event(value)
                        .into_iter()
                        .map(|call| call.tool_name)
                        .collect::<Vec<_>>();
                    self.remove_unmatched_provisional_tool_intents(&matched_tools);
                    self.turn_usage = value.get("usage").cloned();
                    if let Some(tokens) = self.turn_usage.as_ref().and_then(usage_context_tokens) {
                        self.sidebar_tokens = Some(tokens);
                    }
                    self.turn_metadata = value.get("metadata").cloned();
                    self.turn_accounting = value.get("accounting").cloned();
                    let turn_accounting = self.turn_accounting.clone();
                    self.add_sidebar_cost(turn_accounting.as_ref());
                    let message = value.get("message");
                    let allow_visible_answer_meta =
                        message.is_some_and(visible_answer_message_receives_meta);
                    let allow_reasoning_only_meta =
                        message.is_some_and(reasoning_only_message_receives_meta);
                    if allow_visible_answer_meta {
                        self.turn_terminal_visible_answer = true;
                    }
                    self.update_turn_meta(
                        debug,
                        allow_visible_answer_meta,
                        allow_reasoning_only_meta,
                        false,
                    );
                    if value
                        .get("message")
                        .and_then(|message| message.get("role"))
                        .and_then(Value::as_str)
                        == Some("assistant")
                    {
                        self.assistant_row = None;
                        self.assistant_preamble_row = None;
                    }
                }
                active_tool_frame_requested
            }
            "tool_call_pending" => self.apply_streaming_tool_calls(value),
            "agent_session_start" => {
                self.apply_agent_session_start(value);
                false
            }
            "exec_session_yielded" => {
                let Some(session_id) = value.get("session_id").and_then(Value::as_u64) else {
                    return false;
                };
                let tool_call_id = value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let idx = self
                    .tool_rows
                    .get(&tool_id_key(tool_call_id))
                    .copied()
                    .unwrap_or_else(|| {
                        let mut tool_value = serde_json::json!({
                            "args": { "cmd": value.get("cmd").cloned().unwrap_or(Value::Null) }
                        });
                        if value.get("source").and_then(Value::as_str) == Some("user_shell")
                            && let Some(object) = tool_value.as_object_mut()
                        {
                            object.insert(
                                "source".to_string(),
                                Value::String("user_shell".to_string()),
                            );
                        }
                        let mut row = TranscriptRow::with_title(
                            TranscriptKind::Ran,
                            active_tool_title("exec_command", &tool_value),
                            "running",
                        );
                        row.tool_name = Some("exec_command".to_string());
                        row.tool_call_id =
                            (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                        self.insert_evidence_row(row)
                    });
                let row = &mut self.transcript[idx];
                row.kind = TranscriptKind::Ran;
                row.tool_name = Some("exec_command".to_string());
                row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                row.failed = false;
                row.interrupted = false;
                row.tool_elapsed = None;
                row.tool_started = Some(tool_started_instant(value));
                self.exec_session_rows.insert(session_id, idx);
                if !tool_call_id.is_empty() {
                    self.tool_rows.insert(tool_id_key(tool_call_id), idx);
                }
                true
            }
            "exec_session_output_delta" => {
                let Some(session_id) = value.get("session_id").and_then(Value::as_u64) else {
                    return false;
                };
                let Some(output) = value.get("output").and_then(Value::as_str) else {
                    return false;
                };
                let Some(idx) = self.exec_session_rows.get(&session_id).copied() else {
                    return false;
                };
                self.append_exec_session_output(idx, output);
                true
            }
            "exec_session_stdin" => {
                let Some(session_id) = value.get("session_id").and_then(Value::as_u64) else {
                    return false;
                };
                let Some(chars) = value.get("chars").and_then(Value::as_str) else {
                    return false;
                };
                if self.exec_session_rows.contains_key(&session_id) {
                    self.push_exec_stdin_row(session_id, chars);
                    return true;
                }
                false
            }
            "exec_session_finished" => {
                let Some(session_id) = value.get("session_id").and_then(Value::as_u64) else {
                    return false;
                };
                let Some(idx) = self.exec_session_rows.get(&session_id).copied() else {
                    return false;
                };
                let elapsed = value
                    .get("elapsed_ms")
                    .and_then(Value::as_u64)
                    .map(Duration::from_millis);
                let interrupted = value
                    .get("interrupted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.finish_exec_session_row(session_id, idx, elapsed, interrupted, !interrupted);
                true
            }
            "tool_execution_start" => {
                let user_shell = value.get("source").and_then(Value::as_str) == Some("user_shell");
                let tool = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let tool_call_id = value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if !tool_call_id.is_empty()
                    && let Some(args) = value.get("args")
                    && !args.is_null()
                {
                    self.live_tool_args
                        .insert(tool_call_id.clone(), args.clone());
                }
                if tool == "clarify" {
                    if !tool_call_id.is_empty()
                        && let Some(args) = value.get("args")
                        && !args.is_null()
                    {
                        self.clarify_tool_args
                            .insert(tool_call_id.clone(), args.clone());
                    }
                    return false;
                }
                if tool == "write_stdin"
                    && let Some(args) = value.get("args")
                    && let Some(session_id) = exec_session_id_from_args(args)
                    && self.exec_session_rows.contains_key(&session_id)
                {
                    self.remove_streaming_tool_call_row(tool, &tool_call_id, None);
                    return false;
                }
                let id_key = (!tool_call_id.is_empty()).then(|| tool_id_key(&tool_call_id));
                let position_key =
                    event_scoped_tool_position_key(self.streaming_tool_message_seq, value);
                let write_preview = (tool == "write")
                    .then(|| value.get("args").and_then(write_argument_preview_from_args))
                    .flatten();
                let idx = id_key
                    .as_ref()
                    .and_then(|key| self.tool_rows.get(key))
                    .copied()
                    .or_else(|| {
                        position_key
                            .as_ref()
                            .and_then(|key| self.tool_rows.get(key))
                            .copied()
                    })
                    .or_else(|| {
                        (tool != "spawn_agent").then(|| {
                            self.matching_agent_placeholder_index(tool, value, &tool_call_id)
                        })?
                    })
                    .unwrap_or_else(|| {
                        let mut row = TranscriptRow::with_title(
                            evidence_kind_for_value(tool, value),
                            active_tool_title(tool, value),
                            "running",
                        );
                        row.tool_call_id =
                            (!tool_call_id.is_empty()).then_some(tool_call_id.clone());
                        row.tool_name = Some(tool.to_string());
                        row.tool_started = Some(tool_started_instant(value));
                        self.insert_evidence_row(row)
                    });
                if tool == "write"
                    && self
                        .transcript
                        .get(idx)
                        .is_some_and(TranscriptRow::is_terminal_write_row)
                {
                    return false;
                }
                self.remove_turn_meta();
                let row = &mut self.transcript[idx];
                row.kind = evidence_kind_for_value(tool, value);
                row.tool_name = Some(tool.to_string());
                row.title = active_tool_title(tool, value);
                if tool == "spawn_agent" {
                    row.text = agent_child_status_text("Running", 0, None);
                    row.full_text = running_agent_tool_full_text(value);
                    row.agent_child_tool_uses = 0;
                    row.agent_child_latest_tokens = None;
                    row.agent_child_live_text.clear();
                } else if let Some(preview) = write_preview {
                    row.set_write_argument_preview(preview, "writing", None);
                } else if tool == "write" && row.write_argument_preview.is_some() {
                    row.refresh_write_argument_preview("writing", None);
                } else {
                    row.text = "running".to_string();
                }
                row.failed = false;
                row.interrupted = false;
                row.user_shell = user_shell;
                row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.clone());
                if row.tool_started.is_none() {
                    row.tool_started = Some(tool_started_instant(value));
                }
                if let Some(id_key) = id_key {
                    self.tool_rows.insert(id_key, idx);
                }
                if let Some(position_key) = position_key {
                    self.tool_rows.insert(position_key, idx);
                }
                if tool == "spawn_agent" {
                    self.remove_duplicate_agent_placeholders_for_tool_value(idx, value);
                }
                true
            }
            "tool_execution_end" => {
                let user_shell = value.get("source").and_then(Value::as_str) == Some("user_shell");
                let tool = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let tool_call_id = value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let cached_value;
                let value = if tool == "clarify" {
                    cached_value = self.value_with_cached_clarify_args(value, tool_call_id);
                    &cached_value
                } else if tool == "spawn_agent"
                    && value.get("args").is_none_or(Value::is_null)
                    && let Some(args) = self.live_tool_args.get(tool_call_id).cloned()
                {
                    cached_value = value_with_args(value, args);
                    &cached_value
                } else {
                    value
                };
                let outcome = value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .unwrap_or("normal");
                let interrupted = tool_event_interrupted(value);
                let user_confirmed_interrupt = interrupted && self.interrupt_requested;
                let clarify_no_answer = tool == "clarify" && clarify_no_answer_result(value);
                let failed = outcome != "normal" && !interrupted && !clarify_no_answer;
                if tool == "write_stdin"
                    && let Some(args) = self
                        .live_tool_args
                        .remove(tool_call_id)
                        .or_else(|| value.get("args").cloned())
                    && let Some(session_id) = exec_session_id_from_args(&args)
                    && let Some(idx) = self.exec_session_rows.get(&session_id).copied()
                    && !failed
                    && !interrupted
                {
                    self.remove_streaming_tool_call_row(tool, tool_call_id, None);
                    if exec_result_completed(value) {
                        let elapsed = self
                            .transcript
                            .get(idx)
                            .and_then(|row| completed_live_tool_elapsed(row, Some(value)));
                        self.finish_exec_session_row(session_id, idx, elapsed, false, false);
                    }
                    return false;
                }
                if tool == "exec_command"
                    && let Some(session_id) = exec_session_id_from_result(value)
                    && exec_result_running(value)
                {
                    let idx = self
                        .tool_rows
                        .get(&tool_id_key(tool_call_id))
                        .copied()
                        .or_else(|| self.exec_session_rows.get(&session_id).copied())
                        .unwrap_or_else(|| {
                            let mut row = TranscriptRow::with_title(
                                evidence_kind_for_value(tool, value),
                                tool_title(tool, value),
                                String::new(),
                            );
                            row.tool_name = Some(tool.to_string());
                            row.tool_call_id =
                                (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                            self.insert_evidence_row(row)
                        });
                    let already_finished = self.transcript.get(idx).is_some_and(|row| {
                        row.tool_started.is_none() && row.tool_elapsed.is_some()
                    });
                    {
                        let row = &mut self.transcript[idx];
                        row.kind = evidence_kind_for_value(tool, value);
                        row.tool_name = Some(tool.to_string());
                        row.title = tool_title_for_update(tool, value, &row.title);
                        row.failed = false;
                        row.interrupted = false;
                        row.user_shell = user_shell;
                        row.tool_call_id =
                            (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                        if !already_finished && row.tool_started.is_none() {
                            row.tool_started = Some(tool_started_instant(value));
                        }
                        if !already_finished {
                            row.tool_elapsed = None;
                        }
                    }
                    self.prefix_exec_session_output_if_needed(idx, tool_result_output(value));
                    self.exec_session_rows.insert(session_id, idx);
                    if !already_finished && !tool_call_id.is_empty() {
                        self.tool_rows.insert(tool_id_key(tool_call_id), idx);
                    }
                    self.live_tool_args.remove(tool_call_id);
                    return !already_finished;
                }
                if outcome != "normal" && !user_shell && !interrupted && !clarify_no_answer {
                    self.turn_failures += 1;
                }
                if user_confirmed_interrupt {
                    self.turn_interrupted = true;
                }
                let id_key = (!tool_call_id.is_empty()).then(|| tool_id_key(tool_call_id));
                let position_key =
                    event_scoped_tool_position_key(self.streaming_tool_message_seq, value);
                let idx = id_key
                    .as_ref()
                    .and_then(|key| self.tool_rows.get(key))
                    .copied()
                    .or_else(|| {
                        position_key
                            .as_ref()
                            .and_then(|key| self.tool_rows.get(key))
                            .copied()
                    })
                    .or_else(|| {
                        (tool != "spawn_agent").then(|| {
                            self.matching_agent_placeholder_index(tool, value, tool_call_id)
                        })?
                    })
                    .unwrap_or_else(|| {
                        let mut row = TranscriptRow::with_title(
                            evidence_kind_for_value(tool, value),
                            tool_title(tool, value),
                            String::new(),
                        );
                        row.tool_name = Some(tool.to_string());
                        self.insert_evidence_row(row)
                    });
                let row = &mut self.transcript[idx];
                row.kind = evidence_kind_for_value(tool, value);
                row.tool_name = Some(tool.to_string());
                row.title = tool_title_for_update(tool, value, &row.title);
                row.failed = failed;
                row.interrupted = interrupted;
                row.user_shell = user_shell;
                row.tool_elapsed = completed_live_tool_elapsed(row, Some(value));
                row.tool_started = None;
                row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.to_string());
                if tool == "spawn_agent" {
                    if let Some(agent_target) = agent_target_from_tool_event(value) {
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
                    let (collapsed, full) = tool_output_text(value);
                    row.text = if collapsed.is_empty() {
                        format_tool_summary(value)
                    } else {
                        collapsed
                    };
                    row.full_text = full;
                }
                if tool == "write" {
                    if failed || interrupted {
                        if row.write_argument_preview.is_none()
                            && let Some(preview) =
                                value.get("args").and_then(write_argument_preview_from_args)
                        {
                            row.write_argument_preview = Some(preview);
                        }
                        let terminal_detail =
                            row.full_text.clone().unwrap_or_else(|| row.text.clone());
                        let phase = if interrupted { "cancelled" } else { "failed" };
                        row.refresh_write_argument_preview(phase, Some(&terminal_detail));
                    } else {
                        row.clear_write_argument_preview_after_success();
                    }
                }
                if is_write_like_tool(tool) {
                    self.remove_orphan_provisional_tool_intents(tool, Some(idx));
                }
                if tool == "spawn_agent" {
                    self.remove_duplicate_agent_placeholders_for_tool_value(idx, value);
                }
                if let Some(id_key) = id_key {
                    self.tool_rows.insert(id_key, idx);
                }
                if let Some(position_key) = position_key {
                    self.tool_rows.insert(position_key, idx);
                }
                if tool == "clarify" {
                    self.clarify_tool_args.remove(tool_call_id);
                }
                self.live_tool_args.remove(tool_call_id);
                false
            }
            "agent_end" => {
                let outcome = outcome_from_value(value);
                if self.interrupt_requested && outcome == Some(Outcome::Aborted) {
                    self.turn_interrupted = true;
                }
                self.turn_outcome = outcome;
                self.turn_terminal_message = value
                    .get("terminal_message")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.update_turn_meta(debug, false, false, true);
                false
            }
            _ => false,
        }
    }
}

fn clarify_request_from_action_value(value: &Value) -> Option<ClarifyRequestEvent> {
    if value.get("kind").and_then(Value::as_str) != Some("clarify") {
        return None;
    }
    let payload = value.get("payload")?.clone();
    let request = payload.get("raw").cloned().unwrap_or(payload);
    serde_json::from_value(request).ok()
}

fn clarify_resolved_from_action_value(value: &Value) -> Option<ClarifyResolvedEvent> {
    if value.get("kind").and_then(Value::as_str) != Some("clarify") {
        return None;
    }
    let call_id = value
        .get("action_id")
        .or_else(|| value.get("actionId"))
        .and_then(Value::as_str)?
        .to_string();
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .or_else(|| value.get("outcome").and_then(Value::as_str))
        .map(clarify_resolved_reason_from_str)
        .unwrap_or(ClarifyResolvedReason::TurnFinished);
    Some(ClarifyResolvedEvent { call_id, reason })
}

fn clarify_resolved_reason_from_str(value: &str) -> ClarifyResolvedReason {
    match value {
        "answered" | "accepted" => ClarifyResolvedReason::Answered,
        "cancelled" | "canceled" => ClarifyResolvedReason::Cancelled,
        "timed_out" | "timedOut" => ClarifyResolvedReason::TimedOut,
        _ => ClarifyResolvedReason::TurnFinished,
    }
}

fn value_with_args(value: &Value, args: Value) -> Value {
    let mut merged = value.clone();
    if let Some(object) = merged.as_object_mut() {
        object.insert("args".to_string(), args);
    }
    merged
}

pub(super) fn event_scoped_tool_position_key(message_scope: u64, value: &Value) -> Option<String> {
    let content_index = value
        .get("content_index")
        .or_else(|| value.get("content_array_index"))
        .and_then(Value::as_u64)?;
    let call_index = value.get("call_index").and_then(Value::as_u64)?;
    Some(scoped_tool_position_key(
        message_scope,
        &tool_position_key(content_index, call_index),
    ))
}

#[path = "stream_events/agent_preview.rs"]
mod agent_preview;
#[path = "stream_events/clarify.rs"]
mod clarify;
#[path = "stream_events/streaming_tool_calls.rs"]
mod streaming_tool_calls;
#[path = "stream_events/transcript_rows.rs"]
mod transcript_rows;
