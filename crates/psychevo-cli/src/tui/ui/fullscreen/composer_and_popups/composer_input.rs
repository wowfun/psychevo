#[allow(unused_imports)]
pub(crate) use super::*;

impl<'a> FullscreenUi<'a> {
    pub(crate) fn enter_shell_mode(&mut self) {
        self.shell_mode = true;
        self.clear_slash_menu_dismissal();
    }

    pub(crate) fn exit_shell_mode(&mut self) {
        self.shell_mode = false;
        self.clear_slash_menu_dismissal();
    }

    pub(crate) fn clear_composer(&mut self) {
        self.textarea = new_textarea();
        self.shell_mode = false;
        self.composer_cursor_top_row = 0;
    }

    pub(crate) fn select_composer_all(&mut self) -> bool {
        if textarea_text(&self.textarea).is_empty() {
            return false;
        }
        self.textarea.select_all();
        self.close_file_popup();
        self.close_agent_popup();
        self.close_skill_popup();
        self.dismiss_slash_menu();
        true
    }

    pub(crate) fn cancel_composer_selection(&mut self) -> bool {
        if !self.textarea.is_selecting() {
            return false;
        }
        self.textarea.cancel_selection();
        true
    }

    pub(crate) fn set_composer_text(&mut self, text: &str) {
        if let Some(shell) = parse_shell_escape_input(text) {
            self.shell_mode = true;
            self.textarea = if shell.command.is_empty() {
                new_textarea()
            } else {
                textarea_with_text(&shell.command)
            };
        } else {
            self.shell_mode = false;
            self.textarea = if text.is_empty() {
                new_textarea()
            } else {
                textarea_with_text(text)
            };
        }
        self.slash_menu_selected = 0;
        self.clear_slash_menu_dismissal();
        self.composer_cursor_top_row = 0;
    }

    pub(crate) fn composer_submission_text(&self) -> String {
        let text = textarea_text(&self.textarea);
        if self.shell_mode {
            format!("!{text}")
        } else {
            text
        }
    }

    pub(crate) fn next_pending_input_sequence(&mut self) -> u64 {
        self.pending_input_sequence = self.pending_input_sequence.saturating_add(1);
        self.pending_input_sequence
    }

    pub(crate) fn pending_input_entries(&self) -> Vec<PendingInputEntry> {
        let mut entries = Vec::new();
        for steer in &self.pending_steers {
            entries.push(PendingInputEntry {
                target: PendingInputRef::Steer(steer.id),
                kind: PendingInputKind::Steer,
                text: steer.display_prompt.clone(),
                images: steer.images.clone(),
                sequence: steer.sequence,
            });
        }
        for input in &self.queued_inputs {
            if let QueuedInput::Prompt {
                display_prompt,
                images,
                sequence,
                ..
            } = input
            {
                entries.push(PendingInputEntry {
                    target: PendingInputRef::Queue(*sequence),
                    kind: PendingInputKind::Queue,
                    text: display_prompt.clone(),
                    images: images.clone(),
                    sequence: *sequence,
                });
            }
        }
        entries.sort_by_key(|entry| entry.sequence);
        entries
    }

    pub(crate) fn pending_input_entry(&self, target: PendingInputRef) -> Option<PendingInputEntry> {
        self.pending_input_entries()
            .into_iter()
            .find(|entry| entry.target == target)
    }

    pub(crate) fn has_pending_input_preview(&self) -> bool {
        self.pending_input_edit.is_some() || !self.pending_input_entries().is_empty()
    }

    pub(crate) fn start_pending_input_edit(&mut self, target: PendingInputRef) -> bool {
        let Some(entry) = self.pending_input_entry(target) else {
            return false;
        };
        self.pending_input_edit = Some(PendingInputEdit {
            target,
            kind: entry.kind,
            textarea: textarea_with_text(&entry.text),
            images: entry.images,
            cursor_top_row: 0,
        });
        self.clear_slash_menu_dismissal();
        self.close_file_popup();
        self.close_agent_popup();
        self.close_skill_popup();
        true
    }

