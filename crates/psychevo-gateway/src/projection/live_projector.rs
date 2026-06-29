impl GatewayLiveProjector {
    pub fn new(thread_id: Option<String>) -> Self {
        Self {
            thread_id,
            active_turn_id: None,
            assistant_segment: 0,
            stream_seq: 0,
            entries: BTreeMap::new(),
            tool_owners: BTreeMap::new(),
            tool_aliases: BTreeMap::new(),
            tool_positions: BTreeMap::new(),
            tool_args: BTreeMap::new(),
            exec_sessions: BTreeMap::new(),
            child_projectors: BTreeMap::new(),
        }
    }

    pub fn project(&mut self, turn_id: &str, event: &RunStreamEvent) -> Option<GatewayEvent> {
        if let RunStreamEvent::Scoped { session_id, event } = event {
            return self.project_scoped(turn_id, session_id, event);
        }
        self.prepare_turn(turn_id);
        let mut event = match event {
            RunStreamEvent::ReasoningDelta { text } => {
                self.project_reasoning_delta(turn_id, text)?
            }
            RunStreamEvent::ReasoningEnd => self.project_reasoning_end(turn_id)?,
            RunStreamEvent::Event(value) => self
                .project_runtime_value(turn_id, value)
                .or_else(|| gateway_event_from_runtime_value(turn_id, value))?,
            _ => gateway_event_from_run_stream(turn_id, event)?,
        };
        let turn_completed = matches!(event, GatewayEvent::TurnCompleted { .. });
        self.attach_thread_id(&mut event);
        if turn_completed {
            self.reset_turn_state();
        }
        Some(event)
    }

    fn project_scoped(
        &mut self,
        turn_id: &str,
        session_id: &str,
        event: &RunStreamEvent,
    ) -> Option<GatewayEvent> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return None;
        }
        let nested_scoped = matches!(event, RunStreamEvent::Scoped { .. });
        let child = self
            .child_projectors
            .entry(session_id.to_string())
            .or_insert_with(|| GatewayLiveProjector::new(Some(session_id.to_string())));
        let mut projected = child.project(turn_id, event)?;
        if !nested_scoped {
            force_event_thread_id(&mut projected, session_id);
        }
        Some(projected)
    }

    fn project_runtime_value(&mut self, turn_id: &str, value: &Value) -> Option<GatewayEvent> {
        match value.get("type").and_then(Value::as_str) {
            Some("message_update")
                if runtime_message_role(value.get("message")) == Some("assistant") =>
            {
                self.project_assistant_message_event(
                    turn_id,
                    value,
                    TranscriptBlockStatus::Running,
                    false,
                )
            }
            Some("message_end")
                if runtime_message_role(value.get("message")) == Some("assistant") =>
            {
                let event = self.project_assistant_message_event(
                    turn_id,
                    value,
                    TranscriptBlockStatus::Completed,
                    true,
                );
                self.advance_assistant_segment();
                event
            }
            Some("agent_message") => {
                let text = value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToString::to_string)?;
                let segment = self.assistant_segment;
                let block = live_block(
                    live_text_block_id(turn_id, segment, 0),
                    TranscriptBlockKind::Text,
                    TranscriptBlockStatus::Completed,
                    DEFAULT_TEXT_ORDER,
                    None,
                    Some(text),
                    None,
                );
                self.upsert_block(segment, block);
                let event = self.emit_entry_event(turn_id, segment, true, true);
                self.advance_assistant_segment();
                Some(event)
            }
            Some(
                "tool_call_pending"
                | "tool_execution_start"
                | "tool_execution_update"
                | "tool_execution_end",
            ) => self.project_tool_event(turn_id, value),
            Some("agent_session_start") => self.project_agent_session_start(turn_id, value),
            Some("exec_session_output_delta" | "exec_session_finished") => {
                self.project_exec_session_event(turn_id, value)
            }
            Some("acp_peer_plan") => self.project_acp_peer_plan(turn_id, value),
            _ => None,
        }
    }

    fn project_acp_peer_plan(&mut self, turn_id: &str, value: &Value) -> Option<GatewayEvent> {
        let body = value
            .get("body")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|body| !body.is_empty())
            .map(ToString::to_string)?;
        let segment = self.assistant_segment;
        let block = live_block(
            format!("live:{turn_id}:assistant:{segment}:acp-peer-plan"),
            TranscriptBlockKind::Status,
            TranscriptBlockStatus::Running,
            DEFAULT_TEXT_ORDER + 10,
            Some("Plan".to_string()),
            Some(body),
            Some(json!({
                "projection": "acp_peer_plan",
                "origin": "acp_peer",
                "source": "acp_peer",
                "plan": value.get("plan").cloned().unwrap_or(Value::Null),
            })),
        );
        self.upsert_block(segment, block);
        Some(self.emit_entry_event(turn_id, segment, false, false))
    }
}
