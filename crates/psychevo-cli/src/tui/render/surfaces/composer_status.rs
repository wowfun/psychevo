#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn render_composer(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let theme = tui_theme();
    let surface_style = theme.surface_style();
    frame.render_widget(Block::default().style(surface_style), area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let textarea_empty = ui.textarea.is_empty();
    let marker_width = composer_marker_width(area.width, ui.shell_mode, textarea_empty);
    let marker_spans = if ui.shell_mode {
        if marker_width <= 1 {
            vec![Span::styled(
                "!".to_string(),
                surface_style.fg(theme.accent),
            )]
        } else {
            vec![
                Span::styled("!".to_string(), surface_style.fg(theme.accent)),
                Span::styled(" ".to_string(), surface_style),
            ]
        }
    } else if textarea_empty {
        vec![Span::styled("›".to_string(), surface_style.fg(theme.dim))]
    } else {
        vec![
            Span::styled("›".to_string(), surface_style.fg(theme.dim)),
            Span::styled(" ".to_string(), surface_style),
        ]
    };
    frame.render_widget(
        Paragraph::new(Line::from(marker_spans)).style(surface_style),
        Rect {
            x: area.x,
            y: area.y,
            width: marker_width,
            height: area.height,
        },
    );

    let input_area = Rect {
        x: area.x.saturating_add(marker_width),
        y: area.y,
        width: area.width.saturating_sub(marker_width),
        height: area.height,
    };
    if input_area.width == 0 || input_area.height == 0 {
        ui.set_composer_input_area(None);
        return;
    }
    ui.set_composer_input_area(Some(input_area));

    ui.textarea.set_block(Block::default().style(surface_style));
    ui.textarea.set_style(surface_style);
    ui.textarea.set_selection_style(text_selection_style());
    ui.textarea.set_placeholder_text("");
    frame.render_widget(&ui.textarea, input_area);

    if textarea_empty && !ui.shell_mode && input_area.width > 1 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Ask pevo...".to_string(),
                surface_style.fg(theme.dim),
            )))
            .style(surface_style),
            Rect {
                x: input_area.x.saturating_add(1),
                y: input_area.y,
                width: input_area.width.saturating_sub(1),
                height: 1,
            },
        );
    }
    if ui.focus == FocusMode::Composer
        && ui.pending_input_edit.is_none()
        && let Some((x, y)) = ui.composer_terminal_cursor_position(input_area)
    {
        frame.set_cursor_position((x, y));
    }
}

pub(crate) const PENDING_INPUT_PREVIEW_MAX_HEIGHT: u16 = 8;

pub(crate) fn pending_input_preview_height(ui: &FullscreenUi<'_>, width: u16) -> u16 {
    if !ui.has_pending_input_preview() {
        return 0;
    }
    let entries = ui.pending_input_entries();
    let mut edit_rendered = false;
    let mut height = 0u16;
    for entry in &entries {
        if ui
            .pending_input_edit
            .as_ref()
            .is_some_and(|edit| edit.target == entry.target)
        {
            height = height.saturating_add(pending_input_edit_height(ui, width));
            edit_rendered = true;
        } else {
            height = height.saturating_add(2);
        }
    }
    if ui.pending_input_edit.is_some() && !edit_rendered {
        height = height.saturating_add(pending_input_edit_height(ui, width));
    }
    height.min(PENDING_INPUT_PREVIEW_MAX_HEIGHT)
}

pub(crate) fn pending_input_edit_height(ui: &FullscreenUi<'_>, width: u16) -> u16 {
    let input_width = width.saturating_sub(2);
    ui.pending_input_edit
        .as_ref()
        .map(|edit| composer_height(&edit.textarea, input_width).saturating_add(2))
        .unwrap_or(0)
}

