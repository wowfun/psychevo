#[allow(unused_imports)]
pub(crate) use super::*;
impl TuiApp {
    pub(crate) async fn submit_shell_command(&mut self, command: String) -> Result<()> {
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
            self.reset_live_agent_reload_poll();
            self.refresh_current_session_title()?;
            self.force_new_once = false;
        }
        if result.outcome != Outcome::Normal || result.tool_failures > 0 {
            self.had_error = true;
        }
        Ok(())
    }

    pub(crate) fn start_fullscreen_turn(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        prompt: String,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
    ) -> Result<()> {
        if ui.running.is_some() || self.compaction_task.is_some() {
            self.queue_fullscreen_prompt(ui, prompt, display_prompt, images);
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
            session_id: self.current_session.clone(),
            control: control_handle,
            rx,
            task: RunningTask::Agent(task),
        });
        ui.start_assistant();
        ui.refresh_sidebar(self);
        Ok(())
    }

    pub(crate) fn start_fullscreen_shell(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        command: String,
    ) -> Result<()> {
        if ui.running.is_some() || self.compaction_task.is_some() {
            self.queue_fullscreen_shell(ui, command);
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
            session_id: self.current_session.clone(),
            control: control_handle,
            rx,
            task: RunningTask::UserShell(task),
        });
        ui.start_assistant();
        ui.refresh_sidebar(self);
        Ok(())
    }

    pub(crate) fn start_auxiliary_fullscreen_shell(
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
            session_id: self.current_session.clone(),
            control: control_handle,
            rx,
            task,
        });
        ui.refresh_sidebar(self);
        Ok(())
    }

    pub(crate) fn start_pending_auxiliary_shells(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<()> {
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

    pub(crate) fn submit_fullscreen_compaction(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        instructions: Option<String>,
        command_echo: String,
    ) -> Result<()> {
        if self.current_session.is_none() {
            ui.push_command_result(command_echo, None, "error: no session context yet", true);
            return Ok(());
        }
        if ui.running.is_some() || self.compaction_task.is_some() {
            self.queue_fullscreen_compaction(ui, instructions, command_echo);
            ui.set_ephemeral_status("compaction queued");
            return Ok(());
        }
        self.start_compaction_task(
            ui,
            instructions,
            Some(command_echo),
            true,
            CompactionReason::Manual,
            true,
        )
    }

    pub(crate) fn start_compaction_task(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        instructions: Option<String>,
        command_echo: Option<String>,
        manual: bool,
        reason: CompactionReason,
        force: bool,
    ) -> Result<()> {
        if self.compaction_task.is_some() {
            return Ok(());
        }
        let Some(session_id) = self.current_session.clone() else {
            return Ok(());
        };
        let options = CompactSessionOptions {
            state: self.state_runtime.clone(),
            workdir: self.workdir.clone(),
            session: session_id.clone(),
            config_path: self.config_path.clone(),
            model: self.current_model.clone(),
            reasoning_effort: self.current_variant.clone(),
            inherited_env: Some(self.env_map.clone()),
            reason,
            instructions,
            force,
        };
        let task = tokio::spawn(async move {
            compact_session(options)
                .await
                .map_err(|err| format!("{err:#}"))
        });
        self.compaction_task = Some(CompactionTask {
            session_id,
            command_echo,
            manual,
            task,
        });
        ui.set_ephemeral_status("compacting context");
        ui.refresh_sidebar(self);
        Ok(())
    }

    pub(crate) async fn run_scripted_compaction(
        &mut self,
        instructions: Option<String>,
    ) -> Result<()> {
        let session = self
            .current_session
            .clone()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        let result = compact_session(CompactSessionOptions {
            state: self.state_runtime.clone(),
            workdir: self.workdir.clone(),
            session,
            config_path: self.config_path.clone(),
            model: self.current_model.clone(),
            reasoning_effort: self.current_variant.clone(),
            inherited_env: Some(self.env_map.clone()),
            reason: CompactionReason::Manual,
            instructions,
            force: true,
        })
        .await?;
        println!("{}", format_compaction_result(&result, true));
        self.last_context_snapshot = None;
        Ok(())
    }
}

pub(crate) fn fullscreen_context_bar_width(ui: &FullscreenUi<'_>) -> usize {
    if ui.last_transcript_width == 0 {
        return 80;
    }
    normalize_context_bar_width(usize::from(ui.last_transcript_width).saturating_sub(8))
}

pub(crate) fn format_compaction_result(result: &CompactionResult, include_summary: bool) -> String {
    if !result.compacted {
        return format!("not compacted: {}", result.message);
    }
    let before = result
        .tokens_before
        .map(|value| value.to_string())
        .unwrap_or_else(|| "?".to_string());
    let after = result
        .tokens_after
        .map(|value| value.to_string())
        .unwrap_or_else(|| "?".to_string());
    let mut lines = vec![
        format!("compacted: {before} -> {after} tokens"),
        format!(
            "first kept seq: {}",
            result
                .first_kept_session_seq
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_string())
        ),
    ];
    if include_summary
        && let Some(summary) = result.summary.as_deref()
        && !summary.trim().is_empty()
    {
        lines.push(String::new());
        lines.push("summary:".to_string());
        lines.push(summary.trim().to_string());
    }
    lines.join("\n")
}

