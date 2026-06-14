pub(crate) fn bottom_status_session_usage_segments(ui: &FullscreenUi<'_>) -> Vec<String> {
    let Some(summary) = ui.session_usage_summary.as_ref() else {
        return Vec::new();
    };
    let mut segments = Vec::new();
    if let Some(percent) = summary.cache_read_percent {
        segments.push(format!("cache {:.0}%", percent));
    }
    if summary.reported_total_tokens > 0 {
        segments.push(format!(
            "tok {}",
            format_status_compact_count(summary.reported_total_tokens)
        ));
    }
    if summary.estimated_cost_nanodollars > 0 {
        segments.push(format!(
            "cost {}",
            format_status_nanodollars(summary.estimated_cost_nanodollars)
        ));
    }
    segments
}

fn format_status_compact_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn format_status_nanodollars(value: i64) -> String {
    format!("${:.6}", value as f64 / 1_000_000_000.0)
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
