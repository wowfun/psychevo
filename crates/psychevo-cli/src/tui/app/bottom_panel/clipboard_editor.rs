#[allow(unused_imports)]
pub(crate) use super::*;
impl TuiApp {
    pub(crate) async fn drain_model_catalog_fetches(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
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

    pub(crate) fn rebuild_model_panel(
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
        let global = panel.global;
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
            global,
        }));
        Ok(())
    }

    pub(crate) fn copy_selected_text(&self, ui: &mut FullscreenUi<'_>) -> Result<bool> {
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

    pub(crate) fn copy_latest_answer_markdown(&self, ui: &mut FullscreenUi<'_>) {
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

    pub(crate) fn copy_latest_answer_markdown_scripted(&self) -> Result<()> {
        Err(anyhow!("no assistant answer to copy in scripted TUI"))
    }

    pub(crate) fn start_copy_selected_text(&mut self, ui: &mut FullscreenUi<'_>) -> bool {
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

    pub(crate) fn drain_finished_clipboard_copies(&mut self, ui: &mut FullscreenUi<'_>) -> bool {
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

    pub(crate) fn handle_history_search_key(
        &self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
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

pub(crate) fn valid_local_agent_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

pub(crate) fn agent_editor_markdown(panel: &AgentEditorPanel) -> String {
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

pub(crate) fn parse_agent_editor_max_spawn_depth(panel: &AgentEditorPanel) -> Option<u8> {
    let raw = panel.max_spawn_depth.trim();
    if raw.is_empty() {
        return Some(0);
    }
    let value = raw.parse::<u8>().ok()?;
    (value <= MAX_AGENT_SPAWN_DEPTH_CAP).then_some(value)
}

pub(crate) fn yaml_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(crate) fn strip_dotenv_quotes(value: &str) -> &str {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}
