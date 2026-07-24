#[allow(unused_imports)]
pub(crate) use super::*;

impl TuiApp {
    pub(crate) fn open_history_message_actions(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        target: TranscriptHitTarget,
    ) -> Result<bool> {
        let Some(row) = ui
            .target_row_index(target)
            .and_then(|index| ui.transcript.get(index))
            .filter(|row| row.kind == TranscriptKind::Prompt)
        else {
            return Ok(false);
        };
        let (Some(message_id), Some(_message_seq)) =
            (row.transcript_entry_id.clone(), row.transcript_message_seq)
        else {
            return Ok(false);
        };
        let Some(thread_id) = self.current_session.clone() else {
            return Ok(false);
        };
        if ui.status_has_running(Some(&thread_id)) {
            ui.push_status("finish the running turn before editing history");
            return Ok(true);
        }
        if let Some(reason) =
            psychevo_gateway::history_editing::native_history_action_unavailable_reason(
                &self.state_runtime,
                &thread_id,
                "tui",
            )?
        {
            ui.push_status(reason);
            return Ok(true);
        }
        if self
            .state_runtime
            .session_revert_state(&thread_id)?
            .is_some()
        {
            ui.push_status("restore history or redo workspace files before another edit or fork");
            return Ok(true);
        }
        let rows = [
            (
                "Edit",
                "Update this Thread from the selected message",
                HistoryMessageAction::UpdateAndRun,
            ),
            (
                "Fork",
                "Create a new Thread before the selected message",
                HistoryMessageAction::Fork,
            ),
        ]
        .into_iter()
        .map(|(label, description, action)| BottomSelectionRow {
            label: label.to_string(),
            description: Some(description.to_string()),
            detail: None,
            group: None,
            search_text: format!("{label} {description}"),
            is_current: false,
            is_default: false,
            style: BottomRowStyle::Action,
            footer: Some(match action {
                HistoryMessageAction::UpdateAndRun => {
                    "Enter edit in this Thread  Esc cancel".to_string()
                }
                HistoryMessageAction::Fork => "Enter edit in a new Thread  Esc cancel".to_string(),
            }),
            value: BottomSelectionValue::HistoryMessageAction {
                message_id: message_id.clone(),
                action,
            },
        })
        .collect();
        let mut panel =
            BottomSelectionPanel::new("Message Actions", "", "No actions available", rows);
        panel.footer = "Enter select  Esc cancel".to_string();
        ui.bottom_panel = Some(BottomPanel::AgentActions(panel));
        Ok(true)
    }

    pub(crate) fn begin_history_message_edit(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        message_id: String,
        action: HistoryMessageAction,
    ) -> Result<()> {
        let thread_id = self
            .current_session
            .clone()
            .ok_or_else(|| anyhow!("history editing requires an active session"))?;
        let draft = psychevo_gateway::history_editing::read_native_editable_draft(
            &self.state_runtime,
            &self.gateway,
            &thread_id,
            &message_id,
            "tui",
        )?;
        if let Some(reason) = draft.unavailable_reason {
            ui.set_ephemeral_error(reason);
            ui.bottom_panel = None;
            return Ok(());
        }
        let message_seq = draft
            .message_seq
            .ok_or_else(|| anyhow!("history message has no durable sequence"))?;
        set_history_draft_in_composer(ui, &draft.parts);
        ui.history_message_edit = Some(HistoryMessageEdit {
            thread_id,
            message_id,
            message_seq,
            action,
            original_parts: draft.parts,
        });
        ui.bottom_panel = None;
        ui.focus = FocusMode::Composer;
        if draft.fidelity == ThreadEditableDraftFidelity::BestEffort {
            ui.push_status(draft.warning.unwrap_or_else(|| {
                "older message reconstructed with best-effort fidelity".to_string()
            }));
        } else {
            ui.push_status(match action {
                HistoryMessageAction::UpdateAndRun => "editing message in this Thread",
                HistoryMessageAction::Fork => "editing message for a new Thread",
            });
        }
        Ok(())
    }

