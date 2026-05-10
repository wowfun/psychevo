fn render_composer(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let theme = tui_theme();
    let surface_style = theme.surface_style();
    frame.render_widget(Block::default().style(surface_style), area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let textarea_empty = ui.textarea.is_empty();
    let marker_width = if textarea_empty {
        area.width.min(1)
    } else {
        area.width.min(2)
    };
    frame.render_widget(
        Paragraph::new(Line::from(if textarea_empty {
            vec![Span::styled("›".to_string(), surface_style.fg(theme.dim))]
        } else {
            vec![
                Span::styled("›".to_string(), surface_style.fg(theme.dim)),
                Span::styled(" ".to_string(), surface_style),
            ]
        }))
        .style(surface_style),
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

    if textarea_empty && input_area.width > 1 {
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
    let lines = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let marker = if item.upcoming { " upcoming" } else { "" };
            let selected = index == selected_index;
            let prefix = if selected { "> " } else { "  " };
            let row_style = if selected {
                theme.selected_row_style()
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(prefix, row_style.fg(theme.accent)),
                Span::styled(item.command.clone(), row_style.fg(theme.accent)),
                Span::styled(
                    format!("  {}{marker}", item.description),
                    row_style.fg(theme.dim),
                ),
            ])
        })
        .collect::<Vec<_>>();
    for index in 0..items.len() {
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
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::LEFT).title(" commands "))
            .style(theme.menu_style()),
        area,
    );
}

fn render_file_popup(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let theme = tui_theme();
    ui.last_file_popup_areas.clear();
    let Some(popup) = &ui.file_search.popup else {
        return;
    };
    let lines = if popup.matches.is_empty() {
        let text = if popup.waiting {
            "  searching files..."
        } else {
            "  no matches"
        };
        vec![Line::from(Span::styled(
            text.to_string(),
            theme.dim_style(),
        ))]
    } else {
        popup
            .matches
            .iter()
            .take(FILE_POPUP_MAX_ROWS)
            .enumerate()
            .map(|(index, item)| {
                let selected = index == popup.selected;
                let row_style = if selected {
                    theme.selected_row_style()
                } else {
                    Style::default()
                };
                let prefix = if selected { "> " } else { "  " };
                let kind = match item.kind {
                    FileSearchMatchKind::Directory => "dir ",
                    FileSearchMatchKind::File => "file",
                };
                Line::from(vec![
                    Span::styled(prefix, row_style.fg(theme.accent)),
                    Span::styled(kind.to_string(), row_style.fg(theme.identity)),
                    Span::styled("  ".to_string(), row_style),
                    Span::styled(item.path.clone(), row_style),
                ])
            })
            .collect()
    };
    for index in 0..lines.len() {
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
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::LEFT).title(" files "))
            .style(theme.menu_style()),
        area,
    );
}

fn render_skill_popup(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let theme = tui_theme();
    ui.last_skill_popup_areas.clear();
    let Some(popup) = &ui.skill_search.popup else {
        return;
    };
    let lines = if popup.matches.is_empty() {
        vec![Line::from(Span::styled(
            "  no skill matches".to_string(),
            theme.dim_style(),
        ))]
    } else {
        popup
            .matches
            .iter()
            .take(FILE_POPUP_MAX_ROWS)
            .enumerate()
            .map(|(index, item)| {
                let selected = index == popup.selected;
                let row_style = if selected {
                    theme.selected_row_style()
                } else {
                    Style::default()
                };
                let prefix = if selected { "> " } else { "  " };
                Line::from(vec![
                    Span::styled(prefix, row_style.fg(theme.accent)),
                    Span::styled("$".to_string(), row_style.fg(theme.identity)),
                    Span::styled(item.name.clone(), row_style.fg(theme.accent)),
                    Span::styled(format!("  {}", item.description), row_style.fg(theme.dim)),
                ])
            })
            .collect()
    };
    for index in 0..lines.len() {
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
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::LEFT).title(" skills "))
            .style(theme.menu_style()),
        area,
    );
}

