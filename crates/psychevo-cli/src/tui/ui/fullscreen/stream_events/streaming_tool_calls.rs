#[allow(unused_imports)]
pub(crate) use super::*;

impl<'a> FullscreenUi<'a> {
    pub(crate) fn apply_streaming_tool_calls(&mut self, value: &Value) -> bool {
        let Some(event_type) = assistant_message_stream_event_type(value) else {
            return false;
        };
        let calls = streaming_tool_calls_from_event(value)
            .into_iter()
            .filter(|call| call.tool_name != "clarify")
            .collect::<Vec<_>>();
        let reuse_last_scope = event_type == "tool_call_pending"
            && !self.streaming_tool_message_open
            && calls.iter().any(|call| {
                self.tool_rows.contains_key(&scoped_tool_position_key(
                    self.streaming_tool_message_seq,
                    &call.position_key,
                ))
            });
        if !self.streaming_tool_message_open && !reuse_last_scope {
            self.streaming_tool_message_seq = self.streaming_tool_message_seq.saturating_add(1);
        }
        self.streaming_tool_message_open = true;
        let message_scope = self.streaming_tool_message_seq;
        let mut active_tool_frame_requested = false;
        for mut call in calls {
            call.position_key = scoped_tool_position_key(message_scope, &call.position_key);
            active_tool_frame_requested |=
                self.upsert_streaming_tool_call(call, event_type == "message_end");
        }
        if event_type == "message_end" {
            self.streaming_tool_message_open = false;
        }
        active_tool_frame_requested
    }

    pub(crate) fn apply_visible_tool_intent(&mut self, _text: &str) -> bool {
        false
    }

    pub(crate) fn remove_provisional_tool_intent(&mut self, tool: &str) {
        let key = tool_intent_key(tool);
        let Some(index) = self.tool_rows.remove(&key) else {
            return;
        };
        let Some(row) = self.transcript.get(index) else {
            return;
        };
        if row.tool_call_id.is_none() && row.tool_started.is_some() && row.tool_elapsed.is_none() {
            self.remove_transcript_row(index);
        }
    }

    pub(crate) fn remove_unmatched_provisional_tool_intents(&mut self, matched_tools: &[String]) {
        let tools = self
            .tool_rows
            .keys()
            .filter_map(|key| key.strip_prefix("intent:"))
            .filter(|tool| !matched_tools.iter().any(|matched| matched == *tool))
            .map(str::to_string)
            .collect::<Vec<_>>();
        for tool in tools {
            self.remove_provisional_tool_intent(&tool);
        }
    }
}
