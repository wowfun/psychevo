impl TuiApp {
    fn handle_bottom_panel_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        if ui.bottom_panel.is_none() {
            return Ok(false);
        }
        if matches!(ui.bottom_panel, Some(BottomPanel::Help(_))) {
            return self.handle_help_panel_key(ui, key);
        }
        if matches!(ui.bottom_panel, Some(BottomPanel::ProviderWizard(_))) {
            return self.handle_provider_wizard_key(ui, key);
        }
        if matches!(ui.bottom_panel, Some(BottomPanel::AgentEditor(_))) {
            return self.handle_agent_editor_key(ui, key);
        }
        if matches!(ui.bottom_panel, Some(BottomPanel::Agents(_))) {
            return self.handle_agent_panel_key(ui, key);
        }
        if matches!(ui.bottom_panel, Some(BottomPanel::Models(_))) {
            return self.handle_model_panel_key(ui, key);
        }
        match key.code {
            KeyCode::Esc => {
                if let Some(panel) = &mut ui.bottom_panel
                    && panel.selection_mut().cancel_transient_action()
                {
                    return Ok(false);
                }
                if let Some(BottomPanel::Variants { models, .. }) = ui.bottom_panel.take() {
                    ui.bottom_panel = Some(BottomPanel::Models(*models));
                } else {
                    ui.bottom_panel = None;
                }
            }
            KeyCode::Enter => {
                let selected = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(BottomPanel::selected_value);
                self.apply_bottom_panel_selection(ui, selected)?;
            }
            KeyCode::Tab => {
                self.toggle_session_panel_view(ui)?;
            }
            KeyCode::Up => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.move_selection(-1);
                }
            }
            KeyCode::Down => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.move_selection(1);
                }
            }
            KeyCode::PageUp => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().move_selection(-8);
                }
            }
            KeyCode::PageDown => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().move_selection(8);
                }
            }
            KeyCode::Home => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().move_to(0);
                }
            }
            KeyCode::End => {
                if let Some(panel) = &mut ui.bottom_panel {
                    let len = panel.selection().filtered_indices().len();
                    panel.selection_mut().move_to(len.saturating_sub(1));
                }
            }
            KeyCode::Backspace => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().backspace_query();
                }
            }
            KeyCode::Char('k') if key.modifiers == KeyModifiers::CONTROL => {
                if let Some(BottomPanel::Sessions(panel)) = &mut ui.bottom_panel {
                    panel.arm_action_mode();
                }
            }
            KeyCode::Char(c)
                if ui
                    .bottom_panel
                    .as_ref()
                    .is_some_and(|panel| panel.selection().action_armed) =>
            {
                self.apply_session_panel_action(ui, c)?;
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().set_query_char(c);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn apply_bottom_panel_selection(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        selected: Option<BottomSelectionValue>,
    ) -> Result<()> {
        match selected {
            Some(BottomSelectionValue::Session(session_id)) => {
                let archived = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(BottomPanel::session_view)
                    .is_some_and(|view| view == SessionListView::Archived);
                if archived {
                    SqliteStore::open(&self.db_path)?.restore_session(&session_id)?;
                }
                self.switch_session_no_print(&session_id)?;
                ui.bottom_panel = None;
                ui.clear_transcript();
                self.load_current_session_history(ui)?;
                ui.refresh_sidebar(self);
            }
            Some(BottomSelectionValue::AgentRunning {
                id,
                child_session_id,
            }) => {
                let target = if child_session_id.is_empty() {
                    id
                } else {
                    child_session_id
                };
                self.open_agent_target_session(ui, &target)?;
            }
            Some(BottomSelectionValue::AgentAvailable {
                name,
                source,
                path,
                shadowed,
            }) => {
                ui.bottom_panel = Some(BottomPanel::AgentActions(
                    self.agent_action_panel(name, source, path, shadowed),
                ));
            }
            Some(BottomSelectionValue::AgentAction {
                name,
                source,
                path,
                shadowed,
                action,
            }) => self.apply_agent_action(ui, name, source, path, shadowed, action)?,
            Some(BottomSelectionValue::AgentCreate) => {
                ui.bottom_panel = Some(BottomPanel::AgentEditor(AgentEditorPanel::create()));
            }
            Some(BottomSelectionValue::AgentMainDefault) => {
                self.use_default_main_agent(ui)?;
            }
            Some(BottomSelectionValue::AgentSpawningToggle) => {
                self.toggle_agent_spawning(ui);
            }
            Some(BottomSelectionValue::AgentDiagnostic(_)) => {}
            Some(BottomSelectionValue::AddProvider) => {
                if self.config_path.is_some() {
                    ui.set_bottom_panel_notice(
                        "cannot add provider while PSYCHEVO_CONFIG is active",
                    );
                } else {
                    ui.bottom_panel =
                        Some(BottomPanel::ProviderWizard(self.provider_wizard_panel()));
                }
            }
            Some(BottomSelectionValue::FetchAllModels) => {
                self.start_model_catalog_fetch_all(ui)?;
            }
            Some(BottomSelectionValue::FetchProvider(provider)) => {
                self.start_model_catalog_fetch_provider(ui, &provider)?;
            }
            Some(BottomSelectionValue::ProviderInfo(provider)) => {
                let message = if provider == "all" {
                    if self.model_catalog.providers.is_empty() {
                        "no configured providers".to_string()
                    } else if self.model_catalog.any_fetching() {
                        "already fetching".to_string()
                    } else {
                        "no fetchable providers".to_string()
                    }
                } else {
                    self.model_catalog
                        .providers
                        .get(&provider)
                        .map(|state| self.provider_status_text(state))
                        .unwrap_or_else(|| "provider unavailable".to_string())
                };
                ui.set_bottom_panel_notice(message);
            }
            Some(BottomSelectionValue::StatsRow(_)) => {}
            Some(BottomSelectionValue::Model { model, source }) => {
                self.model_catalog.abort_unfinished();
                if let Some(BottomPanel::Models(models)) = ui.bottom_panel.take() {
                    ui.bottom_panel = Some(self.variant_panel(*model, source, models));
                }
            }
            Some(BottomSelectionValue::Variant { model, variant }) => {
                self.set_model_and_variant_no_print(model.clone(), variant.clone())?;
                ui.bottom_panel = None;
                ui.push_status(format!(
                    "model: {model}  variant: {}",
                    variant.as_deref().unwrap_or("config default")
                ));
                ui.refresh_sidebar(self);
            }
            None => {}
        }
        Ok(())
    }

    fn handle_agent_panel_key(&mut self, ui: &mut FullscreenUi<'_>, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => ui.bottom_panel = None,
            KeyCode::Tab | KeyCode::Right => {
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    panel.move_tab(1);
                }
            }
            KeyCode::BackTab | KeyCode::Left => {
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    panel.move_tab(-1);
                }
            }
            KeyCode::Enter => {
                let selected = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(BottomPanel::selected_value);
                self.apply_bottom_panel_selection(ui, selected)?;
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                if let Some(BottomSelectionValue::AgentRunning { id, .. }) = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(BottomPanel::selected_value)
                {
                    self.stop_agent_from_panel(ui, &id)?;
                }
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.toggle_agent_spawning(ui);
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if let Some(BottomSelectionValue::AgentAvailable { name, .. }) = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(BottomPanel::selected_value)
                {
                    ui.bottom_panel =
                        Some(BottomPanel::AgentRunPrompt(AgentRunPromptPanel::new(name)));
                }
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                if let Some(BottomSelectionValue::AgentAvailable {
                    name,
                    source,
                    path,
                    shadowed,
                }) = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(BottomPanel::selected_value)
                {
                    self.apply_agent_action(ui, name, source, path, shadowed, AgentAction::View)?;
                }
            }
            KeyCode::Up => {
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    panel.selection_mut().move_selection(-1);
                }
            }
            KeyCode::Down => {
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    panel.selection_mut().move_selection(1);
                }
            }
            KeyCode::PageUp => {
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    panel.selection_mut().move_selection(-8);
                }
            }
            KeyCode::PageDown => {
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    panel.selection_mut().move_selection(8);
                }
            }
            KeyCode::Home => {
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    panel.selection_mut().move_to(0);
                }
            }
            KeyCode::End => {
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    let len = panel.selection().filtered_indices().len();
                    panel.selection_mut().move_to(len.saturating_sub(1));
                }
            }
            KeyCode::Backspace => {
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    panel.selection_mut().backspace_query();
                }
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    panel.selection_mut().set_query_char(c);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_agent_run_prompt_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                ui.bottom_panel = Some(BottomPanel::Agents(self.agent_panel()));
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    panel.tab = AgentTab::Available;
                }
            }
            KeyCode::Enter => {
                let (agent_name, prompt) = match ui.bottom_panel.as_ref() {
                    Some(BottomPanel::AgentRunPrompt(panel)) => {
                        (panel.agent_name.clone(), panel.prompt.clone())
                    }
                    _ => return Ok(false),
                };
                if prompt.trim().is_empty() {
                    if let Some(BottomPanel::AgentRunPrompt(panel)) = &mut ui.bottom_panel {
                        panel.notice = Some("prompt is required".to_string());
                    }
                    return Ok(false);
                }
                self.start_available_agent_run(ui, agent_name, prompt)
                    .await?;
            }
            KeyCode::Backspace => {
                if let Some(BottomPanel::AgentRunPrompt(panel)) = &mut ui.bottom_panel {
                    panel.prompt.pop();
                    panel.notice = None;
                }
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(BottomPanel::AgentRunPrompt(panel)) = &mut ui.bottom_panel {
                    panel.prompt.push(c);
                    panel.notice = None;
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_agent_editor_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                ui.bottom_panel = Some(BottomPanel::Agents(self.agent_panel()));
                if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
                    panel.tab = AgentTab::Available;
                }
            }
            KeyCode::Enter => {
                let save = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(|panel| match panel {
                        BottomPanel::AgentEditor(panel) => Some(
                            panel.active_field
                                == *AgentEditorField::fields()
                                    .last()
                                    .unwrap_or(&AgentEditorField::Background),
                        ),
                        _ => None,
                    })
                    .unwrap_or(false);
                if save {
                    self.save_agent_editor(ui)?;
                } else if let Some(BottomPanel::AgentEditor(panel)) = &mut ui.bottom_panel {
                    panel.move_field(1);
                }
            }
            KeyCode::Up => {
                if let Some(BottomPanel::AgentEditor(panel)) = &mut ui.bottom_panel {
                    panel.move_field(-1);
                }
            }
            KeyCode::Down | KeyCode::Tab => {
                if let Some(BottomPanel::AgentEditor(panel)) = &mut ui.bottom_panel {
                    panel.move_field(1);
                }
            }
            KeyCode::Backspace => {
                if let Some(BottomPanel::AgentEditor(panel)) = &mut ui.bottom_panel {
                    panel.backspace();
                }
            }
            KeyCode::Char(' ')
                if ui.bottom_panel.as_ref().is_some_and(|panel| {
                    matches!(
                        panel,
                        BottomPanel::AgentEditor(AgentEditorPanel {
                            active_field: AgentEditorField::Background,
                            ..
                        })
                    )
                }) =>
            {
                if let Some(BottomPanel::AgentEditor(panel)) = &mut ui.bottom_panel {
                    panel.background = !panel.background;
                    panel.notice = None;
                }
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(BottomPanel::AgentEditor(panel)) = &mut ui.bottom_panel {
                    panel.insert_char(c);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn apply_agent_action(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        name: String,
        source: AgentSource,
        path: Option<PathBuf>,
        shadowed: bool,
        action: AgentAction,
    ) -> Result<()> {
        match action {
            AgentAction::UseAsMain => {
                self.use_agent_as_main(ui, name, source, path, shadowed)?;
            }
            AgentAction::Run => {
                ui.bottom_panel = Some(BottomPanel::AgentRunPrompt(AgentRunPromptPanel::new(name)));
            }
            AgentAction::View => {
                let text = self.agent_definition_detail_text(&name, path.as_ref(), shadowed)?;
                ui.bottom_panel = None;
                ui.push_command_result("/agents".to_string(), Some("Agent"), text, false);
            }
            AgentAction::Update => {
                if !agent_definition_editable(source, path.as_ref()) {
                    ui.set_bottom_panel_notice("agent definition is read-only");
                    return Ok(());
                }
                let Some(path) = path else {
                    ui.set_bottom_panel_notice("agent path is unavailable");
                    return Ok(());
                };
                let Some(editor) = self.agent_editor_for_path(&path)? else {
                    ui.set_bottom_panel_notice("agent definition could not be loaded");
                    return Ok(());
                };
                ui.bottom_panel = Some(BottomPanel::AgentEditor(editor));
            }
            AgentAction::Delete => {
                if !agent_definition_editable(source, path.as_ref()) {
                    ui.set_bottom_panel_notice("agent definition is read-only");
                    return Ok(());
                }
                let Some(path) = path else {
                    ui.set_bottom_panel_notice("agent path is unavailable");
                    return Ok(());
                };
                fs::remove_file(&path)?;
                let mut panel = self.agent_panel();
                panel.tab = AgentTab::Available;
                panel.available.notice = Some(format!("deleted {}", path.display()));
                ui.bottom_panel = Some(BottomPanel::Agents(panel));
            }
        }
        Ok(())
    }

    fn use_default_main_agent(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        if ui.running.is_some() {
            ui.set_bottom_panel_notice("finish the current turn before switching main agent");
            return Ok(());
        }
        let next_agent = if let Some(session_id) = self.current_session.as_deref() {
            let store = match SqliteStore::open(&self.db_path) {
                Ok(store) => store,
                Err(err) => {
                    ui.set_bottom_panel_notice(format!("failed to save main agent: {err:#}"));
                    return Ok(());
                }
            };
            let metadata = match store.session_metadata(session_id) {
                Ok(metadata) => metadata,
                Err(err) => {
                    ui.set_bottom_panel_notice(format!("failed to save main agent: {err:#}"));
                    return Ok(());
                }
            };
            if let Err(err) = store.set_session_metadata_field(
                session_id,
                SESSION_MAIN_AGENT_METADATA_KEY,
                Some(main_agent_default_metadata()),
            ) {
                ui.set_bottom_panel_notice(format!("failed to save main agent: {err:#}"));
                return Ok(());
            }
            metadata
                .as_ref()
                .and_then(|metadata| session_base_agent_name_from_metadata(Some(metadata)))
        } else {
            None
        };
        self.current_agent = next_agent;
        self.current_agent_explicit_default = true;
        self.refresh_selected_model();
        if let Some(session_id) = self.current_session.as_deref()
            && let Err(err) = self.reload_context_after_main_agent_switch(session_id)
        {
            ui.set_bottom_panel_notice(format!("failed to reload context: {err:#}"));
            return Ok(());
        }
        ui.bottom_panel = None;
        ui.refresh_sidebar(self);
        Ok(())
    }

    fn use_agent_as_main(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        name: String,
        source: AgentSource,
        path: Option<PathBuf>,
        shadowed: bool,
    ) -> Result<()> {
        if ui.running.is_some() {
            ui.set_bottom_panel_notice("finish the current turn before switching main agent");
            return Ok(());
        }
        if shadowed {
            ui.set_bottom_panel_notice("shadowed agent definitions cannot be used as main");
            return Ok(());
        }
        let input = if source == AgentSource::Explicit {
            path.as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| name.clone())
        } else {
            name.clone()
        };
        let metadata = main_agent_metadata(&input, &name, source, path.as_ref());
        if let Some(session_id) = self.current_session.as_deref()
            && let Err(err) = SqliteStore::open(&self.db_path).and_then(|store| {
                store.set_session_metadata_field(
                    session_id,
                    SESSION_MAIN_AGENT_METADATA_KEY,
                    Some(metadata.clone()),
                )
            })
        {
            ui.set_bottom_panel_notice(format!("failed to save main agent: {err:#}"));
            return Ok(());
        }
        self.current_agent = Some(input);
        self.current_agent_explicit_default = false;
        self.refresh_selected_model();
        if let Some(session_id) = self.current_session.as_deref()
            && let Err(err) = self.reload_context_after_main_agent_switch(session_id)
        {
            ui.set_bottom_panel_notice(format!("failed to reload context: {err:#}"));
            return Ok(());
        }
        ui.bottom_panel = None;
        ui.refresh_sidebar(self);
        Ok(())
    }

    fn reload_context_after_main_agent_switch(&self, session_id: &str) -> Result<()> {
        reload_session_context(ReloadContextOptions {
            db_path: self.db_path.clone(),
            session: session_id.to_string(),
            config_path: self.config_path.clone(),
            mode: Some(self.current_mode),
            inherited_env: Some(self.env_map.clone()),
            agent: self.current_agent.clone(),
            no_agents: self.no_agents,
            no_skills: self.no_skills,
            invalidation_reason: "main_agent_changed".to_string(),
            notice: Some("The selected main agent changed; the session prompt prefix was rebuilt before this turn.".to_string()),
        })?;
        Ok(())
    }

    fn stop_agent_from_panel(&mut self, ui: &mut FullscreenUi<'_>, id: &str) -> Result<()> {
        let store = SqliteStore::open(&self.db_path)?;
        let _ = stop_agent_id_with_grace(id, Some(&store), Duration::from_millis(1200))?;
        let mut panel = self.agent_panel();
        panel.tab = AgentTab::Running;
        panel.running.notice = Some("agent subtree stopped".to_string());
        ui.bottom_panel = Some(BottomPanel::Agents(panel));
        Ok(())
    }

    fn toggle_agent_spawning(&mut self, ui: &mut FullscreenUi<'_>) {
        let paused = !agent_spawn_paused();
        set_agent_spawn_paused(paused);
        let mut panel = self.agent_panel();
        panel.tab = AgentTab::Running;
        panel.running.notice = Some(if paused {
            "new agent spawns paused".to_string()
        } else {
            "new agent spawns resumed".to_string()
        });
        ui.bottom_panel = Some(BottomPanel::Agents(panel));
    }

    async fn start_available_agent_run(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        agent_name: String,
        prompt: String,
    ) -> Result<()> {
        let result = spawn_agent_background(AgentSpawnOptions {
            db_path: self.db_path.clone(),
            workdir: self.workdir.clone(),
            parent_session: self.current_session.clone(),
            prompt,
            agent: agent_name.clone(),
            config_path: self.config_path.clone(),
            model: self.current_model.clone(),
            reasoning_effort: self.current_variant.clone(),
            mode: self.current_mode,
            inherited_env: Some(self.env_map.clone()),
            selected_parent_agent: self.current_agent.clone(),
            no_skills: self.no_skills,
            skill_inputs: self.skill_inputs.clone(),
        })
        .await?;
        self.current_session = Some(result.parent_session_id);
        self.refresh_current_session_title()?;
        self.force_new_once = false;
        ui.bottom_panel = None;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        ui.push_status(format!("agent started: {}", result.agent.id));
        ui.refresh_sidebar(self);
        Ok(())
    }

    fn agent_definition_detail_text(
        &self,
        name: &str,
        path: Option<&PathBuf>,
        shadowed: bool,
    ) -> Result<String> {
        let Some(catalog) = self.current_agent_catalog() else {
            return Ok("Agents disabled.".to_string());
        };
        let agents = if shadowed {
            catalog.shadowed_agents
        } else {
            catalog.agents
        };
        let agent = agents.into_iter().find(|agent| {
            agent.name == name
                && path
                    .map(|path| agent.file_path.as_ref() == Some(path))
                    .unwrap_or(true)
        });
        let Some(agent) = agent else {
            return Ok(format!("agent not found: {name}"));
        };
        Ok(serde_json::to_string_pretty(
            &psychevo_runtime::view_agent_value(&agent),
        )?)
    }

    fn agent_editor_for_path(&self, path: &PathBuf) -> Result<Option<AgentEditorPanel>> {
        let Some(catalog) = self.current_agent_catalog() else {
            return Ok(None);
        };
        let agent = catalog
            .agents
            .into_iter()
            .chain(catalog.shadowed_agents)
            .find(|agent| agent.file_path.as_ref() == Some(path));
        let Some(agent) = agent else {
            return Ok(None);
        };
        let tools = agent
            .tool_policy
            .allowed
            .as_ref()
            .map(|tools| tools.iter().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        let permission_mode = agent
            .tool_policy
            .permission_mode
            .map(|mode| match mode {
                psychevo_runtime::AgentPermissionMode::Default => "default",
                psychevo_runtime::AgentPermissionMode::AcceptEdits => "acceptEdits",
                psychevo_runtime::AgentPermissionMode::Plan => "plan",
            })
            .unwrap_or_default()
            .to_string();
        Ok(Some(AgentEditorPanel {
            mode: AgentEditorMode::Update { path: path.clone() },
            name: agent.name,
            description: agent.description,
            instructions: agent.instructions,
            model: agent.model.unwrap_or_default(),
            tools,
            permission_mode,
            background: agent.background.unwrap_or(false),
            max_spawn_depth: agent.max_spawn_depth.to_string(),
            active_field: AgentEditorField::Name,
            notice: None,
        }))
    }

    fn save_agent_editor(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(BottomPanel::AgentEditor(panel)) = ui.bottom_panel.as_ref() else {
            return Ok(());
        };
        let panel = panel.clone();
        let name = panel.name.trim();
        if !valid_local_agent_name(name) {
            ui.set_bottom_panel_notice("name must use lowercase letters, digits, and hyphens");
            return Ok(());
        }
        if panel.description.trim().is_empty() {
            ui.set_bottom_panel_notice("description is required");
            return Ok(());
        }
        if parse_agent_editor_max_spawn_depth(&panel).is_none() {
            ui.set_bottom_panel_notice(format!(
                "max spawn depth must be 0..{}",
                MAX_AGENT_SPAWN_DEPTH_CAP
            ));
            return Ok(());
        }
        let path = match &panel.mode {
            AgentEditorMode::Create => self
                .workdir
                .join(".psychevo")
                .join("agents")
                .join(format!("{}.md", name)),
            AgentEditorMode::Update { path } => path.clone(),
        };
        if matches!(panel.mode, AgentEditorMode::Create) && path.exists() {
            ui.set_bottom_panel_notice("agent file already exists");
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, agent_editor_markdown(&panel))?;
        let mut agent_panel = self.agent_panel();
        agent_panel.tab = AgentTab::Available;
        agent_panel.available.notice = Some(format!("saved {}", path.display()));
        ui.bottom_panel = Some(BottomPanel::Agents(agent_panel));
        Ok(())
    }

    fn handle_model_panel_key(&mut self, ui: &mut FullscreenUi<'_>, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.model_catalog.abort_unfinished();
                ui.bottom_panel = None;
            }
            KeyCode::Char('r') if key.modifiers == KeyModifiers::CONTROL => {
                self.start_model_metadata_refresh(ui, true);
            }
            KeyCode::Tab | KeyCode::Right => {
                if let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel {
                    panel.move_tab(1);
                }
            }
            KeyCode::BackTab | KeyCode::Left => {
                if let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel {
                    panel.move_tab(-1);
                }
            }
            _ => {
                let tab = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(|panel| match panel {
                        BottomPanel::Models(panel) => Some(panel.tab),
                        _ => None,
                    })
                    .unwrap_or(ModelTab::Models);
                match tab {
                    ModelTab::Models => self.handle_model_list_key(ui, key)?,
                    ModelTab::Info => self.handle_model_info_key(ui, key),
                }
            }
        }
        Ok(false)
    }

    fn handle_model_list_key(&mut self, ui: &mut FullscreenUi<'_>, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                let selected = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(BottomPanel::selected_value);
                self.apply_bottom_panel_selection(ui, selected)?;
            }
            KeyCode::Up => {
                if let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel {
                    panel.models.move_selection(-1);
                }
            }
            KeyCode::Down => {
                if let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel {
                    panel.models.move_selection(1);
                }
            }
            KeyCode::PageUp => {
                if let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel {
                    panel.models.move_selection(-8);
                }
            }
            KeyCode::PageDown => {
                if let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel {
                    panel.models.move_selection(8);
                }
            }
            KeyCode::Home => {
                if let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel {
                    panel.models.move_to(0);
                }
            }
            KeyCode::End => {
                if let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel {
                    let len = panel.models.filtered_indices().len();
                    panel.models.move_to(len.saturating_sub(1));
                }
            }
            KeyCode::Backspace => {
                if let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel {
                    panel.models.backspace_query();
                }
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel {
                    panel.models.set_query_char(c);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_model_info_key(&mut self, ui: &mut FullscreenUi<'_>, key: KeyEvent) {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            return;
        };
        match key.code {
            KeyCode::Up => panel.scroll_info_by(-1),
            KeyCode::Down => panel.scroll_info_by(1),
            KeyCode::PageUp => panel.scroll_info_by(-8),
            KeyCode::PageDown => panel.scroll_info_by(8),
            KeyCode::Home => panel.info_scroll = 0,
            KeyCode::End => panel.info_scroll = u16::MAX,
            KeyCode::Enter => {}
            _ => {}
        }
    }

    fn handle_help_panel_key(&mut self, ui: &mut FullscreenUi<'_>, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                ui.bottom_panel = None;
            }
            KeyCode::Tab | KeyCode::Right => {
                if let Some(BottomPanel::Help(panel)) = &mut ui.bottom_panel {
                    panel.move_tab(1);
                }
            }
            KeyCode::BackTab | KeyCode::Left => {
                if let Some(BottomPanel::Help(panel)) = &mut ui.bottom_panel {
                    panel.move_tab(-1);
                }
            }
            KeyCode::Up => {
                if let Some(BottomPanel::Help(panel)) = &mut ui.bottom_panel {
                    panel.scroll_by(-1);
                }
            }
            KeyCode::Down => {
                if let Some(BottomPanel::Help(panel)) = &mut ui.bottom_panel {
                    panel.scroll_by(1);
                }
            }
            KeyCode::PageUp => {
                if let Some(BottomPanel::Help(panel)) = &mut ui.bottom_panel {
                    panel.scroll_by(-8);
                }
            }
            KeyCode::PageDown => {
                if let Some(BottomPanel::Help(panel)) = &mut ui.bottom_panel {
                    panel.scroll_by(8);
                }
            }
            KeyCode::Home => {
                if let Some(BottomPanel::Help(panel)) = &mut ui.bottom_panel {
                    panel.scroll = 0;
                }
            }
            KeyCode::Char('g') | KeyCode::Char('G')
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(BottomPanel::Help(panel)) = &mut ui.bottom_panel {
                    panel.set_tab(HelpTab::General);
                }
            }
            KeyCode::Char('c') | KeyCode::Char('C')
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(BottomPanel::Help(panel)) = &mut ui.bottom_panel {
                    panel.set_tab(HelpTab::Commands);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_provider_wizard_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                ui.bottom_panel = Some(BottomPanel::Models(ModelPanel::new(
                    self.model_selection_panel()?,
                )));
            }
            KeyCode::Enter => {
                let save = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(|panel| match panel {
                        BottomPanel::ProviderWizard(panel) => Some(panel.is_last_field()),
                        _ => None,
                    })
                    .unwrap_or(false);
                if save {
                    self.save_provider_wizard(ui)?;
                } else if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.move_field(1);
                }
            }
            KeyCode::Up => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.move_field(-1);
                }
            }
            KeyCode::Down | KeyCode::Tab => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.move_field(1);
                }
            }
            KeyCode::Home => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.move_to_first_field();
                }
            }
            KeyCode::End => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.move_to_last_field();
                }
            }
            KeyCode::Backspace => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.backspace();
                }
                self.refresh_provider_wizard_env_state(ui);
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.insert_char(c);
                }
                self.refresh_provider_wizard_env_state(ui);
            }
            _ => {}
        }
        Ok(false)
    }

    fn provider_wizard_panel(&self) -> ProviderWizardPanel {
        let mut panel = ProviderWizardPanel::new();
        self.refresh_provider_wizard_panel_env(&mut panel);
        panel
    }

    fn refresh_provider_wizard_env_state(&self, ui: &mut FullscreenUi<'_>) {
        if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
            self.refresh_provider_wizard_panel_env(panel);
        }
    }

    fn refresh_provider_wizard_panel_env(&self, panel: &mut ProviderWizardPanel) {
        panel.api_key_env_present = panel
            .env_var()
            .as_deref()
            .is_some_and(|key| self.global_dotenv_has_value(key));
        if panel.api_key_env_present {
            panel.api_key.clear();
            if panel.active_field == ProviderWizardField::ApiKey {
                panel.move_to_last_field();
            }
        }
    }

    fn global_dotenv_has_value(&self, key: &str) -> bool {
        let Ok(text) = fs::read_to_string(self.home.join(".env")) else {
            return false;
        };
        text.lines().any(|line| {
            let line = line.trim();
            let Some((name, value)) = line.split_once('=') else {
                return false;
            };
            name.trim() == key && !strip_dotenv_quotes(value.trim()).trim().is_empty()
        })
    }

    fn save_provider_wizard(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(BottomPanel::ProviderWizard(panel)) = ui.bottom_panel.as_ref() else {
            return Ok(());
        };
        let panel = panel.clone();
        let api_key = (!panel.api_key_env_present).then_some(panel.api_key.clone());
        let result = create_global_custom_provider(CustomProviderInput {
            home: self.home.clone(),
            provider_id: panel.provider_id.clone(),
            label: panel.label.clone(),
            base_url: panel.base_url.clone(),
            api_key,
        });
        let result = match result {
            Ok(result) => result,
            Err(err) => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.notice = Some(format!("error: {err}"));
                }
                return Ok(());
            }
        };
        self.sync_model_catalog_providers()?;
        let mut panel = ModelPanel::new(self.model_selection_panel()?);
        panel
            .models
            .select_value_key(&format!("fetch:provider:{}", result.provider_id));
        panel.models.notice = Some("provider saved; fetching models".to_string());
        ui.bottom_panel = Some(BottomPanel::Models(panel));
        self.start_model_catalog_fetch_provider(ui, &result.provider_id)?;
        ui.set_bottom_panel_notice("provider saved; fetching models");
        Ok(())
    }

    fn apply_session_panel_action(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        action: char,
    ) -> Result<()> {
        let Some(BottomPanel::Sessions(panel)) = &mut ui.bottom_panel else {
            return Ok(());
        };
        let view = panel.session_view.unwrap_or(SessionListView::Active);
        let selected = panel.selected_value();
        panel.action_armed = false;
        let Some(BottomSelectionValue::Session(session_id)) = selected else {
            panel.notice = Some("no session selected".to_string());
            return Ok(());
        };
        match (view, action.to_ascii_lowercase()) {
            (SessionListView::Active, 'a') => self.archive_session_from_panel(ui, session_id),
            (SessionListView::Archived, 'r') => self.restore_session_from_panel(ui, session_id),
            (_, 'd') => self.delete_session_from_panel(ui, session_id),
            (SessionListView::Active, _) => {
                if let Some(BottomPanel::Sessions(panel)) = &mut ui.bottom_panel {
                    panel.notice = Some("action: A archive  D delete".to_string());
                }
                Ok(())
            }
            (SessionListView::Archived, _) => {
                if let Some(BottomPanel::Sessions(panel)) = &mut ui.bottom_panel {
                    panel.notice = Some("action: R restore  D delete".to_string());
                }
                Ok(())
            }
        }
    }

    fn archive_session_from_panel(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: String,
    ) -> Result<()> {
        if self.is_running_current_session(ui, &session_id) {
            if let Some(BottomPanel::Sessions(panel)) = &mut ui.bottom_panel {
                panel.notice =
                    Some("cannot archive the current session while a turn is running".to_string());
            }
            return Ok(());
        }
        SqliteStore::open(&self.db_path)?.archive_session(&session_id)?;
        if self.current_session.as_deref() == Some(session_id.as_str()) {
            self.clear_current_session_after_management(ui);
        }
        self.rebuild_session_panel(ui, SessionListView::Active, None, Some("session archived"))?;
        Ok(())
    }

    fn restore_session_from_panel(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: String,
    ) -> Result<()> {
        SqliteStore::open(&self.db_path)?.restore_session(&session_id)?;
        self.rebuild_session_panel(
            ui,
            SessionListView::Archived,
            None,
            Some("session restored"),
        )?;
        Ok(())
    }

    fn delete_session_from_panel(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: String,
    ) -> Result<()> {
        if self.is_running_current_session(ui, &session_id) {
            if let Some(BottomPanel::Sessions(panel)) = &mut ui.bottom_panel {
                panel.notice =
                    Some("cannot delete the current session while a turn is running".to_string());
            }
            return Ok(());
        }
        let Some(BottomPanel::Sessions(panel)) = &mut ui.bottom_panel else {
            return Ok(());
        };
        if panel.delete_confirm.as_deref() != Some(session_id.as_str()) {
            panel.delete_confirm = Some(session_id);
            panel.notice =
                Some("delete selected session? press Ctrl+K then D again to confirm".to_string());
            return Ok(());
        }
        let view = panel.session_view.unwrap_or(SessionListView::Active);
        panel.delete_confirm = None;
        let snapshot_dir = self
            .home
            .join("snapshots")
            .join("sessions")
            .join(&session_id);
        SqliteStore::open(&self.db_path)?.delete_session(&session_id)?;
        let cleanup_notice = if snapshot_dir.exists() {
            fs::remove_dir_all(&snapshot_dir).err().map(|err| {
                format!(
                    "session deleted; snapshot cleanup failed: {}",
                    truncate_chars(&err.to_string(), 120)
                )
            })
        } else {
            None
        };
        if self.current_session.as_deref() == Some(session_id.as_str()) {
            self.clear_current_session_after_management(ui);
        }
        self.rebuild_session_panel(
            ui,
            view,
            None,
            Some(cleanup_notice.as_deref().unwrap_or("session deleted")),
        )?;
        Ok(())
    }

    fn is_running_current_session(&self, ui: &FullscreenUi<'_>, session_id: &str) -> bool {
        ui.running
            .as_ref()
            .is_some_and(|running| matches!(running.task, RunningTask::Agent(_)))
            && self.current_session.as_deref() == Some(session_id)
    }

    fn clear_current_session_after_management(&mut self, ui: &mut FullscreenUi<'_>) {
        self.current_session = None;
        self.current_session_title = None;
        self.force_new_once = true;
        ui.clear_transcript();
        ui.replace_session_history_prompts(Vec::new());
        ui.refresh_sidebar(self);
    }

    fn toggle_session_panel_view(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
            return Ok(());
        };
        let query = panel.query.clone();
        let next = match panel.session_view.unwrap_or(SessionListView::Active) {
            SessionListView::Active => SessionListView::Archived,
            SessionListView::Archived => SessionListView::Active,
        };
        let mut panel = self.session_selection_panel(next)?;
        panel.query = query;
        ui.bottom_panel = Some(BottomPanel::Sessions(panel));
        Ok(())
    }

    fn rebuild_session_panel(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        view: SessionListView,
        selected_key: Option<String>,
        notice: Option<&str>,
    ) -> Result<()> {
        let query = ui
            .bottom_panel
            .as_ref()
            .map(|panel| panel.selection().query.clone())
            .unwrap_or_default();
        let mut panel = self.session_selection_panel(view)?;
        panel.query = query;
        if let Some(key) = selected_key {
            panel.select_value_key(&key);
        }
        panel.notice = notice.map(str::to_string);
        ui.bottom_panel = Some(BottomPanel::Sessions(panel));
        Ok(())
    }

    fn start_missing_model_metadata_cache_warmup(&mut self) {
        if self.home.join("models_dev_cache.json").is_file() {
            return;
        }
        self.start_model_metadata_refresh_task(false);
    }

    fn start_model_metadata_refresh(&mut self, ui: &mut FullscreenUi<'_>, user_initiated: bool) {
        if self.model_catalog.metadata_refreshing() {
            if user_initiated {
                ui.set_bottom_panel_notice("refreshing metadata");
            }
            return;
        }
        self.start_model_metadata_refresh_task(user_initiated);
        if user_initiated {
            ui.set_bottom_panel_notice("refreshing metadata");
        }
    }

    fn start_model_metadata_refresh_task(&mut self, user_initiated: bool) {
        if self.model_catalog.metadata_refreshing() {
            return;
        }
        let targets = self.model_metadata_cache_targets();
        if targets.is_empty() {
            return;
        }
        let home = self.home.clone();
        let env_map = self.env_map.clone();
        let task = tokio::spawn(async move {
            refresh_model_metadata_cache(home, env_map, targets)
                .await
                .map_err(|err| short_fetch_error(&err.to_string()))
        });
        self.model_catalog.metadata_refresh = Some(ModelMetadataRefreshTask {
            user_initiated,
            task,
        });
    }

    fn model_metadata_cache_targets(&mut self) -> Vec<ModelMetadataCacheTarget> {
        let _ = self.sync_model_catalog_providers();
        let mut targets = Vec::new();
        let mut seen = BTreeMap::new();
        if let Some(model) = selected_configured_model(&self.run_options(String::new()))
            .ok()
            .flatten()
        {
            push_model_metadata_target(&mut targets, &mut seen, &model, &self.model_catalog);
        }
        if let Some((provider, model)) = self
            .current_model
            .as_deref()
            .and_then(|value| value.split_once('/'))
        {
            push_raw_model_metadata_target(
                &mut targets,
                &mut seen,
                provider,
                model,
                &self.model_catalog,
            );
        }
        if let Ok(models) = configured_models(&self.run_options(String::new())) {
            let mut by_spec = BTreeMap::new();
            for model in &models {
                by_spec.insert(format_model_spec(model), model);
            }
            for recent in &self.state.recent_models {
                if let Some(model) = by_spec.get(recent) {
                    push_model_metadata_target(&mut targets, &mut seen, model, &self.model_catalog);
                } else if let Some((provider, model)) = recent.split_once('/') {
                    push_raw_model_metadata_target(
                        &mut targets,
                        &mut seen,
                        provider,
                        model,
                        &self.model_catalog,
                    );
                }
            }
            for model in &models {
                push_model_metadata_target(&mut targets, &mut seen, model, &self.model_catalog);
            }
        }
        targets
    }

    async fn drain_model_metadata_refresh(&mut self, ui: &mut FullscreenUi<'_>) -> Result<bool> {
        let Some(refresh) = self.model_catalog.metadata_refresh.as_ref() else {
            return Ok(false);
        };
        if !refresh.task.is_finished() {
            return Ok(false);
        }
        let refresh = self
            .model_catalog
            .metadata_refresh
            .take()
            .expect("checked refresh");
        let user_initiated = refresh.user_initiated;
        let result = match refresh.task.await {
            Ok(result) => result,
            Err(err) if err.is_cancelled() => return Ok(true),
            Err(err) => Err(short_fetch_error(&err.to_string())),
        };

        match result {
            Ok(()) => {
                self.refresh_selected_model();
                if matches!(ui.bottom_panel, Some(BottomPanel::Models(_))) {
                    let selected_key = ui
                        .bottom_panel
                        .as_ref()
                        .map(|panel| panel.selection().selected_key());
                    self.rebuild_model_panel(ui, selected_key)?;
                    if user_initiated {
                        ui.set_bottom_panel_notice("metadata refreshed");
                    }
                }
            }
            Err(error) => {
                if user_initiated {
                    ui.set_bottom_panel_notice(format!("metadata refresh failed: {error}"));
                } else if self.debug {
                    ui.push_status(format!("warning: metadata warmup failed: {error}"));
                }
            }
        }
        Ok(true)
    }

    fn start_model_catalog_fetch_all(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        if self.model_catalog.any_fetching() {
            ui.set_bottom_panel_notice("already fetching");
            return Ok(());
        }
        let providers = self
            .model_catalog_provider_order()
            .into_iter()
            .filter(|provider| {
                self.model_catalog
                    .providers
                    .get(provider)
                    .is_some_and(|state| state.provider.fetchable())
            })
            .collect::<Vec<_>>();
        if providers.is_empty() {
            ui.set_bottom_panel_notice(if self.model_catalog.providers.is_empty() {
                "no configured providers"
            } else {
                "no fetchable providers"
            });
            return Ok(());
        }
        for provider in providers {
            self.start_model_catalog_fetch_task(&provider);
        }
        self.rebuild_model_panel(ui, Some("fetch:all".to_string()))?;
        Ok(())
    }

    fn start_model_catalog_fetch_provider(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        provider: &str,
    ) -> Result<()> {
        let Some(state) = self.model_catalog.providers.get(provider) else {
            ui.set_bottom_panel_notice("provider unavailable");
            return Ok(());
        };
        if matches!(state.status, ModelCatalogStatus::Fetching) {
            ui.set_bottom_panel_notice("already fetching");
            return Ok(());
        }
        if !state.provider.fetchable() {
            ui.set_bottom_panel_notice(self.provider_status_text(state));
            return Ok(());
        }
        let key = format!("fetch:provider:{provider}");
        self.start_model_catalog_fetch_task(provider);
        self.rebuild_model_panel(ui, Some(key))?;
        Ok(())
    }

    fn start_model_catalog_fetch_task(&mut self, provider: &str) {
        if self.model_catalog.tasks.contains_key(provider) {
            return;
        }
        let Some(state) = self.model_catalog.providers.get_mut(provider) else {
            return;
        };
        if !state.provider.fetchable() {
            return;
        }
        state.status = ModelCatalogStatus::Fetching;
        let provider_config = state.provider.clone();
        let provider_id = provider_config.provider.clone();
        let task = tokio::spawn(async move {
            let result = fetch_model_catalog(&provider_config)
                .await
                .map_err(|err| short_fetch_error(&err.to_string()));
            ModelCatalogFetchResult {
                provider: provider_id,
                result,
            }
        });
        self.model_catalog.tasks.insert(provider.to_string(), task);
    }

    async fn drain_model_catalog_fetches(&mut self, ui: &mut FullscreenUi<'_>) -> Result<bool> {
        let finished = self
            .model_catalog
            .tasks
            .iter()
            .filter(|(_, task)| task.is_finished())
            .map(|(provider, _)| provider.clone())
            .collect::<Vec<_>>();
        if finished.is_empty() {
            return Ok(false);
        }
        let selected_key = ui
            .bottom_panel
            .as_ref()
            .map(|panel| panel.selection().selected_key());
        for provider in finished {
            let Some(task) = self.model_catalog.tasks.remove(&provider) else {
                continue;
            };
            match task.await {
                Ok(result) => {
                    if let Some(state) = self.model_catalog.providers.get_mut(&result.provider) {
                        match result.result {
                            Ok(models) => {
                                state.fetched = models;
                                state.status = ModelCatalogStatus::Fetched;
                            }
                            Err(error) => {
                                state.status = ModelCatalogStatus::Failed(error);
                            }
                        }
                    }
                }
                Err(err) if err.is_cancelled() => {}
                Err(err) => {
                    if let Some(state) = self.model_catalog.providers.get_mut(&provider) {
                        state.status =
                            ModelCatalogStatus::Failed(short_fetch_error(&err.to_string()));
                    }
                }
            }
        }
        self.rebuild_model_panel(ui, selected_key)?;
        Ok(true)
    }

    fn rebuild_model_panel(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        selected_key: Option<String>,
    ) -> Result<()> {
        let Some(BottomPanel::Models(panel)) = ui.bottom_panel.as_ref() else {
            return Ok(());
        };
        let query = panel.models.query.clone();
        let notice = panel.models.notice.clone();
        let tab = panel.tab;
        let info_scroll = panel.info_scroll;
        let mut models = self.model_selection_panel()?;
        models.query = query;
        models.notice = notice;
        if let Some(key) = selected_key {
            models.select_value_key(&key);
        }
        ui.bottom_panel = Some(BottomPanel::Models(ModelPanel {
            models,
            tab,
            info_scroll,
        }));
        Ok(())
    }

    fn copy_selected_text(&self, ui: &mut FullscreenUi<'_>) -> Result<bool> {
        let Some(text) = ui.selected_text() else {
            return Ok(false);
        };
        if let Err(err) = (self.clipboard)(&text) {
            ui.push_error(format!(
                "copy failed: {}",
                truncate_chars(&err.to_string(), 240)
            ));
            ui.clear_selection();
            return Ok(true);
        }
        ui.clear_selection();
        Ok(true)
    }

    fn copy_latest_answer_markdown(&self, ui: &mut FullscreenUi<'_>) {
        let Some(text) = ui.latest_visible_answer_markdown() else {
            ui.set_ephemeral_error("no assistant answer to copy");
            return;
        };
        match (self.clipboard)(&text) {
            Ok(()) => ui.set_ephemeral_status("copied latest answer Markdown"),
            Err(err) => ui.set_ephemeral_error(format!(
                "copy failed: {}",
                truncate_chars(&err.to_string(), 240)
            )),
        }
    }

    fn copy_latest_answer_markdown_scripted(&self) -> Result<()> {
        Err(anyhow!("no assistant answer to copy in scripted TUI"))
    }

    fn start_copy_selected_text(&mut self, ui: &mut FullscreenUi<'_>) -> bool {
        let Some(text) = ui.selected_text() else {
            return false;
        };
        ui.clear_selection();
        let clipboard = Arc::clone(&self.clipboard);
        let result_tx = self.clipboard_result_tx.clone();
        self.clipboard_copies_in_flight = self.clipboard_copies_in_flight.saturating_add(1);
        std::thread::spawn(move || {
            let result = (clipboard)(&text)
                .map_err(|err| format!("copy failed: {}", truncate_chars(&err.to_string(), 240)));
            let _ = result_tx.send(result);
        });
        true
    }

    fn drain_finished_clipboard_copies(&mut self, ui: &mut FullscreenUi<'_>) -> bool {
        let mut changed = false;
        while let Ok(result) = self.clipboard_result_rx.try_recv() {
            self.clipboard_copies_in_flight = self.clipboard_copies_in_flight.saturating_sub(1);
            changed = true;
            if let Err(message) = result {
                ui.push_error(message);
            }
        }
        changed
    }

    fn handle_history_search_key(&self, ui: &mut FullscreenUi<'_>, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                ui.history_search = false;
                ui.push_status("history search closed");
            }
            KeyCode::Enter => {
                if let Some(entry) = ui
                    .history
                    .iter()
                    .rev()
                    .find(|entry| entry.contains(&ui.history_query))
                    .cloned()
                {
                    ui.set_composer_text(&entry);
                    ui.push_status("history entry selected");
                } else {
                    ui.push_error("no history match");
                }
                ui.history_search = false;
            }
            KeyCode::Backspace => {
                ui.history_query.pop();
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                ui.history_query.push(c);
            }
            _ => {}
        }
        Ok(false)
    }
}

