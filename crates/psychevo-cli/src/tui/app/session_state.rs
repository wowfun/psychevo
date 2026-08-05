use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

#[cfg(test)]
use crate::tui::resolve_session_ref_from_summaries;
use crate::tui::{
    AgentRelationshipStatus, AgentRunStatus, AuxiliaryAgentTask, AuxiliaryShellTask, ConfigScope,
    FrameworkClient, FullscreenUi, GatewayActivity, GatewayThreadSelector, HistoryReplayItem,
    Result, RunMode, RunningTask, RunningTurnEvents, SessionListView, SetThreadMainAgentSelection,
    TUI_INTERNAL_SESSION_SOURCES, ThreadItem, ThreadListQuery, ThreadMainAgentSelection,
    ThreadModelSelection, ThreadSummary, TranscriptKind, TuiApp, TuiLiveEvent,
    TuiSessionDisplaySummary, Value, anyhow, assistant_message_keeps_tool_calls_active,
    canonicalize_cwd, history_tool_calls_from_message, instant_from_wall_timestamp_ms,
    load_effective_tui_slash_config, normalize_reasoning_effort, resolve_agent_definition,
    short_session, user_text_from_item, validate_model_spec, validate_variant,
};
pub(crate) const LIVE_AGENT_RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(250);

impl TuiApp {
    pub(crate) fn refresh_selected_model(&mut self) {
        self.selected_model = self
            .configuration()
            .ok()
            .and_then(|configuration| configuration.selected_model().ok())
            .flatten();
    }

    pub(crate) async fn refresh_current_session_title(&mut self) -> Result<()> {
        let summary = match self.current_session.as_deref() {
            Some(session_id) => Some(
                self.runtime
                    .client()
                    .resume_thread(session_id.to_string())
                    .await?
                    .summary()
                    .await?,
            ),
            None => None,
        };
        self.current_session_title = summary
            .as_ref()
            .and_then(|summary| summary.title.clone())
            .filter(|title| !title.trim().is_empty());
        self.current_session_forked_from =
            summary.and_then(|summary| summary.forked_from_thread_id);
        self.refresh_current_session_relationships().await?;
        Ok(())
    }

    async fn refresh_current_session_relationships(&mut self) -> Result<()> {
        let Some(session_id) = self.current_session.as_deref() else {
            self.current_agent_breadcrumb = None;
            return Ok(());
        };
        let thread = self
            .runtime
            .client()
            .resume_thread(session_id.to_string())
            .await?;
        self.current_agent_breadcrumb =
            if let Some(relationship) = thread.agent_relationship().await? {
                let sibling_count = self
                    .runtime
                    .client()
                    .resume_thread(relationship.parent_thread_id.clone())
                    .await?
                    .agent_children()
                    .await?
                    .len();
                let mut parts = vec![format!(
                    "parent {}",
                    short_session(&relationship.parent_thread_id)
                )];
                if sibling_count > 1 {
                    parts.push("siblings Alt+Up/Right".to_string());
                }
                parts.push("Alt+P".to_string());
                Some(parts.join(" · "))
            } else {
                None
            };
        Ok(())
    }

