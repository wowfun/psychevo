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

    fn switch_session_no_print(&mut self, reference: &str) -> Result<String> {
        let id = self.resolve_session_ref(reference)?;
        SqliteStore::open(&self.db_path)?.resume_session(&id)?;
        self.current_session = Some(id.clone());
        self.force_new_once = false;
        self.refresh_current_session_title()?;
        Ok(id)
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
        let Some(parsed) = RunMode::parse(mode) else {
            return Err(anyhow!("mode must be one of plan, default"));
        };
        self.current_mode = parsed;
        self.state.set_mode(&self.workdir_key, mode.to_string());
        self.state.save(&self.state_path)?;
        Ok(())
    }

    fn set_thinking_no_print(&mut self, enabled: bool) -> Result<()> {
        self.thinking_visible = enabled;
        self.state.set_thinking_visible(enabled);
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
        ui.textarea = textarea_with_text(&result.prompt);
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
        ui.textarea = new_textarea();
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
        let next = match self.current_mode {
            RunMode::Plan => RunMode::Build,
            RunMode::Build => RunMode::Plan,
        };
        self.set_mode_no_print(next.as_str())?;
        ui.refresh_sidebar(self);
        Ok(())
    }

    fn resolve_session_ref(&self, reference: &str) -> Result<String> {
        let sessions = self.sessions_for_workdir()?;
        resolve_session_ref_from_summaries(&sessions, reference)
    }

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
            ui.replace_session_history_prompts(Vec::new());
            ui.refresh_sidebar(self);
            return Ok(());
        };
        let store = SqliteStore::open(&self.db_path)?;
        let mut history_prompts = Vec::new();
        for summary in store.load_tui_message_summaries(session_id)? {
            let value = serde_json::to_value(summary.message)?;
            if value.get("role").and_then(Value::as_str) == Some("user")
                && let Some(text) = user_text_from_message(&value)
            {
                history_prompts.push(text);
            }
            ui.push_history_message(&value, summary.usage.as_ref(), summary.metadata.as_ref());
        }
        ui.replace_session_history_prompts(history_prompts);
        ui.scroll_to_bottom();
        ui.refresh_sidebar(self);
        Ok(())
    }
}
