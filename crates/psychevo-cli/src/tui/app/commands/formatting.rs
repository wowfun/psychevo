use crate::tui::{
    AgentDiscoveryOptions, AgentMissionRegistration, AgentTeamRegistration, AuxiliaryShellTask,
    CompactThreadRequest, CompactionReason, CompactionResult, CompactionTask, FrameworkClient,
    FullscreenUi, PendingAuxiliaryShellCommand, PendingImageAttachment, Result, RunningTask,
    RunningTurn, RunningTurnControl, RunningTurnEvents, SessionArtifactKind, SessionExportFormat,
    ShellCommandEvent, ShellCommandOutcome, SkillTarget, SlashCommand, StartThreadRequest,
    StartedTurn, StartingTurn, TUI_CONTINUE_SESSION_SOURCES, TuiApp, TuiApprovalHandler,
    TurnAdmissionCancellation, TurnPrinter, USER_SHELL_HELP, Value,
    discover_agent_teams_with_catalog, discover_agents, normalize_context_bar_width,
    presented_shell_event_channel, prompt_display_metadata, resolve_agent_team_definition,
};
use anyhow::anyhow;
use psychevo::{Thread, ThreadListQuery};
use std::{
    collections::BTreeMap,
    io,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

pub(crate) enum TuiTurnAdmissionTarget {
    Existing(Thread),
    New {
        request: Box<StartThreadRequest>,
        session_id: String,
    },
}

impl TuiTurnAdmissionTarget {
    pub(crate) fn session_id(&self) -> &str {
        match self {
            Self::Existing(thread) => thread.id(),
            Self::New { session_id, .. } => session_id,
        }
    }

    pub(crate) async fn start(
        self,
        client: &FrameworkClient,
        request: psychevo::TurnRequest,
    ) -> psychevo::Result<psychevo::TurnHandle> {
        match self {
            Self::Existing(thread) => thread.start_turn(request).await,
            Self::New { request: start, .. } => {
                client.start_thread_with_turn(*start, request).await
            }
        }
    }
}

pub(crate) async fn resolve_tui_turn_admission_target(
    client: &FrameworkClient,
    current_session: Option<&str>,
    force_new_once: bool,
    cwd: &std::path::Path,
) -> psychevo::Result<TuiTurnAdmissionTarget> {
    if let Some(thread_id) = current_session
        && let Some(thread) = client.try_resume_thread(thread_id.to_string()).await?
    {
        return Ok(TuiTurnAdmissionTarget::Existing(thread));
    }
    if !force_new_once
        && let Some(snapshot) = client
            .list_threads(ThreadListQuery {
                cwd: Some(cwd.to_path_buf()),
                archived: false,
                sources: TUI_CONTINUE_SESSION_SOURCES
                    .iter()
                    .map(|source| (*source).to_string())
                    .collect(),
                limit: 1,
                ..ThreadListQuery::default()
            })
            .await?
            .threads
            .into_iter()
            .next()
    {
        return Ok(TuiTurnAdmissionTarget::Existing(
            client.resume_thread(snapshot.id).await?,
        ));
    }
    let session_id = uuid::Uuid::now_v7().to_string();
    let mut request = StartThreadRequest::new(cwd);
    request.source = "tui".to_string();
    request.metadata = Some(serde_json::json!({
        "caller": "pevo TUI",
        "pid": std::process::id(),
    }));
    let request = request.with_initial_context(session_id.clone(), None, BTreeMap::new());
    Ok(TuiTurnAdmissionTarget::New {
        request: Box::new(request),
        session_id,
    })
}

impl TuiApp {
    pub(crate) fn mission_registration(
        &self,
        team: Option<&str>,
        goal: &str,
    ) -> Result<AgentMissionRegistration> {
        let mission_id = uuid::Uuid::now_v7().to_string();
        let metadata = Some(serde_json::json!({"source": "tui:/mission"}));
        let request = if let Some(team_name) = team.map(str::trim).filter(|team| !team.is_empty()) {
            let options = AgentDiscoveryOptions {
                home: self.home.clone(),
                cwd: self.cwd.clone(),
                env: self.env_map.clone(),
                explicit_inputs: self.current_agent.iter().cloned().collect(),
                no_agents: self.no_agents,
            };
            let agents = discover_agents(&options)?;
            let teams = discover_agent_teams_with_catalog(&options, &agents)?;
            let team = resolve_agent_team_definition(&teams, team_name)?;
            let team_id = uuid::Uuid::now_v7().to_string();
            let members = serde_json::to_value(&team.members)?;
            let source_path = team
                .file_path
                .as_ref()
                .map(|path| path.display().to_string());
            AgentMissionRegistration {
                id: mission_id,
                goal: goal.to_string(),
                lead_agent_name: team.leader.clone(),
                team: Some(AgentTeamRegistration {
                    id: team_id,
                    name: team.name.clone(),
                    description: Some(team.description.clone()),
                    source_path,
                    leader_agent_name: team.leader.clone(),
                    members,
                    max_parallel_agents: team.max_parallel_agents,
                }),
                metadata,
            }
        } else {
            let lead_agent_name = self.current_agent.as_deref().unwrap_or("general");
            AgentMissionRegistration {
                id: mission_id,
                goal: goal.to_string(),
                lead_agent_name: lead_agent_name.to_string(),
                team: None,
                metadata,
            }
        };
        Ok(request)
    }

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
        let emit = move |event: ShellCommandEvent| {
            let mut turn = turn_for_sink.lock().expect("turn lock poisoned");
            let mut stdout = stdout_for_sink.lock().expect("stdout lock poisoned");
            let _ = turn.render_shell_event(&event, &mut *stdout);
        };
        let shell = self
            .runtime
            .client()
            .shell_command(self.shell_command_request(command))?;
        let result = shell.run(emit).await?;
        {
            let mut turn = turn.lock().expect("turn lock poisoned");
            let mut stdout = stdout.lock().expect("stdout lock poisoned");
            turn.finish(&mut *stdout)?;
        }
        if let Some(session_id) = result.thread_id {
            self.current_session = Some(session_id);
            self.reset_live_agent_reload_poll();
            self.refresh_current_session_title().await?;
            self.clear_new_session_draft();
        }
        if result.outcome != ShellCommandOutcome::Completed || result.tool_failures > 0 {
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
        self.start_fullscreen_turn_with_mission(ui, prompt, display_prompt, images, None)
    }

    pub(crate) fn start_fullscreen_turn_with_mission(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        prompt: String,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
        mission: Option<AgentMissionRegistration>,
    ) -> Result<()> {
        if ui.foreground_turn_active() || self.compaction_task.is_some() {
            self.queue_fullscreen_prompt_with_mission(ui, prompt, display_prompt, images, mission);
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
        let optimistic_start = ui.transcript.len();
        ui.push_user_with_images(display_prompt.clone(), &images);
        ui.mark_optimistic_rows_from(optimistic_start);
        let cancellation = TurnAdmissionCancellation::new();
        let request = self
            .framework_turn_request_with_images(prompt, image_inputs)
            .with_prompt_display(prompt_display_metadata(&display_prompt, &images, &self.cwd))
            .with_framework_context(Some(self.home.join("snapshots")), None, Vec::new(), None)
            .with_admission_cancellation(cancellation.clone());
        let framework = self.runtime.client().clone();
        let current_session = self.current_session.clone();
        let force_new_once = self.force_new_once;
        let cwd = self.cwd.clone();
        let task = tokio::spawn(async move {
            let target = resolve_tui_turn_admission_target(
                &framework,
                current_session.as_deref(),
                force_new_once,
                &cwd,
            )
            .await?;
            let session_id = target.session_id().to_string();
            let (approval_tx, approval_rx) = mpsc::unbounded_channel();
            let request = if let Some(mission) = mission {
                request.with_admission_mission(mission)
            } else {
                request
            };
            let request = request.with_approval(
                Some(Arc::new(TuiApprovalHandler {
                    session_id: Some(session_id.clone()),
                    sender: approval_tx,
                })),
                true,
            );
            let handle = target.start(&framework, request).await?;
            Ok(StartedTurn {
                handle,
                approval_rx,
            })
        });
        ui.scroll_to_bottom();
        ui.approval_rx = None;
        let queue_owner_id = format!("starting:{}", uuid::Uuid::now_v7());
        ui.starting_turn = Some(StartingTurn {
            session_id: self.current_session.clone(),
            queue_owner_id,
            display_prompt,
            images,
            cancellation,
            task,
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
        if ui.foreground_turn_active() || self.compaction_task.is_some() {
            self.queue_fullscreen_shell(ui, command);
            return Ok(());
        }
        if command.trim().is_empty() {
            ui.push_status(USER_SHELL_HELP);
            return Ok(());
        }
        let shell = self
            .runtime
            .client()
            .shell_command(self.shell_command_request(command))?;
        let control = shell.control();
        let (rx, emit) = presented_shell_event_channel();
        let task = tokio::spawn(async move { shell.run(emit).await });
        ui.scroll_to_bottom();
        ui.running = Some(RunningTurn {
            session_id: self.current_session.clone(),
            control: control.into(),
            selector: None,
            turn_id: None,
            events: RunningTurnEvents::Shell(rx),
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
        let Some(running) = ui.running.as_ref() else {
            return self.start_fullscreen_shell(ui, command);
        };
        let owner_session_id = running
            .session_id
            .clone()
            .or_else(|| self.current_session.clone());
        let pending = PendingAuxiliaryShellCommand {
            owner_session_id: owner_session_id.clone(),
            owner_turn_id: running.turn_id.clone(),
            request: self.shell_command_request_for_session(command, owner_session_id.clone()),
        };
        let owner_control = running.control.clone();
        self.start_owned_auxiliary_fullscreen_shell(ui, owner_session_id, owner_control, pending)
    }

    fn start_owned_auxiliary_fullscreen_shell(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session_id: Option<String>,
        owner_control: RunningTurnControl,
        pending: PendingAuxiliaryShellCommand,
    ) -> Result<()> {
        let agent_handle = owner_control
            .agent_handle()
            .ok_or_else(|| anyhow!("active Framework Turn control is unavailable"))?;
        let request = match owner_session_id.as_deref() {
            Some(session_id) => pending.request.thread(session_id),
            None => pending.request,
        }
        .inject_into(agent_handle);
        let shell = self.runtime.client().shell_command(request)?;
        let control = shell.control();
        let (rx, emit) = presented_shell_event_channel();
        let task = tokio::spawn(async move { shell.run(emit).await });
        if self.current_session.as_deref() == owner_session_id.as_deref() {
            ui.scroll_to_bottom();
        }
        ui.auxiliary_shell_tasks.push(AuxiliaryShellTask {
            session_id: owner_session_id,
            control,
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
        let running = ui.running.as_ref().expect("checked Agent Turn");
        let owner_session_id = running
            .session_id
            .clone()
            .or_else(|| self.current_session.clone());
        let owner_turn_id = running.turn_id.clone();
        let owner_control = running.control.clone();
        self.start_pending_auxiliary_shells_for_owner(
            ui,
            owner_session_id,
            owner_turn_id,
            owner_control,
        )
    }

    pub(crate) fn start_pending_auxiliary_shells_for_agent(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        agent: &crate::tui::AuxiliaryAgentTask,
    ) -> Result<()> {
        self.start_pending_auxiliary_shells_for_owner(
            ui,
            agent.session_id.clone(),
            agent.turn_id.clone(),
            agent.control.clone(),
        )
    }

    fn start_pending_auxiliary_shells_for_owner(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        owner_session_id: Option<String>,
        owner_turn_id: Option<String>,
        owner_control: RunningTurnControl,
    ) -> Result<()> {
        while let Some(index) = ui
            .pending_auxiliary_shell_commands
            .iter()
            .position(|pending| {
                pending.matches_owner(owner_session_id.as_deref(), owner_turn_id.as_deref())
            })
        {
            let pending = ui.pending_auxiliary_shell_commands[index].clone();
            self.start_owned_auxiliary_fullscreen_shell(
                ui,
                owner_session_id.clone(),
                owner_control.clone(),
                pending,
            )?;
            ui.pending_auxiliary_shell_commands.remove(index);
        }
        Ok(())
    }

    pub(crate) fn submit_fullscreen_compaction(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        instructions: Option<String>,
        command_echo: String,
    ) -> Result<()> {
        if ui.foreground_turn_active() {
            self.queue_fullscreen_compaction(ui, instructions, command_echo);
            ui.set_ephemeral_status("compaction queued");
            return Ok(());
        }
        if self.current_session.is_none() {
            ui.push_command_result(command_echo, None, "error: no session context yet", true);
            return Ok(());
        }
        if self.compaction_task.is_some() {
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
        let request = CompactThreadRequest {
            config_path: self.config_path.clone(),
            model: self.current_model.clone(),
            reasoning_effort: self.current_variant.clone(),
            inherited_env: Some(self.env_map.clone()),
            reason,
            instructions,
            force,
        };
        let framework = self.runtime.client().clone();
        let task_session_id = session_id.clone();
        let task = tokio::spawn(async move {
            let thread = framework
                .resume_thread(task_session_id)
                .await
                .map_err(|err| format!("{err:#}"))?;
            thread
                .compact(request)
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
        let thread = self.runtime.client().resume_thread(session).await?;
        let result = thread
            .compact(CompactThreadRequest {
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
        SlashCommand::Diff => "/diff".to_string(),
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
        SlashCommand::Sandbox => "/sandbox".to_string(),
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
                != psychevo::session_export::SessionExportIncludeSet::default_for(
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
                != psychevo::session_export::SessionExportIncludeSet::default_for(
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
        SlashCommand::Mission { team, goal } => {
            format!("/mission {}", mission_command_args(team.as_deref(), goal))
        }
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

pub(crate) fn mission_command_args(team: Option<&str>, goal: &str) -> String {
    let mut parts = Vec::new();
    if let Some(team) = team.map(str::trim).filter(|team| !team.is_empty()) {
        parts.push(format!("--team {team}"));
    }
    parts.push(goal.trim().to_string());
    parts.join(" ")
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

pub(crate) fn format_skill_mutation_result(result: psychevo::Result<Value>) -> String {
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
        "Use the spawn_agent tool with agent_type=\"general\", fork_context=true, background=true, a lowercase underscore task_name, and this message:\n\n{}",
        prompt.trim()
    )
}