pub(crate) fn normalize_submitted_slash_echo(value: &str) -> String {
    value.lines().next().unwrap_or_default().trim().to_string()
}

pub(crate) fn slash_command_echo(command: &SlashCommand) -> String {
    match command {
        SlashCommand::Help => "/help".to_string(),
        SlashCommand::Quit => "/quit".to_string(),
        SlashCommand::Status => "/status".to_string(),
        SlashCommand::New => "/new".to_string(),
        SlashCommand::Sessions => "/sessions".to_string(),
        SlashCommand::Usage => "/usage".to_string(),
        SlashCommand::Context => "/context".to_string(),
        SlashCommand::Refresh => "/refresh".to_string(),
        SlashCommand::ReloadContextDeprecated => "/reload-context".to_string(),
        SlashCommand::Btw(prompt) => prompt
            .as_deref()
            .map(|prompt| format!("/btw {}", prompt.trim()))
            .unwrap_or_else(|| "/btw".to_string()),
        SlashCommand::Steer(message) => format!("/steer {}", message.trim()),
        SlashCommand::Queue(message) => format!("/queue {}", message.trim()),
        SlashCommand::PendingCancel => "/pending cancel".to_string(),
        SlashCommand::ModelShowScoped { global } => {
            if *global {
                "/model --global".to_string()
            } else {
                "/model".to_string()
            }
        }
        SlashCommand::VariantSet(variant) => format!("/variant {variant}"),
        SlashCommand::ModeSet(mode) => format!("/mode {mode}"),
        SlashCommand::Permissions => "/permissions".to_string(),
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
        SlashCommand::Skills(args) => args
            .as_deref()
            .map(|args| format!("/skills {}", args.trim()))
            .unwrap_or_else(|| "/skills".to_string()),
        SlashCommand::Tools => "/tools".to_string(),
        SlashCommand::Bundles(args) => args
            .as_deref()
            .map(|args| format!("/bundles {}", args.trim()))
            .unwrap_or_else(|| "/bundles".to_string()),
        SlashCommand::Curator(args) => args
            .as_deref()
            .map(|args| format!("/curator {}", args.trim()))
            .unwrap_or_else(|| "/curator".to_string()),
        SlashCommand::Agents => "/agents".to_string(),
        SlashCommand::Fork(prompt) => format!("/fork {}", prompt.trim()),
        SlashCommand::Compact(instructions) => instructions
            .as_deref()
            .map(|instructions| format!("/compact {}", instructions.trim()))
            .unwrap_or_else(|| "/compact".to_string()),
        SlashCommand::SkillInvoke { name, args } => {
            if args.trim().is_empty() {
                format!("/{name}")
            } else {
                format!("/{name} {}", args.trim())
            }
        }
        SlashCommand::Upcoming(command) => format!("/{command}"),
    }
}

pub(crate) fn skill_prompt_marker(name: &str, args: &str) -> String {
    if args.trim().is_empty() {
        format!("${name} ")
    } else {
        format!("${name} {}", args.trim())
    }
}

pub(crate) fn json_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("-")
        .to_string()
}

pub(crate) fn json_string_array(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_string())
}

pub(crate) fn skill_scope_from_args(args: &[&str]) -> std::result::Result<SkillTarget, String> {
    if args
        .iter()
        .any(|arg| matches!(*arg, "--scope" | "--project"))
    {
        return Err("use --local or -g/--global for skill scope".to_string());
    }
    if args.iter().any(|arg| matches!(*arg, "-g" | "--global")) {
        Ok(SkillTarget::Global)
    } else {
        Ok(SkillTarget::Project)
    }
}

pub(crate) fn skill_option_value<'a>(args: &'a [&str], option: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == option).then_some(window[1]))
}

pub(crate) fn skill_args_without_scope<'a>(args: &'a [&str]) -> Vec<&'a str> {
    let mut filtered = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if *arg == "--scope" {
            skip_next = true;
            continue;
        }
        if matches!(*arg, "--local" | "--global" | "-g") {
            continue;
        }
        filtered.push(*arg);
    }
    filtered
}

pub(crate) fn format_skill_mutation_result(result: psychevo_runtime::Result<Value>) -> String {
    match result {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        Err(err) => format!("error: {err:#}"),
    }
}

pub(crate) fn normalize_dynamic_skill_name(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch)
            } else if ch == '-' || ch == '_' || ch.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub(crate) fn fork_prompt_marker(prompt: &str) -> String {
    format!(
        "Use the Agent tool with agent_type=\"general\", fork_context=true, and background=true for this task:\n\n{}",
        prompt.trim()
    )
}
