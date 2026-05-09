fn transcript_selectable_area(area: Rect) -> Rect {
    Rect {
        height: transcript_viewport_height(area),
        ..area
    }
}

fn transcript_viewport_height(area: Rect) -> u16 {
    area.height.saturating_sub(1)
}

fn render_active_selection(frame: &mut Frame<'_>, ui: &FullscreenUi<'_>) {
    apply_selection_highlight(frame.buffer_mut(), &ui.screen_lines, &ui.selection);
}

fn apply_selection_highlight(
    buffer: &mut ratatui::buffer::Buffer,
    lines: &[ScreenLine],
    selection: &SelectionState,
) {
    let Some(anchor) = selection.anchor else {
        return;
    };
    let Some(focus) = selection.focus else {
        return;
    };
    if anchor == focus {
        return;
    }
    let ((start_x, start_y), (end_x, end_y)) = ordered_selection(anchor, focus);
    for line in lines {
        if selection.region.is_some_and(|region| line.region != region) {
            continue;
        }
        if line.y < start_y || line.y > end_y {
            continue;
        }
        let from = if line.y == start_y { start_x } else { 0 };
        let to = if line.y == end_y { end_x } else { u16::MAX };
        if to <= from {
            continue;
        }
        for cell in &line.cells {
            if !cell_overlaps_range(cell, from, to) {
                continue;
            }
            let cell_end = cell.x.saturating_add(cell.width);
            for x in cell.x..cell_end {
                if let Some(buffer_cell) = buffer.cell_mut((x, line.y)) {
                    buffer_cell.set_bg(TUI_SELECTION_BG);
                }
            }
        }
    }
}

fn render_transcript(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    frame.render_widget(Block::default(), area);
    let viewport_height = transcript_viewport_height(area);
    ui.last_transcript_height = viewport_height;
    ui.last_transcript_width = area.width;
    refresh_transcript_layout(ui, area.width);
    ui.resolve_transcript_scroll_for_render_with_total(ui.transcript_layout.total_height);
    let mut lines = Vec::new();
    let mut surface_rows = Vec::new();
    let mut areas = Vec::new();
    let window_start = usize::from(ui.scroll);
    let window_end = window_start.saturating_add(usize::from(viewport_height));
    let mut slice_start = None;
    let mut rendered_height = 0usize;
    for (index, row) in ui.transcript.iter().enumerate() {
        let Some(layout_row) = ui.transcript_layout.rows.get(index) else {
            continue;
        };
        if layout_row.height == 0 {
            continue;
        }
        let row_start = layout_row.start;
        let row_end = row_start.saturating_add(layout_row.height);
        if row_start >= window_end || row_end <= window_start {
            continue;
        }
        let visible_start = row_start.max(window_start);
        let visible_end = row_end.min(window_end);
        let y = area
            .y
            .saturating_add((visible_start.saturating_sub(window_start)) as u16);
        areas.push((
            index,
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: (visible_end.saturating_sub(visible_start)).min(usize::from(u16::MAX))
                    as u16,
            },
        ));
        let first_slice_start = *slice_start.get_or_insert(row_start);
        let compact_trailing =
            compact_trailing_for(&ui.transcript, index, row, ui.thinking_visible);
        let row_lines = transcript_lines(
            row,
            ui.selected_row == Some(index),
            compact_trailing,
            area.width,
        );
        let prompt_surface_rows = if row.kind == TranscriptKind::Prompt {
            row_lines
                .len()
                .saturating_sub(usize::from(!compact_trailing))
        } else {
            0
        };
        for (line_index, line) in row_lines.iter().enumerate() {
            let has_surface_bg = line_index < prompt_surface_rows;
            for _ in 0..wrapped_line_count(std::slice::from_ref(line), area.width) {
                surface_rows.push(has_surface_bg);
            }
        }
        lines.extend(row_lines);
        rendered_height = rendered_height.saturating_add(layout_row.height);
        if first_slice_start.saturating_add(rendered_height) >= window_end {
            break;
        }
    }
    ui.last_entry_areas = areas;
    let paragraph_scroll =
        window_start.saturating_sub(slice_start.unwrap_or(window_start)) as u16;
    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::BOTTOM))
        .wrap(Wrap { trim: false })
        .scroll((paragraph_scroll, 0));
    frame.render_widget(paragraph, area);
    for (offset, has_surface_bg) in surface_rows
        .iter()
        .skip(usize::from(paragraph_scroll))
        .take(viewport_height as usize)
        .enumerate()
    {
        if !*has_surface_bg {
            continue;
        }
        let y = area.y.saturating_add(offset as u16);
        for x in area.x..area.x.saturating_add(area.width) {
            frame.buffer_mut()[(x, y)].set_bg(TUI_SURFACE_BG);
        }
    }
    ui.capture_selectable_rows(
        frame.buffer_mut(),
        transcript_selectable_area(area),
        SelectableRegion::Transcript,
    );
}