    pub(crate) fn cancel_history_message_edit(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let Some(edit) = ui.history_message_edit.take() else {
            return Ok(false);
        };
        if matches!(
            self.state_runtime
                .session_revert_state(&edit.thread_id)?
                .map(|revert| revert.kind),
            Some(psychevo_runtime::state::SessionRevertKind::ConversationEdit { .. })
        ) {
            let draft = psychevo_gateway::history_editing::restore_native_conversation_edit(
                &self.state_runtime,
                &edit.thread_id,
            )?;
            ui.clear_transcript();
            self.load_current_session_history(ui)?;
            set_history_draft_in_composer(ui, &draft.parts);
            ui.push_status("history restored; edited draft kept in composer");
        } else {
            ui.clear_composer();
            ui.pending_images.clear();
            ui.push_status("message edit cancelled");
        }
        Ok(true)
    }

    pub(crate) fn restore_staged_conversation_edit(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let Some(thread_id) = self.current_session.clone() else {
            return Ok(false);
        };
        if self.state_runtime.session_summary(&thread_id)?.is_none() {
            return Ok(false);
        }
        if !matches!(
            self.state_runtime
                .session_revert_state(&thread_id)?
                .map(|revert| revert.kind),
            Some(psychevo_runtime::state::SessionRevertKind::ConversationEdit { .. })
        ) {
            return Ok(false);
        }
        let draft = psychevo_gateway::history_editing::restore_native_conversation_edit(
            &self.state_runtime,
            &thread_id,
        )?;
        ui.history_message_edit = None;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        set_history_draft_in_composer(ui, &draft.parts);
        ui.push_status("history restored; edited draft kept in composer");
        Ok(true)
    }

    pub(crate) fn show_staged_history_status(
        &self,
        ui: &mut FullscreenUi<'_>,
        thread_id: &str,
    ) -> Result<()> {
        let Some(revert) = self.state_runtime.session_revert_state(thread_id)? else {
            return Ok(());
        };
        if matches!(
            revert.kind,
            psychevo_runtime::state::SessionRevertKind::ConversationEdit { .. }
        ) {
            let hidden = self
                .state_runtime
                .messages_from_count(thread_id, revert.start_seq)?;
            ui.push_status(format!(
                "history edit staged · {hidden} entries hidden · Esc restore history"
            ));
        }
        Ok(())
    }

    pub(crate) fn submit_history_message_edit(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let Some(edit) = ui.history_message_edit.clone() else {
            return Ok(false);
        };
        if ui.status_has_running(Some(&edit.thread_id)) {
            ui.set_ephemeral_error("finish the running turn before editing history");
            return Ok(true);
        }
        ui.sync_pending_images_with_textarea();
        let display_prompt = textarea_text(&ui.textarea);
        let mut draft = history_draft_from_composer(&display_prompt, &ui.pending_images);
        let (original_text, original_images) = history_draft_composer_value(&edit.original_parts);
        let current_images = ui
            .pending_images
            .iter()
            .map(|attachment| attachment.image.clone())
            .collect::<Vec<_>>();
        if display_prompt == original_text && current_images == original_images {
            draft.parts = edit.original_parts.clone();
        }
        match edit.action {
            HistoryMessageAction::UpdateAndRun => {
                let staged = psychevo_gateway::history_editing::stage_native_conversation_edit(
                    &self.state_runtime,
                    &self.gateway,
                    &edit.thread_id,
                    &edit.message_id,
                    &draft,
                    "tui",
                )?;
                if !staged {
                    ui.history_message_edit = None;
                    ui.clear_composer();
                    ui.pending_images.clear();
                    ui.push_status("message unchanged");
                    return Ok(true);
                }
                let original_images = ui.pending_images.clone();
                let images = ui.take_submitted_images(&display_prompt);
                let submit = self.submit_fullscreen_prompt(ui, display_prompt.clone(), images);
                if submit.is_ok() {
                    ui.history_message_edit = None;
                    ui.clear_composer();
                    ui.push_submitted_history(display_prompt);
                } else {
                    ui.pending_images = original_images;
                }
                submit?;
            }
            HistoryMessageAction::Fork => {
                if self
                    .state_runtime
                    .session_revert_state(&edit.thread_id)?
                    .is_some()
                {
                    ui.set_ephemeral_error(
                        "restore history or redo workspace files before forking",
                    );
                    return Ok(true);
                }
                let child_id = psychevo_gateway::history_editing::fork_native_history(
                    &self.state_runtime,
                    &edit.thread_id,
                    Some(edit.message_seq),
                    "tui",
                )?;
                let parts = draft.parts;
                self.open_session_direct(ui, &child_id)?;
                set_history_draft_in_composer(ui, &parts);
                ui.history_message_edit = None;
                ui.push_status(format!("forked from {}", short_session(&edit.thread_id)));
            }
        }
        Ok(true)
    }

