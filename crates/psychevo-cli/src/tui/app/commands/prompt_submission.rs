#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) enum SubmittedSlashInput {
    Command(SlashCommand),
    PassThroughPrompt(String),
    NotSlash,
}

impl TuiApp {
    pub(crate) fn classify_submitted_slash_input(&self, text: &str) -> Result<SubmittedSlashInput> {
        if !should_parse_slash_command_input(text) {
            return Ok(SubmittedSlashInput::NotSlash);
        }
        match parse_tui_slash_with_config(text, &self.slash_config)? {
            TuiSlashParse::NotSlash => Ok(SubmittedSlashInput::NotSlash),
            TuiSlashParse::Unknown { original, .. } => {
                Ok(SubmittedSlashInput::PassThroughPrompt(original))
            }
            TuiSlashParse::Command(SlashCommand::SkillInvoke { name, args })
                if self.skill_or_bundle_marker(&name, &args).is_none() =>
            {
                Ok(SubmittedSlashInput::PassThroughPrompt(text.to_string()))
            }
            TuiSlashParse::Command(command) => Ok(SubmittedSlashInput::Command(command)),
        }
    }

    #[cfg(test)]
    pub(crate) async fn handle_fullscreen_command(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        command: SlashCommand,
    ) -> Result<bool> {
        self.handle_fullscreen_command_with_echo(ui, command, None)
            .await
    }