fn refresh_transcript_layout(ui: &mut FullscreenUi<'_>, width: u16) {
    let old_rows = if ui.transcript_layout.width == width
        && ui.transcript_layout.thinking_visible == ui.thinking_visible
    {
        Some(ui.transcript_layout.rows.as_slice())
    } else {
        None
    };
    let mut rows = Vec::with_capacity(ui.transcript.len());
    let mut total_height = 0usize;
    #[cfg(test)]
    let mut recomputed_rows = 0usize;
    for (index, row) in ui.transcript.iter().enumerate() {
        let visible = row_visible(row, ui.thinking_visible);
        let compact_trailing = visible
            && compact_trailing_for(&ui.transcript, index, row, ui.thinking_visible);
        let selected = ui.selected_row == Some(index);
        let key = transcript_layout_row_key(row, visible, compact_trailing, selected);
        let height = if visible {
            old_rows
                .and_then(|rows| rows.get(index))
                .filter(|cached| cached.key == key)
                .map(|cached| cached.height)
                .unwrap_or_else(|| {
                    #[cfg(test)]
                    {
                        recomputed_rows += 1;
                    }
                    let lines = transcript_lines(row, selected, compact_trailing, width);
                    wrapped_line_count(&lines, width)
                })
        } else {
            0
        };
        rows.push(TranscriptLayoutRow {
            key,
            start: total_height,
            height,
        });
        total_height = total_height.saturating_add(height);
    }
    ui.transcript_layout = TranscriptLayoutCache {
        width,
        thinking_visible: ui.thinking_visible,
        rows,
        total_height,
        #[cfg(test)]
        recomputed_rows,
    };
}

fn transcript_layout_row_key(
    row: &TranscriptRow,
    visible: bool,
    compact_trailing: bool,
    selected: bool,
) -> TranscriptLayoutRowKey {
    let tool_elapsed = tool_elapsed_label(row);
    let active_tool_marker = active_tool_elapsed(row)
        .map(status_spinner_frame)
        .unwrap_or("");
    TranscriptLayoutRowKey {
        visible,
        compact_trailing,
        selected,
        kind: row.kind,
        failed: row.failed,
        expanded: row.expanded,
        expandable: row.is_expandable(),
        tool_elapsed_hash: hash_layout_text(tool_elapsed.as_deref().unwrap_or("")),
        active_tool_marker_hash: hash_layout_text(active_tool_marker),
        title_hash: hash_layout_text(&row.title),
        text_hash: hash_layout_text(row.expandable_text()),
    }
}

fn hash_layout_text(value: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&value, &mut hasher);
    std::hash::Hasher::finish(&hasher)
}