pub(crate) fn render_pending_input_preview(
    frame: &mut Frame<'_>,
    area: Rect,
    ui: &mut FullscreenUi<'_>,
) {
    ui.last_pending_input_action_areas.clear();
    ui.last_pending_input_edit_area = None;
    if area.width == 0 || area.height == 0 || !ui.has_pending_input_preview() {
        return;
    }
    let theme = tui_theme();
    frame.render_widget(Block::default(), area);
    let entries = ui.pending_input_entries();
    let mut y = area.y;
    let bottom = area.y.saturating_add(area.height);
    let mut edit_rendered = false;
    for entry in entries {
        if y >= bottom {
            return;
        }
        if ui
            .pending_input_edit
            .as_ref()
            .is_some_and(|edit| edit.target == entry.target)
        {
            y = render_pending_input_editor(frame, area, y, ui, entry.kind);
            edit_rendered = true;
        } else {
            y = render_pending_input_entry(frame, area, y, ui, &entry, theme);
        }
    }
    if y < bottom
        && !edit_rendered
        && let Some(kind) = ui.pending_input_edit.as_ref().map(|edit| edit.kind)
    {
        render_pending_input_editor(frame, area, y, ui, kind);
    }
}

pub(crate) fn render_pending_input_entry(
    frame: &mut Frame<'_>,
    area: Rect,
    y: u16,
    ui: &mut FullscreenUi<'_>,
    entry: &PendingInputEntry,
    theme: TuiTheme,
) -> u16 {
    let bottom = area.y.saturating_add(area.height);
    let header_area = Rect {
        x: area.x,
        y,
        width: area.width,
        height: 1,
    };
    let action_width = UnicodeWidthStr::width("[edit] [undo]").min(usize::from(u16::MAX)) as u16;
    let title_width = area.width.saturating_sub(action_width.saturating_add(2));
    let title = truncate_display_width(
        &format!("· pending {}", entry.kind.label()),
        usize::from(title_width.max(1)),
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(title, theme.accent_style()))),
        header_area,
    );
    if area.width > action_width.saturating_add(1) {
        let edit_width = 6u16;
        let undo_width = 6u16;
        let undo_x = area.x.saturating_add(area.width.saturating_sub(undo_width));
        let edit_x = undo_x.saturating_sub(edit_width.saturating_add(1));
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("[edit]", theme.dim_style()))),
            Rect {
                x: edit_x,
                y,
                width: edit_width,
                height: 1,
            },
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("[undo]", theme.dim_style()))),
            Rect {
                x: undo_x,
                y,
                width: undo_width,
                height: 1,
            },
        );
        ui.last_pending_input_action_areas.push((
            entry.target,
            PendingInputAction::Edit,
            Rect {
                x: edit_x,
                y,
                width: edit_width,
                height: 1,
            },
        ));
        ui.last_pending_input_action_areas.push((
            entry.target,
            PendingInputAction::Undo,
            Rect {
                x: undo_x,
                y,
                width: undo_width,
                height: 1,
            },
        ));
    }
    let next_y = y.saturating_add(1);
    if next_y < bottom {
        let mut preview = first_pending_input_preview_line(&entry.text);
        if !entry.images.is_empty() {
            let suffix = format!(" · images {}", entry.images.len());
            preview.push_str(&suffix);
        }
        let preview = truncate_display_width(&preview, usize::from(area.width.saturating_sub(4)));
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  ↳ ", theme.dim_style()),
                Span::styled(preview, theme.dim_style()),
            ])),
            Rect {
                x: area.x,
                y: next_y,
                width: area.width,
                height: 1,
            },
        );
    }
    y.saturating_add(2)
}

