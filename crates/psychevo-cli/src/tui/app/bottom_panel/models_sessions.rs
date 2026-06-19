#[allow(unused_imports)]
pub(crate) use super::*;
impl TuiApp {
    pub(crate) async fn start_available_agent_run(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        agent_name: String,
        prompt: String,
    ) -> Result<()> {
        let result = spawn_agent_background(AgentSpawnOptions {
            state: self.state_runtime.clone(),
            workdir: self.workdir.clone(),
            parent_session: self.current_session.clone(),
            prompt,
            agent: agent_name.clone(),
            config_path: self.config_path.clone(),
            model: self.current_model.clone(),
            reasoning_effort: self.current_variant.clone(),
            mode: self.current_mode,
            permission_mode: None,
            approval_mode: None,
            approval_handler: None,
            inherited_env: Some(self.env_map.clone()),
            selected_parent_agent: self.current_agent.clone(),
            no_skills: self.no_skills,
            skill_inputs: self.skill_inputs.clone(),
            mcp_servers: Vec::new(),
        })
        .await?;
        self.current_session = Some(result.parent_session_id);
        self.reset_live_agent_reload_poll();
        self.refresh_current_session_title()?;
        self.clear_new_session_draft();
        ui.bottom_panel = None;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        ui.push_status(format!("agent started: {}", result.agent.id));
        ui.refresh_sidebar(self);
        Ok(())
    }

    pub(crate) fn agent_definition_detail_text(
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

    pub(crate) fn agent_editor_for_path(&self, path: &PathBuf) -> Result<Option<AgentEditorPanel>> {
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

    pub(crate) fn save_agent_editor(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
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

    pub(crate) fn handle_model_panel_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
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

    pub(crate) fn handle_model_list_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<()> {
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

    pub(crate) fn handle_model_info_key(&mut self, ui: &mut FullscreenUi<'_>, key: KeyEvent) {
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

    pub(crate) fn handle_help_panel_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
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

    pub(crate) fn handle_provider_wizard_key(
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

    pub(crate) fn provider_wizard_panel(&self) -> ProviderWizardPanel {
        let mut panel = ProviderWizardPanel::new();
        self.refresh_provider_wizard_panel_env(&mut panel);
        panel
    }

    pub(crate) fn refresh_provider_wizard_env_state(&self, ui: &mut FullscreenUi<'_>) {
        if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
            self.refresh_provider_wizard_panel_env(panel);
        }
    }

    pub(crate) fn refresh_provider_wizard_panel_env(&self, panel: &mut ProviderWizardPanel) {
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

    pub(crate) fn global_dotenv_has_value(&self, key: &str) -> bool {
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

    pub(crate) fn save_provider_wizard(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
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
            no_auth: false,
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

    pub(crate) fn apply_session_panel_action(
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

    pub(crate) fn archive_session_from_panel(
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
        self.state_runtime.store().archive_session(&session_id)?;
        if self.current_session.as_deref() == Some(session_id.as_str()) {
            self.clear_current_session_after_management(ui);
        }
        self.rebuild_session_panel(ui, SessionListView::Active, None, Some("session archived"))?;
        Ok(())
    }

    pub(crate) fn restore_session_from_panel(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        session_id: String,
    ) -> Result<()> {
        self.state_runtime.store().restore_session(&session_id)?;
        self.rebuild_session_panel(
            ui,
            SessionListView::Archived,
            None,
            Some("session restored"),
        )?;
        Ok(())
    }

    pub(crate) fn delete_session_from_panel(
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
        self.state_runtime.delete_session(&session_id)?;
        if self.current_session.as_deref() == Some(session_id.as_str()) {
            self.clear_current_session_after_management(ui);
        }
        self.rebuild_session_panel(ui, view, None, Some("session deleted"))?;
        Ok(())
    }

    pub(crate) fn is_running_current_session(
        &self,
        ui: &FullscreenUi<'_>,
        session_id: &str,
    ) -> bool {
        ui.running
            .as_ref()
            .is_some_and(|running| matches!(running.task, RunningTask::Agent(_)))
            && self.current_session.as_deref() == Some(session_id)
    }

    pub(crate) fn clear_current_session_after_management(&mut self, ui: &mut FullscreenUi<'_>) {
        self.begin_new_session_draft();
        ui.clear_transcript();
        ui.replace_session_history_prompts(Vec::new());
        ui.refresh_sidebar(self);
    }

    pub(crate) fn toggle_session_panel_view(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
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

    pub(crate) fn rebuild_session_panel(
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

    pub(crate) fn start_missing_model_metadata_cache_warmup(&mut self) {
        if self.home.join("models_dev_cache.json").is_file() {
            return;
        }
        self.start_model_metadata_refresh_task(false);
    }

    pub(crate) fn start_model_metadata_refresh(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        user_initiated: bool,
    ) {
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

    pub(crate) fn start_model_metadata_refresh_task(&mut self, user_initiated: bool) {
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

    pub(crate) fn model_metadata_cache_targets(&mut self) -> Vec<ModelMetadataCacheTarget> {
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

    pub(crate) async fn drain_model_metadata_refresh(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
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

    pub(crate) fn start_model_catalog_fetch_all(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<()> {
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

    pub(crate) fn start_model_catalog_fetch_provider(
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

    pub(crate) fn start_model_catalog_fetch_task(&mut self, provider: &str) {
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
}