    pub(crate) fn cancel_pending_input_edit(&mut self) -> bool {
        self.pending_input_edit.take().is_some()
    }

    pub(crate) fn pending_input_edit_text(&self) -> Option<String> {
        self.pending_input_edit
            .as_ref()
            .map(|edit| textarea_text(&edit.textarea))
    }

    pub(crate) fn pending_input_action_hit(
        &self,
        column: u16,
        row: u16,
    ) -> Option<(PendingInputRef, PendingInputAction)> {
        self.last_pending_input_action_areas
            .iter()
            .find_map(|(target, action, area)| {
                rect_contains(*area, column, row).then_some((*target, *action))
            })
    }

    pub(crate) fn absorb_shell_escape_prefix(&mut self) -> bool {
        if self.shell_mode {
            return false;
        }
        let text = textarea_text(&self.textarea);
        let Some(shell) = parse_shell_escape_input(&text) else {
            return false;
        };
        self.shell_mode = true;
        self.textarea = if shell.command.is_empty() {
            new_textarea()
        } else {
            textarea_with_text(&shell.command)
        };
        self.slash_menu_selected = 0;
        self.clear_slash_menu_dismissal();
        self.close_agent_popup();
        self.close_skill_popup();
        true
    }

    pub(crate) fn restore_queued_inputs_to_composer(&mut self) {
        if self.queued_inputs.is_empty() && self.pending_steers.is_empty() {
            return;
        }
        let mut restored = Vec::new();
        let steers = self.pending_steers.drain(..).collect::<Vec<_>>();
        for steer in steers {
            restored.push((steer.sequence, steer.display_prompt, steer.images));
        }
        for input in self.queued_inputs.drain(..) {
            let sequence = queued_input_sequence(&input);
            let images = match &input {
                QueuedInput::Prompt { images, .. } => images.clone(),
                QueuedInput::Shell { .. } | QueuedInput::Compact { .. } => Vec::new(),
            };
            restored.push((sequence, queued_input_text(input), images));
        }
        restored.sort_by_key(|(sequence, _, _)| *sequence);
        let mut parts = Vec::new();
        for (_, text, images) in restored {
            self.pending_images.extend(images);
            parts.push(text);
        }
        let draft = self.composer_submission_text();
        if !draft.is_empty() && draft != "!" {
            parts.push(draft);
        }
        self.set_composer_text(&parts.join("\n"));
        self.reset_history_navigation();
        self.clear_slash_menu_dismissal();
        self.close_file_popup();
        self.close_agent_popup();
        self.close_skill_popup();
        self.pending_input_edit = None;
    }

    pub(crate) fn add_pending_image(&mut self, image: ImageInput) -> String {
        self.sync_pending_images_with_textarea();
        let text = textarea_text(&self.textarea);
        let placeholder = next_image_placeholder(&self.pending_images, &text);
        self.pending_images.push(PendingImageAttachment {
            placeholder: placeholder.clone(),
            image,
        });
        placeholder
    }

    pub(crate) fn sync_pending_images_with_textarea(&mut self) {
        let text = textarea_text(&self.textarea);
        self.pending_images
            .retain(|attachment| text.contains(&attachment.placeholder));
    }

    pub(crate) fn take_submitted_images(&mut self, prompt: &str) -> Vec<PendingImageAttachment> {
        let mut indexed = self
            .pending_images
            .iter()
            .filter_map(|attachment| {
                prompt
                    .find(&attachment.placeholder)
                    .map(|index| (index, attachment.clone()))
            })
            .collect::<Vec<_>>();
        indexed.sort_by_key(|(index, _)| *index);
        self.pending_images.clear();
        indexed
            .into_iter()
            .map(|(_, attachment)| attachment)
            .collect()
    }
}
