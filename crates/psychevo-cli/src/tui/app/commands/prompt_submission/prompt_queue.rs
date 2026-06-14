impl TuiApp {
    pub(crate) async fn submit_fullscreen_text(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        text: String,
        record_history: bool,
    ) -> Result<bool> {
        ui.scroll_to_bottom();
        if self.handle_permission_approval_slash(ui, &text)? {
            if record_history {
                ui.push_submitted_history(text.clone());
            }
            return Ok(false);
        }
        if let Some(shell) = parse_shell_escape_input(&text) {
            if record_history {
                ui.push_submitted_history(shell.history_text.clone());
            }
            self.submit_fullscreen_shell(ui, shell.command)?;
            return Ok(false);
        }

        let display_text = text.clone();
        if record_history && (!display_text.trim().is_empty() || ui.pending_images.is_empty()) {
            let process_input = text
                .trim_start()
                .chars()
                .next()
                .is_some_and(|ch| matches!(ch, '/' | '!'));
            if ui.pending_images.is_empty() || process_input {
                ui.push_submitted_history(display_text.clone());
            }
        }
        match self.classify_submitted_slash_input(&text) {
            Ok(SubmittedSlashInput::Command(command)) => {
                self.handle_fullscreen_command_with_echo(ui, command, Some(text))
                    .await
            }
            Ok(SubmittedSlashInput::PassThroughPrompt(prompt)) => {
                let images = ui.take_submitted_images(&text);
                self.submit_fullscreen_prompt(ui, prompt, images)?;
                Ok(false)
            }
            Ok(SubmittedSlashInput::NotSlash) => {
                let images = ui.take_submitted_images(&text);
                self.submit_fullscreen_prompt(ui, display_text, images)?;
                Ok(false)
            }
            Err(err) => {
                ui.push_command_result(
                    normalize_submitted_slash_echo(&text),
                    None,
                    format!("error: {err:#}"),
                    true,
                );
                Ok(false)
            }
        }
    }

    pub(crate) fn handle_permission_approval_slash(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        text: &str,
    ) -> Result<bool> {
        let trimmed = text.trim();
        let Some(rest) = trimmed.strip_prefix("/approve") else {
            if trimmed == "/deny" {
                self.resolve_permission_approval_from_command(
                    ui,
                    PermissionApprovalDecision::deny(),
                    trimmed,
                )?;
                return Ok(true);
            }
            return Ok(false);
        };
        let decision = match rest.trim() {
            "" | "once" => PermissionApprovalDecision::allow_once(),
            "session" => PermissionApprovalDecision::allow_session(),
            "always" | "permanent" => PermissionApprovalDecision::allow_always(),
            other => {
                ui.push_command_result(
                    trimmed.to_string(),
                    None,
                    format!("error: unsupported approval scope `{other}`"),
                    true,
                );
                return Ok(true);
            }
        };
        self.resolve_permission_approval_from_command(ui, decision, trimmed)?;
        Ok(true)
    }

    pub(crate) fn resolve_permission_approval_from_command(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        decision: PermissionApprovalDecision,
        command_echo: &str,
    ) -> Result<()> {
        let Some(BottomPanel::PermissionApproval(panel)) = ui.bottom_panel.take() else {
            ui.push_command_result(
                command_echo.to_string(),
                None,
                "no pending permission approval".to_string(),
                true,
            );
            return Ok(());
        };
        if decision.outcome == PermissionApprovalOutcome::AllowAlways && !panel.request.allow_always
        {
            ui.bottom_panel = Some(BottomPanel::PermissionApproval(panel));
            ui.push_command_result(
                command_echo.to_string(),
                None,
                "current permission request does not support permanent approval".to_string(),
                true,
            );
            return Ok(());
        }
        ui.resolve_permission_approval(panel, decision);
        Ok(())
    }

    pub(crate) fn start_diff_task(&mut self) {
        if let Some(task) = self.diff_task.take() {
            task.task.abort();
        }
        let workdir = self.workdir.clone();
        let task = tokio::task::spawn_blocking(move || {
            collect_workspace_diff(&workdir).map_err(|err| err.to_string())
        });
        self.diff_task = Some(DiffTask { task });
    }

    pub(crate) fn submit_fullscreen_prompt(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
    ) -> Result<()> {
        let prompt = prompt_without_image_placeholders(&display_prompt, &images);
        self.submit_fullscreen_prompt_with_display(ui, prompt, display_prompt, images)
    }

    pub(crate) fn submit_fullscreen_prompt_with_display(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        prompt: String,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
    ) -> Result<()> {
        if self.compaction_task.is_some() {
            self.queue_fullscreen_prompt(ui, prompt, display_prompt, images);
            return Ok(());
        }
        if let Some(running) = ui.running.as_ref() {
            if matches!(running.task, RunningTask::Agent(_)) {
                return self.steer_fullscreen_prompt(ui, prompt, display_prompt, images);
            }
            self.queue_fullscreen_prompt(ui, prompt, display_prompt, images);
            return Ok(());
        }
        self.start_fullscreen_turn(ui, prompt, display_prompt, images)
    }

    pub(crate) fn submit_explicit_fullscreen_steer(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        message: String,
        command_echo: String,
    ) -> Result<()> {
        if !ui
            .running
            .as_ref()
            .is_some_and(|running| matches!(running.task, RunningTask::Agent(_)))
        {
            ui.push_command_result(
                command_echo,
                None,
                "error: /steer requires a running agent turn",
                true,
            );
            return Ok(());
        }
        let prompt = message.trim().to_string();
        self.steer_fullscreen_prompt(ui, prompt.clone(), prompt, Vec::new())
    }

    pub(crate) fn submit_fullscreen_queue(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        message: String,
    ) -> Result<()> {
        let prompt = message.trim().to_string();
        self.queue_fullscreen_prompt(ui, prompt.clone(), prompt, Vec::new());
        self.start_next_queued_input(ui)
    }

    pub(crate) fn steer_fullscreen_prompt(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        prompt: String,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
    ) -> Result<()> {
        let Some((selector, expected_turn_id)) = self.active_gateway_turn_selector(ui) else {
            if let Some(control) = ui
                .running
                .as_ref()
                .filter(|running| matches!(running.task, RunningTask::Agent(_)))
                .map(|running| running.control.clone())
            {
                return self.steer_fullscreen_prompt_with_control(
                    ui,
                    control,
                    prompt,
                    display_prompt,
                    images,
                );
            }
            self.queue_fullscreen_prompt(ui, prompt, display_prompt, images);
            return Ok(());
        };
        let image_inputs = images
            .iter()
            .map(|attachment| attachment.image.clone())
            .collect::<Vec<_>>();
        if self.image_submission_degrades_to_text(&prompt, &image_inputs) {
            ui.set_ephemeral_error(
                "selected model does not support image input; sent image source as text",
            );
        }
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
        let Some(id) = self
            .gateway
            .steer_turn(selector, expected_turn_id.as_deref(), message)
        else {
            ui.set_ephemeral_error("unable to steer current turn");
            return Ok(());
        };
        let session_id = ui
            .running
            .as_ref()
            .and_then(|running| running.session_id.clone())
            .or_else(|| self.current_session.clone());
        let sequence = ui.next_pending_input_sequence();
        ui.pending_steers.push_back(PendingSteerInput {
            id,
            session_id,
            prompt,
            display_prompt: display_prompt.clone(),
            images,
            sequence,
        });
        Ok(())
    }

    pub(crate) fn steer_fullscreen_prompt_with_control(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        control: RunControlHandle,
        prompt: String,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
    ) -> Result<()> {
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
        let Some(id) = control.steer_user_message(message) else {
            ui.set_ephemeral_error("unable to steer current turn");
            return Ok(());
        };
        let session_id = ui
            .running
            .as_ref()
            .and_then(|running| running.session_id.clone())
            .or_else(|| self.current_session.clone());
        let sequence = ui.next_pending_input_sequence();
        ui.pending_steers.push_back(PendingSteerInput {
            id,
            session_id,
            prompt,
            display_prompt,
            images,
            sequence,
        });
        Ok(())
    }

    pub(crate) fn active_gateway_turn_selector(
        &self,
        ui: &FullscreenUi<'_>,
    ) -> Option<(GatewayThreadSelector, Option<String>)> {
        let Some(running) = ui
            .running
            .as_ref()
            .filter(|running| matches!(running.task, RunningTask::Agent(_)))
        else {
            let session_id = self.current_session.as_deref()?;
            if !ui.foreign_gateway_activity_matches_current_session(Some(session_id)) {
                return None;
            }
            return Some((
                GatewayThreadSelector::thread_id(session_id),
                ui.foreign_gateway_activity_turn_id(session_id),
            ));
        };
        let selector = if let Some(selector) = running.selector.clone() {
            selector
        } else {
            let selector = GatewayThreadSelector::source(self.gateway_source().source_key());
            if !self.gateway.activity_for_selector(selector.clone()).running {
                return None;
            }
            selector
        };
        let expected_turn_id = running.turn_id.clone().or_else(|| {
            self.gateway
                .activity_for_selector(selector.clone())
                .active_turn_id
        });
        Some((selector, expected_turn_id))
    }

    pub(crate) fn queue_fullscreen_prompt(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        prompt: String,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
    ) {
        let sequence = ui.next_pending_input_sequence();
        ui.queued_inputs.push_back(QueuedInput::Prompt {
            session_id: self.current_session.clone(),
            prompt,
            display_prompt,
            images,
            sequence,
        });
    }

    pub(crate) fn queue_fullscreen_shell(&mut self, ui: &mut FullscreenUi<'_>, command: String) {
        let sequence = ui.next_pending_input_sequence();
        ui.queued_inputs.push_back(QueuedInput::Shell {
            session_id: self.current_session.clone(),
            command,
            sequence,
        });
    }

    pub(crate) fn queue_fullscreen_compaction(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        instructions: Option<String>,
        command_echo: String,
    ) {
        let sequence = ui.next_pending_input_sequence();
        ui.queued_inputs.push_back(QueuedInput::Compact {
            session_id: self.current_session.clone(),
            instructions,
            command_echo,
            sequence,
        });
    }

}