    pub(crate) fn fork_session_from_panel(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: String,
    ) -> Result<()> {
        if ui.status_has_running(Some(&session_id)) {
            ui.set_bottom_panel_notice("cannot fork the current session while a turn is running");
            return Ok(());
        }
        if let Some(reason) =
            psychevo_gateway::history_editing::native_history_action_unavailable_reason(
                &self.state_runtime,
                &session_id,
                "tui",
            )?
        {
            ui.set_bottom_panel_notice(reason);
            return Ok(());
        }
        if self
            .state_runtime
            .session_revert_state(&session_id)?
            .is_some()
        {
            ui.set_bottom_panel_notice("restore history or redo workspace files before forking");
            return Ok(());
        }
        let child_id = psychevo_gateway::history_editing::fork_native_history(
            &self.state_runtime,
            &session_id,
            None,
            "tui",
        )?;
        self.open_session_direct(ui, &child_id)?;
        ui.push_status(format!("forked from {}", short_session(&session_id)));
        Ok(())
    }
}

fn set_history_draft_in_composer(ui: &mut FullscreenUi<'_>, parts: &[ThreadEditableInputPart]) {
    let mut text = String::new();
    let mut images = Vec::new();
    for part in parts {
        match part {
            ThreadEditableInputPart::Text { text: part } => text.push_str(part),
            ThreadEditableInputPart::Image { input } => {
                let placeholder = next_image_placeholder(&images, &text);
                text.push_str(&placeholder);
                let image = match input {
                    GatewayImageInput::LocalPath { path } => {
                        ImageInput::LocalPath(PathBuf::from(path))
                    }
                    GatewayImageInput::Url { url } => ImageInput::ImageUrl(url.clone()),
                };
                images.push(PendingImageAttachment { placeholder, image });
            }
        }
    }
    ui.set_composer_text(&text);
    ui.pending_images = images;
}

fn history_draft_composer_value(parts: &[ThreadEditableInputPart]) -> (String, Vec<ImageInput>) {
    let mut text = String::new();
    let mut attachments = Vec::new();
    for part in parts {
        match part {
            ThreadEditableInputPart::Text { text: part } => text.push_str(part),
            ThreadEditableInputPart::Image { input } => {
                let placeholder = next_image_placeholder(&attachments, &text);
                text.push_str(&placeholder);
                let image = match input {
                    GatewayImageInput::LocalPath { path } => {
                        ImageInput::LocalPath(PathBuf::from(path))
                    }
                    GatewayImageInput::Url { url } => ImageInput::ImageUrl(url.clone()),
                };
                attachments.push(PendingImageAttachment { placeholder, image });
            }
        }
    }
    let images = attachments
        .into_iter()
        .map(|attachment| attachment.image)
        .collect();
    (text, images)
}

fn history_draft_from_composer(
    text: &str,
    images: &[PendingImageAttachment],
) -> ThreadEditableDraft {
    let mut indexed = images
        .iter()
        .filter_map(|image| text.find(&image.placeholder).map(|index| (index, image)))
        .collect::<Vec<_>>();
    indexed.sort_by_key(|(index, _)| *index);
    let mut parts = Vec::new();
    let mut cursor = 0usize;
    for (index, attachment) in indexed {
        if index > cursor {
            parts.push(ThreadEditableInputPart::Text {
                text: text[cursor..index].to_string(),
            });
        }
        let input = match &attachment.image {
            ImageInput::LocalPath(path) => GatewayImageInput::LocalPath {
                path: path.display().to_string(),
            },
            ImageInput::ImageUrl(url) => GatewayImageInput::Url { url: url.clone() },
        };
        parts.push(ThreadEditableInputPart::Image { input });
        cursor = index.saturating_add(attachment.placeholder.len());
    }
    if cursor < text.len() || parts.is_empty() {
        parts.push(ThreadEditableInputPart::Text {
            text: text[cursor..].to_string(),
        });
    }
    ThreadEditableDraft { parts }
}
