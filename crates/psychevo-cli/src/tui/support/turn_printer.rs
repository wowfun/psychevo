use crate::tui::support_turn_event::turn_event_presentation_value;
use crate::tui::{
    plain::{TuiRenderer, assistant_text_from_event},
    support_evidence::{
        TurnMetaProjection, active_tool_title, assistant_message_stream_event_type,
        evidence_kind_for_value, format_duration_compact, format_tool_result_summary,
        format_tool_summary, scoped_tool_position_key, streaming_tool_calls_from_event,
        tool_id_key, tool_title_for_update, turn_meta_text, user_shell_title,
    },
    support_history::metadata_elapsed_duration,
    ui_fullscreen::shell_outcome_label,
    ui_types::TranscriptKind,
};
use psychevo::{ShellCommandEvent, ShellCommandOutcome, TurnEvent, application::ToolDisplaySpec};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{self, Write},
    time::Duration,
};

pub(crate) struct TurnPrinter {
    pub(crate) renderer: TuiRenderer,
    pub(crate) last_assistant_text: String,
    pub(crate) reasoning_active: bool,
    pub(crate) thinking_enabled: bool,
    pub(crate) debug: bool,
    pub(crate) run_provider: String,
    pub(crate) run_model: String,
    pub(crate) run_mode: String,
    pub(crate) context_limit: Option<u64>,
    pub(crate) tool_titles: BTreeMap<String, String>,
    pub(crate) pending_tool_keys: BTreeMap<String, String>,
    pub(crate) streaming_tool_message_seq: u64,
    pub(crate) streaming_tool_message_open: bool,
    pub(crate) projection_invalid: bool,
}

impl TurnPrinter {
    pub(crate) fn new(renderer: TuiRenderer, thinking_enabled: bool, debug: bool) -> Self {
        Self {
            renderer,
            last_assistant_text: String::new(),
            reasoning_active: false,
            thinking_enabled,
            debug,
            run_provider: String::new(),
            run_model: String::new(),
            run_mode: String::new(),
            context_limit: None,
            tool_titles: BTreeMap::new(),
            pending_tool_keys: BTreeMap::new(),
            streaming_tool_message_seq: 0,
            streaming_tool_message_open: false,
            projection_invalid: false,
        }
    }

    pub(crate) fn render_event(
        &mut self,
        event: &TurnEvent,
        out: &mut impl Write,
    ) -> io::Result<()> {
        if self.projection_invalid {
            match event {
                TurnEvent::Scoped { event, .. } => return self.render_event(event, out),
                TurnEvent::ResyncRequired { .. } => {}
                _ => return out.flush(),
            }
        }
        match event {
            TurnEvent::MessageDelta { text } => {
                self.last_assistant_text.push_str(text);
            }
            TurnEvent::ReasoningDelta { text } => {
                if self.thinking_enabled {
                    if !self.reasoning_active {
                        self.reasoning_active = true;
                        write!(out, "Thinking: ")?;
                    }
                    write!(out, "{}", self.renderer.dim(text))?;
                }
            }
            TurnEvent::ReasoningCompleted { text } => {
                if !self.reasoning_active
                    && self.thinking_enabled
                    && let Some(text) = text.as_deref().filter(|text| !text.trim().is_empty())
                {
                    write!(out, "Thinking: {}", self.renderer.dim(text))?;
                    self.reasoning_active = true;
                }
                if self.reasoning_active {
                    self.reasoning_active = false;
                    if self.thinking_enabled {
                        writeln!(out)?;
                    }
                }
            }
            event @ (TurnEvent::Runtime { .. }
            | TurnEvent::Message { .. }
            | TurnEvent::Tool { .. }
            | TurnEvent::Warning { .. }) => {
                if let Some(value) = turn_event_presentation_value(event) {
                    self.render_value_event(value.as_ref(), out)?;
                }
            }
            TurnEvent::InteractionRequested { .. } | TurnEvent::InteractionResolved { .. } => {}
            TurnEvent::Scoped { event, .. } => self.render_event(event, out)?,
            TurnEvent::ResyncRequired { missed } => {
                self.projection_invalid = true;
                self.last_assistant_text.clear();
                self.reasoning_active = false;
                self.tool_titles.clear();
                self.pending_tool_keys.clear();
                self.streaming_tool_message_seq = 0;
                self.streaming_tool_message_open = false;
                writeln!(
                    out,
                    "{}",
                    self.renderer
                        .status(&format!("warning: missed {missed} live turn events"))
                )?;
            }
            TurnEvent::ActivityChanged { .. }
            | TurnEvent::Accepted { .. }
            | TurnEvent::Started { .. }
            | TurnEvent::Completed { .. }
            | TurnEvent::Failed { .. } => {}
        }
        out.flush()
    }

