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
                    buffer_cell.set_bg(tui_theme().selection_bg);
                }
            }
        }
    }
}

fn render_transcript(
    frame: &mut Frame<'_>,
    area: Rect,
    ui: &mut FullscreenUi<'_>,
    session_identity: Option<&str>,
) {
    frame.render_widget(Block::default(), area);
    let viewport_height = transcript_viewport_height(area);
    ui.last_transcript_height = viewport_height;
    ui.last_transcript_width = area.width;
    refresh_transcript_layout(ui, area.width);
    ui.resolve_transcript_scroll_for_render_with_total(ui.transcript_layout.total_height);
    let render_blocks = transcript_render_blocks(ui);
    let mut lines = Vec::new();
    let mut surface_rows = Vec::new();
    let mut areas = Vec::new();
    let window_start = usize::from(ui.scroll);
    let window_end = window_start.saturating_add(usize::from(viewport_height));
    let mut slice_start = None;
    let mut rendered_height = 0usize;
    for (block_index, block) in render_blocks.iter().enumerate() {
        let Some(layout_block) = ui.transcript_layout.blocks.get(block_index) else {
            continue;
        };
        if layout_block.height == 0 {
            continue;
        }
        let block_start = layout_block.start;
        let block_end = block_start.saturating_add(layout_block.height);
        if block_start >= window_end || block_end <= window_start {
            continue;
        }
        let visible_start = block_start.max(window_start);
        let visible_end = block_end.min(window_end);
        let y = area
            .y
            .saturating_add((visible_start.saturating_sub(window_start)) as u16);
        let hit_height =
            (visible_end.saturating_sub(visible_start)).min(usize::from(u16::MAX)) as u16;
        if visible_start == block_start
            && let TranscriptHitTarget::Row(row_id) = layout_block.target
            && ui.transcript[render_blocks[block_index].index]
                .agent_target
                .is_some()
        {
            let open_width = area.width.min(20);
            areas.push((
                TranscriptHitTarget::AgentOpen(row_id),
                Rect {
                    x: area.x.saturating_add(area.width.saturating_sub(open_width)),
                    y,
                    width: open_width,
                    height: 1,
                },
            ));
        }
        areas.push((
            layout_block.target,
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: hit_height,
            },
        ));
        let first_slice_start = *slice_start.get_or_insert(block_start);
        let block_lines = render_block_lines(ui, &render_blocks, block_index, area.width);
        let prompt_surface_rows = if block.kind == TranscriptKind::Prompt {
            let compact_trailing =
                compact_trailing_for_render_block(&render_blocks, block_index, ui);
            block_lines
                .len()
                .saturating_sub(usize::from(!compact_trailing))
        } else {
            0
        };
        for (line_index, line) in block_lines.iter().enumerate() {
            let has_surface_bg = line_index < prompt_surface_rows;
            for _ in 0..wrapped_line_count(std::slice::from_ref(line), area.width) {
                surface_rows.push(has_surface_bg);
            }
        }
        lines.extend(block_lines);
        rendered_height = rendered_height.saturating_add(layout_block.height);
        if first_slice_start.saturating_add(rendered_height) >= window_end {
            break;
        }
    }
    ui.last_entry_areas = areas;
    let paragraph_scroll = window_start.saturating_sub(slice_start.unwrap_or(window_start)) as u16;
    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::BOTTOM))
        .wrap(Wrap { trim: false })
        .scroll((paragraph_scroll, 0));
    frame.render_widget(paragraph, area);
    render_session_identity_separator(frame, area, session_identity);
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
            frame.buffer_mut()[(x, y)].set_bg(tui_theme().surface_bg);
        }
    }
    ui.capture_selectable_rows(
        frame.buffer_mut(),
        transcript_selectable_area(area),
        SelectableRegion::Transcript,
    );
}

fn render_session_identity_separator(
    frame: &mut Frame<'_>,
    area: Rect,
    session_identity: Option<&str>,
) {
    let Some(identity) = session_identity
        .map(str::trim)
        .filter(|identity| !identity.is_empty())
    else {
        return;
    };
    if area.width < 6 || area.height == 0 {
        return;
    }
    let available = area.width.saturating_sub(4) as usize;
    let label = format!(" {} ", truncate_display_width(identity, available));
    let label_width = UnicodeWidthStr::width(label.as_str()) as u16;
    if label_width == 0 || label_width.saturating_add(2) > area.width {
        return;
    }
    let y = area.y.saturating_add(area.height.saturating_sub(1));
    let x = area.x.saturating_add(2);
    let theme = tui_theme();
    let style = theme.dim_style();
    for (offset, ch) in label.chars().enumerate() {
        let x = x.saturating_add(offset as u16);
        if x >= area.x.saturating_add(area.width) {
            break;
        }
        if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
            cell.set_symbol(&ch.to_string());
            cell.set_style(style);
        }
    }
}

fn refresh_transcript_layout(ui: &mut FullscreenUi<'_>, width: u16) {
    let old_blocks = if ui.transcript_layout.width == width
        && ui.transcript_layout.thinking_visible == ui.thinking_visible
        && ui.transcript_layout.raw_visible == ui.raw_visible
    {
        Some(ui.transcript_layout.blocks.as_slice())
    } else {
        None
    };
    let render_blocks = transcript_render_blocks(ui);
    let mut blocks = Vec::with_capacity(render_blocks.len());
    let mut total_height = 0usize;
    #[cfg(test)]
    let mut recomputed_rows = 0usize;
    for (block_index, block) in render_blocks.iter().enumerate() {
        let key = transcript_layout_block_key(ui, &render_blocks, block_index);
        let target = block.target;
        let height = old_blocks
            .and_then(|blocks| blocks.get(block_index))
            .filter(|cached| cached.key == key)
            .map(|cached| cached.height)
            .unwrap_or_else(|| {
                #[cfg(test)]
                {
                    recomputed_rows += 1;
                }
                let lines = render_block_lines(ui, &render_blocks, block_index, width);
                wrapped_line_count(&lines, width)
            });
        blocks.push(TranscriptLayoutBlock {
            key,
            target,
            start: total_height,
            height,
        });
        total_height = total_height.saturating_add(height);
    }
    ui.transcript_layout = TranscriptLayoutCache {
        width,
        thinking_visible: ui.thinking_visible,
        raw_visible: ui.raw_visible,
        blocks,
        total_height,
        #[cfg(test)]
        recomputed_rows,
    };
}

