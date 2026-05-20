const SESSION_MAIN_AGENT_METADATA_KEY: &str = "main_agent";

#[derive(Debug, Clone, PartialEq, Eq)]
enum LoadedMainAgent {
    Missing,
    Default,
    Agent(String),
}

impl TuiApp {
    fn refresh_selected_model(&mut self) {
        self.selected_model = selected_configured_model(&self.run_options(String::new()))
            .ok()
            .flatten();
    }

    fn refresh_current_session_title(&mut self) -> Result<()> {
        self.current_session_title = self
            .current_session
            .as_deref()
            .map(|session_id| SqliteStore::open(&self.db_path)?.session_summary(session_id))
            .transpose()?
            .flatten()
            .and_then(|summary| summary.title)
            .filter(|title| !title.trim().is_empty());
        Ok(())
    }

    fn refresh_current_session_agent(&mut self) -> Result<()> {
        let Some(session_id) = self.current_session.as_deref() else {
            if !self.current_agent_explicit_default && self.current_agent.is_none() {
                self.current_agent = self.startup_agent.clone();
            }
            return Ok(());
        };
        let store = SqliteStore::open(&self.db_path)?;
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

    fn session_identity_label(&self) -> Option<String> {
        let agent = self.current_agent.as_deref()?.trim();
        if agent.is_empty() {
            return None;
        }
        self.current_agent_display_name(agent)
    }

    fn current_agent_display_name(&self, input: &str) -> Option<String> {
        let catalog = self.current_agent_catalog()?;
        resolve_agent_definition(&catalog, input, &self.workdir, &self.env_map)
            .ok()
            .map(|agent| agent.name)
            .or_else(|| Some(input.to_string()))
    }

    fn main_agent_metadata_for_input(&self, input: &str) -> Result<Value> {
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

    fn persist_main_agent_selection_for_session(&self, session_id: &str) -> Result<()> {
        if self.current_agent_explicit_default {
            SqliteStore::open(&self.db_path)?.set_session_metadata_field(
                session_id,
                SESSION_MAIN_AGENT_METADATA_KEY,
                Some(main_agent_default_metadata()),
            )?;
        } else if let Some(input) = self.current_agent.as_deref() {
            let value = self.main_agent_metadata_for_input(input)?;
            SqliteStore::open(&self.db_path)?.set_session_metadata_field(
                session_id,
                SESSION_MAIN_AGENT_METADATA_KEY,
                Some(value),
            )?;
        }
        Ok(())
    }

    fn session_sidebar_title(&self) -> String {
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
    fn switch_session_no_print(&mut self, reference: &str) -> Result<String> {
        let id = self.resolve_session_ref(reference)?;
        SqliteStore::open(&self.db_path)?.resume_session(&id)?;
        self.current_session = Some(id.clone());
        self.force_new_once = false;
        self.refresh_current_session_title()?;
        self.refresh_current_session_agent()?;
        Ok(id)
    }

    fn open_agent_target_session(&mut self, ui: &mut FullscreenUi<'_>, target: &str) -> Result<()> {
        if ui
            .running
            .as_ref()
            .is_some_and(|running| matches!(running.task, RunningTask::UserShell(_)))
        {
            ui.push_status("finish the current shell command before opening an agent session");
            return Ok(());
        }
        let store = SqliteStore::open(&self.db_path)?;
        let edge = store
            .find_agent_edge(target)?
            .ok_or_else(|| anyhow!("agent not found: {target}"))?;
        self.detach_running_for_session_switch(ui, Some(edge.child_session_id.clone()));
        store.resume_session(&edge.child_session_id)?;
        self.current_session = Some(edge.child_session_id.clone());
        self.force_new_once = false;
        self.refresh_current_session_title()?;
        self.refresh_current_session_agent()?;
        ui.bottom_panel = None;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        self.replay_session_live_event_backlog(ui, &edge.child_session_id);
        self.replay_agent_child_event_backlog(ui, &edge.child_session_id);
        ui.refresh_sidebar(self);
        Ok(())
    }

    fn maybe_reload_live_agent_session(&mut self, ui: &mut FullscreenUi<'_>) -> Result<bool> {
        if ui.running.is_some() {
            return Ok(false);
        }
        let Some(session_id) = self.current_session.clone() else {
            return Ok(false);
        };
        let store = SqliteStore::open(&self.db_path)?;
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

    fn request_current_session_interrupt(&mut self, ui: &mut FullscreenUi<'_>) -> bool {
        let current_session = self.current_session.clone();
        let mut interrupted = ui.request_interrupt(current_session.as_deref());
        if let Some(session_id) = current_session.as_deref()
            && let Ok(store) = SqliteStore::open(&self.db_path)
        {
            let value = agent_status_value(Some(&store), Some(session_id), false);
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
                if stop_agent_id_with_grace(&target, Some(&store), Duration::ZERO)
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

    fn open_agent_parent_session(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(current) = self.current_session.clone() else {
            return Ok(());
        };
        let store = SqliteStore::open(&self.db_path)?;
        let Some(edge) = store.find_agent_edge(&current)? else {
            ui.push_status("no parent agent session");
            return Ok(());
        };
        self.open_session_direct(ui, &edge.parent_session_id)
    }

    fn open_agent_sibling_session(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        direction: isize,
    ) -> Result<()> {
        let Some(current) = self.current_session.clone() else {
            return Ok(());
        };
        let store = SqliteStore::open(&self.db_path)?;
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

    fn open_session_direct(&mut self, ui: &mut FullscreenUi<'_>, session_id: &str) -> Result<()> {
        self.detach_running_for_session_switch(ui, None);
        SqliteStore::open(&self.db_path)?.resume_session(session_id)?;
        self.current_session = Some(session_id.to_string());
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

    fn detach_running_for_session_switch(
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
            while let Ok(event) = running.rx.try_recv() {
                pending.push_back(event);
            }
        }
        let had_pending = if owner_session.is_some() {
            self.apply_pending_owned_fullscreen_stream_events(ui, owner_session.as_deref(), pending)
        } else {
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
                    rx: running.rx,
                    task,
                });
            }
            RunningTask::UserShell(task) => {
                ui.auxiliary_shell_tasks.push(AuxiliaryShellTask {
                    session_id: owner_session,
                    control: running.control,
                    rx: running.rx,
                    task,
                });
            }
        }
        ui.finish_turn();
    }

    fn replay_agent_child_event_backlog(&mut self, ui: &mut FullscreenUi<'_>, session_id: &str) {
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

    fn replay_session_live_event_backlog(&mut self, ui: &mut FullscreenUi<'_>, session_id: &str) {
        let Some(events) = ui.session_live_event_backlog.remove(session_id) else {
            return;
        };
        let pending = events.into();
        self.apply_pending_owned_fullscreen_stream_events(ui, Some(session_id), pending);
        ui.follow_transcript_if_needed();
    }

    fn agent_breadcrumb_status(&self) -> Option<String> {
        let session_id = self.current_session.as_deref()?;
        let store = SqliteStore::open(&self.db_path).ok()?;
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

    fn set_model_and_variant_no_print(
        &mut self,
        model: String,
        variant: Option<String>,
    ) -> Result<()> {
        validate_model_spec(&model)?;
        if let Some(variant) = &variant {
            validate_variant(variant)?;
        }
        self.current_model = Some(model.clone());
        self.current_variant = variant.clone();
        self.state.set_model(&self.workdir_key, model);
        if let Some(variant) = variant {
            self.state.set_variant(&self.workdir_key, variant);
        } else {
            self.state.clear_variant(&self.workdir_key);
        }
        self.state.save(&self.state_path)?;
        self.refresh_selected_model();
        Ok(())
    }

    fn set_variant_no_print(&mut self, variant: String) -> Result<()> {
        validate_variant(&variant)?;
        self.current_variant = Some(variant.clone());
        self.state.set_variant(&self.workdir_key, variant);
        self.state.save(&self.state_path)?;
        self.refresh_selected_model();
        Ok(())
    }

    fn set_mode_no_print(&mut self, mode: &str) -> Result<()> {
        let (run_mode, permission_mode) = match mode {
            "plan" => (RunMode::Plan, PermissionMode::Default),
            "default" => (RunMode::Build, PermissionMode::Default),
            "acceptEdits" | "accept-edits" => (RunMode::Build, PermissionMode::AcceptEdits),
            "dontAsk" | "dont-ask" => (RunMode::Build, PermissionMode::DontAsk),
            "bypassPermissions" | "bypass-permissions" => {
                (RunMode::Build, PermissionMode::BypassPermissions)
            }
            _ => {
                return Err(anyhow!(
                    "mode must be one of plan, default, acceptEdits, dontAsk, bypassPermissions"
                ));
            }
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

    fn set_thinking_no_print(&mut self, enabled: bool) -> Result<()> {
        self.thinking_visible = enabled;
        self.state.set_thinking_visible(enabled);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    fn set_raw_no_print(&mut self, enabled: bool) -> Result<()> {
        self.raw_visible = enabled;
        self.state.set_raw_visible(enabled);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    fn rename_session_no_print(&mut self, title: String) -> Result<String> {
        let Some(session_id) = self.current_session.as_deref() else {
            return Err(anyhow!("no current session to rename"));
        };
        let title = SqliteStore::open(&self.db_path)?.set_session_title(session_id, &title)?;
        self.current_session_title = Some(title.clone());
        Ok(title)
    }

    fn undo_options(&self) -> Result<SessionUndoOptions> {
        let Some(session_id) = self.current_session.clone() else {
            return Err(anyhow!("no current session to undo"));
        };
        Ok(SessionUndoOptions {
            db_path: self.db_path.clone(),
            workdir: self.workdir.clone(),
            snapshot_root: self.home.join("snapshots"),
            session_id,
        })
    }

    fn undo_session_no_print(&mut self, ui: &mut FullscreenUi<'_>) -> Result<String> {
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

    fn redo_session_no_print(&mut self, ui: &mut FullscreenUi<'_>) -> Result<String> {
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

    fn set_sidebar_visible_no_print(&mut self, visible: bool) -> Result<()> {
        self.state.set_sidebar_visible(visible);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    fn cycle_mode(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let next = match (self.current_mode, self.current_permission_mode) {
            (RunMode::Build, PermissionMode::Default) => "acceptEdits",
            (RunMode::Build, PermissionMode::AcceptEdits) => "plan",
            (RunMode::Plan, _) => "default",
            _ => "default",
        };
        self.set_mode_no_print(next)?;
        ui.refresh_sidebar(self);
        Ok(())
    }

    #[cfg(test)]
    fn resolve_session_ref(&self, reference: &str) -> Result<String> {
        let sessions = self.sessions_for_workdir()?;
        resolve_session_ref_from_summaries(&sessions, reference)
    }

    #[cfg(test)]
    fn sessions_for_workdir(&self) -> Result<Vec<SessionSummary>> {
        SqliteStore::open(&self.db_path)?
            .list_sessions_for_workdir_with_sources(&self.workdir, TUI_SESSION_SOURCES)
            .map_err(Into::into)
    }

    fn tui_sessions_for_workdir(
        &self,
        view: SessionListView,
    ) -> Result<Vec<TuiSessionDisplaySummary>> {
        let store = SqliteStore::open(&self.db_path)?;
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

    fn load_current_session_history(&self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(session_id) = self.current_session.as_deref() else {
            ui.loaded_session_message_count = 0;
            ui.visible_turn_started = None;
            ui.replace_session_history_prompts(Vec::new());
            ui.refresh_sidebar(self);
            return Ok(());
        };
        let store = SqliteStore::open(&self.db_path)?;
        let metadata = store.session_metadata(session_id)?;
        ui.sidebar_context_limit =
            session_context_limit_with_parent_fallback(&store, session_id, metadata.as_ref())?;
        let summaries = store.load_tui_message_summaries(session_id)?;
        ui.loaded_session_message_count = summaries.len();
        let summary_count = summaries.len();
        let suppress_latest_terminal_meta = ui.status_has_running(Some(session_id));
        let mut history_prompts = Vec::new();
        for (index, summary) in summaries.into_iter().enumerate() {
            let value = serde_json::to_value(summary.message)?;
            if value.get("role").and_then(Value::as_str) == Some("user")
                && let Some(text) = user_text_from_message(&value, summary.metadata.as_ref())
            {
                history_prompts.push(text);
            }
            ui.push_history_message_with_accounting_options(
                &value,
                summary.usage.as_ref(),
                summary.metadata.as_ref(),
                summary.accounting.as_ref(),
                suppress_latest_terminal_meta && index + 1 == summary_count,
            );
        }
        let agent_catalog = self.current_agent_catalog();
        let agent_edges = store.list_agent_edges_for_parent(session_id)?;
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

fn session_context_limit_with_parent_fallback(
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

fn session_context_limit(metadata: &Value) -> Option<u64> {
    metadata.get("context_limit").and_then(Value::as_u64)
}

fn main_agent_default_metadata() -> Value {
    serde_json::json!({"mode": "default"})
}

fn main_agent_metadata(
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

fn main_agent_from_session_metadata(metadata: Option<&Value>) -> LoadedMainAgent {
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

fn session_base_agent_name_from_metadata(metadata: Option<&Value>) -> Option<String> {
    metadata?
        .get("agent")
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
