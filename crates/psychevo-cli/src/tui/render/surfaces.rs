fn render_composer(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let surface_style = Style::default().bg(TUI_SURFACE_BG);
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
            vec![Span::styled("›".to_string(), surface_style.fg(TUI_DIM))]
        } else {
            vec![
                Span::styled("›".to_string(), surface_style.fg(TUI_DIM)),
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
                surface_style.fg(TUI_DIM),
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
    row_areas.clear();
    let lines = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let marker = if item.upcoming { " upcoming" } else { "" };
            let selected = index == selected_index;
            let prefix = if selected { "> " } else { "  " };
            let row_style = if selected {
                Style::default().bg(Color::Rgb(24, 24, 28))
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(prefix, row_style.fg(TUI_CYAN)),
                Span::styled(item.command.clone(), row_style.fg(TUI_CYAN)),
                Span::styled(
                    format!("  {}{marker}", item.description),
                    row_style.fg(TUI_DIM),
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
            .style(Style::default().bg(Color::Rgb(16, 16, 20))),
        area,
    );
}

fn render_file_popup(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
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
            Style::default().fg(TUI_DIM),
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
                    Style::default().bg(Color::Rgb(24, 24, 28))
                } else {
                    Style::default()
                };
                let prefix = if selected { "> " } else { "  " };
                let kind = match item.kind {
                    FileSearchMatchKind::Directory => "dir ",
                    FileSearchMatchKind::File => "file",
                };
                Line::from(vec![
                    Span::styled(prefix, row_style.fg(TUI_CYAN)),
                    Span::styled(kind.to_string(), row_style.fg(TUI_MAGENTA)),
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
            .style(Style::default().bg(Color::Rgb(16, 16, 20))),
        area,
    );
}

fn render_skill_popup(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    ui.last_skill_popup_areas.clear();
    let Some(popup) = &ui.skill_search.popup else {
        return;
    };
    let lines = if popup.matches.is_empty() {
        vec![Line::from(Span::styled(
            "  no skill matches".to_string(),
            Style::default().fg(TUI_DIM),
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
                    Style::default().bg(Color::Rgb(24, 24, 28))
                } else {
                    Style::default()
                };
                let prefix = if selected { "> " } else { "  " };
                Line::from(vec![
                    Span::styled(prefix, row_style.fg(TUI_CYAN)),
                    Span::styled("$".to_string(), row_style.fg(TUI_MAGENTA)),
                    Span::styled(item.name.clone(), row_style.fg(TUI_CYAN)),
                    Span::styled(format!("  {}", item.description), row_style.fg(TUI_DIM)),
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
            .style(Style::default().bg(Color::Rgb(16, 16, 20))),
        area,
    );
}

fn render_status(frame: &mut Frame<'_>, area: Rect, app: &TuiApp, ui: &FullscreenUi<'_>) {
    let model = app.model_display_value();
    let variant = app.variant_display_value();
    let mut spans = Vec::new();
    spans.push(Span::raw(model));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(variant, Style::default().fg(TUI_MAGENTA)));
    if app.current_mode != RunMode::Build {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            app.current_mode.as_str().to_string(),
            Style::default().fg(TUI_CYAN),
        ));
    }
    if parse_shell_escape_input(&textarea_text(&ui.textarea)).is_some() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("shell", Style::default().fg(TUI_CYAN)));
    }
    if ui.running.is_some() || ui.running_started.is_some() {
        let elapsed = ui.running_elapsed().unwrap_or_default();
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            status_spinner_frame(elapsed),
            Style::default().fg(TUI_CYAN),
        ));
        spans.push(Span::raw(" "));
        if ui.interrupt_requested {
            spans.push(Span::styled(
                "interrupting",
                Style::default().fg(TUI_RED),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format_duration_compact(elapsed),
                Style::default().fg(TUI_DIM),
            ));
        } else {
            spans.push(Span::styled(
                format_duration_compact(elapsed),
                Style::default().fg(TUI_DIM),
            ));
            spans.push(Span::styled(" · ".to_string(), Style::default().fg(TUI_DIM)));
            spans.push(Span::styled("Esc", Style::default().fg(TUI_CYAN)));
        }
    }
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

fn status_spinner_frame(elapsed: Duration) -> &'static str {
    const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    let index = ((elapsed.as_millis() / 120) % FRAMES.len() as u128) as usize;
    FRAMES[index]
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
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
    lines.push(Line::from(format!(
        "messages: {}  tools: {}",
        ui.sidebar.message_count, ui.sidebar.tool_count
    )));
    if let Some(tokens) = ui.sidebar.tokens {
        lines.push(Line::from(format!("tokens: {}", format_count(tokens))));
    }
    if let Some(percent) = ui.sidebar.context_percent {
        lines.push(Line::from(format!("context: {percent:.1}%")));
    }
    lines.push(Line::from(""));
    lines.push(sidebar_heading("Modified Files"));
    if ui.sidebar.changed_files.is_empty() {
        lines.push(Line::from(Span::styled(
            "(clean)",
            Style::default().fg(TUI_DIM),
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
    row_areas.clear();
    if let BottomPanel::ProviderWizard(panel) = panel {
        render_provider_wizard_panel(frame, area, panel);
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(18, 18, 22))),
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
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(header_padding)),
        Span::styled(esc_hint, Style::default().fg(TUI_DIM)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Search ", Style::default().fg(TUI_DIM)),
        Span::styled(selection.query.clone(), Style::default().fg(Color::Gray)),
    ]));
    let mut row_y = inner.y.saturating_add(lines.len() as u16);

    let filtered = selection.filtered_indices();
    if filtered.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            selection.empty_label.clone(),
            Style::default().fg(TUI_DIM),
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
                    Style::default().fg(TUI_CYAN).add_modifier(Modifier::BOLD),
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
            Style::default().fg(TUI_DIM),
        )));
    }
    lines.push(Line::from(Span::styled(
        selection.footer_text(),
        Style::default().fg(TUI_DIM),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_provider_wizard_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &ProviderWizardPanel,
) {
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(18, 18, 22))),
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
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  OpenAI-compatible global provider",
                Style::default().fg(TUI_DIM),
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
        Span::styled("  API key env ", Style::default().fg(TUI_DIM)),
        Span::styled(env_var, Style::default().fg(Color::Gray)),
        Span::styled(format!("  {env_note}"), Style::default().fg(TUI_DIM)),
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
            Style::default().fg(TUI_DIM),
        )));
    }
    lines.push(Line::from(Span::styled(
        "Enter next/save  Up/Down field  Esc back",
        Style::default().fg(TUI_DIM),
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
    let style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(246, 178, 127))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let value = if value.is_empty() { " " } else { value };
    Line::from(Span::styled(format!("{marker} {label}: {value}"), style))
}

fn bottom_panel_row(row: &BottomSelectionRow, selected: bool, width: u16) -> Line<'static> {
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
        Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(246, 178, 127))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
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
            Style::default().fg(TUI_CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(rest, Style::default().fg(TUI_DIM)),
    ])
}