    pub(crate) async fn handle_fullscreen_command_with_echo(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        command: SlashCommand,
        submitted: Option<String>,
    ) -> Result<bool> {
        let command_echo = submitted
            .as_deref()
            .map(normalize_submitted_slash_echo)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| slash_command_echo(&command));
        if submitted.is_some() {
            ui.scroll_to_bottom();
        }
        if let Some(message) = self.side_command_rejection(&command) {
            ui.push_command_result(command_echo, None, message, true);
            return Ok(false);
        }
        match command {
            SlashCommand::Help => {
                ui.bottom_panel = Some(BottomPanel::Help(self.help_panel()));
            }
            SlashCommand::Quit => return Ok(true),
            SlashCommand::Status => {
                ui.push_command_result(command_echo, None, self.status_text(), false);
            }
            SlashCommand::New => {
                self.detach_running_for_session_switch(ui, None);
                self.begin_new_session_draft();
                self.current_agent = self.startup_agent.clone();
                self.current_agent_explicit_default = false;
                ui.clear_transcript();
                ui.replace_session_history_prompts(Vec::new());
                ui.refresh_sidebar(self);
            }
            SlashCommand::Sessions => {
                ui.bottom_panel = Some(BottomPanel::Sessions(
                    self.session_selection_panel(SessionListView::Active)?,
                ));
            }
            SlashCommand::Usage => {
                ui.bottom_panel = Some(BottomPanel::Stats(self.stats_panel()?));
            }
            SlashCommand::Context => {
                let format_options = ContextFormatOptions {
                    heading: false,
                    bar_width: Some(fullscreen_context_bar_width(ui)),
                };
                let live = ui.last_context_snapshot.clone();
                match self.context_status_snapshot(live.as_ref()) {
                    Ok(snapshot) => {
                        self.last_context_snapshot = Some(snapshot.clone());
                        ui.last_context_snapshot = Some(snapshot.clone());
                        let text =
                            format_context_snapshot_text_with_options(&snapshot, format_options);
                        ui.push_command_result(command_echo, Some("Context Usage"), text, false);
                        ui.refresh_sidebar(self);
                    }
                    Err(err) => {
                        ui.push_command_result(command_echo, None, format!("error: {err:#}"), true);
                    }
                }
            }
            SlashCommand::Diff => {
                ui.diff_overlay = Some(DiffOverlay::computing());
                self.start_diff_task();
            }
            SlashCommand::Refresh => {
                if ui.status_has_running(self.current_session.as_deref()) {
                    ui.push_command_result(
                        command_echo,
                        None,
                        "error: finish the current turn before refreshing",
                        true,
                    );
                    return Ok(false);
                }
                match self.reload_context_for_current_session(ui) {
                    Ok(result) => {
                        let scheduled = self.start_side_cleanup_task();
                        let cleanup = if scheduled {
                            "side cleanup scheduled"
                        } else {
                            "side cleanup already running"
                        };
                        ui.push_command_result(
                            command_echo,
                            None,
                            format!(
                                "reloaded context: {} v{}; {cleanup}",
                                result.prefix_hash, result.version
                            ),
                            false,
                        );
                        ui.refresh_sidebar(self);
                    }
                    Err(err) => {
                        ui.push_command_result(command_echo, None, format!("error: {err:#}"), true);
                    }
                }
            }
            SlashCommand::ReloadContextDeprecated => {
                ui.push_command_result(command_echo, None, RELOAD_CONTEXT_DEPRECATED_MESSAGE, true);
            }
            SlashCommand::Btw(prompt) => {
                self.start_btw_side_conversation(ui, prompt)?;
            }
            SlashCommand::Steer(message) => {
                self.submit_explicit_fullscreen_steer(ui, message, command_echo)?;
            }
            SlashCommand::Queue(message) => {
                self.submit_fullscreen_queue(ui, message)?;
            }
            SlashCommand::PendingCancel => {
                self.cancel_pending_fullscreen_inputs(ui);
            }
            SlashCommand::ModelShowScoped { global } => {
                ui.bottom_panel = Some(BottomPanel::Models(ModelPanel::new_with_scope(
                    self.model_selection_panel()?,
                    global,
                )));
            }
            SlashCommand::VariantSet(variant) => match self.set_variant_no_print(variant.clone()) {
                Ok(()) => {
                    ui.push_command_result(
                        command_echo,
                        None,
                        format!("variant: {variant}"),
                        false,
                    );
                    ui.refresh_sidebar(self);
                }
                Err(err) => {
                    ui.push_command_result(command_echo, None, format!("error: {err:#}"), true);
                }
            },
            SlashCommand::ModeSet(mode) => {
                self.set_mode_no_print(&mode)?;
                ui.refresh_sidebar(self);
            }
            SlashCommand::Permissions => {
                ui.push_command_result(command_echo, None, self.permissions_status_text()?, false);
            }
            SlashCommand::Sandbox => {
                ui.push_command_result(command_echo, None, self.sandbox_status_text()?, false);
            }
            SlashCommand::ThinkingToggle => {
                let enabled = !self.thinking_visible;
                self.set_thinking_no_print(enabled)?;
                ui.set_thinking_visible(enabled);
                ui.refresh_sidebar(self);
            }
            SlashCommand::ThinkingSet(enabled) => {
                self.set_thinking_no_print(enabled)?;
                ui.set_thinking_visible(enabled);
                ui.refresh_sidebar(self);
            }
            SlashCommand::RawToggle => {
                let enabled = !self.raw_visible;
                self.set_raw_no_print(enabled)?;
                ui.set_raw_visible(enabled);
                ui.refresh_sidebar(self);
            }
            SlashCommand::RawSet(enabled) => {
                self.set_raw_no_print(enabled)?;
                ui.set_raw_visible(enabled);
                ui.refresh_sidebar(self);
            }
            SlashCommand::Copy => {
                self.copy_latest_answer_markdown(ui);
            }
            SlashCommand::Export(options) => match self.write_tui_export(&options) {
                Ok(result) => ui.push_command_result(
                    command_echo,
                    None,
                    format!("exported: {}", result.path.display()),
                    false,
                ),
                Err(err) => {
                    ui.push_command_result(command_echo, None, format!("error: {err:#}"), true)
                }
            },
            SlashCommand::Share(options) => match self.write_tui_share(&options) {
                Ok(result) => ui.push_command_result(
                    command_echo,
                    None,
                    format!("share: {}", result.path.display()),
                    false,
                ),
                Err(err) => {
                    ui.push_command_result(command_echo, None, format!("error: {err:#}"), true)
                }
            },
            SlashCommand::Image { source, prompt } => {
                match resolve_image_source(&source, &self.workdir) {
                    Ok(image) => {
                        let placeholder = ui.add_pending_image(image);
                        let prompt = prompt.trim();
                        let text = if prompt.is_empty() {
                            placeholder
                        } else {
                            format!("{placeholder} {prompt}")
                        };
                        ui.set_composer_text(&text);
                        ui.clear_slash_menu_dismissal();
                        ui.close_file_popup();
                        ui.close_agent_popup();
                        ui.close_skill_popup();
                    }
                    Err(err) => {
                        ui.push_command_result(command_echo, None, format!("error: {err:#}"), true);
                    }
                }
            }
            SlashCommand::Rename(title) => match self.rename_session_no_print(title) {
                Ok(title) => {
                    ui.push_command_result(
                        command_echo,
                        None,
                        format!("session renamed: {title}"),
                        false,
                    );
                    ui.refresh_sidebar(self);
                }
                Err(err) => {
                    ui.push_command_result(command_echo, None, format!("error: {err:#}"), true);
                }
            },
            SlashCommand::Undo => {
                if self.request_current_session_interrupt(ui) {
                    ui.push_command_result(
                        command_echo,
                        None,
                        "error: interrupt requested; run /undo again after the turn settles",
                        true,
                    );
                } else {
                    match self.undo_session_no_print(ui) {
                        Ok(message) => ui.push_command_result(command_echo, None, message, false),
                        Err(err) => ui.push_command_result(
                            command_echo,
                            None,
                            format!("error: {err:#}"),
                            true,
                        ),
                    }
                }
            }
            SlashCommand::Redo => {
                if self.request_current_session_interrupt(ui) {
                    ui.push_command_result(
                        command_echo,
                        None,
                        "error: interrupt requested; run /redo again after the turn settles",
                        true,
                    );
                } else {
                    match self.redo_session_no_print(ui) {
                        Ok(message) => ui.push_command_result(command_echo, None, message, false),
                        Err(err) => ui.push_command_result(
                            command_echo,
                            None,
                            format!("error: {err:#}"),
                            true,
                        ),
                    }
                }
            }
            SlashCommand::Skills(args) => {
                ui.push_command_result(
                    command_echo,
                    None,
                    self.skills_command_text(args.as_deref()),
                    false,
                );
            }
            SlashCommand::Tools => {
                ui.bottom_panel = Some(BottomPanel::Tools(self.toolsets_panel()?));
            }
            SlashCommand::Bundles(args) => {
                ui.push_command_result(
                    command_echo,
                    None,
                    self.bundles_command_text(args.as_deref()),
                    false,
                );
            }
            SlashCommand::Curator(args) => {
                ui.push_command_result(
                    command_echo,
                    None,
                    self.curator_command_text(args.as_deref()),
                    false,
                );
            }
            SlashCommand::Agents => {
                ui.bottom_panel = Some(BottomPanel::Agents(self.agent_panel()));
            }
            SlashCommand::Fork(prompt) => {
                let text = fork_prompt_marker(&prompt);
                self.submit_fullscreen_prompt(ui, text, Vec::new())?;
            }
            SlashCommand::Compact(instructions) => {
                self.submit_fullscreen_compaction(ui, instructions, command_echo)?;
            }
            SlashCommand::SkillInvoke { name, args } => {
                if let Some(text) = self.skill_or_bundle_marker(&name, &args) {
                    self.submit_fullscreen_prompt_with_display(ui, text, command_echo, Vec::new())?;
                } else {
                    ui.push_command_result(
                        command_echo,
                        None,
                        format!("error: unknown skill or bundle: {name}"),
                        true,
                    );
                }
            }
            SlashCommand::Upcoming(command) => {
                ui.push_command_result(
                    command_echo,
                    None,
                    format!("/{command} is upcoming; no session changes made"),
                    false,
                );
            }
        }
        Ok(false)
    }

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
        let running = ui
            .running
            .as_ref()
            .filter(|running| matches!(running.task, RunningTask::Agent(_)))?;
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
