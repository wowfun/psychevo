#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

impl TuiApp {
    pub(crate) fn handle_bottom_panel_key(
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
        if matches!(ui.bottom_panel, Some(BottomPanel::Clarify(_))) {
            return self.handle_clarify_panel_key(ui, key);
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

    pub(crate) fn handle_clarify_panel_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        let Some(BottomPanel::Clarify(mut panel)) = ui.bottom_panel.take() else {
            return Ok(false);
        };
        let mut restore = false;
        match panel.mode() {
            ClarifyInputMode::Options => match key.code {
                KeyCode::Esc => {
                    if let Some(running) = ui.running.as_ref() {
                        if running
                            .control
                            .submit_clarify_result(&panel.request.call_id, ClarifyResult::Cancelled)
                        {
                            restore = true;
                        } else {
                            panel.notice = Some("clarify request is no longer active".to_string());
                        }
                    } else {
                        panel.notice = Some("clarify request is no longer active".to_string());
                    }
                }
                KeyCode::Enter => {
                    if panel.selected_is_other() {
                        panel.set_mode(ClarifyInputMode::Other);
                        panel.notice = None;
                    } else {
                        self.answer_clarify_selection(ui, &mut panel)?;
                    }
                    restore = panel.answers.iter().all(Option::is_some) && panel.notice.is_none();
                }
                KeyCode::Tab => {
                    if panel.selected_is_other() {
                        panel.set_mode(ClarifyInputMode::Other);
                    } else {
                        panel.set_mode(ClarifyInputMode::Note);
                    }
                    panel.notice = None;
                }
                KeyCode::Left => panel.move_question(-1),
                KeyCode::Right => panel.move_question(1),
                KeyCode::Up => panel.move_selection(-1),
                KeyCode::Down => panel.move_selection(1),
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    if let Some(index) = c.to_digit(10).and_then(|value| value.checked_sub(1)) {
                        panel.select_index(index as usize);
                    }
                }
                _ => {}
            },
            ClarifyInputMode::Other | ClarifyInputMode::Note => match key.code {
                KeyCode::Esc => {
                    panel.set_mode(ClarifyInputMode::Options);
                    panel.notice = None;
                }
                KeyCode::Enter => {
                    self.answer_clarify_selection(ui, &mut panel)?;
                    restore = panel.answers.iter().all(Option::is_some) && panel.notice.is_none();
                }
                KeyCode::Left => panel.move_input_cursor(-1),
                KeyCode::Right => panel.move_input_cursor(1),
                KeyCode::Home => panel.move_input_cursor_to_start(),
                KeyCode::End => panel.move_input_cursor_to_end(),
                KeyCode::Backspace => {
                    panel.pop_input_char();
                }
                KeyCode::Delete => {
                    panel.delete_input_char();
                }
                KeyCode::Char(c)
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                {
                    panel.push_input_char(c);
                }
                _ => {}
            },
        }

        if restore {
            ui.bottom_panel = panel.restore_panel();
        } else {
            ui.bottom_panel = Some(BottomPanel::Clarify(panel));
        }
        Ok(false)
    }

    pub(crate) fn handle_clarify_panel_click(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        index: usize,
    ) -> Result<()> {
        let Some(BottomPanel::Clarify(mut panel)) = ui.bottom_panel.take() else {
            return Ok(());
        };
        panel.select_index(index);
        if panel.selected_is_other() {
            panel.set_mode(ClarifyInputMode::Other);
            panel.notice = None;
            ui.bottom_panel = Some(BottomPanel::Clarify(panel));
        } else {
            panel.set_mode(ClarifyInputMode::Options);
            panel.notice = None;
            self.answer_clarify_selection(ui, &mut panel)?;
            if panel.answers.iter().all(Option::is_some) && panel.notice.is_none() {
                ui.bottom_panel = panel.restore_panel();
            } else {
                ui.bottom_panel = Some(BottomPanel::Clarify(panel));
            }
        }
        Ok(())
    }