fn transcript_layout_matches_current(ui: &FullscreenUi<'_>, width: u16) -> bool {
    if ui.transcript_layout.width != width
        || ui.transcript_layout.thinking_visible != ui.thinking_visible
        || ui.transcript_layout.raw_visible != ui.raw_visible
    {
        return false;
    }
    let render_blocks = transcript_render_blocks(ui);
    if ui.transcript_layout.blocks.len() != render_blocks.len() {
        return false;
    }
    render_blocks.iter().enumerate().all(|(index, _)| {
        let key = transcript_layout_block_key(ui, &render_blocks, index);
        ui.transcript_layout
            .blocks
            .get(index)
            .is_some_and(|cached| cached.key == key)
    })
}

fn transcript_total_height_for_ui(ui: &FullscreenUi<'_>, width: u16) -> usize {
    let render_blocks = transcript_render_blocks(ui);
    render_blocks
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let lines = render_block_lines(ui, &render_blocks, index, width);
            wrapped_line_count(&lines, width)
        })
        .sum()
}

fn transcript_render_blocks(ui: &FullscreenUi<'_>) -> Vec<TranscriptRenderBlock> {
    ui.transcript
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            row_visible(row, ui.thinking_visible).then_some(TranscriptRenderBlock {
                index,
                target: TranscriptHitTarget::Row(row.id),
                kind: row.kind,
            })
        })
        .collect()
}

fn target_selected(ui: &FullscreenUi<'_>, target: TranscriptHitTarget) -> bool {
    if ui.selected_target == Some(target) {
        return true;
    }
    let row_id = match target {
        TranscriptHitTarget::Row(row_id) | TranscriptHitTarget::AgentOpen(row_id) => row_id,
    };
    ui.selected_row
        .and_then(|index| ui.transcript.get(index))
        .is_some_and(|row| row.id == row_id)
}

fn compact_trailing_for_render_block(
    blocks: &[TranscriptRenderBlock],
    index: usize,
    ui: &FullscreenUi<'_>,
) -> bool {
    let Some(block) = blocks.get(index) else {
        return false;
    };
    let row = &ui.transcript[block.index];
    blocks.get(index + 1).is_some_and(|next| {
        let next_kind = next.kind;
        (matches!(row.kind, TranscriptKind::Prompt | TranscriptKind::Answer)
            && next_kind == TranscriptKind::Meta)
            || (matches!(
                row.kind,
                TranscriptKind::Explored | TranscriptKind::Ran | TranscriptKind::Updated
            ) && matches!(
                next_kind,
                TranscriptKind::Explored | TranscriptKind::Ran | TranscriptKind::Updated
            ))
    })
}

fn render_block_lines(
    ui: &FullscreenUi<'_>,
    blocks: &[TranscriptRenderBlock],
    index: usize,
    width: u16,
) -> Vec<Line<'static>> {
    let Some(block) = blocks.get(index) else {
        return Vec::new();
    };
    let row = &ui.transcript[block.index];
    let target = block.target;
    let selected = target_selected(ui, target);
    let compact_trailing = compact_trailing_for_render_block(blocks, index, ui);
    transcript_lines(
        row,
        selected,
        compact_trailing,
        width,
        &ui.workdir,
        ui.raw_visible,
    )
}

fn transcript_layout_block_key(
    ui: &FullscreenUi<'_>,
    blocks: &[TranscriptRenderBlock],
    index: usize,
) -> TranscriptLayoutBlockKey {
    let block = &blocks[index];
    let target = block.target;
    let selected = target_selected(ui, target);
    let compact_trailing = compact_trailing_for_render_block(blocks, index, ui);
    let row = &ui.transcript[block.index];
    TranscriptLayoutBlockKey {
        target,
        compact_trailing,
        selected,
        row_key: transcript_layout_row_key(row, true, compact_trailing, selected),
    }
}

