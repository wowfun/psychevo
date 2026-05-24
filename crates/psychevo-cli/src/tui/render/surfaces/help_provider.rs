#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn model_detail_capabilities(model: &ConfiguredModel) -> Vec<String> {
    let caps = &model.metadata.capabilities;
    let mut parts = Vec::new();
    push_bool_capability(&mut parts, caps.reasoning, "reasoning", "no reasoning");
    push_bool_capability(&mut parts, caps.tool_call, "tools", "no tools");
    push_bool_capability(
        &mut parts,
        caps.developer_role,
        "developer role",
        "no developer role",
    );
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

pub(crate) fn model_detail_modalities(model: &ConfiguredModel) -> Vec<String> {
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

pub(crate) fn push_bool_capability(
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

pub(crate) fn model_detail_pricing(model: &ConfiguredModel) -> Vec<String> {
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

pub(crate) fn model_detail_source(model: &ConfiguredModel, source: ModelRowSource) -> Vec<String> {
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

pub(crate) fn format_model_rate(value: f64) -> String {
    format!("${value:.3}/M")
}

pub(crate) fn format_model_rate_pair(left: f64, right: f64) -> String {
    format!("${left:.3}/${right:.3}/M")
}

pub(crate) fn render_help_panel(frame: &mut Frame<'_>, area: Rect, panel: &mut HelpPanel) {
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

pub(crate) fn help_panel_tabs(active: HelpTab) -> Line<'static> {
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

pub(crate) fn help_panel_body(panel: &HelpPanel) -> Vec<Line<'static>> {
    let lines = match panel.tab {
        HelpTab::General => panel.sections.general.clone(),
        HelpTab::Commands => panel.sections.commands.clone(),
        HelpTab::CustomCommands => panel.sections.custom_commands.clone(),
    };
    lines
        .into_iter()
        .map(|line| help_panel_body_line(&line))
        .collect()
}

pub(crate) fn help_panel_body_line(line: &str) -> Line<'static> {
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

pub(crate) fn render_provider_wizard_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &ProviderWizardPanel,
) {
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

pub(crate) fn provider_wizard_field_line(
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

pub(crate) fn bottom_panel_row(
    row: &BottomSelectionRow,
    selected: bool,
    width: u16,
    running_activity: bool,
    activity_elapsed: Duration,
) -> Line<'static> {
    let theme = tui_theme();
    let select_marker = if selected { "›" } else { " " };
    let state_marker = if running_activity {
        format!("{} ", activity_spinner_frame(activity_elapsed))
    } else if row.is_current {
        "● ".to_string()
    } else if row.is_default {
        "◆ ".to_string()
    } else {
        "  ".to_string()
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
