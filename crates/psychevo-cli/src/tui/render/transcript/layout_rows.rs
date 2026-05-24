#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn transcript_selectable_area(area: Rect) -> Rect {
    Rect {
        height: transcript_viewport_height(area),
        ..area
    }
}

pub(crate) fn transcript_viewport_height(area: Rect) -> u16 {
    area.height.saturating_sub(1)
}

pub(crate) fn render_active_selection(frame: &mut Frame<'_>, ui: &FullscreenUi<'_>) {
    apply_selection_highlight(frame.buffer_mut(), &ui.screen_lines, &ui.selection);
}

pub(crate) fn apply_selection_highlight(
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
                    buffer_cell.set_style(text_selection_style());
                }
            }
        }
    }
}

pub(crate) fn render_transcript(
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
            && let Some(open_rect) = agent_open_hit_rect(ui, area, y, &render_blocks, block_index)
        {
            areas.push((open_rect.0, open_rect.1));
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

pub(crate) fn agent_open_hit_rect(
    ui: &FullscreenUi<'_>,
    area: Rect,
    y: u16,
    render_blocks: &[TranscriptRenderBlock],
    block_index: usize,
) -> Option<(TranscriptHitTarget, Rect)> {
    let layout_block = ui.transcript_layout.blocks.get(block_index)?;
    let TranscriptHitTarget::Row(row_id) = layout_block.target else {
        return None;
    };
    let row = &ui.transcript[render_blocks.get(block_index)?.index];
    row.agent_target.as_ref()?;
    let selected = target_selected(ui, layout_block.target);
    let phase = ToolRowPhase::from_row(row);
    let active_elapsed = (phase == ToolRowPhase::Active)
        .then(|| active_tool_elapsed(row))
        .flatten();
    let marker = if selected {
        "› ".to_string()
    } else if let Some(elapsed) = active_elapsed {
        format!("{} ", activity_spinner_frame(elapsed))
    } else {
        "• ".to_string()
    };
    let title = tool_display_title(row, phase);
    let title_detail = tool_title_detail(row, title.as_str());
    let hint = row_expand_hint(row, selected, title_detail.as_deref())?;
    let elapsed = tool_elapsed_label(row);
    let line_width = area.width.saturating_sub(1);
    let right_text =
        ledger_title_right_text(Some(hint.as_str()), elapsed.as_deref(), line_width, &marker);
    if !right_text.starts_with("Open") {
        return None;
    }
    let right_width = UnicodeWidthStr::width(right_text.as_str()).min(usize::from(u16::MAX)) as u16;
    let open_width = UnicodeWidthStr::width("Open").min(usize::from(u16::MAX)) as u16;
    if right_width == 0 || open_width == 0 {
        return None;
    }
    Some((
        TranscriptHitTarget::AgentOpen(row_id),
        Rect {
            x: area
                .x
                .saturating_add(line_width.saturating_sub(right_width)),
            y,
            width: open_width,
            height: 1,
        },
    ))
}

pub(crate) fn render_session_identity_separator(
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

pub(crate) fn refresh_transcript_layout(ui: &mut FullscreenUi<'_>, width: u16) {
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

pub(crate) fn transcript_layout_matches_current(ui: &FullscreenUi<'_>, width: u16) -> bool {
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

pub(crate) fn transcript_total_height_for_ui(ui: &FullscreenUi<'_>, width: u16) -> usize {
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

pub(crate) fn transcript_render_blocks(ui: &FullscreenUi<'_>) -> Vec<TranscriptRenderBlock> {
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

pub(crate) fn target_selected(ui: &FullscreenUi<'_>, target: TranscriptHitTarget) -> bool {
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

pub(crate) fn compact_trailing_for_render_block(
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

pub(crate) fn render_block_lines(
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

pub(crate) fn transcript_layout_block_key(
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

pub(crate) fn transcript_layout_row_key(
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

pub(crate) fn hash_layout_text(value: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&value, &mut hasher);
    std::hash::Hasher::finish(&hasher)
}

pub(crate) fn transcript_lines(
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
    if row.kind == TranscriptKind::Status {
        return status_lines(row, selected, compact_trailing, width);
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

pub(crate) fn status_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let theme = tui_theme();
    let marker = if selected { "› " } else { "· " };
    let marker_style = if selected {
        focus_marker_style(row.failed)
    } else if row.failed {
        theme.error_style()
    } else {
        theme.dim_style()
    };
    let body_style = style_for_body(row.kind, row.failed);
    let title = row.title.trim();
    let show_title = !title.is_empty() && title != default_title(TranscriptKind::Status);
    let mut out = Vec::new();
    if show_title {
        let title_style = if row.failed {
            theme.error_style().add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        out.push(ledger_title_line(
            marker,
            marker_style,
            title,
            title_style,
            None,
            row_expand_hint(row, selected, None).as_deref(),
            width,
        ));
    }

    let mut body_lines = Vec::new();
    if show_title {
        append_expandable_evidence_body(&mut body_lines, row);
    } else if !row.details_collapsed {
        body_lines.extend(row.expandable_text().lines().map(ToOwned::to_owned));
    }
    for (index, line) in body_lines.into_iter().enumerate() {
        let prefix = if show_title {
            if index == 0 { "  └ " } else { "    " }
        } else if index == 0 {
            marker
        } else {
            "  "
        };
        let prefix_style = if !show_title && index == 0 {
            marker_style
        } else {
            theme.dim_style()
        };
        let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
        if let Some(context_line) = context_status_line(&line, body_style) {
            spans.extend(context_line.spans);
        } else {
            spans.push(Span::styled(line, body_style));
        }
        out.push(Line::from(spans));
    }

    if out.is_empty() {
        out.push(Line::from(Span::styled(
            marker.trim_end().to_string(),
            marker_style,
        )));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

pub(crate) fn command_lines(
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

pub(crate) fn wrap_command_text(text: &str, prefix: &str, width: u16) -> Vec<String> {
    let content_width = usize::from(width)
        .saturating_sub(UnicodeWidthStr::width(prefix))
        .max(1);
    wrap_display_width(text, content_width)
}

pub(crate) fn wrap_display_width(text: &str, max_width: usize) -> Vec<String> {
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

pub(crate) fn context_status_line(line: &str, body_style: Style) -> Option<Line<'static>> {
    context_status_bar_line(line, body_style)
        .or_else(|| context_status_legend_line(line, body_style))
}

pub(crate) fn context_status_bar_line(line: &str, body_style: Style) -> Option<Line<'static>> {
    let rest = line.strip_prefix('[')?;
    let (cells, suffix) = rest.split_once(']')?;
    if !suffix.is_empty() {
        return None;
    }
    if !cells
        .chars()
        .all(|cell| matches!(cell, 'B' | 'D' | 'P' | 'H' | 'C' | 'U' | 'T' | '.'))
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

pub(crate) fn context_status_legend_line(line: &str, body_style: Style) -> Option<Line<'static>> {
    if line != "B base  D developer  P project  H history  C turn  U prompt  T tools  . free" {
        return None;
    }
    let mut spans = Vec::new();
    for (marker, label) in [
        ('B', " base"),
        ('D', " developer"),
        ('P', " project"),
        ('H', " history"),
        ('C', " turn"),
        ('U', " prompt"),
        ('T', " tools"),
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

pub(crate) fn context_status_marker_style(marker: char) -> Option<Style> {
    let theme = tui_theme();
    match marker {
        'B' => Some(theme.identity_style()),
        'D' => Some(theme.thinking_style()),
        'P' => Some(theme.code_style()),
        'H' => Some(theme.success_style()),
        'C' => Some(theme.accent_style()),
        'U' => Some(theme.identity_style()),
        'T' => Some(theme.accent_style()),
        '.' => Some(theme.dim_style()),
        _ => None,
    }
}
