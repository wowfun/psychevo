impl TuiApp {
    fn handle_bottom_panel_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        if ui.bottom_panel.is_none() {
            return Ok(false);
        }
        if matches!(ui.bottom_panel, Some(BottomPanel::ProviderWizard(_))) {
            return self.handle_provider_wizard_key(ui, key);
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
                    if matches!(ui.bottom_panel, Some(BottomPanel::Models(_))) {
                        self.model_catalog.abort_unfinished();
                    }
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
            Some(BottomSelectionValue::AddProvider) => {
                if self.config_path.is_some() {
                    ui.set_bottom_panel_notice(
                        "cannot add provider while PSYCHEVO_CONFIG is active",
                    );
                } else {
                    ui.bottom_panel = Some(BottomPanel::ProviderWizard(self.provider_wizard_panel()));
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

    fn handle_provider_wizard_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                ui.bottom_panel = Some(BottomPanel::Models(self.model_selection_panel()?));
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
        let mut panel = self.model_selection_panel()?;
        panel.select_value_key(&format!("fetch:provider:{}", result.provider_id));
        panel.notice = Some("provider saved; fetching models".to_string());
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
        let query = panel.query.clone();
        let notice = panel.notice.clone();
        let mut panel = self.model_selection_panel()?;
        panel.query = query;
        panel.notice = notice;
        if let Some(key) = selected_key {
            panel.select_value_key(&key);
        }
        ui.bottom_panel = Some(BottomPanel::Models(panel));
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
                    ui.textarea = textarea_with_text(&entry);
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
