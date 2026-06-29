#[allow(unused_imports)]
pub(crate) use super::*;

impl<'a> FullscreenUi<'a> {
    pub(crate) fn apply_agent_session_start(&mut self, value: &Value) {
        let Some(child_session_id) = value
            .get("child_session_id")
            .or_else(|| value.get("child_thread_id"))
            .or_else(|| value.get("session_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return;
        };
        let tool_call_id = value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let index = tool_call_id
            .and_then(|id| self.tool_rows.get(&tool_id_key(id)).copied())
            .or_else(|| self.completed_agent_invocation_index(value, tool_call_id))
            .unwrap_or_else(|| {
                let mut row = TranscriptRow::with_title(
                    evidence_kind("spawn_agent"),
                    agent_session_start_title(value).unwrap_or_else(|| "spawn_agent".to_string()),
                    agent_child_status_text("Running", 0, None),
                );
                row.tool_name = Some("spawn_agent".to_string());
                row.tool_call_id = tool_call_id.map(str::to_string);
                row.agent_target = Some(child_session_id.to_string());
                row.tool_started = Some(Instant::now());
                self.insert_evidence_row(row)
            });
        let row = &mut self.transcript[index];
        row.tool_name = Some("spawn_agent".to_string());
        row.agent_target = Some(child_session_id.to_string());
        row.tool_call_id = tool_call_id.map(str::to_string);
        if let Some(title) = agent_session_start_title(value) {
            row.title = title;
        }
        if let Some(tool_call_id) = tool_call_id {
            self.tool_rows.insert(tool_id_key(tool_call_id), index);
        }
        self.remove_duplicate_agent_placeholders(index, value);
    }

    pub(crate) fn apply_agent_child_preview_event(
        &mut self,
        child_session_id: &str,
        event: &RunStreamEvent,
    ) -> bool {
        let Some(row) = self
            .transcript
            .iter_mut()
            .find(|row| row.agent_target.as_deref() == Some(child_session_id))
        else {
            return false;
        };
        let mut changed = false;
        match event {
            RunStreamEvent::ReasoningDelta { text } => {
                if append_agent_child_live_fragment(
                    &mut row.agent_child_live_text,
                    "Thinking",
                    text,
                ) {
                    changed = true;
                }
            }
            RunStreamEvent::ReasoningEnd => {}
            RunStreamEvent::ClarifyRequest(_) | RunStreamEvent::ClarifyResolved(_) => {}
            RunStreamEvent::Event(value) => {
                changed |= apply_agent_child_value_preview(row, value);
            }
            RunStreamEvent::Scoped { .. } => {}
        }
        if changed {
            refresh_agent_child_preview(row);
        }
        changed
    }
}
