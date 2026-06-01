#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) const SESSION_MAIN_AGENT_METADATA_KEY: &str = "main_agent";
pub(crate) const LIVE_AGENT_RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoadedMainAgent {
    Missing,
    Default,
    Agent(String),
}

impl TuiApp {
    pub(crate) fn refresh_selected_model(&mut self) {
        self.selected_model = selected_configured_model(&self.run_options(String::new()))
            .ok()
            .flatten();
    }

    pub(crate) fn refresh_current_session_title(&mut self) -> Result<()> {
        let summary = match self.current_session.as_deref() {
            Some(session_id) => self.state_runtime.store().session_summary(session_id)?,
            None => None,
        };
        self.current_session_title = summary
            .and_then(|summary| summary.title)
            .filter(|title| !title.trim().is_empty());
        Ok(())
    }

    pub(crate) fn refresh_current_session_agent(&mut self) -> Result<()> {
        let Some(session_id) = self.current_session.as_deref() else {
            if !self.current_agent_explicit_default && self.current_agent.is_none() {
                self.current_agent = self.startup_agent.clone();
            }
            return Ok(());
        };
        let store = self.state_runtime.store();
        let metadata = store.session_metadata(session_id)?;
        match main_agent_from_session_metadata(metadata.as_ref()) {
            LoadedMainAgent::Default => {
                self.current_agent = session_base_agent_name_from_metadata(metadata.as_ref());
                self.current_agent_explicit_default = true;
            }
            LoadedMainAgent::Agent(agent) => {
                self.current_agent = Some(agent);
                self.current_agent_explicit_default = false;
            }
            LoadedMainAgent::Missing => {
                if let Some(agent) = session_base_agent_name_from_metadata(metadata.as_ref()) {
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
        resolve_agent_definition(&catalog, input, &self.workdir, &self.env_map)
            .ok()
            .map(|agent| agent.name)
            .or_else(|| Some(input.to_string()))
    }

    pub(crate) fn main_agent_metadata_for_input(&self, input: &str) -> Result<Value> {
        let catalog = self
            .current_agent_catalog()
            .ok_or_else(|| anyhow!("agents are disabled"))?;
        let agent = resolve_agent_definition(&catalog, input, &self.workdir, &self.env_map)?;
        Ok(main_agent_metadata(
            input,
            &agent.name,
            agent.source,
            agent.file_path.as_ref(),
        ))
    }

    pub(crate) fn persist_main_agent_selection_for_session(&self, session_id: &str) -> Result<()> {
        let store = self.state_runtime.store();
        if self.current_agent_explicit_default {
            store.set_session_metadata_field(
                session_id,
                SESSION_MAIN_AGENT_METADATA_KEY,
                Some(main_agent_default_metadata()),
            )?;
        } else if let Some(input) = self.current_agent.as_deref() {
            let value = self.main_agent_metadata_for_input(input)?;
            store.set_session_metadata_field(
                session_id,
                SESSION_MAIN_AGENT_METADATA_KEY,
                Some(value),
            )?;
        }
        Ok(())
    }

    pub(crate) fn session_sidebar_title(&self) -> String {
        self.current_session_title
            .clone()
            .or_else(|| {
                self.current_session
                    .as_deref()
                    .map(short_session)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "New session".to_string())
    }

    #[cfg(test)]
    pub(crate) fn switch_session_no_print(&mut self, reference: &str) -> Result<String> {
        let id = self.resolve_session_ref(reference)?;
        self.state_runtime.store().resume_session(&id)?;
        self.current_session = Some(id.clone());
        self.reset_live_agent_reload_poll();
        self.force_new_once = false;
        self.refresh_current_session_title()?;
        self.refresh_current_session_agent()?;
        Ok(id)
    }

    pub(crate) fn open_agent_target_session(
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
        let child_session_id = {
            let store = self.state_runtime.store();
            let edge = store
                .find_agent_edge(target)?
                .ok_or_else(|| anyhow!("agent not found: {target}"))?;
            store.resume_session(&edge.child_session_id)?;
            edge.child_session_id
        };
        self.detach_running_for_session_switch(ui, Some(child_session_id.clone()));
        self.current_session = Some(child_session_id.clone());
        self.reset_live_agent_reload_poll();
        self.force_new_once = false;
        self.refresh_current_session_title()?;
        self.refresh_current_session_agent()?;
        ui.bottom_panel = None;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        self.replay_session_live_event_backlog(ui, &child_session_id);
        self.replay_agent_child_event_backlog(ui, &child_session_id);
        ui.refresh_sidebar(self);
        Ok(())
    }

    pub(crate) fn maybe_reload_live_agent_session(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        if ui.running.is_some() {
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
        let store = self.state_runtime.store();
        let Some(edge) = store.find_agent_edge(&session_id)? else {
            return Ok(false);
        };
        if edge.status != psychevo_runtime::AgentEdgeStatus::Open {
            return Ok(false);
        }
        let message_count = store.load_tui_message_summaries(&session_id)?.len();
        if message_count <= ui.loaded_session_message_count {
            return Ok(false);
        }
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        Ok(true)
    }

    pub(crate) fn reset_live_agent_reload_poll(&mut self) {
        self.last_live_agent_reload_check = None;
    }

    pub(crate) fn request_current_session_interrupt(&mut self, ui: &mut FullscreenUi<'_>) -> bool {
        let current_session = self.current_session.clone();
        let mut interrupted = false;
        if let Some((selector, _)) = self.active_gateway_turn_selector(ui) {
            interrupted |= self.gateway.interrupt_turn(selector);
        }
        interrupted |= ui.request_interrupt(current_session.as_deref());
        if let Some(session_id) = current_session.as_deref() {
            let store = self.state_runtime.store();
            let value = agent_status_value(Some(store), Some(session_id), false);
            let targets = value
                .get("agents")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|agent| {
                    matches!(
                        agent.get("status").and_then(Value::as_str),
                        Some("pending_init" | "running")
                    )
                })
                .filter_map(|agent| {
                    agent
                        .get("id")
                        .and_then(Value::as_str)
                        .or_else(|| agent.get("child_session_id").and_then(Value::as_str))
                        .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>();
            for target in targets {
                if stop_agent_id_with_grace(&target, Some(store), Duration::ZERO)
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

    pub(crate) fn open_agent_parent_session(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(current) = self.current_session.clone() else {
            return Ok(());
        };
        let store = self.state_runtime.store();
        let Some(edge) = store.find_agent_edge(&current)? else {
            ui.push_status("no parent agent session");
            return Ok(());
        };
        self.open_session_direct(ui, &edge.parent_session_id)
    }

    pub(crate) fn open_agent_sibling_session(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        direction: isize,
    ) -> Result<()> {
        let Some(current) = self.current_session.clone() else {
            return Ok(());
        };
        let store = self.state_runtime.store();
        let Some(edge) = store.find_agent_edge(&current)? else {
            ui.push_status("no sibling agent sessions");
            return Ok(());
        };
        let siblings = store.list_agent_edges_for_parent(&edge.parent_session_id)?;
        if siblings.len() <= 1 {
            ui.push_status("no sibling agent sessions");
            return Ok(());
        }
        let current_index = siblings
            .iter()
            .position(|sibling| sibling.child_session_id == current)
            .unwrap_or(0) as isize;
        let next = (current_index + direction).rem_euclid(siblings.len() as isize) as usize;
        self.open_session_direct(ui, &siblings[next].child_session_id)
    }

    pub(crate) fn open_session_direct(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: &str,
    ) -> Result<()> {
        self.detach_running_for_session_switch(ui, None);
        self.state_runtime.store().resume_session(session_id)?;
        self.current_session = Some(session_id.to_string());
        self.reset_live_agent_reload_poll();
        self.force_new_once = false;
        self.refresh_current_session_title()?;
        self.refresh_current_session_agent()?;
        ui.bottom_panel = None;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        self.replay_session_live_event_backlog(ui, session_id);
        self.replay_agent_child_event_backlog(ui, session_id);
        ui.refresh_sidebar(self);
        Ok(())
    }

    pub(crate) fn detach_running_for_session_switch(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        child_session_id: Option<String>,
    ) {
        let owner_session = ui
            .running
            .as_ref()
            .and_then(|running| running.session_id.clone());
        let mut pending = std::mem::take(&mut ui.deferred_stream_events);
        if let Some(running) = &mut ui.running {
            while let Ok(event) = running.events.try_recv() {
                pending.push_back(event);
            }
        }
        let had_pending = if owner_session.is_some() {
            self.apply_pending_owned_fullscreen_live_events(ui, owner_session.as_deref(), pending)
        } else {
            let pending = pending
                .into_iter()
                .filter_map(|event| match event {
                    TuiLiveEvent::Runtime(event) => Some(event),
                    TuiLiveEvent::Gateway(event) => {
                        self.apply_gateway_event(ui, owner_session.as_deref(), *event);
                        None
                    }
                })
                .collect();
            self.apply_pending_fullscreen_stream_events_without_frames(ui, pending)
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
                    child_session_id,
                    visible_live: true,
                    control: running.control,
                    events: running.events,
                    task,
                });
            }
            RunningTask::UserShell(task) => {
                ui.auxiliary_shell_tasks.push(AuxiliaryShellTask {
                    session_id: owner_session,
                    control: running.control,
                    rx: match running.events {
                        RunningTurnEvents::Runtime(rx) => rx,
                        RunningTurnEvents::Gateway(_) => {
                            let (_tx, rx) = mpsc::unbounded_channel();
                            rx
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
            ui.apply_stream_event_for_session(
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
        self.apply_pending_owned_fullscreen_stream_events(ui, Some(session_id), pending);
        ui.follow_transcript_if_needed();
    }

    pub(crate) fn agent_breadcrumb_status(&self) -> Option<String> {
        let session_id = self.current_session.as_deref()?;
        let store = self.state_runtime.store();
        let edge = store.find_agent_edge(session_id).ok().flatten()?;
        let sibling_count = store
            .list_agent_edges_for_parent(&edge.parent_session_id)
            .map(|siblings| siblings.len())
            .unwrap_or(0);
        let mut parts = vec![format!("parent {}", short_session(&edge.parent_session_id))];
        if sibling_count > 1 {
            parts.push("siblings Alt+Up/Right".to_string());
        }
        parts.push("Alt+P".to_string());
        Some(parts.join(" · "))
    }

    pub(crate) fn set_model_default_from_picker(
        &mut self,
        model: String,
        reasoning_effort: Option<String>,
        global: bool,
    ) -> Result<String> {
        validate_model_spec(&model)?;
        let value = set_default_model_with_reasoning(
            &self.home,
            &self.workdir,
            global,
            &model,
            reasoning_effort.as_deref(),
        )?;
        self.current_model = None;
        self.current_variant = None;
        self.state.set_model(&self.workdir_key, model.clone());
        self.state.clear_model(&self.workdir_key);
        self.state.clear_variant(&self.workdir_key);
        self.state.save(&self.state_path)?;
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
                "global model saved: {model}{reasoning}  path: {path}  current workdir still uses local model: {}",
                effective.unwrap()
            ))
        } else {
            Ok(format!(
                "model: {model}{reasoning}  scope: {scope}  path: {path}"
            ))
        }
    }

    pub(crate) fn set_variant_no_print(&mut self, variant: String) -> Result<()> {
        validate_variant(&variant)?;
        self.current_variant = Some(variant.clone());
        self.state.set_variant(&self.workdir_key, variant);
        self.state.save(&self.state_path)?;
        self.refresh_selected_model();
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
            .set_mode(&self.workdir_key, run_mode.as_str().to_string());
        self.state
            .set_permission_mode(&self.workdir_key, permission_mode.as_str().to_string());
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

    pub(crate) fn rename_session_no_print(&mut self, title: String) -> Result<String> {
        let Some(session_id) = self.current_session.as_deref() else {
            return Err(anyhow!("no current session to rename"));
        };
        let title = self
            .state_runtime
            .store()
            .set_session_title(session_id, &title)?;
        self.current_session_title = Some(title.clone());
        Ok(title)
    }

    pub(crate) fn undo_options(&self) -> Result<SessionUndoOptions> {
        let Some(session_id) = self.current_session.clone() else {
            return Err(anyhow!("no current session to undo"));
        };
        Ok(SessionUndoOptions {
            state: self.state_runtime.clone(),
            workdir: self.workdir.clone(),
            snapshot_root: self.home.join("snapshots"),
            session_id,
        })
    }

    pub(crate) fn undo_session_no_print(&mut self, ui: &mut FullscreenUi<'_>) -> Result<String> {
        let result = undo_session(self.undo_options()?)?;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        ui.set_composer_text(&result.prompt);
        ui.refresh_sidebar(self);
        Ok(format!(
            "undone {} messages; prompt restored",
            result.reverted_messages
        ))
    }

    pub(crate) fn redo_session_no_print(&mut self, ui: &mut FullscreenUi<'_>) -> Result<String> {
        let result = redo_session(self.undo_options()?)?;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
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
    pub(crate) fn resolve_session_ref(&self, reference: &str) -> Result<String> {
        let sessions = self.sessions_for_workdir()?;
        resolve_session_ref_from_summaries(&sessions, reference)
    }

    #[cfg(test)]
    pub(crate) fn sessions_for_workdir(&self) -> Result<Vec<SessionSummary>> {
        self.state_runtime
            .store()
            .list_sessions_for_workdir_with_sources(&self.workdir, TUI_SESSION_SOURCES)
            .map_err(Into::into)
    }

    pub(crate) fn tui_sessions_for_workdir(
        &self,
        view: SessionListView,
    ) -> Result<Vec<TuiSessionDisplaySummary>> {
        let store = self.state_runtime.store();
        let sessions = match view {
            SessionListView::Active => {
                store.list_sessions_for_workdir_with_sources(&self.workdir, TUI_SESSION_SOURCES)?
            }
            SessionListView::Archived => store.list_archived_sessions_for_workdir_with_sources(
                &self.workdir,
                TUI_SESSION_SOURCES,
            )?,
        };
        sessions
            .into_iter()
            .map(|summary| {
                let messages = store.load_tui_message_summaries(&summary.id)?;
                Ok(TuiSessionDisplaySummary {
                    summary,
                    visible_message_count: visible_tui_message_count(&messages)?,
                })
            })
            .collect()
    }

    pub(crate) fn load_current_session_history(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(session_id) = self.current_session.clone() else {
            ui.loaded_session_message_count = 0;
            ui.visible_turn_started = None;
            ui.replace_session_history_prompts(Vec::new());
            ui.refresh_sidebar(self);
            return Ok(());
        };
        let metadata = self.state_runtime.store().session_metadata(&session_id)?;
        ui.sidebar_context_limit = session_context_limit_with_parent_fallback(
            self.state_runtime.store(),
            &session_id,
            metadata.as_ref(),
        )?;
        let summaries = self
            .state_runtime
            .store()
            .load_tui_message_summaries(&session_id)?;
        ui.loaded_session_message_count = summaries.len();
        let timeline_items =
            timeline_items_for_transcript(self.gateway.thread_timeline(&session_id)?, &summaries)?;
        if !timeline_items.is_empty() {
            let agent_edges = self
                .state_runtime
                .store()
                .list_agent_edges_for_parent(&session_id)?;
            let mut history_prompts = Vec::new();
            for item in timeline_items {
                if item.kind == TimelineItemKind::Prompt {
                    let text = timeline_history_text(&item);
                    if !text.trim().is_empty() {
                        history_prompts.push(text);
                    }
                }
                self.apply_gateway_timeline_item(ui, Some(&session_id), item);
            }
            let agent_catalog = self.current_agent_catalog();
            ui.reconcile_history_agent_rows(&agent_edges, agent_catalog.as_ref());
            ui.visible_turn_started = ui
                .history_prompt_started_ms
                .and_then(instant_from_wall_timestamp_ms);
            ui.replace_session_history_prompts(history_prompts);
            ui.scroll_to_bottom();
            ui.refresh_sidebar(self);
            return Ok(());
        }
        let summary_count = summaries.len();
        let suppress_latest_terminal_meta = ui.status_has_running(Some(&session_id));
        let active_tool_call_ids =
            history_active_tool_call_ids_for_reload(ui, &session_id, &summaries)?;
        let mut history_prompts = Vec::new();
        for (index, summary) in summaries.into_iter().enumerate() {
            let value = serde_json::to_value(summary.message)?;
            if value.get("role").and_then(Value::as_str) == Some("user")
                && let Some(text) = user_text_from_message(&value, summary.metadata.as_ref())
            {
                history_prompts.push(text);
            }
            ui.push_history_message_with_projection_options(
                &value,
                summary.usage.as_ref(),
                summary.metadata.as_ref(),
                summary.accounting.as_ref(),
                suppress_latest_terminal_meta && index + 1 == summary_count,
                Some(&active_tool_call_ids),
            );
        }
        let agent_catalog = self.current_agent_catalog();
        let agent_edges = self
            .state_runtime
            .store()
            .list_agent_edges_for_parent(&session_id)?;
        ui.reconcile_history_agent_rows(&agent_edges, agent_catalog.as_ref());
        ui.visible_turn_started = ui
            .history_prompt_started_ms
            .and_then(instant_from_wall_timestamp_ms);
        ui.replace_session_history_prompts(history_prompts);
        ui.scroll_to_bottom();
        ui.refresh_sidebar(self);
        Ok(())
    }
}

pub(crate) fn session_context_limit_with_parent_fallback(
    store: &SqliteStore,
    session_id: &str,
    metadata: Option<&Value>,
) -> Result<Option<u64>> {
    if let Some(limit) = metadata.and_then(session_context_limit) {
        return Ok(Some(limit));
    }
    let Some(edge) = store.find_agent_edge(session_id)? else {
        return Ok(None);
    };
    let parent_metadata = store.session_metadata(&edge.parent_session_id)?;
    Ok(parent_metadata.as_ref().and_then(session_context_limit))
}

pub(crate) fn session_context_limit(metadata: &Value) -> Option<u64> {
    metadata.get("context_limit").and_then(Value::as_u64)
}

pub(crate) fn live_agent_reload_due(last_check: Option<Instant>, now: Instant) -> bool {
    match last_check {
        Some(last_check) => now.duration_since(last_check) >= LIVE_AGENT_RELOAD_POLL_INTERVAL,
        None => true,
    }
}

pub(crate) fn history_active_tool_call_ids_for_reload(
    ui: &FullscreenUi<'_>,
    session_id: &str,
    summaries: &[TuiMessageSummary],
) -> Result<BTreeSet<String>> {
    let mut active = BTreeSet::new();
    let live_owner = ui.status_has_running(Some(session_id));
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

pub(crate) fn main_agent_default_metadata() -> Value {
    serde_json::json!({"mode": "default"})
}

pub(crate) fn main_agent_metadata(
    input: &str,
    name: &str,
    source: AgentSource,
    path: Option<&PathBuf>,
) -> Value {
    serde_json::json!({
        "mode": "agent",
        "input": input,
        "name": name,
        "source": source.as_str(),
        "path": path,
    })
}

pub(crate) fn main_agent_from_session_metadata(metadata: Option<&Value>) -> LoadedMainAgent {
    let Some(metadata) = metadata else {
        return LoadedMainAgent::Missing;
    };
    if let Some(main_agent) = metadata.get(SESSION_MAIN_AGENT_METADATA_KEY) {
        if main_agent
            .get("mode")
            .and_then(Value::as_str)
            .is_some_and(|mode| mode == "default")
            || main_agent.is_null()
        {
            return LoadedMainAgent::Default;
        }
        if let Some(input) = main_agent
            .get("input")
            .and_then(Value::as_str)
            .or_else(|| main_agent.get("name").and_then(Value::as_str))
            .or_else(|| main_agent.get("path").and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return LoadedMainAgent::Agent(input.to_string());
        }
    }
    if let Some(name) = metadata
        .get("selected_agent")
        .and_then(|value| {
            value
                .get("input")
                .or_else(|| value.get("name"))
                .or_else(|| value.get("path"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return LoadedMainAgent::Agent(name.to_string());
    }
    LoadedMainAgent::Missing
}

pub(crate) fn session_base_agent_name_from_metadata(metadata: Option<&Value>) -> Option<String> {
    metadata?
        .get("agent")
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn timeline_history_text(item: &TimelineItem) -> String {
    item.body
        .as_ref()
        .or(item.detail.as_ref())
        .or(item.preview.as_ref())
        .cloned()
        .unwrap_or_default()
}

type TimelineHistoryOrder = (i64, i64, i64);

fn timeline_items_for_transcript(
    mut items: Vec<TimelineItem>,
    summaries: &[TuiMessageSummary],
) -> Result<Vec<TimelineItem>> {
    let (order, tool_metadata) = timeline_history_projection(summaries)?;
    for item in &mut items {
        if let Some(metadata) = tool_metadata.get(&item.id) {
            item.metadata = Some(merge_json_objects(item.metadata.take(), metadata.clone()));
        }
    }
    items.sort_by_key(|item| {
        order
            .get(&item.id)
            .copied()
            .unwrap_or((i64::MAX / 2, item.sequence, 0))
    });
    Ok(merge_write_stdin_history_items(items))
}

fn merge_write_stdin_history_items(items: Vec<TimelineItem>) -> Vec<TimelineItem> {
    let mut projected = Vec::with_capacity(items.len());
    let mut exec_session_items = BTreeMap::<u64, usize>::new();

    for item in items {
        let tool = timeline_item_tool_name(&item);
        match tool.as_deref() {
            Some("exec_command") => {
                let index = projected.len();
                if let Some(metadata) = item.metadata.as_ref()
                    && let Some(session_id) = exec_session_id_from_result(metadata)
                    && exec_result_running(metadata)
                {
                    exec_session_items.insert(session_id, index);
                }
                projected.push(item);
            }
            Some("write_stdin") => {
                let Some(metadata) = item.metadata.as_ref() else {
                    continue;
                };
                let Some(session_id) = write_stdin_target_session_id(metadata) else {
                    continue;
                };
                let Some(parent_index) = exec_session_items.get(&session_id).copied() else {
                    continue;
                };
                if let Some(parent) = projected.get_mut(parent_index) {
                    merge_write_stdin_into_exec_item(parent, &item);
                }
                if exec_result_completed(metadata) {
                    exec_session_items.remove(&session_id);
                }
            }
            _ => projected.push(item),
        }
    }

    projected
}

fn timeline_item_tool_name(item: &TimelineItem) -> Option<String> {
    item.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("tool_name"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| item.title.clone())
}

fn write_stdin_target_session_id(metadata: &Value) -> Option<u64> {
    metadata
        .get("args")
        .and_then(exec_session_id_from_args)
        .or_else(|| {
            metadata
                .get("arguments")
                .and_then(exec_session_id_from_args)
        })
        .or_else(|| exec_session_id_from_result(metadata))
}

fn merge_write_stdin_into_exec_item(exec_item: &mut TimelineItem, write_item: &TimelineItem) {
    let Some(write_metadata) = write_item.metadata.as_ref() else {
        return;
    };
    let Some(exec_metadata) = exec_item.metadata.as_mut() else {
        return;
    };
    append_tool_result_output(exec_metadata, &tool_result_output(write_metadata));
    if exec_result_completed(write_metadata) {
        merge_terminal_exec_result(exec_metadata, write_metadata);
        exec_item.status = write_item.status;
    }
    accumulate_elapsed_ms(exec_metadata, write_metadata);
}

fn append_tool_result_output(metadata: &mut Value, output: &str) {
    if output.is_empty() {
        return;
    }
    let result = ensure_json_object_field(metadata, "result");
    let next = match result.get("output").and_then(Value::as_str) {
        Some(existing) if existing.ends_with(output) => existing.to_string(),
        Some(existing) => format!("{existing}{output}"),
        None => output.to_string(),
    };
    result.insert("output".to_string(), Value::String(next));
}

fn merge_terminal_exec_result(metadata: &mut Value, write_metadata: &Value) {
    let result = ensure_json_object_field(metadata, "result");
    if let Some(exit_code) = write_metadata
        .get("result")
        .and_then(|result| result.get("exit_code"))
        .filter(|value| !value.is_null())
    {
        result.insert("exit_code".to_string(), exit_code.clone());
    }
    if let Some(outcome) = write_metadata.get("outcome") {
        ensure_json_object(metadata).insert("outcome".to_string(), outcome.clone());
    }
}

fn accumulate_elapsed_ms(metadata: &mut Value, write_metadata: &Value) {
    let Some(delta) = write_metadata.get("elapsed_ms").and_then(Value::as_u64) else {
        return;
    };
    let object = ensure_json_object(metadata);
    let total = object
        .get("elapsed_ms")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_add(delta);
    object.insert("elapsed_ms".to_string(), Value::from(total));
}

fn ensure_json_object_field<'a>(
    value: &'a mut Value,
    key: &str,
) -> &'a mut serde_json::Map<String, Value> {
    let object = ensure_json_object(value);
    let entry = object
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(serde_json::Map::new());
    }
    entry.as_object_mut().expect("object field")
}

fn ensure_json_object(value: &mut Value) -> &mut serde_json::Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(serde_json::Map::new());
    }
    value.as_object_mut().expect("object")
}

fn timeline_history_projection(
    summaries: &[TuiMessageSummary],
) -> Result<(
    BTreeMap<String, TimelineHistoryOrder>,
    BTreeMap<String, Value>,
)> {
    let mut order = BTreeMap::new();
    let mut tool_metadata = BTreeMap::new();
    for summary in summaries {
        let seq = summary.session_seq;
        let value = serde_json::to_value(&summary.message)?;
        match value.get("role").and_then(Value::as_str) {
            Some("user") => {
                order.insert(format!("message:{seq}:prompt"), (seq, 0, 0));
                order.insert(format!("message:{seq}:skill-loaded"), (seq, 1, 0));
            }
            Some("assistant") => {
                let mut assistant_text_seen = false;
                if let Some(content) = value.get("content").and_then(Value::as_array) {
                    for (index, block) in content.iter().enumerate() {
                        let block_order = 10 + (index as i64 * 10);
                        match block.get("type").and_then(Value::as_str) {
                            Some("reasoning") => {
                                order.insert(
                                    format!("message:{seq}:reasoning:{index}"),
                                    (seq, block_order, 0),
                                );
                            }
                            Some("text") if !assistant_text_seen => {
                                assistant_text_seen = true;
                                order.insert(
                                    format!("message:{seq}:assistant"),
                                    (seq, block_order, 1),
                                );
                            }
                            Some("tool_call") => {
                                let Some(tool_call_id) = block.get("id").and_then(Value::as_str)
                                else {
                                    continue;
                                };
                                let item_id = format!("tool:{tool_call_id}");
                                order.insert(item_id.clone(), (seq, block_order, 2));
                                let tool_name =
                                    block.get("name").and_then(Value::as_str).unwrap_or("tool");
                                upsert_tool_history_metadata(
                                    &mut tool_metadata,
                                    &item_id,
                                    serde_json::json!({
                                        "projection": "tool",
                                        "tool_name": tool_name,
                                        "tool_call_id": tool_call_id,
                                        "outcome": "normal",
                                        "message_session_seq": seq,
                                        "content_array_index": index,
                                        "content_index": block.get("content_index").cloned().unwrap_or(Value::Null),
                                        "call_index": block.get("call_index").cloned().unwrap_or(Value::Null),
                                        "arguments": block.get("arguments").cloned().unwrap_or(Value::Null),
                                        "args": block.get("arguments").cloned().unwrap_or(Value::Null),
                                        "arguments_error": block.get("arguments_error").cloned().unwrap_or(Value::Null),
                                    }),
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
            Some("tool_result") => {
                let Some(tool_call_id) = value.get("tool_call_id").and_then(Value::as_str) else {
                    continue;
                };
                let item_id = format!("tool:{tool_call_id}");
                let tool_name = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let is_error = value
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let content = value
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let result = serde_json::from_str::<Value>(content)
                    .unwrap_or_else(|_| serde_json::json!({ "content": content }));
                upsert_tool_history_metadata(
                    &mut tool_metadata,
                    &item_id,
                    serde_json::json!({
                        "projection": "tool",
                        "tool_name": tool_name,
                        "tool_call_id": tool_call_id,
                        "outcome": if is_error { "failed" } else { "normal" },
                        "is_error": is_error,
                        "tool_result_message_session_seq": seq,
                        "result": result,
                    }),
                );
            }
            _ => {}
        }
    }
    Ok((order, tool_metadata))
}

fn upsert_tool_history_metadata(
    metadata: &mut BTreeMap<String, Value>,
    item_id: &str,
    update: Value,
) {
    let existing = metadata.remove(item_id);
    metadata.insert(item_id.to_string(), merge_json_objects(existing, update));
}

fn merge_json_objects(existing: Option<Value>, update: Value) -> Value {
    let mut object = match existing {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = serde_json::Map::new();
            object.insert("value".to_string(), value);
            object
        }
        None => serde_json::Map::new(),
    };
    match update {
        Value::Object(update) => {
            for (key, value) in update {
                object.insert(key, value);
            }
        }
        value => {
            object.insert("value".to_string(), value);
        }
    }
    Value::Object(object)
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
