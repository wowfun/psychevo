#[allow(unused_imports)]
pub(crate) use super::*;

impl TuiApp {
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
}
