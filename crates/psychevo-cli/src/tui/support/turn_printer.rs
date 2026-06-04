#[allow(unused_imports)]
pub(crate) use super::*;
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
    pub(crate) gateway_reasoning_texts: BTreeMap<String, String>,
    pub(crate) streaming_tool_message_seq: u64,
    pub(crate) streaming_tool_message_open: bool,
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
            gateway_reasoning_texts: BTreeMap::new(),
            streaming_tool_message_seq: 0,
            streaming_tool_message_open: false,
        }
    }

    pub(crate) fn render_event(
        &mut self,
        event: &RunStreamEvent,
        out: &mut impl Write,
    ) -> io::Result<()> {
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
            RunStreamEvent::ClarifyRequest(_) | RunStreamEvent::ClarifyResolved(_) => {}
            RunStreamEvent::Scoped { event, .. } => self.render_event(event, out)?,
        }
        out.flush()
    }

    pub(crate) fn render_gateway_event(
        &mut self,
        event: &GatewayEvent,
        out: &mut impl Write,
    ) -> io::Result<()> {
        match event {
            GatewayEvent::EntryDelta { delta, .. } => {
                if self.thinking_enabled {
                    if !self.reasoning_active {
                        self.reasoning_active = true;
                        write!(out, "Thinking: ")?;
                    }
                    write!(out, "{}", self.renderer.dim(delta))?;
                }
            }
            GatewayEvent::EntryStarted { entry, .. }
            | GatewayEvent::EntryUpdated { entry, .. }
            | GatewayEvent::EntryCompleted { entry, .. } => {
                self.render_gateway_entry(entry, out)?;
            }
            GatewayEvent::Warning {
                message,
                suggestion,
                ..
            } => {
                writeln!(
                    out,
                    "{}",
                    self.renderer.status(&format!("warning: {message}"))
                )?;
                if let Some(suggestion) = suggestion {
                    writeln!(
                        out,
                        "{}",
                        self.renderer.dim(&format!("suggestion: {suggestion}"))
                    )?;
                }
            }
            GatewayEvent::TurnCompleted {
                committed_entries, ..
            } => {
                for entry in committed_entries {
                    self.render_gateway_entry(entry, out)?;
                }
            }
            GatewayEvent::TurnStarted { .. }
            | GatewayEvent::TurnQueued { .. }
            | GatewayEvent::PermissionRequested { .. }
            | GatewayEvent::PermissionResolved { .. }
            | GatewayEvent::ClarifyRequested { .. }
            | GatewayEvent::ClarifyResolved { .. } => {}
        }
        out.flush()
    }

    fn render_gateway_entry(
        &mut self,
        entry: &TranscriptEntry,
        out: &mut impl Write,
    ) -> io::Result<()> {
        for block in &entry.blocks {
            self.render_gateway_block(entry.role, block, out)?;
        }
        Ok(())
    }

    fn render_gateway_block(
        &mut self,
        role: TranscriptEntryRole,
        block: &TranscriptBlock,
        out: &mut impl Write,
    ) -> io::Result<()> {
        match (role, block.kind) {
            (_, TranscriptBlockKind::Reasoning) => {
                self.render_gateway_reasoning_block(block, out)?;
            }
            (TranscriptEntryRole::Assistant, TranscriptBlockKind::Text) => {
                if let Some(text) = block.body.as_deref().or(block.preview.as_deref()) {
                    self.last_assistant_text = text.to_string();
                    if block.status == TranscriptBlockStatus::Completed && !text.trim().is_empty() {
                        writeln!(out, "Answer:\n{text}")?;
                    }
                }
                if block.status == TranscriptBlockStatus::Completed {
                    self.render_gateway_block_meta(block, out)?;
                }
            }
            (TranscriptEntryRole::User, TranscriptBlockKind::Text) => {}
            _ => self.render_gateway_evidence_block(block, out)?,
        }
        Ok(())
    }

    fn render_gateway_reasoning_block(
        &mut self,
        block: &TranscriptBlock,
        out: &mut impl Write,
    ) -> io::Result<()> {
        if self.thinking_enabled {
            let text = block
                .body
                .as_deref()
                .or(block.preview.as_deref())
                .unwrap_or("");
            let previous = self
                .gateway_reasoning_texts
                .get(&block.id)
                .map(String::as_str)
                .unwrap_or("");
            let delta = text.strip_prefix(previous).unwrap_or(text);
            if !delta.is_empty() {
                if !self.reasoning_active {
                    self.reasoning_active = true;
                    write!(out, "Thinking: ")?;
                }
                write!(out, "{}", self.renderer.dim(delta))?;
                self.gateway_reasoning_texts
                    .insert(block.id.clone(), text.to_string());
            }
        }
        if matches!(
            block.status,
            TranscriptBlockStatus::Completed
                | TranscriptBlockStatus::Failed
                | TranscriptBlockStatus::Cancelled
        ) {
            self.gateway_reasoning_texts.remove(&block.id);
            if self.reasoning_active {
                self.reasoning_active = false;
                if self.thinking_enabled {
                    writeln!(out)?;
                }
            }
        }
        Ok(())
    }

    fn render_gateway_block_meta(
        &mut self,
        block: &TranscriptBlock,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let metadata = block.metadata.as_ref();
        let usage = metadata.and_then(|value| value.get("usage"));
        let response_metadata = metadata.and_then(|value| value.get("metadata"));
        let accounting = metadata.and_then(|value| value.get("accounting"));
        let meta = turn_meta_text(TurnMetaProjection {
            mode: &self.run_mode,
            provider: &self.run_provider,
            model: &self.run_model,
            started: None,
            usage,
            metadata: response_metadata,
            accounting,
            failures: 0,
            interrupted: false,
            debug: self.debug,
        });
        if !meta.is_empty() {
            writeln!(out, "Meta: {meta}")?;
        }
        Ok(())
    }

    fn render_gateway_evidence_block(
        &mut self,
        block: &TranscriptBlock,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let title = block
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(match block.kind {
                TranscriptBlockKind::Shell => "shell",
                TranscriptBlockKind::File => "file",
                TranscriptBlockKind::Web => "web",
                TranscriptBlockKind::Mcp => "mcp",
                TranscriptBlockKind::Clarify => "clarify",
                TranscriptBlockKind::Permission => "permission",
                TranscriptBlockKind::Skill => "skill",
                TranscriptBlockKind::Agent => "agent",
                TranscriptBlockKind::Mailbox => "mailbox",
                TranscriptBlockKind::Diff => "diff",
                TranscriptBlockKind::Artifact => "artifact",
                TranscriptBlockKind::Status => "status",
                TranscriptBlockKind::Tool | TranscriptBlockKind::ToolCall => "tool",
                TranscriptBlockKind::ToolResult => "result",
                TranscriptBlockKind::Text | TranscriptBlockKind::Reasoning => "item",
            });
        match block.status {
            TranscriptBlockStatus::Pending => writeln!(out, "{title}: preparing")?,
            TranscriptBlockStatus::Running => writeln!(out, "{title}: running")?,
            TranscriptBlockStatus::Completed | TranscriptBlockStatus::Info => {
                let summary = block
                    .body
                    .as_deref()
                    .or(block.preview.as_deref())
                    .unwrap_or("done");
                writeln!(
                    out,
                    "{}",
                    self.renderer.success(&format!("{title}: {summary}"))
                )?;
            }
            TranscriptBlockStatus::Failed
            | TranscriptBlockStatus::Cancelled
            | TranscriptBlockStatus::NeedsInput => {
                let summary = block
                    .body
                    .as_deref()
                    .or(block.preview.as_deref())
                    .unwrap_or("failed");
                writeln!(
                    out,
                    "{}",
                    self.renderer.error(&format!("{title}: {summary}"))
                )?;
            }
        }
        Ok(())
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
                self.gateway_reasoning_texts.clear();
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
}