    pub(crate) fn render_shell_event(
        &mut self,
        event: &ShellCommandEvent,
        out: &mut impl Write,
    ) -> io::Result<()> {
        match event {
            ShellCommandEvent::Started { command, .. } => {
                let title = user_shell_title(command.lines().find(|line| !line.trim().is_empty()));
                self.tool_titles
                    .insert("user_shell".to_string(), title.clone());
                writeln!(out, "{title}: running")?;
            }
            ShellCommandEvent::Completed {
                output,
                outcome,
                elapsed_ms,
                ..
            } => {
                let title = self
                    .tool_titles
                    .get("user_shell")
                    .map(String::as_str)
                    .unwrap_or("!");
                let display = ToolDisplaySpec::for_name("exec_command");
                let summary = format_tool_result_summary(
                    "exec_command",
                    shell_outcome_label(*outcome),
                    output,
                    &display,
                );
                let elapsed = format!(
                    " {}",
                    format_duration_compact(Duration::from_millis(*elapsed_ms))
                );
                if *outcome == ShellCommandOutcome::Completed {
                    writeln!(
                        out,
                        "{}",
                        self.renderer
                            .success(&format!("{title}{elapsed}: {summary}"))
                    )?;
                } else {
                    writeln!(
                        out,
                        "{}",
                        self.renderer
                            .error(&format!("{title}{elapsed}: failed {summary}"))
                    )?;
                }
            }
            ShellCommandEvent::Warning { message, .. } => {
                writeln!(
                    out,
                    "{}",
                    self.renderer.status(&format!("warning: {message}"))
                )?;
            }
        }
        out.flush()
    }

