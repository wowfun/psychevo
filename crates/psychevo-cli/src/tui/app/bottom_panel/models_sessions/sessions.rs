#[allow(unused_imports)]
pub(crate) use super::*;

impl TuiApp {
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
            (SessionListView::Active, 'f') => self.fork_session_from_panel(ui, session_id),
            (SessionListView::Archived, 'r') => self.restore_session_from_panel(ui, session_id),
            (_, 'd') => self.delete_session_from_panel(ui, session_id),
            (SessionListView::Active, _) => {
                if let Some(BottomPanel::Sessions(panel)) = &mut ui.bottom_panel {
                    panel.notice = Some("action: F fork  A archive  D delete".to_string());
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
        self.state_runtime.archive_session(&session_id)?;
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
        self.state_runtime.restore_session(&session_id)?;
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
}