fn transcript_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    if row.kind == TranscriptKind::Prompt {
        return prompt_lines(row, selected, compact_trailing, width);
    }
    if row.kind == TranscriptKind::Answer {
        return answer_lines(row, selected, compact_trailing);
    }
    if row.kind == TranscriptKind::Thinking {
        return thinking_lines(row, selected, compact_trailing);
    }
    if matches!(
        row.kind,
        TranscriptKind::Explored | TranscriptKind::Ran | TranscriptKind::Changed
    ) {
        return tool_lines(row, selected, compact_trailing, width);
    }

    let style = label_style(row.kind, row.failed);
    let marker = if selected { ">" } else { "▌" };
    let mut out = Vec::new();
    let title = row.title.trim();
    if !title.is_empty() {
        let suffix = if row.is_expandable() {
            if row.expanded { " [-]" } else { " [+]" }
        } else {
            ""
        };
        out.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(
                format!("{title}{suffix}"),
                style.add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    let body_style = style_for_body(row.kind, row.failed);
    for line in row.expandable_text().lines() {
        let mut span = Span::styled(line.to_string(), body_style);
        if row.kind == TranscriptKind::Prompt {
            span = span.style(body_style.bg(Color::Rgb(24, 24, 28)));
        }
        out.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            span,
        ]));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(marker.to_string(), style)));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn prompt_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let body_style = style_for_body(row.kind, row.failed).bg(TUI_SURFACE_BG);
    for (index, line) in row.expandable_text().lines().enumerate() {
        let first_prefix = if selected {
            "> "
        } else if index == 0 {
            "› "
        } else {
            "  "
        };
        let continuation_prefix = if selected { "> " } else { "  " };
        for (wrapped_index, wrapped) in wrap_prompt_text(line, first_prefix, width)
            .into_iter()
            .enumerate()
        {
            let prefix = if wrapped_index == 0 {
                first_prefix
            } else {
                continuation_prefix
            };
            out.push(prompt_line(prefix, &wrapped, width, body_style));
        }
    }
    if out.is_empty() {
        out.push(prompt_line("› ", "", width, body_style));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn prompt_line(prefix: &str, text: &str, width: u16, style: Style) -> Line<'static> {
    let content_width = UnicodeWidthStr::width(prefix).saturating_add(UnicodeWidthStr::width(text));
    let padding = usize::from(width).saturating_sub(content_width);
    Line::from(vec![
        Span::styled(prefix.to_string(), style.fg(TUI_DIM)),
        Span::styled(text.to_string(), style),
        Span::styled(" ".repeat(padding), style),
    ])
}