pub(crate) fn render_pending_input_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    y: u16,
    ui: &mut FullscreenUi<'_>,
    kind: PendingInputKind,
) -> u16 {
    let bottom = area.y.saturating_add(area.height);
    if y >= bottom {
        return y;
    }
    let theme = tui_theme();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("· editing {}", kind.label()),
            theme.accent_style(),
        ))),
        Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        },
    );
    let Some(edit) = ui.pending_input_edit.as_mut() else {
        return y.saturating_add(1);
    };
    let input_y = y.saturating_add(1);
    if input_y >= bottom {
        return input_y;
    }
    let input_width = area.width.saturating_sub(2);
    let edit_height =
        composer_height(&edit.textarea, input_width).min(bottom.saturating_sub(input_y));
    let input_area = Rect {
        x: area.x.saturating_add(2),
        y: input_y,
        width: input_width,
        height: edit_height,
    };
    ui.last_pending_input_edit_area = Some(input_area);
    edit.textarea
        .set_block(Block::default().style(theme.surface_style()));
    edit.textarea.set_style(theme.surface_style());
    edit.textarea.set_selection_style(text_selection_style());
    frame.render_widget(&edit.textarea, input_area);
    if let Some((x, y)) =
        composer_terminal_cursor_position(&edit.textarea, input_area, &mut edit.cursor_top_row)
    {
        frame.set_cursor_position((x, y));
    }
    let hint_y = input_y.saturating_add(edit_height);
    if hint_y < bottom {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  Enter", theme.accent_style()),
                Span::styled(" confirm · ", theme.dim_style()),
                Span::styled("Esc", theme.accent_style()),
                Span::styled(" cancel", theme.dim_style()),
            ])),
            Rect {
                x: area.x,
                y: hint_y,
                width: area.width,
                height: 1,
            },
        );
    }
    hint_y.saturating_add(1)
}

pub(crate) fn first_pending_input_preview_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("(empty)")
        .to_string()
}

pub(crate) fn render_slash_menu(
    frame: &mut Frame<'_>,
    area: Rect,
    items: &[self::slash::SlashMenuItem],
    selected_index: usize,
    row_areas: &mut Vec<(usize, Rect)>,
) {
    let theme = tui_theme();
    row_areas.clear();
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let rows = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let mut description = item.description.clone();
            if item.upcoming {
                description.push_str(" upcoming");
            }
            DisplayRow {
                marker: "  ",
                label: item.command.clone(),
                description: Some(description),
                selected: index == selected_index,
                disabled: item.upcoming,
                tone: DisplayRowTone::Accent,
                ..DisplayRow::default()
            }
        })
        .collect::<Vec<_>>();
    for index in 0..rows.len().min(area.height as usize) {
        row_areas.push((
            index,
            Rect {
                x: area.x,
                y: area.y.saturating_add(index as u16),
                width: area.width,
                height: 1,
            },
        ));
    }
    render_display_rows(area, frame.buffer_mut(), &rows);
}

pub(crate) fn render_file_popup(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let theme = tui_theme();
    ui.last_file_popup_areas.clear();
    let Some(popup) = &ui.file_search.popup else {
        return;
    };
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let rows = if popup.matches.is_empty() {
        let label = if popup.waiting {
            "searching files..."
        } else {
            "no matches"
        };
        vec![DisplayRow {
            marker: "  ",
            label: label.to_string(),
            tone: DisplayRowTone::Dim,
            ..DisplayRow::default()
        }]
    } else {
        popup
            .matches
            .iter()
            .take(FILE_POPUP_MAX_ROWS)
            .enumerate()
            .map(|(index, item)| {
                let kind = match item.kind {
                    FileSearchMatchKind::Directory => "dir",
                    FileSearchMatchKind::File => "file",
                };
                DisplayRow {
                    marker: "  ",
                    label: item.path.clone(),
                    meta: Some(kind.to_string()),
                    selected: index == popup.selected,
                    tone: DisplayRowTone::Accent,
                    ..DisplayRow::default()
                }
            })
            .collect()
    };
    for index in 0..rows.len().min(area.height as usize) {
        ui.last_file_popup_areas.push((
            index,
            Rect {
                x: area.x,
                y: area.y.saturating_add(index as u16),
                width: area.width,
                height: 1,
            },
        ));
    }
    render_display_rows(area, frame.buffer_mut(), &rows);
}

