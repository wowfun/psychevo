#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) const SIDE_CONVERSATION_NO_SESSION_MESSAGE: &str = "'/btw' is unavailable until the current conversation has started. Send a message first, then try /btw again.";
pub(crate) const SIDE_CONVERSATION_ALREADY_OPEN_MESSAGE: &str =
    "A /btw side chat is already open. Press Ctrl+C to return before starting another.";
pub(crate) const SIDE_CONVERSATION_RETURNED_MESSAGE: &str = "returned from /btw side chat";
pub(crate) const RELOAD_CONTEXT_DEPRECATED_MESSAGE: &str =
    "/reload-context is hidden in the TUI; use /refresh";

impl TuiApp {
    pub(crate) fn in_side_conversation(&self) -> bool {
        self.side_conversation.is_some()
    }

    pub(crate) fn start_side_conversation(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        initial_prompt: Option<String>,
    ) -> Result<()> {
        if self.side_conversation.is_some() {
            ui.set_ephemeral_error(SIDE_CONVERSATION_ALREADY_OPEN_MESSAGE);
            return Ok(());
        }
        let Some(parent_session) = self.current_session.clone() else {
            ui.set_ephemeral_error(SIDE_CONVERSATION_NO_SESSION_MESSAGE);
            return Ok(());
        };

        let (provider, model) = self.side_conversation_provider_model()?;
        let store = self.state_runtime.store();
        let side_thread_id =
            store.create_child_session_from_parent_snapshot(ChildSessionSnapshotInput {
                parent_session_id: &parent_session,
                workdir: &self.workdir,
                source: TUI_SIDE_CONVERSATION_SESSION_SOURCE,
                model: &model,
                provider: &provider,
                metadata: Some(serde_json::json!({
                    SIDE_CONVERSATION_METADATA_KEY: {
                        "ephemeral": true,
                        "parent_session_id": parent_session.clone(),
                    },
                    "provider_label": provider,
                    "reasoning_effort": self.current_variant.clone(),
                    "mode": self.current_mode.as_str(),
                    "permission_mode": self.current_permission_mode.as_str(),
                    "selected_agent": self.current_agent.clone(),
                })),
                max_context_messages: self.run_options(String::new()).max_context_messages,
                inherited_message_metadata: serde_json::json!({
                    SIDE_INHERITED_METADATA_KEY: {
                        "hidden": true,
                        "parent_session_id": parent_session.clone(),
                    }
                }),
                boundary_text: side_conversation_boundary_prompt(),
            })?;

        let side_state = SideConversationState {
            parent_session: parent_session.clone(),
            parent_session_title: self.current_session_title.clone(),
            parent_model: self.current_model.clone(),
            parent_variant: self.current_variant.clone(),
            parent_mode: self.current_mode,
            parent_permission_mode: self.current_permission_mode,
            parent_agent: self.current_agent.clone(),
            parent_agent_explicit_default: self.current_agent_explicit_default,
            side_thread_id: side_thread_id.clone(),
        };

        self.detach_running_for_session_switch(ui, None);
        self.side_conversation = Some(side_state);
        self.current_session = Some(side_thread_id.clone());
        self.reset_live_agent_reload_poll();
        self.current_session_title = Some(format!("Side {}", short_session(&side_thread_id)));
        self.clear_new_session_draft();
        ui.bottom_panel = None;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        ui.set_ephemeral_status("side chat; Ctrl+C returns");
        ui.refresh_sidebar(self);

        if let Some(prompt) = initial_prompt {
            self.start_fullscreen_turn(ui, prompt.clone(), prompt, Vec::new())?;
        }
        Ok(())
    }

    pub(crate) fn close_side_conversation(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(side) = self.side_conversation.take() else {
            return Ok(());
        };
        let side_thread_id = side.side_thread_id.clone();
        self.current_session = Some(side.parent_session.clone());
        self.reset_live_agent_reload_poll();
        self.current_session_title = side.parent_session_title;
        self.current_model = side.parent_model;
        self.current_variant = side.parent_variant;
        self.current_mode = side.parent_mode;
        self.current_permission_mode = side.parent_permission_mode;
        self.current_agent = side.parent_agent;
        self.current_agent_explicit_default = side.parent_agent_explicit_default;
        self.restore_parent_tui_state()?;
        self.refresh_selected_model();
        self.refresh_current_session_title()?;

        ui.bottom_panel = None;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        self.replay_session_live_event_backlog(ui, &side.parent_session);
        ui.session_live_event_backlog.remove(&side_thread_id);
        ui.agent_child_event_backlog.remove(&side_thread_id);
        ui.set_ephemeral_status(SIDE_CONVERSATION_RETURNED_MESSAGE);
        ui.refresh_sidebar(self);

        match self.state_runtime.delete_session(&side_thread_id) {
            Ok(()) => {}
            Err(err) => ui.set_ephemeral_error(format!("failed to delete /btw side chat: {err}")),
        }
        Ok(())
    }

