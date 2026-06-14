impl TuiApp {
    pub(crate) async fn handle_fullscreen_mouse(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        mouse: MouseEvent,
    ) -> Result<bool> {
        if let Some(overlay) = ui.diff_overlay.as_mut() {
            let viewport_height = ui
                .last_diff_overlay_area
                .map(|area| area.height)
                .unwrap_or(12);
            match mouse.kind {
                MouseEventKind::ScrollUp => overlay.scroll_by(-3, viewport_height),
                MouseEventKind::ScrollDown => overlay.scroll_by(3, viewport_height),
                _ => {}
            }
            return Ok(false);
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.handle_fullscreen_mouse_wheel(ui, mouse.column, mouse.row, -3);
            }
            MouseEventKind::ScrollDown => {
                self.handle_fullscreen_mouse_wheel(ui, mouse.column, mouse.row, 3);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if matches!(ui.bottom_panel, Some(BottomPanel::PermissionApproval(_))) {
                    if let Some(index) = ui.bottom_panel_hit(mouse.column, mouse.row) {
                        ui.clear_selection();
                        self.handle_permission_approval_panel_click(ui, index)?;
                    } else {
                        ui.clear_selection();
                    }
                } else if matches!(ui.bottom_panel, Some(BottomPanel::Clarify(_))) {
                    if let Some(index) = ui.bottom_panel_hit(mouse.column, mouse.row) {
                        ui.clear_selection();
                        self.handle_clarify_panel_click(ui, index)?;
                    } else {
                        ui.clear_selection();
                    }
                } else if let Some(index) = ui.bottom_panel_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    if let Some(panel) = &mut ui.bottom_panel {
                        panel.selection_mut().set_selected(index);
                    }
                    let selected = ui
                        .bottom_panel
                        .as_ref()
                        .and_then(BottomPanel::selected_value);
                    self.apply_bottom_panel_selection(ui, selected)?;
                } else if let Some(index) = ui.agent_popup_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    ui.set_agent_popup_selection(index);
                    ui.insert_selected_agent_marker();
                    self.sync_agent_popup(ui);
                    self.sync_skill_popup(ui);
                } else if let Some(index) = ui.file_popup_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    ui.set_file_popup_selection(index);
                    ui.insert_selected_file_path();
                    ui.sync_file_popup(&self.workdir);
                    self.sync_agent_popup(ui);
                    self.sync_skill_popup(ui);
                } else if let Some(index) = ui.skill_popup_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    ui.set_skill_popup_selection(index);
                    ui.insert_selected_skill_marker();
                    self.sync_agent_popup(ui);
                    self.sync_skill_popup(ui);
                } else if let Some(index) = ui.slash_menu_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    let line = textarea_text(&ui.textarea);
                    ui.set_slash_menu_selection(index, self.slash_menu_items(&line).len());
                    if let Some(command) = selected_slash_menu_command_with_items(
                        &line,
                        ui.slash_menu_selected,
                        &self.slash_items(),
                    ) {
                        let submitted = command;
                        ui.clear_composer();
                        ui.slash_menu_selected = 0;
                        ui.clear_slash_menu_dismissal();
                        ui.close_agent_popup();
                        ui.close_skill_popup();
                        ui.push_submitted_history(submitted.clone());
                        match parse_slash_command_with_config(&submitted, &self.slash_config) {
                            Ok(Some(command)) => {
                                return self
                                    .handle_fullscreen_command_with_echo(
                                        ui,
                                        command,
                                        Some(submitted),
                                    )
                                    .await;
                            }
                            Ok(None) => {}
                            Err(err) => {
                                ui.push_command_result(
                                    normalize_submitted_slash_echo(&submitted),
                                    None,
                                    format!("error: {err:#}"),
                                    true,
                                );
                                return Ok(false);
                            }
                        }
                    }
                } else if let Some((target, action)) =
                    ui.pending_input_action_hit(mouse.column, mouse.row)
                {
                    ui.clear_selection();
                    self.handle_pending_input_action(ui, target, action)?;
                } else if ui.start_composer_mouse_selection(mouse.column, mouse.row) {
                    return Ok(false);
                } else if let Some(target) = ui.transcript_hit(mouse.column, mouse.row) {
                    ui.mouse_down_target = Some(target);
                    ui.mouse_dragged = false;
                    if ui.selectable_hit(mouse.column, mouse.row) {
                        ui.start_selection(mouse.column, mouse.row);
                    } else {
                        ui.clear_selection();
                    }
                } else if ui.selectable_hit(mouse.column, mouse.row) {
                    ui.mouse_down_target = None;
                    ui.mouse_dragged = false;
                    ui.start_selection(mouse.column, mouse.row);
                } else {
                    ui.mouse_down_target = None;
                    ui.mouse_dragged = false;
                    ui.clear_selection();
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                ui.mouse_dragged = true;
                if !ui.update_composer_mouse_selection(mouse.column, mouse.row) {
                    ui.update_selection(mouse.column, mouse.row);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if ui.composer_mouse_selecting {
                    ui.update_composer_mouse_selection(mouse.column, mouse.row);
                    ui.finish_composer_mouse_selection();
                    return Ok(false);
                }
                ui.update_selection(mouse.column, mouse.row);
                let up_target = ui.transcript_hit(mouse.column, mouse.row);
                let click_target = (!ui.mouse_dragged
                    && ui.mouse_down_target.is_some()
                    && ui.mouse_down_target == up_target)
                    .then_some(ui.mouse_down_target)
                    .flatten();
                if !self.start_copy_selected_text(ui) {
                    if let Some(target) = click_target {
                        if let Some(agent_target) = ui.agent_target_for_target(target) {
                            self.open_agent_target_session(ui, &agent_target)?;
                        } else {
                            ui.toggle_target(target);
                        }
                    }
                    ui.clear_selection();
                }
                ui.mouse_down_target = None;
                ui.mouse_dragged = false;
            }
            _ => {}
        }
        Ok(false)
    }

    pub(crate) fn handle_fullscreen_mouse_wheel(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        column: u16,
        row: u16,
        amount: isize,
    ) {
        match ui.mouse_wheel_target(column, row) {
            Some(MouseWheelTarget::BottomPanel) => {
                if let Some(panel) = &mut ui.bottom_panel {
                    scroll_bottom_panel(panel, amount);
                }
            }
            Some(MouseWheelTarget::Transcript) => ui.scroll_transcript(amount),
            None => {}
        }
    }
}