    pub(crate) fn answer_clarify_selection(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        panel: &mut ClarifyPanel,
    ) -> Result<()> {
        let question_index = panel.question_index;
        let Some(question) = panel.current_question().cloned() else {
            panel.notice = Some("clarify question is missing".to_string());
            return Ok(());
        };
        let selected = panel.selected();
        let mut answers = Vec::new();
        if selected >= question.options.len() {
            if panel.mode() != ClarifyInputMode::Other {
                panel.set_mode(ClarifyInputMode::Other);
                panel.notice = None;
                return Ok(());
            }
            panel.set_mode(ClarifyInputMode::Other);
            let text = panel.other_draft().trim().to_string();
            if text.is_empty() {
                panel.notice = Some("type an answer for Other".to_string());
                return Ok(());
            }
            answers.push(text);
        } else if let Some(option) = question.options.get(selected) {
            answers.push(option.label.clone());
            let note = panel.note_draft(selected).trim().to_string();
            if !note.is_empty() {
                answers.push(format!("user_note: {}", note.trim()));
            }
        }
        if let Some(slot) = panel.answers.get_mut(question_index) {
            *slot = Some(ClarifyAnswer { answers });
        }
        panel.set_mode(ClarifyInputMode::Options);
        panel.notice = None;
        if panel.answers.iter().all(Option::is_some) {
            let Some(running) = ui.running.as_ref() else {
                panel.notice = Some("clarify request is no longer active".to_string());
                return Ok(());
            };
            let response = ClarifyResponse {
                answers: panel
                    .answers
                    .iter()
                    .cloned()
                    .map(|answer| {
                        answer.unwrap_or(ClarifyAnswer {
                            answers: Vec::new(),
                        })
                    })
                    .collect(),
            };
            if !running
                .control
                .submit_clarify_result(&panel.request.call_id, ClarifyResult::Answered(response))
            {
                panel.notice = Some("clarify request is no longer active".to_string());
            }
            return Ok(());
        }
        panel.move_to_next_unanswered();
        Ok(())
    }

    pub(crate) fn apply_bottom_panel_selection(
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
                    self.state_runtime.store().restore_session(&session_id)?;
                }
                self.open_session_direct(ui, &session_id)?;
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
            Some(BottomSelectionValue::Toolset { name, enabled }) => {
                if self.config_path.is_some() {
                    ui.set_bottom_panel_notice(
                        "cannot change toolsets while PSYCHEVO_CONFIG is active",
                    );
                    return Ok(());
                }
                set_local_toolset_enabled(
                    self.workdir.join(".psychevo"),
                    self.current_mode,
                    &name,
                    !enabled,
                )?;
                ui.bottom_panel = Some(BottomPanel::Tools(self.toolsets_panel()?));
                ui.set_bottom_panel_notice(format!(
                    "{} toolset `{name}` for {} mode",
                    if enabled { "disabled" } else { "enabled" },
                    self.current_mode.as_str()
                ));
            }
            Some(BottomSelectionValue::Model { model, source }) => {
                self.model_catalog.abort_unfinished();
                if let Some(BottomPanel::Models(models)) = ui.bottom_panel.take() {
                    ui.bottom_panel = Some(self.variant_panel(*model, source, models));
                }
            }
            Some(BottomSelectionValue::Variant {
                model,
                variant: _,
                reasoning_effort,
            }) => {
                let global = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(|panel| match panel {
                        BottomPanel::Variants { models, .. } => Some(models.global),
                        _ => None,
                    })
                    .unwrap_or(false);
                let status = self.set_model_default_from_picker(
                    model.clone(),
                    reasoning_effort.clone(),
                    global,
                )?;
                ui.bottom_panel = None;
                ui.push_status(status);
                ui.refresh_sidebar(self);
            }
            None => {}
        }
        Ok(())
    }

    pub(crate) fn handle_agent_panel_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
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

    pub(crate) async fn handle_agent_run_prompt_key(
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

    pub(crate) fn handle_agent_editor_key(
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

    pub(crate) fn apply_agent_action(
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

    pub(crate) fn use_default_main_agent(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        if ui.running.is_some() {
            ui.set_bottom_panel_notice("finish the current turn before switching main agent");
            return Ok(());
        }
        let next_agent = if let Some(session_id) = self.current_session.as_deref() {
            let store = self.state_runtime.store();
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

    pub(crate) fn use_agent_as_main(
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
            && let Err(err) = self.state_runtime.store().set_session_metadata_field(
                session_id,
                SESSION_MAIN_AGENT_METADATA_KEY,
                Some(metadata.clone()),
            )
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

    pub(crate) fn reload_context_after_main_agent_switch(&self, session_id: &str) -> Result<()> {
        reload_session_context(ReloadContextOptions {
            state: self.state_runtime.clone(),
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

    pub(crate) fn stop_agent_from_panel(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        id: &str,
    ) -> Result<()> {
        let store = self.state_runtime.store();
        let _ = stop_agent_id_with_grace(id, Some(store), Duration::from_millis(1200))?;
        let mut panel = self.agent_panel();
        panel.tab = AgentTab::Running;
        panel.running.notice = Some("agent subtree stopped".to_string());
        ui.bottom_panel = Some(BottomPanel::Agents(panel));
        Ok(())
    }

    pub(crate) fn toggle_agent_spawning(&mut self, ui: &mut FullscreenUi<'_>) {
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
}