fn wrap_prompt_text(text: &str, prefix: &str, width: u16) -> Vec<String> {
    let content_width = usize::from(width)
        .saturating_sub(UnicodeWidthStr::width(prefix))
        .saturating_sub(1)
        .max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if current_width > 0 && current_width.saturating_add(ch_width) > content_width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width = current_width.saturating_add(ch_width);
        if current_width >= content_width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

fn answer_lines(row: &TranscriptRow, selected: bool, compact_trailing: bool) -> Vec<Line<'static>> {
    let body_style = style_for_body(row.kind, row.failed);
    let mut out = Vec::new();
    for line in row.expandable_text().lines() {
        if selected {
            out.push(Line::from(vec![
                Span::styled("> ".to_string(), label_style(row.kind, row.failed)),
                Span::styled(line.to_string(), body_style),
            ]));
        } else {
            out.push(Line::from(Span::styled(line.to_string(), body_style)));
        }
    }
    if out.is_empty() && selected {
        out.push(Line::from(Span::styled(
            ">".to_string(),
            label_style(row.kind, row.failed),
        )));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn thinking_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
) -> Vec<Line<'static>> {
    let rail_style = if row.failed {
        Style::default().fg(TUI_RED)
    } else {
        Style::default().fg(TUI_DIM)
    };
    let prefix_style = label_style(row.kind, row.failed);
    let body_style = style_for_body(row.kind, row.failed);
    let marker = if selected { ">" } else { "▌" };
    let mut out = Vec::new();
    for (index, line) in row.expandable_text().lines().enumerate() {
        let mut spans = vec![Span::styled(format!("{marker} "), rail_style)];
        if index == 0 {
            spans.push(Span::styled("Thinking: ".to_string(), prefix_style));
        }
        spans.push(Span::styled(line.to_string(), body_style));
        out.push(Line::from(spans));
    }
    if out.is_empty() {
        out.push(Line::from(vec![
            Span::styled(format!("{marker} "), rail_style),
            Span::styled("Thinking:".to_string(), prefix_style),
        ]));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn tool_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let active_elapsed = active_tool_elapsed(row);
    let bullet_style = if row.failed {
        Style::default().fg(TUI_RED)
    } else if active_elapsed.is_some() || selected {
        Style::default().fg(TUI_CYAN)
    } else {
        Style::default().fg(Color::Green)
    };
    let title_style = if row.failed {
        Style::default().fg(TUI_RED).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    let body_style = style_for_body(row.kind, row.failed);
    let marker = if selected {
        "> ".to_string()
    } else if let Some(elapsed) = active_elapsed {
        format!("{} ", status_spinner_frame(elapsed))
    } else {
        "• ".to_string()
    };
    let mut out = Vec::new();
    let title = row.title.trim();
    if !title.is_empty() {
        let suffix = if row.is_expandable() {
            if row.expanded { " [-]" } else { " [+]" }
        } else {
            ""
        };
        let title = format!("{title}{suffix}");
        let elapsed = tool_elapsed_label(row);
        out.push(tool_title_line(
            &marker,
            bullet_style,
            &title,
            title_style,
            elapsed.as_deref(),
            width,
        ));
    }
    for (index, line) in row.expandable_text().lines().enumerate() {
        let prefix = if index == 0 { "  └ " } else { "    " };
        out.push(Line::from(vec![
            Span::styled(prefix.to_string(), Style::default().fg(TUI_DIM)),
            Span::styled(line.to_string(), body_style),
        ]));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(marker, bullet_style)));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn tool_title_line(
    marker: &str,
    marker_style: Style,
    title: &str,
    title_style: Style,
    elapsed: Option<&str>,
    width: u16,
) -> Line<'static> {
    let Some(elapsed) = elapsed.filter(|value| !value.is_empty()) else {
        return Line::from(vec![
            Span::styled(marker.to_string(), marker_style),
            Span::styled(title.to_string(), title_style),
        ]);
    };
    let marker_width = UnicodeWidthStr::width(marker);
    let width = usize::from(width);
    let elapsed = truncate_display_width(elapsed, width.saturating_sub(marker_width));
    let elapsed_width = UnicodeWidthStr::width(elapsed.as_str());
    let separator_width = usize::from(elapsed_width > 0);
    let title_width = width
        .saturating_sub(marker_width)
        .saturating_sub(elapsed_width)
        .saturating_sub(separator_width);
    let title = truncate_display_width(title, title_width);
    let padding = width
        .saturating_sub(marker_width)
        .saturating_sub(UnicodeWidthStr::width(title.as_str()))
        .saturating_sub(elapsed_width);
    Line::from(vec![
        Span::styled(marker.to_string(), marker_style),
        Span::styled(title, title_style),
        Span::raw(" ".repeat(padding)),
        Span::styled(elapsed, Style::default().fg(TUI_DIM)),
    ])
}

fn tool_elapsed_label(row: &TranscriptRow) -> Option<String> {
    row.tool_elapsed
        .or_else(|| active_tool_elapsed(row))
        .map(format_duration_compact)
}

fn active_tool_elapsed(row: &TranscriptRow) -> Option<Duration> {
    if row.tool_elapsed.is_some() {
        return None;
    }
    row.tool_started.map(|started| started.elapsed())
}

fn truncate_display_width(value: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let ellipsis = "…";
    if max_width == 1 {
        return ellipsis.to_string();
    }
    let keep_width = max_width.saturating_sub(UnicodeWidthStr::width(ellipsis));
    let mut out = String::new();
    let mut width = 0usize;
    for ch in value.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if width.saturating_add(ch_width) > keep_width {
            break;
        }
        out.push(ch);
        width = width.saturating_add(ch_width);
    }
    out.push_str(ellipsis);
    out
}

fn label_style(kind: TranscriptKind, failed: bool) -> Style {
    if failed {
        return Style::default().fg(TUI_RED);
    }
    match kind {
        TranscriptKind::Prompt
        | TranscriptKind::Explored
        | TranscriptKind::Ran
        | TranscriptKind::Changed => Style::default().fg(TUI_CYAN),
        TranscriptKind::Answer => Style::default().fg(TUI_MAGENTA),
        TranscriptKind::Thinking => Style::default().fg(TUI_PAPER),
        TranscriptKind::Meta => Style::default().fg(TUI_DIM),
        TranscriptKind::Status => Style::default().fg(TUI_CYAN),
        TranscriptKind::Error => Style::default().fg(TUI_RED),
    }
}

fn style_for_body(kind: TranscriptKind, failed: bool) -> Style {
    if failed {
        return Style::default().fg(TUI_RED);
    }
    match kind {
        TranscriptKind::Thinking => Style::default().fg(TUI_DIM),
        TranscriptKind::Meta | TranscriptKind::Status => Style::default().fg(TUI_DIM),
        TranscriptKind::Error => Style::default().fg(TUI_RED),
        _ => Style::default(),
    }
}
