impl TuiApp {
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
                if let Some(BottomSelectionValue::AgentAvailable {
                    name, entrypoints, ..
                }) = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(BottomPanel::selected_value)
                {
                    if entrypoints.contains(&AgentEntrypoint::Subagent) {
                        ui.bottom_panel =
                            Some(BottomPanel::AgentRunPrompt(AgentRunPromptPanel::new(name)));
                    } else {
                        ui.set_bottom_panel_notice("agent does not support subagent runs");
                    }
                }
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                if let Some(BottomSelectionValue::AgentAvailable {
                    name,
                    source,
                    path,
                    entrypoints: _,
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