    pub(crate) async fn refresh_current_session_agent(&mut self) -> Result<()> {
        let Some(session_id) = self.current_session.as_deref() else {
            if !self.current_agent_explicit_default && self.current_agent.is_none() {
                self.current_agent = self.startup_agent.clone();
            }
            return Ok(());
        };
        match self
            .runtime
            .client()
            .resume_thread(session_id.to_string())
            .await?
            .main_agent_selection()
            .await?
        {
            ThreadMainAgentSelection::Default { base_agent } => {
                self.current_agent = base_agent;
                self.current_agent_explicit_default = true;
            }
            ThreadMainAgentSelection::Agent { input } => {
                self.current_agent = Some(input);
                self.current_agent_explicit_default = false;
            }
            ThreadMainAgentSelection::Missing { base_agent } => {
                if let Some(agent) = base_agent {
                    self.current_agent = Some(agent);
                    self.current_agent_explicit_default = true;
                } else {
                    self.current_agent = self.startup_agent.clone();
                    self.current_agent_explicit_default = false;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn session_identity_label(&self) -> Option<String> {
        let agent = self.current_agent.as_deref()?.trim();
        if agent.is_empty() {
            return None;
        }
        self.current_agent_display_name(agent)
    }

    pub(crate) fn current_agent_display_name(&self, input: &str) -> Option<String> {
        let catalog = self.current_agent_catalog()?;
        resolve_agent_definition(&catalog, input, &self.cwd, &self.env_map)
            .ok()
            .map(|agent| agent.name)
            .or_else(|| Some(input.to_string()))
    }

    pub(crate) fn main_agent_selection_for_input(
        &self,
        input: &str,
    ) -> Result<SetThreadMainAgentSelection> {
        let catalog = self
            .current_agent_catalog()
            .ok_or_else(|| anyhow!("agents are disabled"))?;
        let agent = resolve_agent_definition(&catalog, input, &self.cwd, &self.env_map)?;
        Ok(SetThreadMainAgentSelection::Agent {
            input: input.to_string(),
            name: agent.name,
            source: agent.source,
            path: agent.file_path,
        })
    }

    pub(crate) fn schedule_main_agent_selection_persistence(&self, session_id: &str) -> Result<()> {
        let selection = if self.current_agent_explicit_default {
            Some(SetThreadMainAgentSelection::Default)
        } else if let Some(input) = self.current_agent.as_deref() {
            Some(self.main_agent_selection_for_input(input)?)
        } else {
            None
        };
        let Some(selection) = selection else {
            return Ok(());
        };
        let framework = self.runtime.client().clone();
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            let result = async {
                framework
                    .resume_thread(session_id.clone())
                    .await?
                    .set_main_agent_selection(selection)
                    .await?;
                Ok::<(), psychevo::Error>(())
            }
            .await;
            if let Err(error) = result {
                eprintln!(
                    "failed to persist main agent selection for session {session_id}: {error:#}"
                );
            }
        });
        Ok(())
    }

    pub(crate) fn session_sidebar_title(&self) -> String {
        let title = self
            .current_session_title
            .clone()
            .or_else(|| {
                self.current_session
                    .as_deref()
                    .map(short_session)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "New session".to_string());
        match self.current_session_forked_from.as_deref() {
            Some(source) => format!("{title} · forked from {}", short_session(source)),
            None => title,
        }
    }

    #[cfg(test)]
    pub(crate) async fn switch_session_no_print(&mut self, reference: &str) -> Result<String> {
        let id = self.resolve_session_ref(reference).await?;
        let summary = self.session_summary_required(&id).await?;
        self.adopt_session_cwd(&summary)?;
        self.runtime.client().resume_thread(id.clone()).await?;
        self.current_session = Some(id.clone());
        self.reset_live_agent_reload_poll();
        self.clear_new_session_draft();
        self.refresh_current_session_title().await?;
        self.refresh_current_session_agent().await?;
        Ok(id)
    }

    pub(crate) async fn open_agent_target_session(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        target: &str,
    ) -> Result<()> {
        if ui
            .running
            .as_ref()
            .is_some_and(|running| matches!(running.task, RunningTask::UserShell(_)))
        {
            ui.push_status("finish the current shell command before opening an agent session");
            return Ok(());
        }
        let child_session_id = self
            .runtime
            .client()
            .agent_relationship(target)
            .await?
            .ok_or_else(|| anyhow!("agent not found: {target}"))?
            .child_thread_id;
        self.runtime
            .client()
            .resume_thread(child_session_id.clone())
            .await?;
        let summary = self.session_summary_required(&child_session_id).await?;
        self.adopt_session_cwd(&summary)?;
        self.detach_foreground_for_session_switch(ui, Some(child_session_id.clone()))
            .await;
        self.current_session = Some(child_session_id.clone());
        self.reset_live_agent_reload_poll();
        self.clear_new_session_draft();
        self.refresh_current_session_title().await?;
        self.refresh_current_session_agent().await?;
        ui.clear_session_local_bottom_panel();
        ui.clear_transcript();
        self.load_current_session_history(ui).await?;
        self.replay_session_live_event_backlog(ui, &child_session_id);
        self.replay_agent_child_event_backlog(ui, &child_session_id);
        ui.refresh_sidebar(self);
        Ok(())
    }

    pub(crate) async fn maybe_reload_live_agent_session(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        if ui.foreground_turn_active() {
            return Ok(false);
        }
        let Some(session_id) = self.current_session.clone() else {
            return Ok(false);
        };
        let now = Instant::now();
        if !live_agent_reload_due(self.last_live_agent_reload_check, now) {
            return Ok(false);
        }
        self.last_live_agent_reload_check = Some(now);
        let thread = self
            .runtime
            .client()
            .resume_thread(session_id.clone())
            .await?;
        let Some(relationship) = thread.agent_relationship().await? else {
            return Ok(false);
        };
        if relationship.status != AgentRelationshipStatus::Open {
            return Ok(false);
        }
        let message_count = thread.summary().await?.message_count.max(0) as usize;
        if message_count <= ui.loaded_session_message_count {
            return Ok(false);
        }
        ui.clear_transcript();
        self.load_current_session_history(ui).await?;
        Ok(true)
    }

    pub(crate) fn reset_live_agent_reload_poll(&mut self) {
        self.last_live_agent_reload_check = None;
    }

    pub(crate) async fn request_current_session_interrupt(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> bool {
        if let Some(starting) = ui.starting_turn.take() {
            let queue_owner_id = starting.queue_owner_id.clone();
            let (cleanup, display_prompt, images) = starting.into_cleanup_with_input();
            ui.starting_turn_cleanups.push(cleanup);
            ui.discard_unbound_optimistic_rows();
            ui.finish_turn();
            ui.restore_failed_turn_start_to_composer(&queue_owner_id, display_prompt, images);
            ui.interrupt_requested = true;
            ui.follow_transcript_if_needed();
            ui.refresh_sidebar(self);
            return true;
        }
        let current_session = self.current_session.clone();
        let mut interrupted = false;
        if let Some((selector, _)) = self.active_gateway_turn_selector(ui) {
            interrupted |= self.runtime.gateway().interrupt_turn(selector).await;
        }
        interrupted |= ui.request_interrupt(current_session.as_deref());
        if let Some(session_id) = current_session.as_deref() {
            let control = self.runtime.application().agent_control();
            let targets = control
                .status_records(Some(session_id), false)
                .await
                .into_iter()
                .filter_map(|agent| {
                    matches!(
                        agent.status,
                        AgentRunStatus::PendingInit | AgentRunStatus::Running
                    )
                    .then_some(agent.id)
                })
                .collect::<Vec<_>>();
            for target in targets {
                if control
                    .stop_with_grace(&target, Duration::ZERO)
                    .await
                    .ok()
                    .flatten()
                    .is_some()
                {
                    interrupted = true;
                }
            }
        }
        if interrupted {
            ui.interrupt_requested = true;
        }
        interrupted
    }

    pub(crate) async fn open_agent_parent_session(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<()> {
        let Some(current) = self.current_session.clone() else {
            return Ok(());
        };
        let Some(relationship) = self
            .runtime
            .client()
            .resume_thread(current)
            .await?
            .agent_relationship()
            .await?
        else {
            ui.push_status("no parent agent session");
            return Ok(());
        };
        self.open_session_direct(ui, &relationship.parent_thread_id)
            .await
    }

    pub(crate) async fn open_agent_sibling_session(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        direction: isize,
    ) -> Result<()> {
        let Some(current) = self.current_session.clone() else {
            return Ok(());
        };
        let Some(relationship) = self
            .runtime
            .client()
            .resume_thread(current.clone())
            .await?
            .agent_relationship()
            .await?
        else {
            ui.push_status("no sibling agent sessions");
            return Ok(());
        };
        let siblings = self
            .runtime
            .client()
            .resume_thread(relationship.parent_thread_id)
            .await?
            .agent_children()
            .await?;
        if siblings.len() <= 1 {
            ui.push_status("no sibling agent sessions");
            return Ok(());
        }
        let current_index = siblings
            .iter()
            .position(|sibling| sibling.child_thread_id == current)
            .unwrap_or(0) as isize;
        let next = (current_index + direction).rem_euclid(siblings.len() as isize) as usize;
        self.open_session_direct(ui, &siblings[next].child_thread_id)
            .await
    }

    pub(crate) async fn open_session_direct(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
    ) -> Result<()> {
        let summary = self.session_summary_required(session_id).await?;
        self.adopt_session_cwd(&summary)?;
        self.detach_foreground_for_session_switch(ui, None).await;
        self.current_session = Some(session_id.to_string());
        self.reset_live_agent_reload_poll();
        self.clear_new_session_draft();
        self.refresh_current_session_title().await?;
        self.refresh_current_session_agent().await?;
        ui.clear_session_local_bottom_panel();
        ui.clear_transcript();
        self.load_current_session_history(ui).await?;
        self.replay_session_live_event_backlog(ui, session_id);
        self.replay_agent_child_event_backlog(ui, session_id);
        self.show_staged_history_status(ui, session_id).await?;
        ui.refresh_sidebar(self);
        Ok(())
    }

    pub(crate) async fn detach_foreground_for_session_switch(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        child_session_id: Option<String>,
    ) {
        if let Some(starting) = ui.starting_turn.take() {
            let queue_owner_id = starting.queue_owner_id.clone();
            ui.starting_turn_cleanups.push(starting.into_cleanup());
            ui.queued_inputs.retain(|input| {
                crate::tui::queued_input_session_id(input) != Some(queue_owner_id.as_str())
            });
            ui.discard_unbound_optimistic_rows();
            ui.finish_turn();
        }
        let owner_session = ui
            .running
            .as_ref()
            .and_then(|running| running.session_id.clone());
        let mut pending = std::mem::take(&mut ui.deferred_stream_events);
        if let Some(running) = &mut ui.running {
            while let Some(event) = running.events.try_recv() {
                pending.push_back(event);
            }
        }
        let had_pending = if owner_session.is_some() {
            self.apply_pending_owned_fullscreen_live_events(ui, owner_session.as_deref(), pending)
        } else {
            let pending = pending
                .into_iter()
                .filter_map(|event| match event {
                    TuiLiveEvent::Turn(event) => Some(event),
                    TuiLiveEvent::Gateway(event) => {
                        self.apply_gateway_event(ui, owner_session.as_deref(), *event);
                        None
                    }
                    TuiLiveEvent::Shell(event) => {
                        self.apply_fullscreen_shell_event(ui, event);
                        None
                    }
                })
                .collect();
            self.apply_pending_fullscreen_turn_events_without_frames(ui, pending)
        };
        if had_pending {
            ui.follow_transcript_if_needed();
            ui.refresh_sidebar(self);
        }
        let Some(running) = ui.running.take() else {
            return;
        };
        let owner_session = running.session_id.or_else(|| self.current_session.clone());
        match running.task {
            RunningTask::Agent(task) => {
                ui.auxiliary_agent_tasks.push(AuxiliaryAgentTask {
                    session_id: owner_session,
                    turn_id: running.turn_id,
                    child_session_id,
                    visible_live: true,
                    pending_unowned_live_events: Vec::new(),
                    approval_rx: ui.approval_rx.take(),
                    control: running.control,
                    events: running.events,
                    task,
                });
            }
            RunningTask::UserShell(task) => {
                ui.auxiliary_shell_tasks.push(AuxiliaryShellTask {
                    session_id: owner_session,
                    control: running
                        .control
                        .shell_control()
                        .expect("a foreground user shell owns typed Shell control"),
                    rx: match running.events {
                        RunningTurnEvents::Shell(rx) => rx,
                        RunningTurnEvents::Turn(_) => {
                            unreachable!("a foreground user shell owns typed Shell events")
                        }
                        #[cfg(test)]
                        RunningTurnEvents::TurnTest(_) => {
                            unreachable!("a foreground user shell owns typed Shell events")
                        }
                        #[cfg(test)]
                        RunningTurnEvents::Gateway(_) => {
                            unreachable!("a foreground user shell owns typed Shell events")
                        }
                    },
                    task,
                });
            }
        }
        ui.finish_turn();
    }

    pub(crate) fn replay_agent_child_event_backlog(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
    ) {
        let Some(events) = ui.agent_child_event_backlog.remove(session_id) else {
            return;
        };
        for event in events {
            ui.apply_turn_event_for_session(
                event,
                self.thinking_visible,
                self.debug,
                Some(session_id),
            );
        }
        ui.follow_transcript_if_needed();
    }

    pub(crate) fn replay_session_live_event_backlog(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
    ) {
        let Some(events) = ui.session_live_event_backlog.remove(session_id) else {
            return;
        };
        let pending = events.into();
        self.apply_pending_owned_fullscreen_live_events(ui, Some(session_id), pending);
        ui.follow_transcript_if_needed();
    }

    pub(crate) fn agent_breadcrumb_status(&self) -> Option<String> {
        self.current_agent_breadcrumb.clone()
    }

    pub(crate) async fn set_model_default_from_picker(
        &mut self,
        model: String,
        reasoning_effort: Option<String>,
        global: bool,
    ) -> Result<String> {
        validate_model_spec(&model)?;
        if !global {
            self.current_model = Some(model.clone());
            self.current_variant = normalize_reasoning_effort(reasoning_effort.clone());
            self.model_state
                .set_model(&self.cwd_key, model.clone(), reasoning_effort);
            self.model_state.save(&self.model_state_path)?;
            self.persist_current_session_model_selection().await?;
            self.refresh_selected_model();
            let reasoning = self
                .current_variant
                .as_deref()
                .map(|value| format!("  reasoning_effort: {value}"))
                .unwrap_or_default();
            return Ok(format!(
                "model: {model}{reasoning}  scope: composer  path: {}",
                self.model_state_path.display()
            ));
        }
        let value = self.configuration()?.set_default_model(
            if global {
                ConfigScope::Global
            } else {
                ConfigScope::Local
            },
            &model,
            reasoning_effort.as_deref(),
        )?;
        self.current_model = None;
        self.current_variant = None;
        self.model_state
            .push_recent_model(model.clone(), reasoning_effort);
        self.model_state.save(&self.model_state_path)?;
        self.refresh_selected_model();
        let scope = value["scope"]
            .as_str()
            .unwrap_or(if global { "global" } else { "local" });
        let path = value["path"].as_str().unwrap_or("-");
        let effective = self
            .selected_model
            .as_ref()
            .map(|model| format!("{}/{}", model.provider, model.model));
        let reasoning = value
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .map(|value| format!("  reasoning_effort: {value}"))
            .unwrap_or_default();
        if global
            && effective
                .as_deref()
                .is_some_and(|effective| effective != model)
        {
            Ok(format!(
                "global model saved: {model}{reasoning}  path: {path}  current cwd still uses local model: {}",
                effective.unwrap()
            ))
        } else {
            Ok(format!(
                "model: {model}{reasoning}  scope: {scope}  path: {path}"
            ))
        }
    }

    pub(crate) async fn set_variant_no_print(&mut self, variant: String) -> Result<()> {
        validate_variant(&variant)?;
        let reasoning_effort = normalize_reasoning_effort(Some(variant));
        self.current_variant = reasoning_effort.clone();
        self.model_state
            .set_reasoning_effort(&self.cwd_key, reasoning_effort);
        self.model_state.save(&self.model_state_path)?;
        self.persist_current_session_model_selection().await?;
        self.refresh_selected_model();
        Ok(())
    }

    async fn persist_current_session_model_selection(&self) -> Result<()> {
        let Some(session_id) = self.current_session.as_deref() else {
            return Ok(());
        };
        let Some(model) = self.current_model.as_deref() else {
            return Ok(());
        };
        let Some((provider, model_id)) = model.split_once('/') else {
            return Ok(());
        };
        let thread = match self
            .runtime
            .client()
            .resume_thread(session_id.to_string())
            .await
        {
            Ok(thread) => thread,
            Err(psychevo::Error::Message(message))
                if message == format!("thread not found: {session_id}") =>
            {
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        thread
            .set_model_selection(ThreadModelSelection {
                provider: provider.to_string(),
                model: model_id.to_string(),
                reasoning_effort: self.current_variant.clone(),
            })
            .await?;
        Ok(())
    }

    pub(crate) fn set_mode_no_print(&mut self, mode: &str) -> Result<()> {
        let (run_mode, permission_mode) = match mode {
            "plan" => (RunMode::Plan, self.current_permission_mode),
            "default" => (RunMode::Default, self.current_permission_mode),
            _ => return Err(anyhow!("mode must be one of plan, default")),
        };
        self.current_mode = run_mode;
        self.current_permission_mode = permission_mode;
        self.state
            .set_mode(&self.cwd_key, run_mode.as_str().to_string());
        self.state
            .set_permission_mode(&self.cwd_key, permission_mode.as_str().to_string());
        self.state.save(&self.state_path)?;
        Ok(())
    }

    pub(crate) fn set_thinking_no_print(&mut self, enabled: bool) -> Result<()> {
        self.thinking_visible = enabled;
        self.state.set_thinking_visible(enabled);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    pub(crate) fn set_raw_no_print(&mut self, enabled: bool) -> Result<()> {
        self.raw_visible = enabled;
        self.state.set_raw_visible(enabled);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    pub(crate) async fn rename_session_no_print(&mut self, title: String) -> Result<String> {
        let Some(session_id) = self.current_session.as_deref() else {
            return Err(anyhow!("no current session to rename"));
        };
        let title = self
            .runtime
            .client()
            .resume_thread(session_id.to_string())
            .await?
            .set_title(&title)
            .await?;
        self.current_session_title = Some(title.clone());
        Ok(title)
    }

    pub(crate) async fn current_framework_thread(&self) -> Result<psychevo::Thread> {
        let Some(session_id) = self.current_session.as_ref() else {
            return Err(anyhow!("no current session to undo"));
        };
        Ok(self.runtime.client().resume_thread(session_id).await?)
    }

    pub(crate) async fn undo_session_no_print(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<String> {
        let result = self.current_framework_thread().await?.undo().await?;
        ui.clear_transcript();
        self.load_current_session_history(ui).await?;
        ui.set_composer_text(&result.prompt);
        ui.refresh_sidebar(self);
        Ok(format!(
            "undone {} messages; prompt restored",
            result.reverted_messages
        ))
    }

    pub(crate) async fn redo_session_no_print(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<String> {
        let result = self.current_framework_thread().await?.redo().await?;
        ui.clear_transcript();
        self.load_current_session_history(ui).await?;
        ui.clear_composer();
        ui.refresh_sidebar(self);
        let suffix = if result.complete {
            "complete"
        } else {
            "partial"
        };
        Ok(format!(
            "redone {} messages; {suffix}",
            result.restored_messages
        ))
    }

    pub(crate) fn set_sidebar_visible_no_print(&mut self, visible: bool) -> Result<()> {
        self.state.set_sidebar_visible(visible);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    pub(crate) fn cycle_mode(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let next = match self.current_mode {
            RunMode::Default => "plan",
            RunMode::Plan => "default",
        };
        self.set_mode_no_print(next)?;
        ui.refresh_sidebar(self);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn resolve_session_ref(&self, reference: &str) -> Result<String> {
        let sessions = self.sessions().await?;
        resolve_session_ref_from_summaries(&sessions, reference)
    }

    #[cfg(test)]
    pub(crate) async fn sessions(&self) -> Result<Vec<ThreadSummary>> {
        Ok(all_thread_summaries(self.runtime.client(), false)
            .await?
            .into_iter()
            .filter(human_visible_tui_thread_summary)
            .collect())
    }

    pub(crate) async fn tui_sessions(
        &self,
        view: SessionListView,
    ) -> Result<Vec<TuiSessionDisplaySummary>> {
        tui_sessions_for_client(self.runtime.client(), view).await
    }

    pub(crate) async fn session_summary_required(&self, session_id: &str) -> Result<ThreadSummary> {
        Ok(self
            .runtime
            .client()
            .resume_thread(session_id.to_string())
            .await?
            .summary()
            .await?)
    }

    pub(crate) fn adopt_session_cwd(&mut self, summary: &ThreadSummary) -> Result<()> {
        let next_cwd = canonicalize_cwd(Path::new(&summary.cwd))?;
        if next_cwd == self.cwd {
            return Ok(());
        }
        let next_slash_config = load_effective_tui_slash_config(
            self.runtime.client(),
            &self.env_map,
            next_cwd.clone(),
        )?;
        self.cwd = next_cwd;
        self.cwd_key = self.cwd.to_string_lossy().to_string();
        self.slash_config = next_slash_config;
        self.refresh_selected_model();
        Ok(())
    }

    pub(crate) async fn load_current_session_history(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<()> {
        let Some(session_id) = self.current_session.clone() else {
            ui.loaded_session_message_count = 0;
            ui.visible_turn_started = None;
            ui.session_usage_summary = None;
            ui.replace_session_history_prompts(Vec::new());
            ui.refresh_sidebar(self);
            return Ok(());
        };
        let thread = self
            .runtime
            .client()
            .resume_thread(session_id.clone())
            .await?;
        ui.sidebar_context_limit = thread.context_limit_with_parent_fallback().await?;
        let history = thread.history();
        ui.session_usage_summary = Some(thread.usage_summary().await?);
        let activity = self
            .sync_gateway_activity_for_session(ui, &session_id)
            .await;
        let live_owner = ui.local_status_has_running(Some(&session_id)) || activity.running;
        let suppress_latest_terminal_meta = live_owner;
        let mut active_tool_call_ids = BTreeSet::new();
        let mut after = None;
        loop {
            let page = history.replay_after(after, Some(200)).await?;
            if let Some(warning) = page.warnings.first() {
                return Err(anyhow!(
                    "stored session history is invalid at message {} ({:?})",
                    warning.session_seq,
                    warning.kind
                ));
            }
            let summaries = page
                .items
                .into_iter()
                .filter_map(|item| match item {
                    HistoryReplayItem::Available { item } => Some(*item),
                    HistoryReplayItem::Unavailable { .. } => None,
                })
                .collect::<Vec<_>>();
            active_tool_call_ids.extend(history_active_tool_call_ids_for_reload(
                &summaries, live_owner,
            )?);
            let Some(next_after) = page.next_after else {
                break;
            };
            after = Some(next_after);
        }

        let mut history_prompts = Vec::new();
        let mut loaded_message_count = 0usize;
        let mut after = None;
        loop {
            let page = history.replay_after(after, Some(200)).await?;
            if let Some(warning) = page.warnings.first() {
                return Err(anyhow!(
                    "stored session history is invalid at message {} ({:?})",
                    warning.session_seq,
                    warning.kind
                ));
            }
            let next_after = page.next_after;
            let summaries = page
                .items
                .into_iter()
                .filter_map(|item| match item {
                    HistoryReplayItem::Available { item } => Some(*item),
                    HistoryReplayItem::Unavailable { .. } => None,
                })
                .collect::<Vec<_>>();
            let page_len = summaries.len();
            for (index, summary) in summaries.into_iter().enumerate() {
                loaded_message_count = loaded_message_count.saturating_add(1);
                let session_seq = summary.session_seq;
                let value = serde_json::to_value(&summary.message)?;
                let is_user = value.get("role").and_then(Value::as_str) == Some("user");
                if is_user && let Some(text) = user_text_from_item(&value, &summary) {
                    history_prompts.push(text);
                }
                let first_new_row = ui.transcript.len();
                ui.push_thread_item_with_projection_options(
                    &summary,
                    &value,
                    suppress_latest_terminal_meta && next_after.is_none() && index + 1 == page_len,
                    Some(&active_tool_call_ids),
                );
                if is_user
                    && let Some(row) = ui.transcript[first_new_row..]
                        .iter_mut()
                        .find(|row| row.kind == TranscriptKind::Prompt)
                {
                    row.transcript_entry_id = Some(format!("message:{session_seq}"));
                    row.transcript_message_seq = Some(session_seq);
                }
            }
            let Some(next_after) = next_after else {
                break;
            };
            after = Some(next_after);
        }
        ui.loaded_session_message_count = loaded_message_count;
        let agent_catalog = self.current_agent_catalog();
        let agent_relationships = thread.agent_children().await?;
        ui.reconcile_history_agent_rows(&agent_relationships, agent_catalog.as_ref());
        ui.visible_turn_started = ui
            .foreign_gateway_activity_started(&session_id)
            .or_else(|| {
                ui.history_prompt_started_ms
                    .and_then(instant_from_wall_timestamp_ms)
            });
        if live_owner {
            ui.turn_session_id = Some(session_id.clone());
        }
        ui.replace_session_history_prompts(history_prompts);
        if activity.running
            && activity.owner_id.as_deref() != Some(self.runtime.gateway().owner_id())
        {
            self.replay_foreign_gateway_live_events_for_session(ui, &session_id)
                .await?;
        }
        ui.scroll_to_bottom();
        ui.refresh_sidebar(self);
        Ok(())
    }

    pub(crate) async fn reload_invalidated_turn_projection(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        if !ui.turn_projection_invalid || self.current_session.is_none() {
            return Ok(false);
        }
        ui.clear_transcript();
        self.load_current_session_history(ui).await?;
        ui.turn_projection_invalid = false;
        ui.push_status("live turn projection reloaded from authoritative history");
        Ok(true)
    }

    pub(crate) async fn sync_gateway_activity_for_session(
        &self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
    ) -> GatewayActivity {
        let activity = self
            .runtime
            .gateway()
            .activity_for_selector(GatewayThreadSelector::thread_id(session_id))
            .await;
        if activity.running
            && activity.owner_id.as_deref() != Some(self.runtime.gateway().owner_id())
        {
            ui.observe_foreign_gateway_activity(session_id, &activity);
        } else {
            ui.clear_foreign_gateway_activity(session_id);
        }
        activity
    }
}

pub(crate) async fn latest_human_visible_session_id(
    client: &FrameworkClient,
) -> Result<Option<String>> {
    Ok(tui_sessions_for_client(client, SessionListView::Active)
        .await?
        .into_iter()
        .next()
        .map(|session| session.summary.id))
}

pub(crate) async fn tui_sessions_for_client(
    client: &FrameworkClient,
    view: SessionListView,
) -> Result<Vec<TuiSessionDisplaySummary>> {
    let sessions = all_thread_summaries(client, view == SessionListView::Archived).await?;
    let mut visible = Vec::new();
    for summary in sessions {
        if !human_visible_tui_thread_summary(&summary) {
            continue;
        }
        let thread = client.resume_thread(summary.id.clone()).await?;
        let visible_message_count = thread.history().display_message_count().await?;
        let forked_from_thread_id = summary.forked_from_thread_id.clone();
        visible.push(TuiSessionDisplaySummary {
            project_label: session_project_label(&summary.cwd),
            project_display_path: session_project_display_path(&summary.cwd),
            summary,
            visible_message_count,
            forked_from_thread_id,
        });
    }
    Ok(visible)
}

pub(crate) async fn all_thread_summaries(
    client: &FrameworkClient,
    archived: bool,
) -> Result<Vec<ThreadSummary>> {
    let mut summaries = Vec::new();
    let mut cursor = None;
    loop {
        let page = client
            .list_threads(ThreadListQuery {
                archived,
                cursor,
                limit: 200,
                ..ThreadListQuery::default()
            })
            .await?;
        summaries.extend(page.threads);
        let Some(next) = page.next_cursor else {
            return Ok(summaries);
        };
        cursor = Some(next);
    }
}

pub(crate) fn human_visible_tui_thread_summary(summary: &ThreadSummary) -> bool {
    summary.parent_thread_id.is_none()
        && !TUI_INTERNAL_SESSION_SOURCES.contains(&summary.source.as_str())
}

pub(crate) fn session_project_label(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("cwd")
        .to_string()
}

pub(crate) fn session_project_display_path(cwd: &str) -> String {
    Path::new(cwd).display().to_string()
}

pub(crate) fn live_agent_reload_due(last_check: Option<Instant>, now: Instant) -> bool {
    match last_check {
        Some(last_check) => now.duration_since(last_check) >= LIVE_AGENT_RELOAD_POLL_INTERVAL,
        None => true,
    }
}

pub(crate) fn history_active_tool_call_ids_for_reload(
    summaries: &[ThreadItem],
    live_owner: bool,
) -> Result<BTreeSet<String>> {
    let mut active = BTreeSet::new();
    for summary in summaries {
        let value = serde_json::to_value(&summary.message)?;
        if value.get("role").and_then(Value::as_str) == Some("tool_result") {
            if let Some(tool_call_id) = value.get("tool_call_id").and_then(Value::as_str) {
                active.insert(tool_call_id.to_string());
            }
            continue;
        }
        if live_owner && assistant_message_keeps_tool_calls_active(&value) {
            for call in history_tool_calls_from_message(&value) {
                active.insert(call.id);
            }
        }
    }
    Ok(active)
}

#[cfg(test)]
pub(crate) mod live_agent_reload_tests {
    pub(crate) use super::*;

    #[test]
    fn live_agent_reload_first_check_is_immediate() {
        assert!(live_agent_reload_due(None, Instant::now()));
    }

    #[test]
    fn live_agent_reload_checks_are_gated_for_250ms() {
        let last = Instant::now();
        assert!(!live_agent_reload_due(
            Some(last),
            last + LIVE_AGENT_RELOAD_POLL_INTERVAL - Duration::from_millis(1)
        ));
        assert!(live_agent_reload_due(
            Some(last),
            last + LIVE_AGENT_RELOAD_POLL_INTERVAL
        ));
    }
}
