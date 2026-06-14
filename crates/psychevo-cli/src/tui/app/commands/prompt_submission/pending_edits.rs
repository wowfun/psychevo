impl TuiApp {
    pub(crate) fn cancel_pending_fullscreen_inputs(&mut self, ui: &mut FullscreenUi<'_>) {
        let active = self.active_gateway_turn_selector(ui);
        let legacy_control = active
            .is_none()
            .then(|| ui.running.as_ref().map(|running| running.control.clone()))
            .flatten();
        let mut cancelled_steers = 0usize;
        let mut retained = VecDeque::new();
        while let Some(input) = ui.pending_steers.pop_front() {
            let cancelled = active.as_ref().is_some_and(|(selector, expected)| {
                self.gateway
                    .cancel_steer(selector.clone(), expected.as_deref(), input.id)
            }) || legacy_control
                .as_ref()
                .is_some_and(|control| control.cancel_pending_user_message(input.id));
            if cancelled {
                cancelled_steers += 1;
            } else {
                retained.push_back(input);
            }
        }
        ui.pending_steers = retained;
        let queued = ui.queued_inputs.len();
        ui.queued_inputs.clear();
        ui.pending_input_edit = None;
        let total = cancelled_steers + queued;
        if total == 0 {
            ui.set_ephemeral_status("no pending input");
        } else {
            ui.set_ephemeral_status(format!("pending input canceled: {total}"));
        }
    }

    pub(crate) fn handle_pending_input_action(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        target: PendingInputRef,
        action: PendingInputAction,
    ) -> Result<()> {
        match action {
            PendingInputAction::Edit => {
                if !ui.start_pending_input_edit(target) {
                    ui.set_ephemeral_error("pending input no longer editable");
                }
                Ok(())
            }
            PendingInputAction::Undo => self.undo_pending_input(ui, target),
        }
    }

    pub(crate) fn undo_pending_input(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        target: PendingInputRef,
    ) -> Result<()> {
        match target {
            PendingInputRef::Steer(id) => {
                let cancelled =
                    self.active_gateway_turn_selector(ui)
                        .is_some_and(|(selector, expected)| {
                            self.gateway.cancel_steer(selector, expected.as_deref(), id)
                        })
                        || ui
                            .running
                            .as_ref()
                            .is_some_and(|running| running.control.cancel_pending_user_message(id));
                if !cancelled {
                    ui.set_ephemeral_error("steer already sent");
                    return Ok(());
                }
                ui.pending_steers.retain(|input| input.id != id);
                if ui
                    .pending_input_edit
                    .as_ref()
                    .is_some_and(|edit| edit.target == target)
                {
                    ui.pending_input_edit = None;
                }
                ui.set_ephemeral_status("pending input canceled");
                Ok(())
            }
            PendingInputRef::Queue(sequence) => {
                let Some(index) = ui.queued_inputs.iter().position(|input| {
                    matches!(input, QueuedInput::Prompt { sequence: value, .. } if *value == sequence)
                }) else {
                    ui.set_ephemeral_error("queued input already started");
                    return Ok(());
                };
                ui.queued_inputs.remove(index);
                if ui
                    .pending_input_edit
                    .as_ref()
                    .is_some_and(|edit| edit.target == target)
                {
                    ui.pending_input_edit = None;
                }
                ui.set_ephemeral_status("pending input canceled");
                Ok(())
            }
        }
    }

    pub(crate) fn confirm_pending_input_edit(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(display_prompt) = ui.pending_input_edit_text() else {
            return Ok(());
        };
        let images = ui
            .pending_input_edit
            .as_ref()
            .map(|edit| edit.images.clone())
            .unwrap_or_default();
        if display_prompt.trim().is_empty() && images.is_empty() {
            ui.set_ephemeral_error("pending input cannot be empty");
            return Ok(());
        }
        let edit = ui.pending_input_edit.take().expect("pending input edit");
        match edit.target {
            PendingInputRef::Steer(id) => {
                if self.update_pending_steer_input(ui, id, display_prompt.clone(), &images)? {
                    ui.set_ephemeral_status("pending input updated");
                    return Ok(());
                }
                ui.pending_steers.retain(|input| input.id != id);
                self.submit_pending_edit_as_new(ui, display_prompt, images)
            }
            PendingInputRef::Queue(sequence) => {
                if self.update_queued_prompt_input(ui, sequence, display_prompt.clone(), &images) {
                    ui.set_ephemeral_status("pending input updated");
                    return Ok(());
                }
                self.submit_pending_edit_as_new(ui, display_prompt, images)
            }
        }
    }

    pub(crate) fn update_pending_steer_input(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        id: PendingInputId,
        display_prompt: String,
        images: &[PendingImageAttachment],
    ) -> Result<bool> {
        let prompt = prompt_without_image_placeholders(&display_prompt, images);
        let image_inputs = images
            .iter()
            .map(|attachment| attachment.image.clone())
            .collect::<Vec<_>>();
        let metadata = self
            .selected_model
            .as_ref()
            .map(|model| model.metadata.clone())
            .unwrap_or_default();
        let message = prompt_message_from_inputs_with_options(
            &prompt,
            &image_inputs,
            &self.workdir,
            &metadata,
            false,
        )?
        .message;
        let updated =
            self.active_gateway_turn_selector(ui)
                .is_some_and(|(selector, expected_turn_id)| {
                    self.gateway.update_steer(
                        selector,
                        expected_turn_id.as_deref(),
                        id,
                        message.clone(),
                    )
                })
                || ui.running.as_ref().is_some_and(|running| {
                    running.control.update_pending_user_message(id, message)
                });
        if !updated {
            return Ok(false);
        }
        if let Some(input) = ui.pending_steers.iter_mut().find(|input| input.id == id) {
            input.prompt = prompt;
            input.display_prompt = display_prompt;
            input.images = images.to_vec();
        }
        Ok(true)
    }

    pub(crate) fn update_queued_prompt_input(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        sequence: u64,
        display_prompt: String,
        images: &[PendingImageAttachment],
    ) -> bool {
        let Some(input) = ui.queued_inputs.iter_mut().find(|input| {
            matches!(input, QueuedInput::Prompt { sequence: value, .. } if *value == sequence)
        }) else {
            return false;
        };
        let QueuedInput::Prompt {
            prompt,
            display_prompt: queued_display_prompt,
            images: queued_images,
            ..
        } = input
        else {
            return false;
        };
        *prompt = prompt_without_image_placeholders(&display_prompt, images);
        *queued_display_prompt = display_prompt;
        *queued_images = images.to_vec();
        true
    }

    pub(crate) fn submit_pending_edit_as_new(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
    ) -> Result<()> {
        self.submit_fullscreen_prompt(ui, display_prompt, images)
    }

    pub(crate) fn submit_fullscreen_shell(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        command: String,
    ) -> Result<()> {
        if ui
            .running
            .as_ref()
            .is_some_and(|running| matches!(running.task, RunningTask::Agent(_)))
        {
            let (had_pending, _active_tool_frame_requested) =
                self.drain_available_fullscreen_stream_events(ui);
            if had_pending {
                ui.follow_transcript_if_needed();
                ui.refresh_sidebar(self);
            }
            if self.current_session.is_none() || ui.turn_started.is_none() {
                if command.trim().is_empty() {
                    ui.push_status(USER_SHELL_HELP);
                } else {
                    ui.pending_auxiliary_shell_commands.push_back(command);
                    ui.refresh_sidebar(self);
                }
                return Ok(());
            }
            return self.start_auxiliary_fullscreen_shell(ui, command);
        }
        if ui.running.is_some() {
            self.queue_fullscreen_shell(ui, command);
            return Ok(());
        }
        self.start_fullscreen_shell(ui, command)
    }

    pub(crate) fn start_next_queued_input(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        while ui.running.is_none() && self.compaction_task.is_none() {
            let Some(index) =
                ui.queued_inputs
                    .iter()
                    .position(|input| match queued_input_session_id(input) {
                        Some(session_id) => Some(session_id) == self.current_session.as_deref(),
                        None => true,
                    })
            else {
                break;
            };
            let next = ui.queued_inputs.remove(index).expect("queued input index");
            match next {
                QueuedInput::Prompt {
                    prompt,
                    display_prompt,
                    images,
                    ..
                } => self.start_fullscreen_turn(ui, prompt, display_prompt, images)?,
                QueuedInput::Shell { command, .. } => self.start_fullscreen_shell(ui, command)?,
                QueuedInput::Compact {
                    instructions,
                    command_echo,
                    ..
                } => self.start_compaction_task(
                    ui,
                    instructions,
                    Some(command_echo),
                    true,
                    CompactionReason::Manual,
                    true,
                )?,
            }
        }
        Ok(())
    }
}