fn render_status(frame: &mut Frame<'_>, area: Rect, app: &TuiApp, ui: &FullscreenUi<'_>) {
    let theme = tui_theme();
    let model = app.model_display_value();
    let variant = app.variant_display_value();
    let mut spans = Vec::new();
    spans.push(Span::raw(model));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(variant, theme.identity_style()));
    if app.current_mode != RunMode::Build {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            app.current_mode.as_str().to_string(),
            theme.accent_style(),
        ));
    }
    if parse_shell_escape_input(&textarea_text(&ui.textarea)).is_some() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("shell", theme.accent_style()));
    }
    if ui.running.is_some() || ui.running_started.is_some() {
        let elapsed = ui.running_elapsed().unwrap_or_default();
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            activity_spinner_frame(elapsed),
            theme.accent_style(),
        ));
        spans.push(Span::raw(" "));
        if ui.interrupt_requested {
            spans.push(Span::styled("interrupting", theme.error_style()));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format_duration_compact(elapsed),
                theme.dim_style(),
            ));
        } else {
            spans.push(Span::styled(
                format_duration_compact(elapsed),
                theme.dim_style(),
            ));
            spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
            spans.push(Span::styled("Esc", theme.accent_style()));
        }
    }
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
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
        sidebar_heading("Context"),
        Line::from(format!("workdir: {}", ui.sidebar.workdir)),
        Line::from(format!("branch: {}", ui.sidebar.branch)),
    ];
    lines.push(Line::from(format!("messages: {}", ui.sidebar.message_count)));
    lines.push(Line::from(format!("tool calls: {}", ui.sidebar.tool_count)));
    if let Some(tokens) = ui.sidebar.tokens {
        lines.push(Line::from(format!("tokens: {}", format_count(tokens))));
    }
    if let Some(percent) = ui.sidebar.context_percent {
        lines.push(Line::from(format!("context: {percent:.1}%")));
    }
    if let Some(cost) = ui.sidebar.cost_nanodollars {
        lines.push(Line::from(format!("cost: {}", format_nanodollars(cost))));
    }
    lines.push(Line::from(""));
    lines.push(sidebar_heading("Modified Files"));
    if ui.sidebar.changed_files.is_empty() {
        lines.push(Line::from(Span::styled(
            "(clean)",
            theme.dim_style(),
        )));
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
    if let BottomPanel::ProviderWizard(panel) = panel {
        render_provider_wizard_panel(frame, area, panel);
        return;
    }
    frame.render_widget(
        Block::default().style(theme.menu_style()),
        area,
    );
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    let selection = panel.selection_mut();
    let reserved = 4 + if selection.notice.is_some() { 1 } else { 0 };
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
        lines.push(Line::from(Span::styled(
            notice.clone(),
            theme.dim_style(),
        )));
    }
    lines.push(Line::from(Span::styled(
        selection.footer_text(),
        theme.dim_style(),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_provider_wizard_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &ProviderWizardPanel,
) {
    let theme = tui_theme();
    frame.render_widget(
        Block::default().style(theme.menu_style()),
        area,
    );
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
            Span::styled(
                "  OpenAI-compatible global provider",
                theme.dim_style(),
            ),
        ]),
        provider_wizard_field_line(panel, ProviderWizardField::Label, "Label", &panel.label),
        provider_wizard_field_line(
            panel,
            ProviderWizardField::ProviderId,
            "Provider ID",
            &panel.provider_id,
        ),
        provider_wizard_field_line(panel, ProviderWizardField::BaseUrl, "Base URL", &panel.base_url),
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
        lines.push(Line::from(Span::styled(
            notice.clone(),
            theme.dim_style(),
        )));
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
        Span::styled(
            prefix,
            theme.accent_style().add_modifier(Modifier::BOLD),
        ),
        Span::styled(rest, theme.dim_style()),
    ])
}