    pub(crate) fn render_value_event(
        &mut self,
        value: &Value,
        out: &mut impl Write,
    ) -> io::Result<()> {
        match value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "run_start" => {
                self.run_provider = value
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("provider")
                    .to_string();
                self.run_model = value
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("model")
                    .to_string();
                self.run_mode = value
                    .get("mode")
                    .and_then(Value::as_str)
                    .unwrap_or("default")
                    .to_string();
                self.context_limit = value.get("context_limit").and_then(Value::as_u64);
                self.tool_titles.clear();
                self.pending_tool_keys.clear();
                self.streaming_tool_message_seq = 0;
                self.streaming_tool_message_open = false;
            }
            "warning" => {
                if let Some(message) = value.get("message").and_then(Value::as_str) {
                    writeln!(
                        out,
                        "{}",
                        self.renderer.status(&format!("warning: {message}"))
                    )?;
                }
                if let Some(suggestion) = value.get("suggestion").and_then(Value::as_str) {
                    writeln!(
                        out,
                        "{}",
                        self.renderer.dim(&format!("suggestion: {suggestion}"))
                    )?;
                }
            }
            "message_update" => {
                self.render_streaming_tool_calls(value, out)?;
                if let Some(text) = assistant_text_from_event(value) {
                    self.last_assistant_text = text;
                }
            }
            "tool_call_pending" => {
                self.render_streaming_tool_calls(value, out)?;
            }
            "message_end" => {
                self.render_streaming_tool_calls(value, out)?;
                if let Some(text) = assistant_text_from_event(value) {
                    self.last_assistant_text = text.clone();
                    if !text.trim().is_empty() {
                        writeln!(out, "Answer:\n{text}")?;
                    }
                }
                let meta = turn_meta_text(TurnMetaProjection {
                    mode: &self.run_mode,
                    provider: &self.run_provider,
                    model: &self.run_model,
                    started: None,
                    usage: value.get("usage"),
                    metadata: value.get("metadata"),
                    accounting: value.get("accounting"),
                    failures: 0,
                    interrupted: false,
                    debug: self.debug,
                });
                if !meta.is_empty() {
                    writeln!(out, "Meta: {meta}")?;
                }
            }
            "tool_execution_start" => {
                let tool = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let title = active_tool_title(tool, value);
                let mut already_announced = false;
                if let Some(tool_call_id) = value.get("tool_call_id").and_then(Value::as_str) {
                    let key = tool_id_key(tool_call_id);
                    already_announced = self.pending_tool_keys.contains_key(&key);
                    self.tool_titles
                        .insert(tool_call_id.to_string(), title.clone());
                    self.pending_tool_keys.insert(key, title.clone());
                }
                if !already_announced {
                    writeln!(out, "{title}: running")?;
                }
            }
            "tool_execution_end" => {
                let outcome = value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .unwrap_or("normal");
                let summary = format_tool_summary(value);
                let tool = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let existing_title = value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .and_then(|tool_call_id| self.tool_titles.get(tool_call_id))
                    .map(String::as_str)
                    .unwrap_or("");
                let title = match evidence_kind_for_value(tool, value) {
                    TranscriptKind::Explored | TranscriptKind::Ran | TranscriptKind::Updated => {
                        tool_title_for_update(tool, value, existing_title)
                    }
                    _ => "Tool".to_string(),
                };
                let elapsed = metadata_elapsed_duration(Some(value))
                    .map(|elapsed| format!(" {}", format_duration_compact(elapsed)))
                    .unwrap_or_default();
                if outcome == "normal" {
                    writeln!(
                        out,
                        "{}",
                        self.renderer
                            .success(&format!("{title}{elapsed}: {summary}"))
                    )?;
                } else {
                    writeln!(
                        out,
                        "{}",
                        self.renderer
                            .error(&format!("{title}{elapsed}: failed {summary}"))
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn render_streaming_tool_calls(
        &mut self,
        value: &Value,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let Some(event_type) = assistant_message_stream_event_type(value) else {
            return Ok(());
        };
        if !self.streaming_tool_message_open {
            self.streaming_tool_message_seq = self.streaming_tool_message_seq.saturating_add(1);
            self.streaming_tool_message_open = true;
        }
        let message_scope = self.streaming_tool_message_seq;
        for mut call in streaming_tool_calls_from_event(value) {
            call.position_key = scoped_tool_position_key(message_scope, &call.position_key);
            let key = if let Some(id) = &call.id {
                let id_key = tool_id_key(id);
                if let Some(title) = self.pending_tool_keys.remove(&call.position_key) {
                    self.pending_tool_keys.insert(id_key.clone(), title);
                }
                id_key
            } else {
                call.position_key.clone()
            };
            let value = serde_json::json!({ "args": call.args });
            let title = active_tool_title(&call.tool_name, &value);
            if let Some(id) = &call.id {
                self.tool_titles.insert(id.clone(), title.clone());
            }
            if let std::collections::btree_map::Entry::Occupied(mut entry) =
                self.pending_tool_keys.entry(key.clone())
            {
                entry.insert(title);
                continue;
            }
            self.pending_tool_keys.insert(key, title.clone());
            writeln!(out, "{title}: preparing")?;
        }
        if event_type == "message_end" {
            self.streaming_tool_message_open = false;
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.reasoning_active {
            writeln!(out)?;
            self.reasoning_active = false;
        }
        out.flush()
    }

    pub(crate) fn needs_authoritative_reload(&self) -> bool {
        self.projection_invalid
    }

    pub(crate) fn finish_after_authoritative_reload(
        &mut self,
        authoritative_answer: &str,
        out: &mut impl Write,
    ) -> io::Result<()> {
        if self.projection_invalid {
            if !authoritative_answer.trim().is_empty() {
                writeln!(out, "Answer:\n{authoritative_answer}")?;
                self.last_assistant_text = authoritative_answer.to_string();
            }
            self.projection_invalid = false;
        }
        self.finish(out)
    }
}