pub(crate) fn render_skill_popup(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let theme = tui_theme();
    ui.last_skill_popup_areas.clear();
    let Some(popup) = &ui.skill_search.popup else {
        return;
    };
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let rows = if popup.matches.is_empty() {
        vec![DisplayRow {
            marker: "  ",
            label: "no skill matches".to_string(),
            tone: DisplayRowTone::Dim,
            ..DisplayRow::default()
        }]
    } else {
        popup
            .matches
            .iter()
            .take(FILE_POPUP_MAX_ROWS)
            .enumerate()
            .map(|(index, item)| DisplayRow {
                marker: "  ",
                label: format!("${}", item.name),
                description: Some(item.description.clone()),
                selected: index == popup.selected,
                tone: DisplayRowTone::Identity,
                ..DisplayRow::default()
            })
            .collect()
    };
    for index in 0..rows.len().min(area.height as usize) {
        ui.last_skill_popup_areas.push((
            index,
            Rect {
                x: area.x,
                y: area.y.saturating_add(index as u16),
                width: area.width,
                height: 1,
            },
        ));
    }
    render_display_rows(area, frame.buffer_mut(), &rows);
}

pub(crate) fn render_agent_popup(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let theme = tui_theme();
    ui.last_agent_popup_areas.clear();
    let Some(popup) = &ui.agent_search.popup else {
        return;
    };
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let rows = popup
        .matches
        .iter()
        .take(FILE_POPUP_MAX_ROWS)
        .enumerate()
        .map(|(index, item)| DisplayRow {
            marker: "  ",
            label: format!("@{} (agent)", item.name),
            description: Some(item.description.clone()),
            selected: index == popup.selected,
            tone: DisplayRowTone::Identity,
            ..DisplayRow::default()
        })
        .collect::<Vec<_>>();
    for index in 0..rows.len().min(area.height as usize) {
        ui.last_agent_popup_areas.push((
            index,
            Rect {
                x: area.x,
                y: area.y.saturating_add(index as u16),
                width: area.width,
                height: 1,
            },
        ));
    }
    render_display_rows(area, frame.buffer_mut(), &rows);
}

pub(crate) fn render_status(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &TuiApp,
    ui: &FullscreenUi<'_>,
) {
    let theme = tui_theme();
    let model = app.model_display_value();
    let variant = app.variant_display_value();
    let mut spans = Vec::new();
    if app.current_mode != RunMode::Default {
        spans.push(Span::styled(
            app.current_mode.as_str().to_string(),
            theme.accent_style(),
        ));
        spans.push(Span::raw("  "));
    }
    spans.push(Span::raw(model));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(variant, theme.identity_style()));
    if ui.shell_mode || parse_shell_escape_input(&textarea_text(&ui.textarea)).is_some() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("shell", theme.accent_style()));
    }
    if ui.focus == FocusMode::Transcript {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("transcript", theme.accent_style()));
        spans.push(Span::styled(" · Esc", theme.dim_style()));
    }
    if let Some(label) = app.btw_parent_status_label(ui) {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(label, theme.accent_style()));
    }
    let auxiliary_shell_count =
        ui.auxiliary_shell_tasks.len() + ui.pending_auxiliary_shell_commands.len();
    if auxiliary_shell_count > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("shell {auxiliary_shell_count}"),
            theme.accent_style(),
        ));
    }
    if !ui.pending_images.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("images {}", ui.pending_images.len()),
            theme.accent_style(),
        ));
    }
    if let Some(elapsed) = ui.status_running_elapsed(app.current_session.as_deref()) {
        spans.push(Span::raw("  "));
        if ui.interrupt_requested {
            spans.push(Span::styled("interrupting", theme.error_style()));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format_duration_compact(elapsed),
                theme.dim_style(),
            ));
        } else {
            spans.push(Span::styled(
                activity_spinner_frame(elapsed),
                theme.accent_style(),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format_duration_compact(elapsed),
                theme.dim_style(),
            ));
            spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
            spans.push(Span::styled("Esc", theme.accent_style()));
        }
    }
    if let Some(status) = &ui.ephemeral_status {
        append_ephemeral_status(&mut spans, status, area.width);
    }
    if let Some(context) = bottom_status_context(app, ui, area.width, spans_width(&spans)) {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(context, theme.dim_style()));
    }
    let status_line = StatusLineView {
        line: Line::from(spans),
    };
    let _ = status_line.desired_height(area.width);
    status_line.render(area, frame.buffer_mut());
}

