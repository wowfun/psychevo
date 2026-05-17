impl TuiApp {
    #[cfg(test)]
    async fn handle_fullscreen_command(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        command: SlashCommand,
    ) -> Result<bool> {
        self.handle_fullscreen_command_with_echo(ui, command, None)
            .await
    }

    async fn handle_fullscreen_command_with_echo(
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
        match command {
            SlashCommand::Help => {
                ui.bottom_panel = Some(BottomPanel::Help(self.help_panel()));
            }
            SlashCommand::Quit => return Ok(true),
            SlashCommand::Status => {
                ui.push_command_result(command_echo, None, self.status_text(), false);
            }
            SlashCommand::New => {
                self.current_session = None;
                self.current_session_title = None;
                self.force_new_once = true;
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
            SlashCommand::ReloadContext => match self.reload_context_for_current_session(ui) {
                Ok(result) => {
                    ui.push_command_result(
                        command_echo,
                        None,
                        format!("reloaded context: {} v{}", result.prefix_hash, result.version),
                        false,
                    );
                    ui.refresh_sidebar(self);
                }
                Err(err) => {
                    ui.push_command_result(command_echo, None, format!("error: {err:#}"), true);
                }
            },
            SlashCommand::ModelShow => {
                ui.bottom_panel = Some(BottomPanel::Models(ModelPanel::new(
                    self.model_selection_panel()?,
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
            SlashCommand::Skills => {
                ui.push_command_result(command_echo, None, self.skills_status_text(), false);
            }
            SlashCommand::Agents => {
                ui.bottom_panel = Some(BottomPanel::Agents(self.agent_panel()));
            }
            SlashCommand::Fork(prompt) => {
                let text = fork_prompt_marker(&prompt);
                self.submit_fullscreen_prompt(ui, text, Vec::new())?;
            }
            SlashCommand::SkillInvoke { name, args } => {
                let text = skill_prompt_marker(&name, &args);
                ui.set_composer_text(&text);
                self.sync_skill_popup(ui);
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

    async fn submit_fullscreen_text(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        text: String,
        record_history: bool,
    ) -> Result<bool> {
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
        let slash_command = if should_parse_slash_command_input(&text) {
            parse_slash_command(&text)
        } else {
            Ok(None)
        };
        match slash_command {
            Ok(Some(command)) => {
                self.handle_fullscreen_command_with_echo(ui, command, Some(text))
                    .await
            }
            Ok(None) => {
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

    fn submit_fullscreen_prompt(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
    ) -> Result<()> {
        let prompt = prompt_without_image_placeholders(&display_prompt, &images);
        if ui.running.is_some() {
            ui.queued_inputs.push_back(QueuedInput::Prompt {
                prompt,
                display_prompt,
                images,
            });
            return Ok(());
        }
        self.start_fullscreen_turn(ui, prompt, display_prompt, images)
    }

    fn submit_fullscreen_shell(
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
            ui.queued_inputs.push_back(QueuedInput::Shell(command));
            return Ok(());
        }
        self.start_fullscreen_shell(ui, command)
    }

    fn start_next_queued_input(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        while ui.running.is_none() {
            let Some(next) = ui.queued_inputs.pop_front() else {
                break;
            };
            match next {
                QueuedInput::Prompt {
                    prompt,
                    display_prompt,
                    images,
                } => self.start_fullscreen_turn(ui, prompt, display_prompt, images)?,
                QueuedInput::Shell(command) => self.start_fullscreen_shell(ui, command)?,
            }
        }
        Ok(())
    }

    async fn handle_line(&mut self, line: &str) -> Result<bool> {
        if let Some(shell) = parse_shell_escape_input(line) {
            if let Err(err) = self.submit_shell_command(shell.command).await {
                self.had_error = true;
                eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
            }
            return Ok(false);
        }
        let slash_command = if should_parse_slash_command_input(line) {
            parse_slash_command(line)
        } else {
            Ok(None)
        };
        match slash_command {
            Ok(Some(command)) => self.handle_command(command).await,
            Ok(None) => {
                if let Err(err) = self.submit_prompt(line.to_string()).await {
                    self.had_error = true;
                    eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
                }
                Ok(false)
            }
            Err(err) => {
                self.had_error = true;
                eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
                Ok(false)
            }
        }
    }

    async fn handle_command(&mut self, command: SlashCommand) -> Result<bool> {
        let result = match command {
            SlashCommand::Help => {
                println!("{}", self.help_status_text());
                Ok(())
            }
            SlashCommand::Quit => return Ok(true),
            SlashCommand::Status => self.show_status(),
            SlashCommand::New => {
                self.current_session = None;
                self.current_session_title = None;
                self.force_new_once = true;
                Ok(())
            }
            SlashCommand::Sessions => self.show_session_list(),
            SlashCommand::Usage => {
                println!("{}", self.stats_status_text()?);
                Ok(())
            }
            SlashCommand::Context => {
                let live = self.last_context_snapshot.clone();
                let snapshot = self.context_status_snapshot(live.as_ref())?;
                self.last_context_snapshot = Some(snapshot.clone());
                println!(
                    "{}",
                    format_context_snapshot_text_with_options(
                        &snapshot,
                        ContextFormatOptions {
                            heading: true,
                            bar_width: None,
                        },
                    )
                );
                Ok(())
            }
            SlashCommand::ReloadContext => {
                let session = self
                    .current_session
                    .clone()
                    .ok_or_else(|| anyhow!("no session context yet"))?;
                let result = reload_session_context(ReloadContextOptions {
                    db_path: self.db_path.clone(),
                    session,
                    config_path: self.config_path.clone(),
                    mode: Some(self.current_mode),
                    inherited_env: Some(self.env_map.clone()),
                    agent: self.current_agent.clone(),
                    no_agents: self.no_agents,
                    no_skills: self.no_skills,
                    invalidation_reason: "manual_reload".to_string(),
                    notice: None,
                })?;
                println!("reloaded context: {} v{}", result.prefix_hash, result.version);
                Ok(())
            }
            SlashCommand::ModelShow => self.show_model(),
            SlashCommand::VariantSet(variant) => self.set_variant(variant),
            SlashCommand::ModeSet(mode) => self.set_mode(mode),
            SlashCommand::ThinkingToggle => self.toggle_thinking(),
            SlashCommand::ThinkingSet(enabled) => self.set_thinking(enabled),
            SlashCommand::RawToggle => self.toggle_raw(),
            SlashCommand::RawSet(enabled) => self.set_raw(enabled),
            SlashCommand::Copy => self.copy_latest_answer_markdown_scripted(),
            SlashCommand::Export(options) => self
                .write_tui_export(&options)
                .map(|result| println!("exported: {}", result.path.display())),
            SlashCommand::Share(options) => self
                .write_tui_share(&options)
                .map(|result| println!("share: {}", result.path.display())),
            SlashCommand::Image { .. } => {
                Err(anyhow!("/image is only available in fullscreen TUI"))
            }
            SlashCommand::Rename(title) => self.rename_session(title),
            SlashCommand::Undo => self.undo_session_print(),
            SlashCommand::Redo => self.redo_session_print(),
            SlashCommand::Skills => {
                println!("{}", self.skills_status_text());
                Ok(())
            }
            SlashCommand::Agents => {
                println!("{}", self.agents_status_text());
                Ok(())
            }
            SlashCommand::Fork(prompt) => {
                let prompt = fork_prompt_marker(&prompt);
                return self.submit_prompt(prompt).await.map(|_| false);
            }
            SlashCommand::SkillInvoke { name, args } => {
                let prompt = skill_prompt_marker(&name, &args);
                return self.submit_prompt(prompt).await.map(|_| false);
            }
            SlashCommand::Upcoming(command) => {
                println!(
                    "{}",
                    self.renderer
                        .status(&format!("/{command} is upcoming; no session changes made"))
                );
                Ok(())
            }
        };
        if let Err(err) = result {
            self.had_error = true;
            eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
        }
        Ok(false)
    }

    fn image_submission_degrades_to_text(&self, prompt: &str, images: &[ImageInput]) -> bool {
        let has_image = !images.is_empty();
        let _ = prompt;
        has_image
            && self.selected_model.as_ref().is_some_and(|model| {
                model_metadata_explicitly_disallows_image_input(&model.metadata)
            })
    }

    fn skills_status_text(&self) -> String {
        let Some(catalog) = self.current_skill_catalog() else {
            return "No skills found.".to_string();
        };
        if catalog.skills.is_empty() {
            return "No skills found.".to_string();
        }
        catalog
            .skills
            .iter()
            .map(|skill| format!("{}: {}", skill.name, skill.description))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn agents_status_text(&self) -> String {
        let Some(catalog) = self.current_agent_catalog() else {
            return "Agents disabled.".to_string();
        };
        let mut sections = Vec::new();
        if catalog.agents.is_empty() {
            sections.push("Library\nNo agents found.".to_string());
        } else {
            sections.push(format!(
                "Library\n{}",
                catalog
                    .agents
                    .iter()
                    .map(|agent| format!("{}: {}", agent.name, agent.description))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if let Some(parent) = self.current_session.as_deref()
            && let Ok(store) = SqliteStore::open(&self.db_path)
        {
            let value = agent_status_value(Some(&store), Some(parent), false);
            let running = value
                .get("agents")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if running.is_empty() {
                sections.push("Running/Completed\nNo child agents for this session.".to_string());
            } else {
                sections.push(format!(
                    "Running/Completed\n{}",
                    running
                        .iter()
                        .map(|agent| format!(
                            "{}\t{}\t{}",
                            agent.get("id").and_then(Value::as_str).unwrap_or_default(),
                            agent
                                .get("agent_name")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                            agent
                                .get("status")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
        }
        sections.join("\n\n")
    }

    fn help_status_text(&self) -> String {
        format_slash_help(self.current_skill_count())
    }

    fn help_panel(&self) -> HelpPanel {
        HelpPanel::new(self.current_skill_count())
    }

    fn current_skill_count(&self) -> Option<usize> {
        self.current_skill_catalog()
            .map(|catalog| catalog.skills.len())
    }

    fn stats_status_text(&self) -> Result<String> {
        let report = usage_stats(StatsOptions {
            db_path: self.db_path.clone(),
            workdir: self.workdir.clone(),
            all: false,
            days: None,
            limit: 5,
        })?;
        let totals = report.get("totals").unwrap_or(&Value::Null);
        Ok(format!(
            "sessions: {}  messages: {}  tokens: {}  cost: {}",
            json_i64(totals, "sessions"),
            json_i64(totals, "messages"),
            json_i64(totals, "reported_total_tokens"),
            format_nanodollars(json_i64(totals, "estimated_cost_nanodollars"))
        ))
    }

    fn write_tui_export(
        &self,
        options: &crate::tui::slash::TuiExportOptions,
    ) -> Result<SessionExportWriteResult> {
        let session_id = self
            .current_session
            .as_deref()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        let output = self.resolve_tui_export_path(
            options.path.as_deref(),
            options.format,
            SessionArtifactKind::Export,
            session_id,
        );
        let store = SqliteStore::open(&self.db_path)?;
        Ok(write_session_export(
            &store,
            session_id,
            &output,
            SessionExportOptions {
                format: options.format,
                include: options.include.clone(),
                artifact_kind: SessionArtifactKind::Export,
            },
        )?)
    }

    fn write_tui_share(
        &self,
        options: &crate::tui::slash::TuiShareOptions,
    ) -> Result<SessionExportWriteResult> {
        let session_id = self
            .current_session
            .as_deref()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        let output = self.resolve_tui_export_path(
            options.path.as_deref(),
            SessionExportFormat::Markdown,
            SessionArtifactKind::Share,
            session_id,
        );
        let store = SqliteStore::open(&self.db_path)?;
        Ok(write_session_export(
            &store,
            session_id,
            &output,
            SessionExportOptions {
                format: SessionExportFormat::Markdown,
                include: options.include.clone(),
                artifact_kind: SessionArtifactKind::Share,
            },
        )?)
    }

    fn resolve_tui_export_path(
        &self,
        path: Option<&str>,
        format: SessionExportFormat,
        artifact_kind: SessionArtifactKind,
        session_id: &str,
    ) -> PathBuf {
        let path = path.map(PathBuf::from).unwrap_or_else(|| {
            self.workdir.join(default_session_export_filename(
                session_id,
                format,
                artifact_kind,
            ))
        });
        if path.is_absolute() {
            path
        } else {
            self.workdir.join(path)
        }
    }

    fn context_status_snapshot(&self, live: Option<&ContextSnapshot>) -> Result<ContextSnapshot> {
        if let Some(snapshot) = live {
            return Ok(snapshot.clone());
        }
        if let Some(snapshot) = self.last_context_snapshot.as_ref() {
            return Ok(snapshot.clone());
        }
        let session = self
            .current_session
            .clone()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        Ok(context_snapshot(ContextOptions {
            db_path: self.db_path.clone(),
            workdir: self.workdir.clone(),
            session,
            config_path: self.config_path.clone(),
            inherited_env: Some(self.env_map.clone()),
        })?)
    }

    fn reload_context_for_current_session(
        &self,
        ui: &FullscreenUi<'_>,
    ) -> Result<psychevo_runtime::ReloadContextResult> {
        if ui.running.is_some() {
            return Err(anyhow!("finish the current turn before reloading context"));
        }
        let session = self
            .current_session
            .clone()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        Ok(reload_session_context(ReloadContextOptions {
            db_path: self.db_path.clone(),
            session,
            config_path: self.config_path.clone(),
            mode: Some(self.current_mode),
            inherited_env: Some(self.env_map.clone()),
            agent: self.current_agent.clone(),
            no_agents: self.no_agents,
            no_skills: self.no_skills,
            invalidation_reason: "manual_reload".to_string(),
            notice: None,
        })?)
    }

    async fn submit_prompt(&mut self, prompt: String) -> Result<()> {
        let stdout = Arc::new(Mutex::new(io::stdout()));
        let turn = Arc::new(Mutex::new(TurnPrinter::new(
            self.renderer,
            self.thinking_visible,
            self.debug,
        )));
        {
            let mut stdout = stdout.lock().expect("stdout lock poisoned");
            writeln!(stdout, "Prompt: {prompt}")?;
        }
        let turn_for_sink = Arc::clone(&turn);
        let stdout_for_sink = Arc::clone(&stdout);
        let sink: RunStreamSink = Arc::new(move |event| {
            let mut turn = turn_for_sink.lock().expect("turn lock poisoned");
            let mut stdout = stdout_for_sink.lock().expect("stdout lock poisoned");
            let _ = turn.render_event(&event, &mut *stdout);
        });
        let options = self.run_options(prompt);
        let result = run_live_streaming(options, "tui", TUI_SESSION_SOURCES, sink).await?;
        self.last_context_snapshot = result.context_snapshot.clone();
        {
            let mut turn = turn.lock().expect("turn lock poisoned");
            let mut stdout = stdout.lock().expect("stdout lock poisoned");
            turn.finish(&mut *stdout)?;
        }
        self.current_session = Some(result.session_id);
        self.refresh_current_session_title()?;
        self.force_new_once = false;
        let success = result.outcome == Outcome::Normal && result.tool_failures == 0;
        if !success {
            self.had_error = true;
        }
        Ok(())
    }

    async fn submit_shell_command(&mut self, command: String) -> Result<()> {
        if command.trim().is_empty() {
            println!("{}", self.renderer.status(USER_SHELL_HELP));
            return Ok(());
        }
        let stdout = Arc::new(Mutex::new(io::stdout()));
        let turn = Arc::new(Mutex::new(TurnPrinter::new(
            self.renderer,
            self.thinking_visible,
            self.debug,
        )));
        let turn_for_sink = Arc::clone(&turn);
        let stdout_for_sink = Arc::clone(&stdout);
        let sink: RunStreamSink = Arc::new(move |event| {
            let mut turn = turn_for_sink.lock().expect("turn lock poisoned");
            let mut stdout = stdout_for_sink.lock().expect("stdout lock poisoned");
            let _ = turn.render_event(&event, &mut *stdout);
        });
        let (_control_handle, control) = run_control();
        let result = run_user_shell_command_streaming_controlled(
            UserShellOptions {
                workdir: self.workdir.clone(),
                command,
                context: Some(self.user_shell_context_options()),
                inject_into: None,
            },
            sink,
            control,
        )
        .await?;
        {
            let mut turn = turn.lock().expect("turn lock poisoned");
            let mut stdout = stdout.lock().expect("stdout lock poisoned");
            turn.finish(&mut *stdout)?;
        }
        if let Some(session_id) = result.session_id {
            self.current_session = Some(session_id);
            self.refresh_current_session_title()?;
            self.force_new_once = false;
        }
        if result.outcome != Outcome::Normal || result.tool_failures > 0 {
            self.had_error = true;
        }
        Ok(())
    }

    fn start_fullscreen_turn(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        prompt: String,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
    ) -> Result<()> {
        if ui.running.is_some() {
            ui.queued_inputs.push_back(QueuedInput::Prompt {
                prompt,
                display_prompt,
                images,
            });
            return Ok(());
        }
        let image_inputs = images
            .iter()
            .map(|attachment| attachment.image.clone())
            .collect::<Vec<_>>();
        if self.image_submission_degrades_to_text(&prompt, &image_inputs) {
            ui.set_ephemeral_error(
                "selected model does not support image input; sent image source as text",
            );
        }
        ui.push_user_with_images(display_prompt.clone(), &images);
        let (tx, rx) = mpsc::unbounded_channel();
        let sink: RunStreamSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let (control_handle, control) = run_control();
        let mut options = self.run_options_with_images(prompt, image_inputs);
        options.prompt_display = prompt_display_metadata(display_prompt, &images, &self.workdir);
        let task = tokio::spawn(async move {
            run_live_streaming_controlled(options, "tui", TUI_SESSION_SOURCES, sink, control).await
        });
        ui.scroll_to_bottom();
        ui.running = Some(RunningTurn {
            control: control_handle,
            rx,
            task: RunningTask::Agent(task),
        });
        ui.start_assistant();
        ui.refresh_sidebar(self);
        Ok(())
    }

    fn start_fullscreen_shell(&mut self, ui: &mut FullscreenUi<'_>, command: String) -> Result<()> {
        if ui.running.is_some() {
            ui.queued_inputs.push_back(QueuedInput::Shell(command));
            return Ok(());
        }
        if command.trim().is_empty() {
            ui.push_status(USER_SHELL_HELP);
            return Ok(());
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let sink: RunStreamSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let (control_handle, control) = run_control();
        let options = UserShellOptions {
            workdir: self.workdir.clone(),
            command,
            context: Some(self.user_shell_context_options()),
            inject_into: None,
        };
        let task = tokio::spawn(async move {
            run_user_shell_command_streaming_controlled(options, sink, control).await
        });
        ui.scroll_to_bottom();
        ui.running = Some(RunningTurn {
            control: control_handle,
            rx,
            task: RunningTask::UserShell(task),
        });
        ui.start_assistant();
        ui.refresh_sidebar(self);
        Ok(())
    }

    fn start_auxiliary_fullscreen_shell(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        command: String,
    ) -> Result<()> {
        if command.trim().is_empty() {
            ui.push_status(USER_SHELL_HELP);
            return Ok(());
        }
        let Some(inject_into) = ui.running.as_ref().map(|running| running.control.clone()) else {
            return self.start_fullscreen_shell(ui, command);
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let sink: RunStreamSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let (control_handle, control) = run_control();
        let options = UserShellOptions {
            workdir: self.workdir.clone(),
            command,
            context: Some(self.user_shell_context_options()),
            inject_into: Some(inject_into),
        };
        let task = tokio::spawn(async move {
            run_user_shell_command_streaming_controlled(options, sink, control).await
        });
        ui.scroll_to_bottom();
        ui.auxiliary_shell_tasks.push(AuxiliaryShellTask {
            control: control_handle,
            rx,
            task,
        });
        ui.refresh_sidebar(self);
        Ok(())
    }

    fn start_pending_auxiliary_shells(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        if self.current_session.is_none()
            || ui.turn_started.is_none()
            || !ui
                .running
                .as_ref()
                .is_some_and(|running| matches!(running.task, RunningTask::Agent(_)))
        {
            return Ok(());
        }
        while let Some(command) = ui.pending_auxiliary_shell_commands.pop_front() {
            self.start_auxiliary_fullscreen_shell(ui, command)?;
        }
        Ok(())
    }
}

fn fullscreen_context_bar_width(ui: &FullscreenUi<'_>) -> usize {
    if ui.last_transcript_width == 0 {
        return 80;
    }
    normalize_context_bar_width(usize::from(ui.last_transcript_width).saturating_sub(8))
}

fn normalize_submitted_slash_echo(value: &str) -> String {
    value.lines().next().unwrap_or_default().trim().to_string()
}

fn slash_command_echo(command: &SlashCommand) -> String {
    match command {
        SlashCommand::Help => "/help".to_string(),
        SlashCommand::Quit => "/quit".to_string(),
        SlashCommand::Status => "/status".to_string(),
        SlashCommand::New => "/new".to_string(),
        SlashCommand::Sessions => "/sessions".to_string(),
        SlashCommand::Usage => "/usage".to_string(),
        SlashCommand::Context => "/context".to_string(),
        SlashCommand::ReloadContext => "/reload-context".to_string(),
        SlashCommand::ModelShow => "/model".to_string(),
        SlashCommand::VariantSet(variant) => format!("/variant {variant}"),
        SlashCommand::ModeSet(mode) => format!("/mode {mode}"),
        SlashCommand::ThinkingToggle => "/show-thinking".to_string(),
        SlashCommand::ThinkingSet(enabled) => {
            format!("/show-thinking {}", if *enabled { "on" } else { "off" })
        }
        SlashCommand::RawToggle => "/show-raw".to_string(),
        SlashCommand::RawSet(enabled) => {
            format!("/show-raw {}", if *enabled { "on" } else { "off" })
        }
        SlashCommand::Copy => "/copy".to_string(),
        SlashCommand::Export(options) => {
            let mut parts = vec!["/export".to_string()];
            if let Some(path) = &options.path {
                parts.push(path.clone());
            }
            if options.format == SessionExportFormat::Json {
                parts.push("--format json".to_string());
            }
            if options.include
                != psychevo_runtime::SessionExportIncludeSet::default_for(
                    SessionArtifactKind::Export,
                )
            {
                parts.push(format!("--include {}", options.include.tokens().join(",")));
            }
            parts.join(" ")
        }
        SlashCommand::Share(options) => {
            let mut parts = vec!["/share".to_string()];
            if let Some(path) = &options.path {
                parts.push(path.clone());
            }
            if options.include
                != psychevo_runtime::SessionExportIncludeSet::default_for(
                    SessionArtifactKind::Share,
                )
            {
                parts.push(format!("--include {}", options.include.tokens().join(",")));
            }
            parts.join(" ")
        }
        SlashCommand::Image { source, prompt } => {
            if prompt.trim().is_empty() {
                format!("/image {source}")
            } else {
                format!("/image {source} {}", prompt.trim())
            }
        }
        SlashCommand::Rename(title) => {
            format!(
                "/rename {}",
                title.split_whitespace().collect::<Vec<_>>().join(" ")
            )
        }
        SlashCommand::Undo => "/undo".to_string(),
        SlashCommand::Redo => "/redo".to_string(),
        SlashCommand::Skills => "/skills".to_string(),
        SlashCommand::Agents => "/agents".to_string(),
        SlashCommand::Fork(prompt) => format!("/fork {}", prompt.trim()),
        SlashCommand::SkillInvoke { name, args } => {
            if args.trim().is_empty() {
                format!("/skill:{name}")
            } else {
                format!("/skill:{name} {}", args.trim())
            }
        }
        SlashCommand::Upcoming(command) => format!("/{command}"),
    }
}

fn skill_prompt_marker(name: &str, args: &str) -> String {
    if args.trim().is_empty() {
        format!("${name} ")
    } else {
        format!("${name} {}", args.trim())
    }
}

fn fork_prompt_marker(prompt: &str) -> String {
    format!(
        "Use the Agent tool with agent_type=\"general\", fork_context=true, and background=true for this task:\n\n{}",
        prompt.trim()
    )
}
