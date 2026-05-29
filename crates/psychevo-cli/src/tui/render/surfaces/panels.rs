#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn render_bottom_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &mut BottomPanel,
    row_areas: &mut Vec<(usize, Rect)>,
    activity_elapsed: Duration,
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
    if let BottomPanel::PermissionApproval(panel) = panel {
        render_permission_approval_panel(frame, area, panel, row_areas);
        return;
    }
    if let BottomPanel::Clarify(panel) = panel {
        render_clarify_panel(frame, area, panel, row_areas);
        return;
    }
    if let BottomPanel::AgentRunPrompt(panel) = panel {
        render_agent_run_prompt_panel(frame, area, panel);
        return;
    }
    if let BottomPanel::AgentEditor(panel) = panel {
        render_agent_editor_panel(frame, area, panel);
        return;
    }
    if let BottomPanel::Agents(panel) = panel {
        render_agent_panel(frame, area, panel, row_areas);
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
                selection.row_has_running_activity(row),
                activity_elapsed,
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

pub(crate) fn render_permission_approval_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &mut PermissionApprovalPanel,
    row_areas: &mut Vec<(usize, Rect)>,
) {
    let theme = tui_theme();
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    let source = panel
        .session_id
        .as_deref()
        .map(short_session)
        .unwrap_or("current");
    let mut entries: Vec<(Option<usize>, Line<'static>)> = Vec::new();
    entries.push((
        None,
        Line::from(Span::styled(
            format!("Permission required  source: {source}"),
            theme.accent_style().add_modifier(Modifier::BOLD),
        )),
    ));
    entries.push((
        None,
        Line::from(Span::styled(panel.request.reason.clone(), Style::default())),
    ));
    entries.push((
        None,
        Line::from(Span::styled(
            format!(
                "tool: {}  action: {}",
                panel.request.tool_name, panel.request.summary
            ),
            theme.dim_style(),
        )),
    ));
    if let Some(rule) = &panel.request.matched_rule {
        entries.push((
            None,
            Line::from(Span::styled(format!("matched: {rule}"), theme.dim_style())),
        ));
    }
    if let Some(rule) = &panel.request.suggested_rule {
        entries.push((
            None,
            Line::from(Span::styled(format!("grant: {rule}"), theme.dim_style())),
        ));
    }
    entries.push((None, Line::from("")));

    for (index, (_outcome, label, description)) in panel.options().iter().enumerate() {
        let selected = index == panel.selected;
        let marker = if selected { "›" } else { " " };
        let key = match *label {
            "Allow once" => "y",
            "Allow session" => "a",
            "Allow permanent" => "p",
            _ => "d",
        };
        let style = if selected {
            theme.accent_style().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        entries.push((
            Some(index),
            Line::from(vec![
                Span::styled(format!("{marker} [{key}] "), theme.dim_style()),
                Span::styled((*label).to_string(), style),
                Span::styled("  ", theme.dim_style()),
                Span::styled((*description).to_string(), theme.dim_style()),
            ]),
        ));
    }

    entries.push((None, Line::from("")));
    if let Some(notice) = &panel.notice {
        entries.push((
            None,
            Line::from(Span::styled(notice.clone(), theme.dim_style())),
        ));
    }
    entries.push((
        None,
        Line::from(Span::styled(
            "↑/↓ or j/k select | enter confirm | y once | a session | p permanent | d/esc deny",
            theme.dim_style(),
        )),
    ));

    let mut visual_y = 0u16;
    let total_height = entries.iter().fold(0u16, |height, (_index, line)| {
        height.saturating_add(line_wrapped_height(line, inner.width))
    });
    let max_scroll = total_height.saturating_sub(inner.height);
    panel.scroll = panel.scroll.min(max_scroll);
    for (index, line) in &entries {
        let row_height = line_wrapped_height(line, inner.width);
        if let Some(index) = index
            && let Some(area) = visible_wrapped_row_area(inner, visual_y, row_height, panel.scroll)
        {
            row_areas.push((*index, area));
        }
        visual_y = visual_y.saturating_add(row_height);
    }
    let lines = entries
        .into_iter()
        .map(|(_index, line)| line)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((panel.scroll, 0)),
        inner,
    );
}

pub(crate) fn line_wrapped_height(line: &Line<'_>, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let display_width = line
        .spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum::<usize>();
    display_width.div_ceil(width).max(1) as u16
}

pub(crate) fn visible_wrapped_row_area(
    inner: Rect,
    visual_y: u16,
    height: u16,
    scroll: u16,
) -> Option<Rect> {
    let row_start = visual_y;
    let row_end = visual_y.saturating_add(height);
    let viewport_start = scroll;
    let viewport_end = scroll.saturating_add(inner.height);
    if row_end <= viewport_start || row_start >= viewport_end {
        return None;
    }
    let visible_start = row_start.max(viewport_start);
    let visible_end = row_end.min(viewport_end);
    Some(Rect {
        x: inner.x,
        y: inner.y.saturating_add(visible_start.saturating_sub(scroll)),
        width: inner.width,
        height: visible_end.saturating_sub(visible_start).max(1),
    })
}

pub(crate) fn render_clarify_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &mut ClarifyPanel,
    row_areas: &mut Vec<(usize, Rect)>,
) {
    let theme = tui_theme();
    frame.render_widget(Block::default().style(theme.menu_style()), area);
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    let Some(question) = panel.current_question() else {
        frame.render_widget(
            Paragraph::new("No clarify question").style(theme.dim_style()),
            inner,
        );
        return;
    };

    let mut lines = Vec::new();
    let mut cursor_position: Option<(u16, u16)> = None;
    lines.push(Line::from(vec![Span::styled(
        panel.question_progress(),
        theme.dim_style().add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(Span::styled(
        question.question.clone(),
        theme.accent_style(),
    )));
    lines.push(Line::from(""));

    let mode = panel.mode();
    let selected_index = panel.selected();
    for (index, option) in question.options.iter().enumerate() {
        let row_y = inner.y.saturating_add(lines.len() as u16);
        row_areas.push((
            index,
            Rect {
                x: inner.x,
                y: row_y,
                width: inner.width,
                height: 1,
            },
        ));
        let selected = index == selected_index;
        let marker = if selected { "›" } else { " " };
        let prefix = format!("{marker} {}. ", index + 1);
        let style = if selected {
            theme.accent_style().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let note = panel.note_draft(index);
        let editing_note = selected && mode == ClarifyInputMode::Note;
        let detail_spans = if editing_note || !note.is_empty() {
            if editing_note {
                cursor_position = Some((
                    clarify_input_cursor_x(
                        inner,
                        &[prefix.as_str(), option.label.as_str(), "  ", "note: "],
                        note,
                        panel.note_cursor(index),
                    ),
                    row_y,
                ));
            }
            vec![
                Span::styled("note: ".to_string(), theme.dim_style()),
                Span::styled(note.to_string(), Style::default()),
            ]
        } else {
            let detail_style = if selected {
                theme.accent_style()
            } else {
                theme.dim_style()
            };
            vec![Span::styled(option.description.clone(), detail_style)]
        };
        let mut spans = vec![Span::styled(prefix, theme.dim_style())];
        spans.extend(clarify_option_label_spans(&option.label, style, &theme));
        spans.push(Span::styled("  ", theme.dim_style()));
        spans.extend(detail_spans);
        lines.push(Line::from(spans));
    }
    let other_index = question.options.len();
    let row_y = inner.y.saturating_add(lines.len() as u16);
    row_areas.push((
        other_index,
        Rect {
            x: inner.x,
            y: row_y,
            width: inner.width,
            height: 1,
        },
    ));
    let selected = other_index == selected_index;
    let marker = if selected { "›" } else { " " };
    let prefix = format!("{marker} {}. ", other_index + 1);
    let style = if selected {
        theme.accent_style().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let other_draft = panel.other_draft();
    let editing_other = selected && mode == ClarifyInputMode::Other;
    let other_detail_spans = if editing_other || !other_draft.is_empty() {
        if editing_other {
            cursor_position = Some((
                clarify_input_cursor_x(
                    inner,
                    &[prefix.as_str(), "Other", "  ", "answer: "],
                    other_draft,
                    panel.other_cursor(),
                ),
                row_y,
            ));
        }
        vec![
            Span::styled("answer: ".to_string(), theme.dim_style()),
            Span::styled(other_draft.to_string(), Style::default()),
        ]
    } else {
        vec![Span::styled(
            "Type a custom answer".to_string(),
            theme.dim_style(),
        )]
    };
    let mut spans = vec![
        Span::styled(prefix, theme.dim_style()),
        Span::styled("Other", style),
        Span::styled("  ", theme.dim_style()),
    ];
    spans.extend(other_detail_spans);
    lines.push(Line::from(spans));

    lines.push(Line::from(""));
    if let Some(notice) = &panel.notice {
        lines.push(Line::from(Span::styled(notice.clone(), theme.dim_style())));
    }
    lines.push(Line::from(Span::styled(
        "tab to edit note/custom answer | enter to submit answer | ←/→ to navigate questions | esc to interrupt",
        theme.dim_style(),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    if let Some((x, y)) = cursor_position
        && rect_contains(inner, x, y)
    {
        frame.set_cursor_position((x, y));
    }
}

pub(crate) fn clarify_option_label_spans(
    label: &str,
    style: Style,
    theme: &TuiTheme,
) -> Vec<Span<'static>> {
    let Some((base, marker, separator)) = split_recommended_marker(label) else {
        return vec![Span::styled(label.to_string(), style)];
    };
    let marker_style = if style == Style::default() {
        theme.accent_style().add_modifier(Modifier::BOLD)
    } else {
        style
    };
    let mut spans = Vec::new();
    if !base.is_empty() {
        spans.push(Span::styled(base.to_string(), style));
        spans.push(Span::styled(separator.to_string(), style));
    }
    spans.push(Span::styled(marker.to_string(), marker_style));
    spans
}

pub(crate) fn split_recommended_marker(label: &str) -> Option<(&str, &str, &'static str)> {
    pub(crate) const MARKERS: &[&str] = &[
        "(Recommended)",
        "(recommended)",
        "（Recommended）",
        "（recommended）",
        "（推荐）",
    ];
    for marker in MARKERS {
        if let Some(index) = label.find(marker) {
            let base = label[..index].trim_end();
            let separator = if marker.starts_with('(') && !base.is_empty() {
                " "
            } else {
                ""
            };
            return Some((base, &label[index..index + marker.len()], separator));
        }
    }
    None
}

pub(crate) fn clarify_input_cursor_x(
    inner: Rect,
    prefixes: &[&str],
    value: &str,
    cursor: usize,
) -> u16 {
    let prefix_width = prefixes
        .iter()
        .map(|part| UnicodeWidthStr::width(*part))
        .sum::<usize>();
    let cursor_width = value
        .chars()
        .take(cursor)
        .map(|ch| ch.width().unwrap_or(0))
        .sum::<usize>();
    let offset = prefix_width.saturating_add(cursor_width);
    inner.x.saturating_add(
        offset
            .min(inner.width.saturating_sub(1) as usize)
            .min(u16::MAX as usize) as u16,
    )
}

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
