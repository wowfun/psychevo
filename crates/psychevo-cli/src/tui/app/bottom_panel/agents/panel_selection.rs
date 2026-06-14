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
        if matches!(ui.bottom_panel, Some(BottomPanel::PermissionApproval(_))) {
            return self.handle_permission_approval_panel_key(ui, key);
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

    pub(crate) fn handle_permission_approval_panel_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        let Some(BottomPanel::PermissionApproval(mut panel)) = ui.bottom_panel.take() else {
            return Ok(false);
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('d') | KeyCode::Char('D') => {
                ui.resolve_permission_approval(panel, PermissionApprovalDecision::deny());
            }
            KeyCode::Enter => {
                let decision = match panel.select_outcome() {
                    PermissionApprovalOutcome::AllowOnce => {
                        PermissionApprovalDecision::allow_once()
                    }
                    PermissionApprovalOutcome::AllowSession => {
                        PermissionApprovalDecision::allow_session()
                    }
                    PermissionApprovalOutcome::AllowAlways => {
                        PermissionApprovalDecision::allow_always()
                    }
                    PermissionApprovalOutcome::Deny => PermissionApprovalDecision::deny(),
                };
                ui.resolve_permission_approval(panel, decision);
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                ui.resolve_permission_approval(panel, PermissionApprovalDecision::allow_once());
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                ui.resolve_permission_approval(panel, PermissionApprovalDecision::allow_session());
            }
            KeyCode::Char('p') | KeyCode::Char('P') if panel.request.allow_always => {
                ui.resolve_permission_approval(panel, PermissionApprovalDecision::allow_always());
            }
            KeyCode::Up | KeyCode::Char('k') => {
                panel.move_selection(-1);
                ui.bottom_panel = Some(BottomPanel::PermissionApproval(panel));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                panel.move_selection(1);
                ui.bottom_panel = Some(BottomPanel::PermissionApproval(panel));
            }
            _ => {
                ui.bottom_panel = Some(BottomPanel::PermissionApproval(panel));
            }
        }
        Ok(false)
    }

    pub(crate) fn handle_permission_approval_panel_click(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        index: usize,
    ) -> Result<()> {
        let Some(BottomPanel::PermissionApproval(mut panel)) = ui.bottom_panel.take() else {
            return Ok(());
        };
        panel.selected = index.min(panel.options().len().saturating_sub(1));
        let decision = match panel.select_outcome() {
            PermissionApprovalOutcome::AllowOnce => PermissionApprovalDecision::allow_once(),
            PermissionApprovalOutcome::AllowSession => PermissionApprovalDecision::allow_session(),
            PermissionApprovalOutcome::AllowAlways => PermissionApprovalDecision::allow_always(),
            PermissionApprovalOutcome::Deny => PermissionApprovalDecision::deny(),
        };
        ui.resolve_permission_approval(panel, decision);
        Ok(())
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
                    if let Some((selector, _)) = self.active_gateway_turn_selector(ui) {
                        if self.gateway.submit_clarify(
                            selector,
                            &panel.request.call_id,
                            ClarifyResult::Cancelled,
                        ) {
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
            let Some((selector, _)) = self.active_gateway_turn_selector(ui) else {
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
            if !self.gateway.submit_clarify(
                selector,
                &panel.request.call_id,
                ClarifyResult::Answered(response),
            ) {
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
                entrypoints,
                shadowed,
            }) => {
                ui.bottom_panel = Some(BottomPanel::AgentActions(self.agent_action_panel(
                    name,
                    source,
                    path,
                    entrypoints,
                    shadowed,
                )));
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

}
