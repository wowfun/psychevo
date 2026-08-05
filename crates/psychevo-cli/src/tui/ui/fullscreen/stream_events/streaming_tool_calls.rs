use crate::tui::{
    FullscreenUi, Value, assistant_message_stream_event_type, scoped_tool_position_key,
    streaming_tool_calls_from_event,
};

impl<'a> FullscreenUi<'a> {
    pub(crate) fn apply_streaming_tool_calls(&mut self, value: &Value) -> bool {
        let Some(event_type) = assistant_message_stream_event_type(value) else {
            return false;
        };
        let calls = streaming_tool_calls_from_event(value)
            .into_iter()
            .filter(|call| call.tool_name != "clarify")
            .collect::<Vec<_>>();
        if !self.streaming_tool_message_open {
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
}