    pub(crate) fn handle_side_conversation_ctrl_c(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        if !self.in_side_conversation() {
            return Ok(false);
        }
        if self.request_current_session_interrupt(ui) {
            return Ok(true);
        }
        self.close_side_conversation(ui)?;
        Ok(true)
    }

    pub(crate) fn side_command_rejection(&self, command: &SlashCommand) -> Option<&'static str> {
        if !self.in_side_conversation() || side_command_allowed(command) {
            return None;
        }
        Some("command is unavailable inside a /btw side chat; press Ctrl+C to return")
    }

    pub(crate) fn side_conversation_provider_model(&self) -> Result<(String, String)> {
        if let Some(model) = selected_configured_model(&self.run_options(String::new()))
            .ok()
            .flatten()
        {
            return Ok((model.provider, model.model));
        }
        if let Some((provider, model)) = self
            .current_model
            .as_deref()
            .and_then(|value| value.split_once('/'))
        {
            return Ok((provider.to_string(), model.to_string()));
        }
        Ok(("config".to_string(), "config".to_string()))
    }

    pub(crate) fn restore_parent_tui_state(&mut self) -> Result<()> {
        if let Some(model) = self.current_model.clone() {
            self.model_state
                .set_model(&self.workdir_key, model, self.current_variant.clone());
        } else {
            self.model_state.clear_workdir_model(&self.workdir_key);
        }
        self.model_state.save(&self.model_state_path)?;
        self.state
            .set_mode(&self.workdir_key, self.current_mode.as_str().to_string());
        self.state.set_permission_mode(
            &self.workdir_key,
            self.current_permission_mode.as_str().to_string(),
        );
        self.state.save(&self.state_path)?;
        Ok(())
    }

    pub(crate) fn start_side_cleanup_task(&mut self) -> bool {
        if self.side_cleanup_task.is_some() {
            return false;
        }
        let state = self.state_runtime.clone();
        let workdir = self.workdir.clone();
        let task = tokio::spawn(async move {
            state
                .delete_sessions_for_workdir_with_source(
                    &workdir,
                    TUI_SIDE_CONVERSATION_SESSION_SOURCE,
                )
                .map_err(|err| err.to_string())
        });
        self.side_cleanup_task = Some(SideCleanupTask { task });
        true
    }

    pub(crate) async fn drain_side_cleanup_task(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let Some(task) = self.side_cleanup_task.as_ref() else {
            return Ok(false);
        };
        if !task.task.is_finished() {
            return Ok(false);
        }
        let task = self.side_cleanup_task.take().expect("checked task");
        match task.task.await {
            Ok(Ok(count)) => ui.set_ephemeral_status(format!("side cleanup deleted {count}")),
            Ok(Err(err)) => ui.set_ephemeral_error(format!("side cleanup failed: {err}")),
            Err(err) if err.is_cancelled() => {}
            Err(err) => ui.set_ephemeral_error(format!("side cleanup failed: {err}")),
        }
        Ok(true)
    }

    pub(crate) fn side_parent_status_label(&self, ui: &FullscreenUi<'_>) -> Option<String> {
        let side = self.side_conversation.as_ref()?;
        let parent = side.parent_session.as_str();
        if ui
            .session_live_event_backlog
            .get(parent)
            .is_some_and(|events| {
                events
                    .iter()
                    .any(|event| matches!(event, RunStreamEvent::ClarifyRequest(_)))
            })
        {
            return Some("side - main needs input - Ctrl+C".to_string());
        }
        if ui
            .auxiliary_agent_tasks
            .iter()
            .any(|task| task.session_id.as_deref() == Some(parent))
            || ui
                .auxiliary_shell_tasks
                .iter()
                .any(|task| task.session_id.as_deref() == Some(parent))
        {
            return Some("side - main running - Ctrl+C".to_string());
        }
        Some("side - main idle - Ctrl+C".to_string())
    }
}

pub(crate) fn side_command_allowed(command: &SlashCommand) -> bool {
    matches!(
        command,
        SlashCommand::Help
            | SlashCommand::Quit
            | SlashCommand::Status
            | SlashCommand::Context
            | SlashCommand::Diff
            | SlashCommand::ModelShowScoped { .. }
            | SlashCommand::VariantSet(_)
            | SlashCommand::ModeSet(_)
            | SlashCommand::Permissions
            | SlashCommand::Sandbox
            | SlashCommand::Tools
            | SlashCommand::ThinkingToggle
            | SlashCommand::ThinkingSet(_)
            | SlashCommand::RawToggle
            | SlashCommand::RawSet(_)
            | SlashCommand::Copy
            | SlashCommand::Export(_)
            | SlashCommand::Share(_)
    )
}
