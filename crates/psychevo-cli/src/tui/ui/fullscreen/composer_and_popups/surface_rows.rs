#[allow(unused_imports)]
pub(crate) use super::*;

impl<'a> FullscreenUi<'a> {
    pub(crate) fn bottom_panel_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_bottom_panel_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn set_render_areas(
        &mut self,
        transcript: Rect,
        composer: Option<Rect>,
        status: Rect,
        bottom_panel: Option<Rect>,
    ) {
        self.last_transcript_area = Some(transcript);
        self.last_composer_area = composer;
        self.last_composer_input_area = None;
        self.last_status_area = Some(status);
        self.last_bottom_panel_area = bottom_panel;
    }

    pub(crate) fn set_composer_input_area(&mut self, area: Option<Rect>) {
        self.last_composer_input_area = area;
    }

    pub(crate) fn composer_terminal_cursor_position(&mut self, area: Rect) -> Option<(u16, u16)> {
        composer_terminal_cursor_position(&self.textarea, area, &mut self.composer_cursor_top_row)
    }

    pub(crate) fn mouse_wheel_target(&self, column: u16, row: u16) -> Option<MouseWheelTarget> {
        if self
            .last_bottom_panel_area
            .is_some_and(|area| rect_contains(area, column, row))
        {
            return Some(MouseWheelTarget::BottomPanel);
        }
        if self
            .last_transcript_area
            .is_some_and(|area| rect_contains(area, column, row))
        {
            return Some(MouseWheelTarget::Transcript);
        }
        if self
            .last_composer_area
            .is_some_and(|area| rect_contains(area, column, row))
            || self
                .last_status_area
                .is_some_and(|area| rect_contains(area, column, row))
        {
            return None;
        }
        None
    }

    pub(crate) fn set_bottom_panel_notice(&mut self, text: impl Into<String>) {
        if let Some(panel) = &mut self.bottom_panel {
            match panel {
                BottomPanel::ProviderWizard(panel) => panel.notice = Some(text.into()),
                BottomPanel::AgentRunPrompt(panel) => panel.notice = Some(text.into()),
                BottomPanel::AgentEditor(panel) => panel.notice = Some(text.into()),
                _ => panel.selection_mut().notice = Some(text.into()),
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn push_user(&mut self, text: String) {
        self.push_user_with_images(text, &[]);
    }

    pub(crate) fn push_user_with_attachment_meta(
        &mut self,
        text: String,
        attachment_meta: Option<String>,
    ) {
        self.transcript
            .push(TranscriptRow::with_title(TranscriptKind::Prompt, "", text));
        if let Some(meta) = attachment_meta {
            self.transcript
                .push(TranscriptRow::simple(TranscriptKind::Meta, meta));
        }
    }

    pub(crate) fn push_user_with_images(
        &mut self,
        text: String,
        images: &[PendingImageAttachment],
    ) {
        self.push_user_with_attachment_meta(text, attachment_metadata_text(images, &self.cwd));
    }

    pub(crate) fn commit_pending_steer_from_event(&mut self, value: &Value) -> bool {
        let Some(message) = value.get("message") else {
            return false;
        };
        if message.get("role").and_then(Value::as_str) != Some("user") {
            return false;
        }
        let Some(pending_id) = pending_input_id_from_message_end(value) else {
            return false;
        };
        if let Some(index) = self
            .pending_steers
            .iter()
            .position(|input| input.id.as_u64() == pending_id)
        {
            let input = self
                .pending_steers
                .remove(index)
                .expect("pending steer index");
            self.push_user_with_images(input.display_prompt, &input.images);
        } else {
            self.push_history_message_with_accounting_options(
                message,
                value.get("usage"),
                value.get("metadata"),
                value.get("accounting"),
                false,
            );
        }
        self.history_prompt_started_ms = message_timestamp_ms(message);
        true
    }

    pub(crate) fn start_assistant(&mut self) {
        self.assistant_row = None;
        self.assistant_preamble_row = None;
        self.reasoning_row = None;
        self.meta_row = None;
        self.gateway_item_rows.clear();
        self.tool_rows.clear();
        self.live_tool_args.clear();
        self.streaming_tool_message_seq = 0;
        self.streaming_tool_message_open = false;
        self.turn_started = None;
        self.visible_turn_started = Some(Instant::now());
        self.interrupt_requested = false;
        self.turn_provider.clear();
        self.turn_model.clear();
        self.turn_context_limit = None;
        self.turn_usage = None;
        self.turn_metadata = None;
        self.turn_accounting = None;
        self.turn_session_id = None;
        self.active_event_session_id = None;
        self.turn_failures = 0;
        self.turn_interrupted = false;
        self.turn_outcome = None;
        self.turn_terminal_message = None;
        self.turn_had_reasoning = false;
        self.turn_terminal_visible_answer = false;
    }

    pub(crate) fn push_status(&mut self, text: impl Into<String>) {
        let mut row = TranscriptRow::simple(TranscriptKind::Status, text);
        self.tag_active_turn_local_row(&mut row);
        self.transcript.push(row);
    }

    pub(crate) fn push_turn_start_status(&mut self, text: impl Into<String>) {
        let text = text.into();
        let turn_id = self
            .running
            .as_ref()
            .and_then(|running| running.turn_id.clone());
        if let Some(row) = self.transcript.iter_mut().find(|row| {
            row.kind == TranscriptKind::Status
                && row.text == text
                && row.transcript_source.as_deref() == Some(TUI_TURN_START_TRANSCRIPT_SOURCE)
                && (turn_id.is_none()
                    || row.transcript_turn_id.as_deref() == turn_id.as_deref()
                    || row.transcript_turn_id.is_none())
        }) {
            if row.transcript_turn_id.is_none() {
                row.transcript_turn_id = turn_id;
            }
            return;
        }
        let mut row = TranscriptRow::simple(TranscriptKind::Status, text);
        self.tag_active_turn_start_row(&mut row);
        self.transcript.push(row);
    }

    pub(crate) fn set_ephemeral_status(&mut self, text: impl Into<String>) {
        self.ephemeral_status = Some(UiEphemeralStatus {
            text: text.into(),
            failed: false,
        });
    }

    pub(crate) fn clear_ephemeral_status(&mut self) {
        self.ephemeral_status = None;
    }

    pub(crate) fn set_ephemeral_error(&mut self, text: impl Into<String>) {
        self.ephemeral_status = Some(UiEphemeralStatus {
            text: text.into(),
            failed: true,
        });
    }

    pub(crate) fn push_command_result(
        &mut self,
        command: impl Into<String>,
        title: Option<&str>,
        text: impl Into<String>,
        failed: bool,
    ) {
        let mut body = String::new();
        if let Some(title) = title
            && !title.trim().is_empty()
        {
            body.push_str(title.trim());
        }
        let text = text.into();
        if !text.is_empty() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&text);
        }
        let mut row = TranscriptRow::with_title(TranscriptKind::Command, command, body);
        row.failed = failed;
        self.tag_active_turn_local_row(&mut row);
        self.transcript.push(row);
    }

    pub(crate) fn push_error(&mut self, text: impl Into<String>) {
        self.transcript
            .push(TranscriptRow::simple(TranscriptKind::Error, text));
    }
}
