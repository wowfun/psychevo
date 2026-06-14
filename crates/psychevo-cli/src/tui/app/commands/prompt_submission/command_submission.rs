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
}