pub(crate) struct StatusLineView {
    pub(crate) line: Line<'static>,
}

impl TuiRenderable for StatusLineView {
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }

    fn render(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.is_empty() {
            return;
        }
        Paragraph::new(self.line.clone()).render(area, buf);
    }
}

pub(crate) fn append_ephemeral_status(
    spans: &mut Vec<Span<'static>>,
    status: &UiEphemeralStatus,
    width: u16,
) {
    let used = spans_width(spans);
    let separator = "  ";
    let available = usize::from(width)
        .saturating_sub(used)
        .saturating_sub(UnicodeWidthStr::width(separator));
    if available == 0 {
        return;
    }
    let text = truncate_display_width(&status.text, available);
    if text.is_empty() {
        return;
    }
    let style = if status.failed {
        tui_theme().error_style()
    } else {
        tui_theme().success_style()
    };
    spans.push(Span::raw(separator.to_string()));
    spans.push(Span::styled(text, style));
}

pub(crate) fn spans_width(spans: &[Span<'_>]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

pub(crate) fn bottom_status_context(
    app: &TuiApp,
    ui: &FullscreenUi<'_>,
    area_width: u16,
    stable_width: usize,
) -> Option<String> {
    const STATUS_CONTEXT_GAP: usize = 2;
    let available = usize::from(area_width)
        .saturating_sub(stable_width)
        .saturating_sub(STATUS_CONTEXT_GAP);
    if available == 0 {
        return None;
    }
    bottom_status_context_for_width(app, ui, available)
}

pub(crate) fn bottom_status_context_for_width(
    app: &TuiApp,
    ui: &FullscreenUi<'_>,
    available_width: usize,
) -> Option<String> {
    const STATUS_WORKDIR_MIN_WIDTH: usize = 8;
    const SEP_WIDTH: usize = 3;

    let home = home_dir_for_display(app);
    let full_workdir = directory_display_value(&app.workdir, home.as_deref());
    let branch = bottom_status_branch(&ui.sidebar.branch);
    let context = bottom_status_context_usage(ui);
    let agent_hint = app.agent_breadcrumb_status();
    let profile = bottom_status_profile(app);

    if let Some(context) = context.as_deref() {
        let mut segments = vec![context];
        if let Some(profile) = profile.as_deref() {
            segments.push(profile);
        }
        segments.push(full_workdir.as_str());
        if let Some(branch) = branch.as_deref() {
            segments.push(branch);
        }
        if let Some(agent_hint) = agent_hint.as_deref() {
            segments.push(agent_hint);
        }
        if let Some(value) = joined_segments_if_fits(&segments, available_width) {
            return Some(value);
        }
    }

    if let Some(context) = context.as_deref() {
        if let Some(profile) = profile.as_deref()
            && let Some(value) =
                joined_segments_if_fits(&[context, profile, full_workdir.as_str()], available_width)
        {
            return Some(value);
        }
        if let Some(value) =
            joined_segments_if_fits(&[context, full_workdir.as_str()], available_width)
        {
            return Some(value);
        }
        let context_width = UnicodeWidthStr::width(context);
        if available_width > context_width.saturating_add(SEP_WIDTH) {
            let workdir_width = available_width
                .saturating_sub(context_width)
                .saturating_sub(SEP_WIDTH);
            if workdir_width >= STATUS_WORKDIR_MIN_WIDTH {
                let workdir = format_directory_display_with_home(
                    &app.workdir,
                    home.as_deref(),
                    workdir_width,
                );
                return Some(format!("{context} · {workdir}"));
            }
        }
        if context_width <= available_width {
            return Some(context.to_string());
        }
    }

    let mut segments = Vec::new();
    if let Some(profile) = profile.as_deref() {
        segments.push(profile);
    }
    segments.push(full_workdir.as_str());
    if let Some(branch) = branch.as_deref() {
        segments.push(branch);
    }
    if let Some(agent_hint) = agent_hint.as_deref() {
        segments.push(agent_hint);
    }
    if let Some(value) = joined_segments_if_fits(&segments, available_width) {
        return Some(value);
    }

    if let Some(agent_hint) = agent_hint.as_deref() {
        if available_width > STATUS_WORKDIR_MIN_WIDTH.saturating_add(SEP_WIDTH) {
            let mut compact_segments = Vec::new();
            let branch_width = branch.as_deref().map(UnicodeWidthStr::width).unwrap_or(0);
            let hint_width = UnicodeWidthStr::width(agent_hint);
            let fixed_width = hint_width
                .saturating_add(branch_width)
                .saturating_add(usize::from(branch.is_some()).saturating_add(1) * SEP_WIDTH);
            let workdir_width = available_width.saturating_sub(fixed_width);
            if workdir_width >= STATUS_WORKDIR_MIN_WIDTH {
                let compact_workdir = format_directory_display_with_home(
                    &app.workdir,
                    home.as_deref(),
                    workdir_width,
                );
                if !compact_workdir.is_empty() {
                    compact_segments.push(compact_workdir.as_str());
                    if let Some(branch) = branch.as_deref() {
                        compact_segments.push(branch);
                    }
                    compact_segments.push(agent_hint);
                    if let Some(value) = joined_segments_if_fits(&compact_segments, available_width)
                    {
                        return Some(value);
                    }
                }
            }
        }
        if UnicodeWidthStr::width(agent_hint) <= available_width {
            return Some(agent_hint.to_string());
        }
    }

    if available_width < STATUS_WORKDIR_MIN_WIDTH {
        return None;
    }
    let workdir =
        format_directory_display_with_home(&app.workdir, home.as_deref(), available_width);
    (!workdir.is_empty()).then_some(workdir)
}

pub(crate) fn bottom_status_branch(branch: &str) -> Option<String> {
    let branch = branch.trim();
    if branch.is_empty() || branch == "(none)" {
        None
    } else {
        Some(branch.to_string())
    }
}

pub(crate) fn bottom_status_profile(app: &TuiApp) -> Option<String> {
    app.env_map
        .get(crate::profiles::PROFILE_ENV)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && *value != crate::profiles::DEFAULT_PROFILE)
        .map(|value| format!("profile {value}"))
}

pub(crate) fn bottom_status_context_usage(ui: &FullscreenUi<'_>) -> Option<String> {
    if let Some(snapshot) = ui
        .last_context_snapshot
        .as_ref()
        .filter(|snapshot| snapshot.context_limit.is_some())
    {
        return Some(format_context_total_value(snapshot));
    }
    let tokens = ui.sidebar_tokens?;
    let limit = ui.sidebar_context_limit.filter(|limit| *limit > 0)?;
    let percent = tokens as f64 / limit as f64 * 100.0;
    Some(format_context_total_value_parts(
        tokens,
        false,
        Some(limit),
        Some(percent),
    ))
}

pub(crate) fn joined_segments_if_fits(segments: &[&str], available_width: usize) -> Option<String> {
    if segments.is_empty() {
        return None;
    }
    let width = segments
        .iter()
        .map(|segment| UnicodeWidthStr::width(*segment))
        .sum::<usize>()
        .saturating_add(segments.len().saturating_sub(1) * 3);
    (width <= available_width).then(|| segments.join(" · "))
}

pub(crate) fn render_sidebar(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let theme = tui_theme();
    frame.render_widget(Clear, area);
    let mut lines = vec![
        Line::from(Span::styled(
            ui.sidebar.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("session: {}", ui.sidebar.session)),
        Line::from(""),
        sidebar_heading("Modified Files"),
    ];
    if ui.sidebar.changed_files.is_empty() {
        lines.push(Line::from(Span::styled("(clean)", theme.dim_style())));
    } else {
        for file in &ui.sidebar.changed_files {
            lines.push(Line::from(file.clone()));
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
    ui.capture_selectable_rows(frame.buffer_mut(), area, SelectableRegion::Sidebar);
}
