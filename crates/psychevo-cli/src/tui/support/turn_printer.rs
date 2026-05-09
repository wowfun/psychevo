struct TurnPrinter {
    renderer: TuiRenderer,
    last_assistant_text: String,
    reasoning_active: bool,
    thinking_enabled: bool,
    debug: bool,
    run_provider: String,
    run_model: String,
    run_mode: String,
    context_limit: Option<u64>,
    tool_titles: BTreeMap<String, String>,
    pending_tool_keys: BTreeMap<String, String>,
    streaming_tool_message_seq: u64,
    streaming_tool_message_open: bool,
}

impl TurnPrinter {
    fn new(renderer: TuiRenderer, thinking_enabled: bool, debug: bool) -> Self {
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
        }
    }

    fn render_event(&mut self, event: &RunStreamEvent, out: &mut impl Write) -> io::Result<()> {
        match event {
            RunStreamEvent::ReasoningDelta { text } => {
                if self.thinking_enabled {
                    if !self.reasoning_active {
                        self.reasoning_active = true;
                        write!(out, "Thinking: ")?;
                    }
                    write!(out, "{}", self.renderer.dim(text))?;
                }
            }
            RunStreamEvent::ReasoningEnd => {
                if self.reasoning_active {
                    self.reasoning_active = false;
                    if self.thinking_enabled {
                        writeln!(out)?;
                    }
                }
            }
            RunStreamEvent::Event(value) => self.render_value_event(value, out)?,
        }
        out.flush()
    }

    fn render_value_event(&mut self, value: &Value, out: &mut impl Write) -> io::Result<()> {
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
            "message_update" => {
                self.render_streaming_tool_calls(value, out)?;
                if let Some(text) = assistant_text_from_event(value) {
                    self.last_assistant_text = text;
                }
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
                    failures: 0,
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
                let title = match evidence_kind(tool) {
                    TranscriptKind::Explored | TranscriptKind::Ran | TranscriptKind::Changed => {
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

    fn render_streaming_tool_calls(
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

    fn finish(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.reasoning_active {
            writeln!(out)?;
            self.reasoning_active = false;
        }
        out.flush()
    }
}
