impl TuiApp {
    async fn handle_fullscreen_command(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        command: SlashCommand,
    ) -> Result<bool> {
        match command {
            SlashCommand::Quit => return Ok(true),
            SlashCommand::Status => {
                ui.push_status(self.status_text());
            }
            SlashCommand::New => {
                self.current_session = None;
                self.current_session_title = None;
                self.force_new_once = true;
                ui.clear_transcript();
                ui.replace_session_history_prompts(Vec::new());
                ui.refresh_sidebar(self);
            }
            SlashCommand::Sessions => {
                ui.bottom_panel = Some(BottomPanel::Sessions(
                    self.session_selection_panel(SessionListView::Active)?,
                ));
            }
            SlashCommand::Stats => {
                ui.bottom_panel = Some(BottomPanel::Stats(self.stats_panel()?));
            }
            SlashCommand::ModelShow => {
                ui.bottom_panel = Some(BottomPanel::Models(self.model_selection_panel()?));
            }
            SlashCommand::VariantSet(variant) => {
                self.set_variant_no_print(variant.clone())?;
                ui.push_status(format!("variant: {variant}"));
                ui.refresh_sidebar(self);
            }
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
            SlashCommand::Rename(title) => match self.rename_session_no_print(title) {
                Ok(title) => {
                    ui.push_status(format!("session renamed: {title}"));
                    ui.refresh_sidebar(self);
                }
                Err(err) => ui.push_error(format!("error: {err:#}")),
            },
            SlashCommand::Undo => {
                if ui.request_interrupt() {
                    ui.push_error("interrupt requested; run /undo again after the turn settles");
                } else {
                    match self.undo_session_no_print(ui) {
                        Ok(message) => ui.push_status(message),
                        Err(err) => ui.push_error(format!("error: {err:#}")),
                    }
                }
            }
            SlashCommand::Redo => {
                if ui.request_interrupt() {
                    ui.push_error("interrupt requested; run /redo again after the turn settles");
                } else {
                    match self.redo_session_no_print(ui) {
                        Ok(message) => ui.push_status(message),
                        Err(err) => ui.push_error(format!("error: {err:#}")),
                    }
                }
            }
            SlashCommand::Skills => {
                ui.push_status(self.skills_status_text());
            }
            SlashCommand::SkillInvoke { name, args } => {
                let text = skill_prompt_marker(&name, &args);
                ui.textarea = textarea_with_text(&text);
                self.sync_skill_popup(ui);
            }
            SlashCommand::Upcoming(command) => {
                ui.push_status(format!("/{command} upcoming"));
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

        if record_history {
            ui.push_submitted_history(text.clone());
        }
        match parse_slash_command(&text) {
            Ok(Some(command)) => self.handle_fullscreen_command(ui, command).await,
            Ok(None) => {
                self.submit_fullscreen_prompt(ui, text)?;
                Ok(false)
            }
            Err(err) => {
                ui.push_error(format!("error: {err:#}"));
                Ok(false)
            }
        }
    }

    fn submit_fullscreen_prompt(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        prompt: String,
    ) -> Result<()> {
        if ui.running.is_some() {
            ui.queued_inputs.push_back(QueuedInput::Prompt(prompt));
            return Ok(());
        }
        self.start_fullscreen_turn(ui, prompt)
    }

    fn submit_fullscreen_shell(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        command: String,
    ) -> Result<()> {
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
                QueuedInput::Prompt(prompt) => self.start_fullscreen_turn(ui, prompt)?,
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
        match parse_slash_command(line) {
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
            SlashCommand::Quit => return Ok(true),
            SlashCommand::Status => self.show_status(),
            SlashCommand::New => {
                self.current_session = None;
                self.current_session_title = None;
                self.force_new_once = true;
                Ok(())
            }
            SlashCommand::Sessions => self.show_session_list(),
            SlashCommand::Stats => {
                println!("{}", self.stats_status_text()?);
                Ok(())
            }
            SlashCommand::ModelShow => self.show_model(),
            SlashCommand::VariantSet(variant) => self.set_variant(variant),
            SlashCommand::ModeSet(mode) => self.set_mode(mode),
            SlashCommand::ThinkingToggle => self.toggle_thinking(),
            SlashCommand::ThinkingSet(enabled) => self.set_thinking(enabled),
            SlashCommand::Rename(title) => self.rename_session(title),
            SlashCommand::Undo => self.undo_session_print(),
            SlashCommand::Redo => self.redo_session_print(),
            SlashCommand::Skills => {
                println!("{}", self.skills_status_text());
                Ok(())
            }
            SlashCommand::SkillInvoke { name, args } => {
                let prompt = skill_prompt_marker(&name, &args);
                return self.submit_prompt(prompt).await.map(|_| false);
            }
            SlashCommand::Upcoming(command) => {
                println!("{}", self.renderer.status(&format!("/{command} upcoming")));
                Ok(())
            }
        };
        if let Err(err) = result {
            self.had_error = true;
            eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
        }
        Ok(false)
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
        if result.outcome != Outcome::Normal || result.tool_failures > 0 {
            self.had_error = true;
        }
        Ok(())
    }

    fn start_fullscreen_turn(&mut self, ui: &mut FullscreenUi<'_>, prompt: String) -> Result<()> {
        if ui.running.is_some() {
            ui.queued_inputs.push_back(QueuedInput::Prompt(prompt));
            return Ok(());
        }
        ui.push_user(prompt.clone());
        let (tx, rx) = mpsc::unbounded_channel();
        let sink: RunStreamSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let (control_handle, control) = run_control();
        let options = self.run_options(prompt);
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

}

fn skill_prompt_marker(name: &str, args: &str) -> String {
    if args.trim().is_empty() {
        format!("${name} ")
    } else {
        format!("${name} {}", args.trim())
    }
}
