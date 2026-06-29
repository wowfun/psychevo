#[allow(unused_imports)]
pub(crate) use super::*;

impl<'a> FullscreenUi<'a> {
    pub(crate) fn insert_transcript_row(&mut self, index: usize, row: TranscriptRow) -> usize {
        let index = index.min(self.transcript.len());
        self.transcript.insert(index, row);
        increment_row_index(&mut self.assistant_row, index);
        increment_row_index(&mut self.assistant_preamble_row, index);
        increment_row_index(&mut self.reasoning_row, index);
        increment_row_index(&mut self.meta_row, index);
        increment_row_index(&mut self.selected_row, index);
        for row_index in self.gateway_item_rows.values_mut() {
            if *row_index >= index {
                *row_index += 1;
            }
        }
        for row_index in self.tool_rows.values_mut() {
            if *row_index >= index {
                *row_index += 1;
            }
        }
        for row_index in self.exec_session_rows.values_mut() {
            if *row_index >= index {
                *row_index += 1;
            }
        }
        index
    }

    pub(crate) fn remove_transcript_row(&mut self, index: usize) {
        if index >= self.transcript.len() {
            return;
        }
        self.transcript.remove(index);
        decrement_row_index(&mut self.assistant_row, index);
        decrement_row_index(&mut self.assistant_preamble_row, index);
        decrement_row_index(&mut self.reasoning_row, index);
        decrement_row_index(&mut self.meta_row, index);
        decrement_row_index(&mut self.selected_row, index);
        self.gateway_item_rows
            .retain(|_, row_index| *row_index != index);
        for row_index in self.gateway_item_rows.values_mut() {
            if *row_index > index {
                *row_index -= 1;
            }
        }
        self.tool_rows.retain(|_, row_index| *row_index != index);
        for row_index in self.tool_rows.values_mut() {
            if *row_index > index {
                *row_index -= 1;
            }
        }
        self.exec_session_rows
            .retain(|_, row_index| *row_index != index);
        for row_index in self.exec_session_rows.values_mut() {
            if *row_index > index {
                *row_index -= 1;
            }
        }
    }

    pub(crate) fn insert_evidence_row(&mut self, row: TranscriptRow) -> usize {
        let index = if let Some(assistant_row) = self.assistant_row
            && self.transcript.get(assistant_row).is_some_and(|row| {
                row.kind == TranscriptKind::Answer && !row.text.trim().is_empty()
            }) {
            assistant_row.saturating_add(1)
        } else {
            self.assistant_row
                .or(self.meta_row)
                .unwrap_or(self.transcript.len())
        };
        self.insert_transcript_row(index, row)
    }

    pub(crate) fn insert_answer_row(&mut self, row: TranscriptRow) -> usize {
        let index = self.meta_row.unwrap_or(self.transcript.len());
        self.insert_transcript_row(index, row)
    }

    pub(crate) fn append_thinking_text(&mut self, index: usize, text: &str) {
        let Some(row) = self.transcript.get_mut(index) else {
            return;
        };
        if row.kind != TranscriptKind::Thinking {
            row.text.push_str(text);
            return;
        }
        let mut full = row
            .full_text
            .as_ref()
            .cloned()
            .unwrap_or_else(|| row.text.clone());
        full.push_str(text);
        row.set_evidence_body_text(full);
    }

    pub(crate) fn thinking_full_text(&self, index: usize) -> String {
        self.transcript
            .get(index)
            .and_then(|row| row.full_text.as_ref().or(Some(&row.text)))
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn finish_thinking_row(&mut self, index: usize) {
        let Some(row) = self.transcript.get_mut(index) else {
            return;
        };
        if row.kind != TranscriptKind::Thinking {
            return;
        }
        if let Some(started) = row.tool_started.take() {
            row.tool_elapsed = Some(started.elapsed());
        }
    }

    pub(crate) fn apply_assistant_preamble_text(&mut self, text: String, completed: bool) {
        if let Some(idx) = self.reasoning_row.take() {
            self.finish_thinking_row(idx);
        }
        let idx = self
            .assistant_preamble_row
            .or_else(|| self.assistant_row.take())
            .unwrap_or_else(|| {
                let mut row =
                    TranscriptRow::with_title(TranscriptKind::Thinking, "Thinking", String::new());
                if !completed {
                    row.tool_started = Some(Instant::now());
                }
                let idx = self.insert_evidence_row(row);
                self.assistant_preamble_row = Some(idx);
                idx
            });
        let Some(row) = self.transcript.get_mut(idx) else {
            self.assistant_preamble_row = None;
            return;
        };
        row.kind = TranscriptKind::Thinking;
        row.title = "Thinking".to_string();
        row.set_evidence_body_text(text);
        if !completed && row.tool_started.is_none() {
            row.tool_started = Some(Instant::now());
        }
        self.assistant_preamble_row = Some(idx);
        self.turn_had_reasoning = true;
        self.remove_turn_meta();
        if completed {
            self.finish_thinking_row(idx);
            self.assistant_preamble_row = None;
        }
    }
}