fn valid_local_agent_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn agent_editor_markdown(panel: &AgentEditorPanel) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", yaml_single_quote(panel.name.trim())));
    out.push_str(&format!(
        "description: {}\n",
        yaml_single_quote(panel.description.trim())
    ));
    if !panel.model.trim().is_empty() {
        out.push_str(&format!(
            "model: {}\n",
            yaml_single_quote(panel.model.trim())
        ));
    }
    if !panel.tools.trim().is_empty() {
        out.push_str(&format!(
            "tools: {}\n",
            yaml_single_quote(panel.tools.trim())
        ));
    }
    if !panel.permission_mode.trim().is_empty() {
        out.push_str(&format!(
            "permissionMode: {}\n",
            yaml_single_quote(panel.permission_mode.trim())
        ));
    }
    if panel.background {
        out.push_str("background: true\n");
    }
    let max_spawn_depth = parse_agent_editor_max_spawn_depth(panel).unwrap_or(0);
    out.push_str(&format!("maxSpawnDepth: {max_spawn_depth}\n"));
    out.push_str("---\n\n");
    out.push_str(panel.instructions.trim());
    out.push('\n');
    out
}

fn parse_agent_editor_max_spawn_depth(panel: &AgentEditorPanel) -> Option<u8> {
    let raw = panel.max_spawn_depth.trim();
    if raw.is_empty() {
        return Some(0);
    }
    let value = raw.parse::<u8>().ok()?;
    (value <= MAX_AGENT_SPAWN_DEPTH_CAP).then_some(value)
}

fn yaml_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn strip_dotenv_quotes(value: &str) -> &str {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}
