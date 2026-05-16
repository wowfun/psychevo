fn render_composer(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let theme = tui_theme();
    let surface_style = theme.surface_style();
    frame.render_widget(Block::default().style(surface_style), area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let textarea_empty = ui.textarea.is_empty();
    let marker_width = if ui.shell_mode {
        area.width.min(2)
    } else if textarea_empty {
        area.width.min(1)
    } else {
        area.width.min(2)
    };
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
        return;
    }

    ui.textarea.set_block(Block::default().style(surface_style));
    ui.textarea.set_style(surface_style);
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
}

fn render_slash_menu(
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

fn render_file_popup(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
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

fn render_skill_popup(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
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

fn render_status(frame: &mut Frame<'_>, area: Rect, app: &TuiApp, ui: &FullscreenUi<'_>) {
    let theme = tui_theme();
    let model = app.model_display_value();
    let variant = app.variant_display_value();
    let mut spans = Vec::new();
    if app.current_mode != RunMode::Build {
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
    if ui.running.is_some() || ui.running_started.is_some() {
        let elapsed = ui.running_elapsed().unwrap_or_default();
        spans.push(Span::raw("  "));
        if ui.interrupt_requested {
            spans.push(Span::styled("interrupting", theme.error_style()));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format_duration_compact(elapsed),
                theme.dim_style(),
            ));
        } else {
            spans.push(Span::styled("•", theme.accent_style()));
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

struct StatusLineView {
    line: Line<'static>,
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

fn append_ephemeral_status(spans: &mut Vec<Span<'static>>, status: &UiEphemeralStatus, width: u16) {
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

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn bottom_status_context(
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

fn bottom_status_context_for_width(
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

    if let Some(context) = context.as_deref() {
        let mut segments = vec![context, full_workdir.as_str()];
        if let Some(branch) = branch.as_deref() {
            segments.push(branch);
        }
        if let Some(value) = joined_segments_if_fits(&segments, available_width) {
            return Some(value);
        }
    }

    if let Some(context) = context.as_deref() {
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

    let mut segments = vec![full_workdir.as_str()];
    if let Some(branch) = branch.as_deref() {
        segments.push(branch);
    }
    if let Some(value) = joined_segments_if_fits(&segments, available_width) {
        return Some(value);
    }

    if available_width < STATUS_WORKDIR_MIN_WIDTH {
        return None;
    }
    let workdir =
        format_directory_display_with_home(&app.workdir, home.as_deref(), available_width);
    (!workdir.is_empty()).then_some(workdir)
}

fn bottom_status_branch(branch: &str) -> Option<String> {
    let branch = branch.trim();
    if branch.is_empty() || branch == "(none)" {
        None
    } else {
        Some(branch.to_string())
    }
}

fn bottom_status_context_usage(ui: &FullscreenUi<'_>) -> Option<String> {
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

fn joined_segments_if_fits(segments: &[&str], available_width: usize) -> Option<String> {
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

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
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

fn render_bottom_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &mut BottomPanel,
    row_areas: &mut Vec<(usize, Rect)>,
) {
    let theme = tui_theme();
    row_areas.clear();
    if let BottomPanel::Help(panel) = panel {
        render_help_panel(frame, area, panel);
        return;
    }
    if let BottomPanel::ProviderWizard(panel) = panel {
        render_provider_wizard_panel(frame, area, panel);
        return;
    }
    if let BottomPanel::Models(panel) = panel {
        render_model_panel(frame, area, panel, row_areas);
        return;
    }
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    let selection = panel.selection_mut();
    let notice_rows = if selection.notice.is_some() { 1 } else { 0 };
    let reserved = 4 + notice_rows;
    let visible_rows = inner.height.saturating_sub(reserved).max(1);
    selection.ensure_selected_visible(visible_rows);

    let mut lines = Vec::new();
    let title_width = selection.title.chars().count() as u16;
    let esc_hint = "esc";
    let header_padding = inner
        .width
        .saturating_sub(title_width)
        .saturating_sub(esc_hint.len() as u16) as usize;
    lines.push(Line::from(vec![
        Span::styled(
            selection.title.clone(),
            theme.dim_style().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(header_padding)),
        Span::styled(esc_hint, theme.dim_style()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Search ", theme.dim_style()),
        Span::styled(selection.query.clone(), Style::default()),
    ]));
    let mut row_y = inner.y.saturating_add(lines.len() as u16);

    let filtered = selection.filtered_indices();
    if filtered.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            selection.empty_label.clone(),
            theme.dim_style(),
        )));
    } else {
        let mut last_group: Option<String> = None;
        for (visible_index, row_index) in filtered
            .iter()
            .enumerate()
            .skip(selection.scroll as usize)
            .take(visible_rows as usize)
        {
            let row = &selection.rows[*row_index];
            if row.group != last_group
                && let Some(group) = row.group.clone()
            {
                lines.push(Line::from(Span::styled(
                    group.clone(),
                    theme.accent_style().add_modifier(Modifier::BOLD),
                )));
                row_y = row_y.saturating_add(1);
                last_group = Some(group);
            }
            row_areas.push((
                visible_index,
                Rect {
                    x: inner.x,
                    y: row_y,
                    width: inner.width,
                    height: 1,
                },
            ));
            lines.push(bottom_panel_row(
                row,
                visible_index == selection.selected,
                inner.width,
            ));
            row_y = row_y.saturating_add(1);
        }
    }
    lines.push(Line::from(""));
    if let Some(notice) = &selection.notice {
        lines.push(Line::from(Span::styled(notice.clone(), theme.dim_style())));
    }
    lines.push(Line::from(Span::styled(
        selection.footer_text(),
        theme.dim_style(),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_model_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &mut ModelPanel,
    row_areas: &mut Vec<(usize, Rect)>,
) {
    let theme = tui_theme();
    row_areas.clear();
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    match panel.tab {
        ModelTab::Models => render_model_list_tab(frame, inner, panel, row_areas),
        ModelTab::Info => render_model_info_tab(frame, inner, panel),
    }
}

fn render_model_list_tab(
    frame: &mut Frame<'_>,
    inner: Rect,
    panel: &mut ModelPanel,
    row_areas: &mut Vec<(usize, Rect)>,
) {
    let theme = tui_theme();
    let selection = &mut panel.models;
    let notice_rows = if selection.notice.is_some() { 1 } else { 0 };
    let reserved = 4 + notice_rows;
    let visible_rows = inner.height.saturating_sub(reserved).max(1);
    selection.ensure_selected_visible(visible_rows);

    let mut lines = Vec::new();
    lines.push(model_panel_tabs(panel.tab));
    lines.push(Line::from(vec![
        Span::styled("Search ", theme.dim_style()),
        Span::styled(selection.query.clone(), Style::default()),
    ]));
    let mut row_y = inner.y.saturating_add(lines.len() as u16);

    let filtered = selection.filtered_indices();
    if filtered.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            selection.empty_label.clone(),
            theme.dim_style(),
        )));
    } else {
        let mut last_group: Option<String> = None;
        for (visible_index, row_index) in filtered
            .iter()
            .enumerate()
            .skip(selection.scroll as usize)
            .take(visible_rows as usize)
        {
            let row = &selection.rows[*row_index];
            if row.group != last_group
                && let Some(group) = row.group.clone()
            {
                lines.push(Line::from(Span::styled(
                    group.clone(),
                    theme.accent_style().add_modifier(Modifier::BOLD),
                )));
                row_y = row_y.saturating_add(1);
                last_group = Some(group);
            }
            row_areas.push((
                visible_index,
                Rect {
                    x: inner.x,
                    y: row_y,
                    width: inner.width,
                    height: 1,
                },
            ));
            lines.push(bottom_panel_row(
                row,
                visible_index == selection.selected,
                inner.width,
            ));
            row_y = row_y.saturating_add(1);
        }
    }
    lines.push(Line::from(""));
    if let Some(notice) = &selection.notice {
        lines.push(Line::from(Span::styled(notice.clone(), theme.dim_style())));
    }
    lines.push(Line::from(Span::styled(
        model_list_footer_text(selection),
        theme.dim_style(),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_model_info_tab(frame: &mut Frame<'_>, inner: Rect, panel: &mut ModelPanel) {
    let theme = tui_theme();
    let body = model_info_body(panel);
    let body_height = inner.height.saturating_sub(3).max(1);
    let max_scroll = body.len().saturating_sub(body_height as usize) as u16;
    panel.info_scroll = panel.info_scroll.min(max_scroll);

    let mut lines = vec![model_panel_tabs(panel.tab), Line::from("")];
    lines.extend(
        body.into_iter()
            .skip(panel.info_scroll as usize)
            .take(body_height as usize),
    );
    while lines.len() < inner.height.saturating_sub(1) as usize {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "Esc close  Ctrl+R refresh metadata  Tab/Left/Right section  Up/Down scroll",
        theme.dim_style(),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn model_panel_tabs(active: ModelTab) -> Line<'static> {
    let theme = tui_theme();
    let mut spans = vec![Span::styled("Model", theme.accent_style())];
    for tab in ModelPanel::tabs() {
        spans.push(Span::raw("  "));
        let style = if *tab == active {
            theme.selected_row_style()
        } else {
            Style::default()
        };
        spans.push(Span::styled(format!(" {} ", tab.label()), style));
    }
    Line::from(spans)
}

fn model_list_footer_text(selection: &BottomSelectionPanel) -> String {
    let footer = selection.footer_text();
    let footer = if footer.contains("Ctrl+R refresh metadata") {
        footer
    } else if let Some((left, right)) = footer.split_once("  Esc close") {
        format!("{left}  Ctrl+R refresh metadata  Esc close{right}")
    } else {
        format!("{footer}  Ctrl+R refresh metadata")
    };
    if footer.contains("Tab/Right info") {
        return footer;
    }
    if let Some((left, right)) = footer.split_once("  Esc close") {
        return format!("{left}  Tab/Right info  Esc close{right}");
    }
    format!("{footer}  Tab/Right info")
}

fn model_info_body(panel: &ModelPanel) -> Vec<Line<'static>> {
    let theme = tui_theme();
    let Some(row) = panel.models.selected_row() else {
        return vec![Line::from(Span::styled(
            "No model selected",
            theme.dim_style(),
        ))];
    };
    let BottomSelectionValue::Model { model, source } = &row.value else {
        return vec![Line::from(Span::styled(
            "Select a model row to view metadata.",
            theme.dim_style(),
        ))];
    };
    model_info_lines(model, *source, row)
}

fn model_info_lines(
    model: &ConfiguredModel,
    source: ModelRowSource,
    row: &BottomSelectionRow,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(format!(
        "model: {}  provider: {} ({})",
        format_model_spec(model),
        model.provider_label,
        model.provider
    )));
    let mut state = Vec::new();
    if row.is_current {
        state.push("current".to_string());
    }
    if row.is_default {
        state.push("config default".to_string());
    }
    state.extend(model_detail_source(model, source));
    if !state.is_empty() {
        lines.push(Line::from(format!("source: {}", state.join("  "))));
    }

    let limits = model_detail_limits(model);
    if !limits.is_empty() {
        lines.push(Line::from(format!("limits: {}", limits.join("  "))));
    }

    let capabilities = model_detail_capabilities(model);
    if !capabilities.is_empty() {
        lines.push(Line::from(format!(
            "capabilities: {}",
            capabilities.join("  ")
        )));
    }

    let modalities = model_detail_modalities(model);
    if !modalities.is_empty() {
        lines.push(Line::from(format!("modalities: {}", modalities.join("  "))));
    }

    let mut pricing = model_detail_pricing(model).into_iter();
    if let Some(first) = pricing.next() {
        lines.push(Line::from(format!("pricing: {first}")));
        lines.extend(pricing.map(Line::from));
    }
    lines
}

fn model_detail_limits(model: &ConfiguredModel) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(limit) = model.context_limit {
        lines.push(format!("context {}", format_count(limit)));
    }
    if let Some(limit) = model.metadata.limits.input {
        lines.push(format!("input {}", format_count(limit)));
    }
    if let Some(limit) = model.metadata.limits.output {
        lines.push(format!("output {}", format_count(limit)));
    }
    lines
}

fn model_detail_capabilities(model: &ConfiguredModel) -> Vec<String> {
    let caps = &model.metadata.capabilities;
    let mut parts = Vec::new();
    push_bool_capability(&mut parts, caps.reasoning, "reasoning", "no reasoning");
    push_bool_capability(&mut parts, caps.tool_call, "tools", "no tools");
    push_bool_capability(
        &mut parts,
        caps.temperature,
        "temperature",
        "no temperature",
    );
    push_bool_capability(&mut parts, caps.attachment, "attachments", "no attachments");
    push_bool_capability(
        &mut parts,
        caps.structured_output,
        "structured output",
        "no structured output",
    );
    match caps.interleaved.as_ref() {
        Some(Value::Bool(false)) => parts.push("no interleaved".to_string()),
        Some(_) => parts.push("interleaved".to_string()),
        None => {}
    }
    parts
}

fn model_detail_modalities(model: &ConfiguredModel) -> Vec<String> {
    let caps = &model.metadata.capabilities;
    let mut lines = Vec::new();
    if !caps.input_modalities.is_empty() {
        lines.push(format!("input: {}", caps.input_modalities.join(", ")));
    }
    if !caps.output_modalities.is_empty() {
        lines.push(format!("output: {}", caps.output_modalities.join(", ")));
    }
    lines
}

fn push_bool_capability(
    parts: &mut Vec<String>,
    value: Option<bool>,
    enabled: &str,
    disabled: &str,
) {
    match value {
        Some(true) => parts.push(enabled.to_string()),
        Some(false) => parts.push(disabled.to_string()),
        None => {}
    }
}

fn model_detail_pricing(model: &ConfiguredModel) -> Vec<String> {
    let Some(cost) = &model.metadata.cost else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    match (cost.input, cost.output) {
        (Some(0.0), Some(0.0)) => {
            parts.push("standard: free".to_string());
        }
        (Some(input), Some(output)) => {
            parts.push(format!(
                "standard: in/out {}",
                format_model_rate_pair(input, output)
            ));
        }
        (Some(value), None) => parts.push(format!("standard: input {}", format_model_rate(value))),
        (None, Some(value)) => {
            parts.push(format!("standard: output {}", format_model_rate(value)));
        }
        (None, None) => {}
    }
    match (cost.cache_read, cost.cache_write) {
        (Some(read), Some(write)) => {
            parts.push(format!(
                "cache: read/write {}",
                format_model_rate_pair(read, write)
            ));
        }
        (Some(value), None) => parts.push(format!("cache: read {}", format_model_rate(value))),
        (None, Some(value)) => parts.push(format!("cache: write {}", format_model_rate(value))),
        (None, None) => {}
    }
    if let Some(tier) = &cost.context_over_200k {
        let mut tier_parts = Vec::new();
        match (tier.input, tier.output) {
            (Some(input), Some(output)) => {
                tier_parts.push(format!("in/out {}", format_model_rate_pair(input, output)));
            }
            (Some(value), None) => tier_parts.push(format!("input {}", format_model_rate(value))),
            (None, Some(value)) => {
                tier_parts.push(format!("output {}", format_model_rate(value)));
            }
            (None, None) => {}
        }
        match (tier.cache_read, tier.cache_write) {
            (Some(read), Some(write)) => tier_parts.push(format!(
                "cache read/write {}",
                format_model_rate_pair(read, write)
            )),
            (Some(value), None) => {
                tier_parts.push(format!("cache read {}", format_model_rate(value)))
            }
            (None, Some(value)) => {
                tier_parts.push(format!("cache write {}", format_model_rate(value)))
            }
            (None, None) => {}
        }
        if !tier_parts.is_empty() {
            parts.push(format!("over-200k: {}", tier_parts.join(" ")));
        }
    }
    if let Some(source) = &cost.source {
        parts.push(format!("source: {source}"));
    }
    parts
}

fn model_detail_source(model: &ConfiguredModel, source: ModelRowSource) -> Vec<String> {
    let mut parts = vec![match source {
        ModelRowSource::Local => "local".to_string(),
        ModelRowSource::Fetched => "fetched".to_string(),
        ModelRowSource::CurrentOnly => "current only".to_string(),
    }];
    if let Some(source) = &model.metadata.source {
        parts.push(format!("metadata {source}"));
    }
    if let Some(variant) = &model.reasoning_effort {
        parts.push(format!("default {variant}"));
    }
    parts
}

fn format_model_rate(value: f64) -> String {
    format!("${value:.3}/M")
}

fn format_model_rate_pair(left: f64, right: f64) -> String {
    format!("${left:.3}/${right:.3}/M")
}

fn render_help_panel(frame: &mut Frame<'_>, area: Rect, panel: &mut HelpPanel) {
    let theme = tui_theme();
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };

    let body = help_panel_body(panel);
    let body_height = inner.height.saturating_sub(4).max(1);
    let max_scroll = body.len().saturating_sub(body_height as usize) as u16;
    panel.scroll = panel.scroll.min(max_scroll);

    let mut lines = vec![help_panel_tabs(panel.tab), Line::from("")];
    lines.extend(
        body.into_iter()
            .skip(panel.scroll as usize)
            .take(body_height as usize),
    );
    while lines.len() < inner.height.saturating_sub(1) as usize {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "Esc close  Tab/Left/Right section  Up/Down scroll",
        theme.dim_style(),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn help_panel_tabs(active: HelpTab) -> Line<'static> {
    let theme = tui_theme();
    let mut spans = vec![Span::styled("Help", theme.accent_style())];
    for tab in HelpPanel::tabs() {
        spans.push(Span::raw("  "));
        let style = if *tab == active {
            theme.selected_row_style()
        } else {
            Style::default()
        };
        spans.push(Span::styled(format!(" {} ", tab.label()), style));
    }
    Line::from(spans)
}

fn help_panel_body(panel: &HelpPanel) -> Vec<Line<'static>> {
    let sections = slash_help_sections(panel.skill_count);
    let lines = match panel.tab {
        HelpTab::General => sections.general,
        HelpTab::Commands => sections.commands,
        HelpTab::CustomCommands => sections.custom_commands,
    };
    lines
        .into_iter()
        .map(|line| help_panel_body_line(&line))
        .collect()
}

fn help_panel_body_line(line: &str) -> Line<'static> {
    let theme = tui_theme();
    if line.is_empty() {
        return Line::from("");
    }
    if matches!(line, "Shortcuts" | "Common commands") {
        return Line::from(Span::styled(
            line.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
    }
    if line.starts_with("  ") {
        return Line::from(Span::styled(line.to_string(), theme.dim_style()));
    }
    if line == "No custom commands available" {
        return Line::from(Span::styled(line.to_string(), theme.dim_style()));
    }
    Line::from(line.to_string())
}

fn render_provider_wizard_panel(frame: &mut Frame<'_>, area: Rect, panel: &ProviderWizardPanel) {
    let theme = tui_theme();
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Add Provider",
                theme.dim_style().add_modifier(Modifier::BOLD),
            ),
            Span::styled("  OpenAI-compatible global provider", theme.dim_style()),
        ]),
        provider_wizard_field_line(panel, ProviderWizardField::Label, "Label", &panel.label),
        provider_wizard_field_line(
            panel,
            ProviderWizardField::ProviderId,
            "Provider ID",
            &panel.provider_id,
        ),
        provider_wizard_field_line(
            panel,
            ProviderWizardField::BaseUrl,
            "Base URL",
            &panel.base_url,
        ),
    ];
    let env_var = panel
        .env_var()
        .unwrap_or_else(|| "(generated after provider id)".to_string());
    let env_note = if panel.api_key_env_present {
        "existing key reused"
    } else {
        "new key variable"
    };
    lines.push(Line::from(vec![
        Span::styled("  API key env ", theme.dim_style()),
        Span::styled(env_var, Style::default()),
        Span::styled(format!("  {env_note}"), theme.dim_style()),
    ]));
    if !panel.api_key_env_present {
        lines.push(provider_wizard_field_line(
            panel,
            ProviderWizardField::ApiKey,
            "API key",
            &"*".repeat(panel.api_key.chars().count()),
        ));
    }
    lines.push(Line::from(""));
    if let Some(notice) = &panel.notice {
        lines.push(Line::from(Span::styled(notice.clone(), theme.dim_style())));
    }
    lines.push(Line::from(Span::styled(
        "Enter next/save  Up/Down field  Esc back",
        theme.dim_style(),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn provider_wizard_field_line(
    panel: &ProviderWizardPanel,
    field: ProviderWizardField,
    label: &str,
    value: &str,
) -> Line<'static> {
    let selected = panel.active_field == field;
    let marker = if selected { "›" } else { " " };
    let theme = tui_theme();
    let style = if selected {
        theme.panel_field_style()
    } else {
        Style::default()
    };
    let value = if value.is_empty() { " " } else { value };
    Line::from(Span::styled(format!("{marker} {label}: {value}"), style))
}

fn bottom_panel_row(row: &BottomSelectionRow, selected: bool, width: u16) -> Line<'static> {
    let theme = tui_theme();
    let select_marker = if selected { "›" } else { " " };
    let state_marker = if row.is_current {
        "● "
    } else if row.is_default {
        "◆ "
    } else {
        "  "
    };
    let prefix = format!("{select_marker} {state_marker}{}", row.label);
    let mut left = prefix.clone();
    if let Some(description) = &row.description {
        left.push_str("  ");
        left.push_str(description);
    }
    let detail = row.detail.as_deref().unwrap_or_default();
    let text = if detail.is_empty() {
        truncate_display_width(&left, width as usize)
    } else {
        let width = usize::from(width);
        let detail = truncate_display_width(detail, width);
        let detail_width = UnicodeWidthStr::width(detail.as_str());
        let separator_width = 2.min(width.saturating_sub(detail_width));
        let available = width
            .saturating_sub(detail_width)
            .saturating_sub(separator_width);
        let left = truncate_display_width(&left, available);
        let padding = width
            .saturating_sub(UnicodeWidthStr::width(left.as_str()))
            .saturating_sub(detail_width);
        format!("{left}{}{detail}", " ".repeat(padding))
    };
    let style = if selected {
        theme.selected_row_style()
    } else {
        Style::default()
    };
    if selected || row.style == BottomRowStyle::Normal || !detail.is_empty() {
        return Line::from(Span::styled(text, style));
    }
    let prefix = truncate_display_width(&prefix, width as usize);
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());
    let rest = text
        .chars()
        .skip(prefix.chars().count())
        .collect::<String>();
    let rest = truncate_display_width(&rest, (width as usize).saturating_sub(prefix_width));
    Line::from(vec![
        Span::styled(prefix, theme.accent_style().add_modifier(Modifier::BOLD)),
        Span::styled(rest, theme.dim_style()),
    ])
}
