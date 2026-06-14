pub(crate) fn render_agent_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &mut AgentPanel,
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
    let selection = match panel.tab {
        AgentTab::Running => &mut panel.running,
        AgentTab::Available => &mut panel.available,
    };
    let notice_rows = if selection.notice.is_some() { 1 } else { 0 };
    let reserved = 4 + notice_rows;
    let visible_rows = inner.height.saturating_sub(reserved).max(1);
    selection.ensure_selected_visible(visible_rows);

    let mut lines = Vec::new();
    lines.push(agent_panel_tabs(panel.tab));
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
                false,
                Duration::default(),
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

pub(crate) fn agent_panel_tabs(active: AgentTab) -> Line<'static> {
    let theme = tui_theme();
    let mut spans = vec![Span::styled("Agents", theme.accent_style())];
    for tab in AgentPanel::tabs() {
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

pub(crate) fn render_agent_run_prompt_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &AgentRunPromptPanel,
) {
    let theme = tui_theme();
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    let prompt_value = if panel.prompt.is_empty() {
        Span::styled("required", theme.dim_style())
    } else {
        Span::raw(panel.prompt.clone())
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Run Agent", theme.dim_style().add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {}", panel.agent_name), theme.accent_style()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("› Prompt ", theme.selected_row_style()),
            prompt_value,
        ]),
        Line::from(""),
    ];
    if let Some(notice) = &panel.notice {
        lines.push(Line::from(Span::styled(notice.clone(), theme.dim_style())));
    }
    lines.push(Line::from(Span::styled(
        "Enter run in background  Esc back",
        theme.dim_style(),
    )));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

pub(crate) fn render_agent_editor_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &AgentEditorPanel,
) {
    let theme = tui_theme();
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    let title = match panel.mode {
        AgentEditorMode::Create => "Create Agent",
        AgentEditorMode::Update { .. } => "Update Agent",
    };
    let mut lines = vec![Line::from(Span::styled(
        title,
        theme.dim_style().add_modifier(Modifier::BOLD),
    ))];
    lines.push(agent_editor_field_line(
        panel,
        AgentEditorField::Name,
        &panel.name,
    ));
    lines.push(agent_editor_field_line(
        panel,
        AgentEditorField::Description,
        &panel.description,
    ));
    lines.push(agent_editor_field_line(
        panel,
        AgentEditorField::Instructions,
        &panel.instructions,
    ));
    lines.push(agent_editor_field_line(
        panel,
        AgentEditorField::Model,
        &panel.model,
    ));
    lines.push(agent_editor_field_line(
        panel,
        AgentEditorField::Tools,
        &panel.tools,
    ));
    lines.push(agent_editor_field_line(
        panel,
        AgentEditorField::PermissionMode,
        &panel.permission_mode,
    ));
    lines.push(agent_editor_field_line(
        panel,
        AgentEditorField::Background,
        if panel.background { "true" } else { "false" },
    ));
    lines.push(agent_editor_field_line(
        panel,
        AgentEditorField::MaxSpawnDepth,
        &panel.max_spawn_depth,
    ));
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

pub(crate) fn agent_editor_field_line(
    panel: &AgentEditorPanel,
    field: AgentEditorField,
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
    Line::from(Span::styled(
        format!("{marker} {}: {value}", field.label()),
        style,
    ))
}

pub(crate) fn render_model_panel(
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

pub(crate) fn render_model_list_tab(
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
                false,
                Duration::default(),
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

pub(crate) fn render_model_info_tab(frame: &mut Frame<'_>, inner: Rect, panel: &mut ModelPanel) {
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

pub(crate) fn model_panel_tabs(active: ModelTab) -> Line<'static> {
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

pub(crate) fn model_list_footer_text(selection: &BottomSelectionPanel) -> String {
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

pub(crate) fn model_info_body(panel: &ModelPanel) -> Vec<Line<'static>> {
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

pub(crate) fn model_info_lines(
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

pub(crate) fn model_detail_limits(model: &ConfiguredModel) -> Vec<String> {
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