fn transcript_layout_row_key(
    row: &TranscriptRow,
    visible: bool,
    compact_trailing: bool,
    selected: bool,
) -> TranscriptLayoutRowKey {
    let tool_elapsed = tool_elapsed_label(row);
    let active_tool_marker = active_tool_elapsed(row)
        .map(activity_spinner_frame)
        .unwrap_or("");
    TranscriptLayoutRowKey {
        visible,
        compact_trailing,
        selected,
        kind: row.kind,
        failed: row.failed,
        interrupted: row.interrupted,
        user_shell: row.user_shell,
        agent_tool: is_agent_tool_row(row),
        agent_open: row.agent_target.is_some(),
        expanded: row.expanded,
        details_collapsed: row.details_collapsed,
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
    workdir: &Path,
    raw_visible: bool,
) -> Vec<Line<'static>> {
    if row.kind == TranscriptKind::Prompt {
        return prompt_lines(row, selected, compact_trailing, width);
    }
    if row.kind == TranscriptKind::Answer {
        return answer_lines(row, selected, compact_trailing, width, workdir, raw_visible);
    }
    if row.kind == TranscriptKind::Thinking {
        return thinking_lines(row, selected, compact_trailing, width);
    }
    if row.user_shell {
        return user_shell_lines(row, selected, compact_trailing, width);
    }
    if is_agent_tool_row(row) {
        return tool_lines(row, selected, compact_trailing, width);
    }
    if matches!(
        row.kind,
        TranscriptKind::Explored | TranscriptKind::Ran | TranscriptKind::Updated
    ) {
        return tool_lines(row, selected, compact_trailing, width);
    }
    if row.kind == TranscriptKind::Command {
        return command_lines(row, selected, compact_trailing, width);
    }

    let style = label_style(row.kind, row.failed);
    let marker = if selected { "›" } else { "▌" };
    let marker_style = if selected {
        focus_marker_style(row.failed)
    } else {
        style
    };
    let mut out = Vec::new();
    let title = row.title.trim();
    if !title.is_empty() {
        let suffix = if row.is_expandable() {
            row_expand_hint(row, selected, None)
                .map(|hint| format!(" {hint}"))
                .unwrap_or_default()
        } else {
            String::new()
        };
        out.push(Line::from(vec![
            Span::styled(format!("{marker} "), marker_style),
            Span::styled(
                format!("{title}{suffix}"),
                style.add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    let body_style = style_for_body(row.kind, row.failed);
    for (index, line) in row.expandable_text().lines().enumerate() {
        let mut spans = if row.kind == TranscriptKind::Status {
            context_status_line(line, body_style)
                .map(|line| line.spans)
                .unwrap_or_else(|| vec![Span::styled(line.to_string(), body_style)])
        } else {
            vec![Span::styled(line.to_string(), body_style)]
        };
        if row.kind == TranscriptKind::Prompt {
            spans = spans
                .into_iter()
                .map(|span| span.style(body_style.bg(tui_theme().menu_selected_bg)))
                .collect();
        }
        let prefix = if selected && (!title.is_empty() || index > 0) {
            "  ".to_string()
        } else {
            format!("{marker} ")
        };
        let prefix_style = if selected && (!title.is_empty() || index > 0) {
            tui_theme().dim_style()
        } else {
            marker_style
        };
        let mut row_spans = vec![Span::styled(prefix, prefix_style)];
        row_spans.extend(spans);
        out.push(Line::from(row_spans));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(marker.to_string(), marker_style)));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn command_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let command_style = if row.failed {
        tui_theme().error_style()
    } else {
        Style::default()
    };
    let marker_style = if selected || row.failed {
        focus_marker_style(row.failed)
    } else {
        tui_theme().accent_style()
    };
    let title = if let Some(hint) = row_expand_hint(row, selected, None) {
        format!("{} {hint}", row.title.trim())
    } else {
        row.title.trim().to_string()
    };
    for (index, wrapped) in wrap_command_text(&title, "> ", width)
        .into_iter()
        .enumerate()
    {
        let prefix = if index == 0 { "> " } else { "  " };
        let prefix_style = if index == 0 {
            marker_style
        } else {
            tui_theme().dim_style()
        };
        out.push(Line::from(vec![
            Span::styled(prefix.to_string(), prefix_style),
            Span::styled(wrapped, command_style),
        ]));
    }

    let body_style = if row.failed {
        tui_theme().error_style()
    } else {
        tui_theme().dim_style()
    };
    if row.details_collapsed {
        if !compact_trailing {
            out.push(Line::from(""));
        }
        return out;
    }
    let mut wrote_body = false;
    for (line_index, line) in row.expandable_text().lines().enumerate() {
        let first_prefix = if line_index == 0 { "  └  " } else { "     " };
        if let Some(context_line) = context_status_line(line, body_style) {
            let mut spans = vec![Span::styled(
                first_prefix.to_string(),
                tui_theme().dim_style(),
            )];
            spans.extend(context_line.spans);
            out.push(Line::from(spans));
            wrote_body = true;
            continue;
        }
        for (wrapped_index, wrapped) in wrap_command_text(line, first_prefix, width)
            .into_iter()
            .enumerate()
        {
            let prefix = if wrapped_index == 0 {
                first_prefix
            } else {
                "     "
            };
            let mut row_spans = vec![Span::styled(prefix.to_string(), tui_theme().dim_style())];
            row_spans.push(Span::styled(wrapped, body_style));
            out.push(Line::from(row_spans));
            wrote_body = true;
        }
    }
    if !wrote_body {
        out.push(Line::from(Span::styled(
            "  └".to_string(),
            tui_theme().dim_style(),
        )));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn wrap_command_text(text: &str, prefix: &str, width: u16) -> Vec<String> {
    let content_width = usize::from(width)
        .saturating_sub(UnicodeWidthStr::width(prefix))
        .max(1);
    wrap_display_width(text, content_width)
}

fn wrap_display_width(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if current_width > 0 && current_width.saturating_add(ch_width) > max_width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width = current_width.saturating_add(ch_width);
        if current_width >= max_width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

fn context_status_line(line: &str, body_style: Style) -> Option<Line<'static>> {
    context_status_bar_line(line, body_style)
        .or_else(|| context_status_legend_line(line, body_style))
}

fn context_status_bar_line(line: &str, body_style: Style) -> Option<Line<'static>> {
    let rest = line.strip_prefix('[')?;
    let (cells, suffix) = rest.split_once(']')?;
    if !suffix.is_empty() {
        return None;
    }
    if !cells
        .chars()
        .all(|cell| matches!(cell, 'S' | 'T' | 'K' | 'M' | '.'))
    {
        return None;
    }
    let mut spans = vec![Span::styled("[".to_string(), body_style)];
    spans.extend(cells.chars().map(|cell| {
        Span::styled(
            cell.to_string(),
            context_status_marker_style(cell).unwrap_or(body_style),
        )
    }));
    spans.push(Span::styled("]".to_string(), body_style));
    Some(Line::from(spans))
}

fn context_status_legend_line(line: &str, body_style: Style) -> Option<Line<'static>> {
    if line != "S system  T tools  K skills  M input_messages  . free" {
        return None;
    }
    let mut spans = Vec::new();
    for (marker, label) in [
        ('S', " system"),
        ('T', " tools"),
        ('K', " skills"),
        ('M', " input_messages"),
        ('.', " free"),
    ] {
        if !spans.is_empty() {
            spans.push(Span::styled("  ".to_string(), body_style));
        }
        spans.push(Span::styled(
            marker.to_string(),
            context_status_marker_style(marker).unwrap_or(body_style),
        ));
        spans.push(Span::styled(label.to_string(), body_style));
    }
    Some(Line::from(spans))
}

fn context_status_marker_style(marker: char) -> Option<Style> {
    let theme = tui_theme();
    match marker {
        'S' => Some(theme.identity_style()),
        'T' => Some(theme.accent_style()),
        'K' => Some(theme.thinking_style()),
        'M' => Some(theme.success_style()),
        '.' => Some(theme.dim_style()),
        _ => None,
    }
}

fn prompt_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let body_style = style_for_body(row.kind, row.failed).bg(tui_theme().surface_bg);
    for (index, line) in row.expandable_text().lines().enumerate() {
        let first_prefix = if index == 0 { "› " } else { "  " };
        let continuation_prefix = "  ";
        for (wrapped_index, wrapped) in wrap_prompt_text(line, first_prefix, width)
            .into_iter()
            .enumerate()
        {
            let prefix = if wrapped_index == 0 {
                first_prefix
            } else {
                continuation_prefix
            };
            let prefix_style = if selected && index == 0 && wrapped_index == 0 {
                focus_marker_style(row.failed)
            } else {
                tui_theme().dim_style()
            };
            out.push(prompt_line(
                prefix,
                &wrapped,
                width,
                body_style,
                prefix_style,
            ));
        }
    }
    if out.is_empty() {
        let prefix_style = if selected {
            focus_marker_style(row.failed)
        } else {
            tui_theme().dim_style()
        };
        out.push(prompt_line("› ", "", width, body_style, prefix_style));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn prompt_line(
    prefix: &str,
    text: &str,
    width: u16,
    style: Style,
    prefix_style: Style,
) -> Line<'static> {
    let content_width = UnicodeWidthStr::width(prefix).saturating_add(UnicodeWidthStr::width(text));
    let padding = usize::from(width).saturating_sub(content_width);
    Line::from(vec![
        Span::styled(prefix.to_string(), prefix_style.bg(tui_theme().surface_bg)),
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

fn user_shell_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let theme = tui_theme();
    let command_style = style_for_body(TranscriptKind::Prompt, row.failed).bg(theme.surface_bg);
    let marker_style = theme.accent_style();
    let mut command = user_shell_command_text(row);
    if let Some(hint) = row_expand_hint(row, selected, None) {
        command.push(' ');
        command.push_str(&hint);
    }
    let mut out = Vec::new();
    for (index, wrapped) in wrap_prompt_text(&command, "! ", width)
        .into_iter()
        .enumerate()
    {
        let (prefix, prefix_style) = if index == 0 {
            ("! ", marker_style)
        } else {
            ("  ", theme.dim_style())
        };
        out.push(prompt_line(
            prefix,
            &wrapped,
            width,
            command_style,
            prefix_style,
        ));
    }

    if !row.details_collapsed {
        let body_style = if row.interrupted {
            interruption_style()
        } else {
            style_for_body(row.kind, row.failed)
        };
        let mut body_lines = row
            .expandable_text()
            .lines()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if row.is_expandable()
            && !row.expanded
            && body_lines
                .last()
                .is_some_and(|line| collapsed_more_line_count(line).is_some())
        {
            body_lines.pop();
        }
        for (line_index, line) in body_lines.into_iter().enumerate() {
            let first_prefix = if line_index == 0 { "  └ " } else { "    " };
            for (wrapped_index, wrapped) in wrap_command_text(&line, first_prefix, width)
                .into_iter()
                .enumerate()
            {
                let prefix = if wrapped_index == 0 {
                    first_prefix
                } else {
                    "    "
                };
                out.push(Line::from(vec![
                    Span::styled(prefix.to_string(), theme.dim_style()),
                    Span::styled(wrapped, body_style),
                ]));
            }
        }
    }

    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn user_shell_command_text(row: &TranscriptRow) -> String {
    let title = row.title.trim();
    for prefix in ["Running ! ", "Ran ! ", "! ", "Running ", "Ran "] {
        if let Some(command) = title.strip_prefix(prefix) {
            return command.trim().to_string();
        }
    }
    title.to_string()
}

fn wrap_detail_text(text: &str, prefix: &str, width: u16) -> Vec<String> {
    let max_width = usize::from(width)
        .saturating_sub(UnicodeWidthStr::width(prefix))
        .saturating_sub(1)
        .max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for segment in text.split_inclusive(' ') {
        let segment_width = UnicodeWidthStr::width(segment);
        if current_width > 0 && current_width.saturating_add(segment_width) > max_width {
            lines.push(current.trim_end().to_string());
            current.clear();
            current_width = 0;
        }
        if segment_width > max_width {
            if !current.is_empty() {
                lines.push(current.trim_end().to_string());
                current.clear();
                current_width = 0;
            }
            lines.extend(wrap_prompt_text(segment.trim_start(), "", max_width as u16));
            continue;
        }
        current.push_str(segment.trim_start_matches(|ch| current_width == 0 && ch == ' '));
        current_width = UnicodeWidthStr::width(current.as_str());
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current.trim_end().to_string());
    }
    lines
}

fn answer_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
    workdir: &Path,
    raw_visible: bool,
) -> Vec<Line<'static>> {
    let body_style = style_for_body(row.kind, row.failed);
    let mut out = if raw_visible {
        raw_markdown_source_lines(row.expandable_text(), body_style)
    } else {
        render_markdown_lines(row.expandable_text(), workdir, Some(width))
    };
    if out.is_empty() {
        out.extend(
            row.expandable_text()
                .lines()
                .map(|line| Line::from(Span::styled(line.to_string(), body_style))),
        );
    }
    if selected && let Some(line) = out.first_mut() {
        line.spans.insert(
            0,
            Span::styled("› ".to_string(), focus_marker_style(row.failed)),
        );
    }
    if out.is_empty() && selected {
        out.push(Line::from(Span::styled(
            "›".to_string(),
            focus_marker_style(row.failed),
        )));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn raw_markdown_source_lines(input: &str, style: Style) -> Vec<Line<'static>> {
    input
        .lines()
        .map(|line| Line::from(Span::styled(line.to_string(), style)))
        .collect()
}

struct LedgerEvidenceRowView {
    marker: String,
    marker_style: Style,
    title: String,
    title_style: Style,
    elapsed: Option<String>,
    expand_hint: Option<String>,
    body_lines: Vec<String>,
    body_style: Style,
    compact_trailing: bool,
}

fn ledger_evidence_lines(view: LedgerEvidenceRowView, width: u16) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    if !view.title.is_empty() {
        out.push(ledger_title_line(
            &view.marker,
            view.marker_style,
            view.title.as_str(),
            view.title_style,
            view.elapsed.as_deref(),
            view.expand_hint.as_deref(),
            width,
        ));
    }
    for (index, line) in view.body_lines.into_iter().enumerate() {
        let prefix = if index == 0 { "  └ " } else { "    " };
        out.push(Line::from(vec![
            Span::styled(prefix.to_string(), tui_theme().dim_style()),
            Span::styled(line, view.body_style),
        ]));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(view.marker, view.marker_style)));
    }
    if !view.compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn thinking_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    ledger_evidence_lines(thinking_ledger_view(row, selected, compact_trailing), width)
}

fn thinking_ledger_view(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
) -> LedgerEvidenceRowView {
    let active_elapsed = active_tool_elapsed(row);
    let marker_style = if row.failed {
        tui_theme().error_style()
    } else if selected || active_elapsed.is_some() {
        tui_theme().accent_style()
    } else {
        tui_theme().success_style()
    };
    let marker = if selected {
        "› ".to_string()
    } else if let Some(elapsed) = active_elapsed {
        format!("{} ", activity_spinner_frame(elapsed))
    } else {
        "• ".to_string()
    };
    let title_style = if row.failed {
        tui_theme().error_style().add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    let hint = row_expand_hint(row, selected, None);
    let body_style = style_for_body(row.kind, row.failed);
    let title = row.title.trim();
    let title = if title.is_empty() { "Thinking" } else { title };
    LedgerEvidenceRowView {
        marker,
        marker_style,
        title: title.to_string(),
        title_style,
        elapsed: active_elapsed.map(format_duration_compact),
        expand_hint: hint,
        body_lines: thinking_body_lines(row),
        body_style,
        compact_trailing,
    }
}

fn thinking_body_lines(row: &TranscriptRow) -> Vec<String> {
    let mut lines = Vec::new();
    append_expandable_evidence_body(&mut lines, row);
    lines
}

fn tool_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    ledger_evidence_lines(
        tool_ledger_view(row, selected, compact_trailing, width),
        width,
    )
}

fn is_agent_tool_row(row: &TranscriptRow) -> bool {
    row.tool_name.as_deref() == Some("Agent")
}

fn tool_ledger_view(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> LedgerEvidenceRowView {
    let phase = ToolRowPhase::from_row(row);
    let active_elapsed = (phase == ToolRowPhase::Active)
        .then(|| active_tool_elapsed(row))
        .flatten();
    let bullet_style = if row.interrupted {
        interruption_style()
    } else if row.failed {
        tui_theme().error_style()
    } else if active_elapsed.is_some() || selected {
        tui_theme().accent_style()
    } else {
        tui_theme().success_style()
    };
    let title_style = if row.interrupted {
        interruption_style().add_modifier(Modifier::BOLD)
    } else if row.failed {
        tui_theme().error_style().add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    let body_style = if row.interrupted {
        interruption_style()
    } else {
        style_for_body(row.kind, row.failed)
    };
    let marker = if selected {
        "› ".to_string()
    } else if let Some(elapsed) = active_elapsed {
        format!("{} ", activity_spinner_frame(elapsed))
    } else {
        "• ".to_string()
    };
    let title = tool_display_title(row, phase);
    let title_detail = tool_title_detail(row, title.as_str());
    let elapsed = tool_elapsed_label(row);
    let hint = row_expand_hint(row, selected, title_detail.as_deref());
    LedgerEvidenceRowView {
        marker,
        marker_style: bullet_style,
        title,
        title_style,
        elapsed,
        expand_hint: hint,
        body_lines: tool_body_lines(row, phase, title_detail, width),
        body_style,
        compact_trailing,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolRowPhase {
    Active,
    Completed,
}

impl ToolRowPhase {
    fn from_row(row: &TranscriptRow) -> Self {
        if !row.failed && active_tool_elapsed(row).is_some() {
            Self::Active
        } else {
            Self::Completed
        }
    }
}

fn tool_display_title(row: &TranscriptRow, phase: ToolRowPhase) -> String {
    let title = row.title.trim();
    if phase != ToolRowPhase::Active {
        return title.to_string();
    }
    let Some((active_prefix, completed_prefix, fallback)) = tool_title_prefixes(row.kind) else {
        return title.to_string();
    };
    if title.starts_with(active_prefix) {
        return title.to_string();
    }
    let rest = title.strip_prefix(completed_prefix).unwrap_or(title).trim();
    if rest.is_empty() {
        fallback.to_string()
    } else {
        format!("{active_prefix} {rest}")
    }
}

fn tool_title_prefixes(kind: TranscriptKind) -> Option<(&'static str, &'static str, &'static str)> {
    match kind {
        TranscriptKind::Explored => Some(("Exploring", "Explored", "Exploring")),
        TranscriptKind::Ran => Some(("Running", "Ran", "Running command")),
        TranscriptKind::Updated => Some(("Updating", "Updated", "Updating files")),
        _ => None,
    }
}

fn foldable_evidence_body(row: &TranscriptRow) -> bool {
    matches!(
        row.kind,
        TranscriptKind::Thinking
            | TranscriptKind::Explored
            | TranscriptKind::Ran
            | TranscriptKind::Updated
            | TranscriptKind::Command
    ) && !row.text.trim().is_empty()
        && !row.interrupted
        && !suppressed_active_tool_body(row, &[row.text.trim()])
}

fn foldable_tool_title(row: &TranscriptRow) -> bool {
    !row.user_shell
        && matches!(row.kind, TranscriptKind::Ran)
        && tool_title_detail(row, &row.title).is_some()
}

fn toggle_transcript_row_details(row: &mut TranscriptRow) {
    if row.details_collapsed {
        row.details_collapsed = false;
        return;
    }
    if row.full_text.as_ref().is_some_and(|full| full != &row.text) || foldable_tool_title(row) {
        row.expanded = !row.expanded;
        return;
    }
    if foldable_evidence_body(row) {
        row.details_collapsed = true;
    }
}

const TOOL_TITLE_DETAIL_WIDTH: usize = 80;

fn tool_title_detail(row: &TranscriptRow, title: &str) -> Option<String> {
    if row.kind != TranscriptKind::Ran || row.user_shell {
        return None;
    }
    let command = title
        .trim()
        .strip_prefix("Running ")
        .or_else(|| title.trim().strip_prefix("Ran "))
        .unwrap_or_else(|| title.trim());
    if UnicodeWidthStr::width(command) <= TOOL_TITLE_DETAIL_WIDTH {
        return None;
    }
    Some(format!("command: {command}"))
}

fn suppressed_active_tool_body(row: &TranscriptRow, lines: &[&str]) -> bool {
    ToolRowPhase::from_row(row) == ToolRowPhase::Active
        && lines.len() == 1
        && matches!(lines[0].trim(), "running" | "preparing")
}

fn tool_body_lines(
    row: &TranscriptRow,
    phase: ToolRowPhase,
    title_detail: Option<String>,
    width: u16,
) -> Vec<String> {
    let mut lines = Vec::new();
    if row.expanded
        && let Some(title_detail) = title_detail
    {
        lines.extend(wrap_detail_text(&title_detail, "  └ ", width));
    }
    append_expandable_evidence_body(&mut lines, row);
    if phase == ToolRowPhase::Active {
        lines.retain(|line| !matches!(line.trim(), "running" | "preparing"));
    }
    lines
}

fn append_expandable_evidence_body(lines: &mut Vec<String>, row: &TranscriptRow) {
    if row.details_collapsed {
        return;
    }
    lines.extend(row.expandable_text().lines().map(ToOwned::to_owned));
    if row.is_expandable()
        && !row.expanded
        && lines
            .last()
            .is_some_and(|line| collapsed_more_line_count(line).is_some())
    {
        lines.pop();
    }
}

fn row_expand_hint(
    row: &TranscriptRow,
    selected: bool,
    title_detail: Option<&str>,
) -> Option<String> {
    let expand_hint = if !row.is_expandable() {
        None
    } else if row.details_collapsed {
        Some("▸ details".to_string())
    } else if row.expanded {
        Some("▾ collapse".to_string())
    } else if let Some(count) = omitted_line_count(row) {
        Some(format!("▸ {count} more lines"))
    } else if row_has_collapsed_body(row) {
        Some("▸ more output".to_string())
    } else if title_detail.is_some() {
        Some("▸ command".to_string())
    } else {
        selected.then(|| "▾ collapse".to_string())
    };
    if row.agent_target.is_some() {
        return Some(match expand_hint {
            Some(hint) => format!("Open  {hint}"),
            None => "Open".to_string(),
        });
    }
    expand_hint
}

fn omitted_line_count(row: &TranscriptRow) -> Option<usize> {
    if let Some(count) = row.text.lines().find_map(collapsed_more_line_count) {
        return Some(count);
    }
    if row.text.trim_end().ends_with('…') {
        return None;
    }
    None
}

fn collapsed_more_line_count(line: &str) -> Option<usize> {
    line.trim()
        .strip_prefix("... ")
        .and_then(|value| value.strip_suffix(" more lines"))
        .and_then(|value| value.parse::<usize>().ok())
}

fn row_has_collapsed_body(row: &TranscriptRow) -> bool {
    row.full_text.as_ref().is_some_and(|full| full != &row.text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LedgerBodyCollapsePolicy {
    head_lines: usize,
    tail_lines: usize,
    max_tokens: usize,
    max_width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LedgerBodyCollapse {
    preview: String,
    full_text: Option<String>,
}

const LEDGER_BODY_COLLAPSE_HEAD_LINES: usize = 2;
const LEDGER_BODY_COLLAPSE_TAIL_LINES: usize = 4;
const LEDGER_BODY_COLLAPSE_TOKENS: usize = 200;
const LEDGER_BODY_COLLAPSE_WIDTH: usize = 1200;
const DISPLAY_TOKEN_LONG_RUN_FREE_CELLS: usize = 16;
const DISPLAY_TOKEN_CHUNK_CELLS: usize = 4;

fn ledger_body_collapse_policy() -> LedgerBodyCollapsePolicy {
    LedgerBodyCollapsePolicy {
        head_lines: LEDGER_BODY_COLLAPSE_HEAD_LINES,
        tail_lines: LEDGER_BODY_COLLAPSE_TAIL_LINES,
        max_tokens: LEDGER_BODY_COLLAPSE_TOKENS,
        max_width: LEDGER_BODY_COLLAPSE_WIDTH,
    }
}

impl LedgerBodyCollapsePolicy {
    fn should_collapse(self, text: &str) -> bool {
        text.lines().count() > self.head_lines.saturating_add(self.tail_lines)
            || display_token_count(text) > self.max_tokens
            || UnicodeWidthStr::width(text) > self.max_width
    }

    fn collapse(self, text: &str) -> LedgerBodyCollapse {
        let lines = text.lines().collect::<Vec<_>>();
        let max_lines = self.head_lines.saturating_add(self.tail_lines);
        if lines.len() > max_lines {
            let collapsed = middle_fold_lines(&lines, self.head_lines, self.tail_lines);
            if display_token_count(&collapsed) > self.max_tokens
                || UnicodeWidthStr::width(collapsed.as_str()) > self.max_width
            {
                return self.collapse_by_token_or_width(text);
            }
            return LedgerBodyCollapse {
                preview: collapsed,
                full_text: Some(text.to_string()),
            };
        }
        if display_token_count(text) > self.max_tokens
            || UnicodeWidthStr::width(text) > self.max_width
        {
            return self.collapse_by_token_or_width(text);
        }
        LedgerBodyCollapse {
            preview: text.to_string(),
            full_text: None,
        }
    }

    fn collapse_by_token_or_width(self, text: &str) -> LedgerBodyCollapse {
        let mut preview = text.to_string();
        if display_token_count(text) > self.max_tokens {
            preview = middle_fold_display_tokens(text, self.max_tokens);
        }
        if UnicodeWidthStr::width(preview.as_str()) > self.max_width {
            preview = middle_fold_display_width(&preview, self.max_width);
        }
        LedgerBodyCollapse {
            preview,
            full_text: Some(text.to_string()),
        }
    }
}

fn middle_fold_lines(lines: &[&str], head_lines: usize, tail_lines: usize) -> String {
    let visible = head_lines.saturating_add(tail_lines);
    if lines.len() <= visible {
        return lines.join("\n");
    }
    let omitted = lines.len().saturating_sub(visible);
    let mut preview = lines
        .iter()
        .take(head_lines)
        .map(|line| (*line).to_string())
        .collect::<Vec<_>>();
    preview.push(format!("... {omitted} more lines"));
    preview.extend(
        lines
            .iter()
            .skip(lines.len().saturating_sub(tail_lines))
            .map(|line| (*line).to_string()),
    );
    preview.join("\n")
}

fn middle_fold_display_tokens(text: &str, max_tokens: usize) -> String {
    if display_token_count(text) <= max_tokens {
        return text.to_string();
    }
    if max_tokens == 0 {
        return "…".to_string();
    }
    let content_budget = max_tokens.saturating_sub(8).max(1);
    let head_budget = (content_budget / 3).max(1).min(content_budget);
    let tail_budget = content_budget.saturating_sub(head_budget).max(1);
    let head = trim_trailing_ellipsis(truncate_display_tokens(text, head_budget));
    let tail = suffix_display_tokens(text, tail_budget);
    join_middle_fold_parts(&head, &tail)
}

fn middle_fold_display_width(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return "…".to_string();
    }
    let content_budget = max_width.saturating_sub(20).max(1);
    let head_budget = (content_budget / 3).max(1).min(content_budget);
    let tail_budget = content_budget.saturating_sub(head_budget).max(1);
    let head = prefix_display_width(text, head_budget);
    let tail = suffix_display_width(text, tail_budget);
    join_middle_fold_parts(&head, &tail)
}

fn join_middle_fold_parts(head: &str, tail: &str) -> String {
    if head.contains('\n') || tail.contains('\n') {
        format!(
            "{}\n... omitted middle\n{}",
            head.trim_end(),
            tail.trim_start()
        )
    } else {
        format!("{}…{}", head.trim_end(), tail.trim_start())
    }
}

fn trim_trailing_ellipsis(mut text: String) -> String {
    while text.ends_with('…') {
        text.pop();
    }
    text.trim_end().to_string()
}

fn display_token_count(text: &str) -> usize {
    text.split_whitespace()
        .map(display_token_count_segment)
        .sum()
}

fn display_token_count_segment(segment: &str) -> usize {
    if segment.is_empty() {
        return 0;
    }
    let width = UnicodeWidthStr::width(segment);
    if width <= DISPLAY_TOKEN_LONG_RUN_FREE_CELLS {
        1
    } else {
        1 + width
            .saturating_sub(DISPLAY_TOKEN_LONG_RUN_FREE_CELLS)
            .div_ceil(DISPLAY_TOKEN_CHUNK_CELLS)
    }
}

fn truncate_display_tokens(text: &str, max_tokens: usize) -> String {
    if display_token_count(text) <= max_tokens {
        return text.to_string();
    }
    if max_tokens == 0 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut token_count = 0usize;
    let mut index = 0usize;
    while index < text.len() {
        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        let start = index;
        let whitespace = ch.is_whitespace();
        index += ch.len_utf8();
        while index < text.len() {
            let Some(next) = text[index..].chars().next() else {
                break;
            };
            if next.is_whitespace() != whitespace {
                break;
            }
            index += next.len_utf8();
        }
        let segment = &text[start..index];
        if whitespace {
            if !out.is_empty() && token_count < max_tokens {
                out.push_str(segment);
            }
            continue;
        }
        let segment_tokens = display_token_count_segment(segment);
        if token_count.saturating_add(segment_tokens) <= max_tokens {
            out.push_str(segment);
            token_count = token_count.saturating_add(segment_tokens);
        } else {
            let remaining = max_tokens.saturating_sub(token_count);
            if remaining > 0 {
                let width = DISPLAY_TOKEN_LONG_RUN_FREE_CELLS
                    + remaining.saturating_sub(1) * DISPLAY_TOKEN_CHUNK_CELLS;
                out.push_str(&prefix_display_width(segment, width));
            }
            break;
        }
    }
    format!("{}…", out.trim_end())
}

fn prefix_display_width(value: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut width = 0usize;
    for ch in value.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if width.saturating_add(ch_width) > max_width {
            break;
        }
        out.push(ch);
        width = width.saturating_add(ch_width);
    }
    out
}

fn suffix_display_width(value: &str, max_width: usize) -> String {
    let mut chars = Vec::new();
    let mut width = 0usize;
    for ch in value.chars().rev() {
        let ch_width = ch.width().unwrap_or(0);
        if width.saturating_add(ch_width) > max_width {
            break;
        }
        chars.push(ch);
        width = width.saturating_add(ch_width);
    }
    chars.into_iter().rev().collect()
}

fn suffix_display_tokens(text: &str, max_tokens: usize) -> String {
    if display_token_count(text) <= max_tokens {
        return text.to_string();
    }
    if max_tokens == 0 {
        return String::new();
    }
    let mut out = Vec::new();
    let mut token_count = 0usize;
    let mut index = text.len();
    while index > 0 {
        let Some((mut start, ch)) = text[..index].char_indices().next_back() else {
            break;
        };
        let whitespace = ch.is_whitespace();
        while start > 0 {
            let Some((prev_start, prev)) = text[..start].char_indices().next_back() else {
                break;
            };
            if prev.is_whitespace() != whitespace {
                break;
            }
            start = prev_start;
        }
        let segment = &text[start..index];
        index = start;
        if whitespace {
            if !out.is_empty() && token_count < max_tokens {
                out.push(segment.to_string());
            }
            continue;
        }
        let segment_tokens = display_token_count_segment(segment);
        if token_count.saturating_add(segment_tokens) <= max_tokens {
            out.push(segment.to_string());
            token_count = token_count.saturating_add(segment_tokens);
        } else {
            let remaining = max_tokens.saturating_sub(token_count);
            if remaining > 0 {
                let width = DISPLAY_TOKEN_LONG_RUN_FREE_CELLS
                    + remaining.saturating_sub(1) * DISPLAY_TOKEN_CHUNK_CELLS;
                out.push(suffix_display_width(segment, width));
            }
            break;
        }
    }
    out.into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("")
        .trim_start()
        .to_string()
}

fn collapse_ledger_body(text: &str) -> (String, Option<String>) {
    let collapsed = ledger_body_collapse_policy().collapse(text);
    (collapsed.preview, collapsed.full_text)
}

fn ledger_title_line(
    marker: &str,
    marker_style: Style,
    title: &str,
    title_style: Style,
    elapsed: Option<&str>,
    expand_hint: Option<&str>,
    width: u16,
) -> Line<'static> {
    let line_width = width.saturating_sub(1);
    let right_text = ledger_title_right_text(expand_hint, elapsed, line_width, marker);
    if right_text.is_empty() {
        return Line::from(vec![
            Span::styled(marker.to_string(), marker_style),
            Span::styled(title.to_string(), title_style),
        ]);
    };
    let marker_width = UnicodeWidthStr::width(marker);
    let width = usize::from(line_width);
    let right_width = UnicodeWidthStr::width(right_text.as_str());
    let separator_width = usize::from(right_width > 0);
    let title_width = width
        .saturating_sub(marker_width)
        .saturating_sub(right_width)
        .saturating_sub(separator_width);
    let title = truncate_display_width(title, title_width);
    let padding = width
        .saturating_sub(marker_width)
        .saturating_sub(UnicodeWidthStr::width(title.as_str()))
        .saturating_sub(right_width);
    Line::from(vec![
        Span::styled(marker.to_string(), marker_style),
        Span::styled(title, title_style),
        Span::raw(" ".repeat(padding)),
        Span::styled(right_text, tui_theme().dim_style()),
    ])
}

fn ledger_title_right_text(
    expand_hint: Option<&str>,
    elapsed: Option<&str>,
    width: u16,
    marker: &str,
) -> String {
    let budget = usize::from(width).saturating_sub(UnicodeWidthStr::width(marker));
    if budget == 0 {
        return String::new();
    }
    let elapsed = elapsed
        .filter(|value| !value.is_empty())
        .map(|value| truncate_display_width(value, budget));
    let Some(hint) = expand_hint.filter(|value| !value.is_empty()) else {
        return elapsed.unwrap_or_default();
    };
    let Some(elapsed) = elapsed else {
        return fit_expand_hint(hint, budget);
    };
    let elapsed_width = UnicodeWidthStr::width(elapsed.as_str());
    if elapsed_width >= budget {
        return elapsed;
    }
    let separator = if budget.saturating_sub(elapsed_width) >= 2 {
        "  "
    } else {
        " "
    };
    let hint_budget = budget
        .saturating_sub(elapsed_width)
        .saturating_sub(UnicodeWidthStr::width(separator));
    let hint = fit_expand_hint(hint, hint_budget);
    if hint.is_empty() {
        elapsed
    } else {
        format!("{hint}{separator}{elapsed}")
    }
}

fn fit_expand_hint(hint: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(hint) <= max_width {
        return hint.to_string();
    }
    if let Some(rest) = hint.strip_prefix("▸ ")
        && let Some(count) = rest.split_whitespace().next()
    {
        let compact = format!("▸ {count}");
        if UnicodeWidthStr::width(compact.as_str()) <= max_width {
            return compact;
        }
    }
    let tiny = if hint.starts_with('▾') {
        "▾"
    } else {
        "▸"
    };
    if UnicodeWidthStr::width(tiny) <= max_width {
        return tiny.to_string();
    }
    String::new()
}

fn tool_elapsed_label(row: &TranscriptRow) -> Option<String> {
    row.tool_elapsed
        .or_else(|| active_tool_elapsed(row))
        .map(format_duration_compact)
}

fn active_tool_elapsed(row: &TranscriptRow) -> Option<Duration> {
    if row.failed || row.interrupted || row.tool_elapsed.is_some() {
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
    let theme = tui_theme();
    if failed {
        return theme.error_style();
    }
    match kind {
        TranscriptKind::Prompt
        | TranscriptKind::Explored
        | TranscriptKind::Ran
        | TranscriptKind::Updated => theme.accent_style(),
        TranscriptKind::Answer => theme.identity_style(),
        TranscriptKind::Thinking => theme.thinking_style(),
        TranscriptKind::Meta => theme.dim_style(),
        TranscriptKind::Command => theme.accent_style(),
        TranscriptKind::Status => theme.accent_style(),
        TranscriptKind::Error => theme.error_style(),
    }
}

fn focus_marker_style(failed: bool) -> Style {
    if failed {
        tui_theme().error_style()
    } else {
        tui_theme().accent_style()
    }
}

fn interruption_style() -> Style {
    tui_theme().thinking_style()
}

fn style_for_body(kind: TranscriptKind, failed: bool) -> Style {
    let theme = tui_theme();
    if failed {
        return theme.error_style();
    }
    match kind {
        TranscriptKind::Thinking => theme.dim_style(),
        TranscriptKind::Meta | TranscriptKind::Command | TranscriptKind::Status => {
            theme.dim_style()
        }
        TranscriptKind::Error => theme.error_style(),
        _ => Style::default(),
    }
}
