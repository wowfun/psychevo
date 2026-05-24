#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn prompt_lines(
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

pub(crate) fn prompt_line(
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

pub(crate) fn wrap_prompt_text(text: &str, prefix: &str, width: u16) -> Vec<String> {
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

pub(crate) fn user_shell_lines(
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

pub(crate) fn user_shell_command_text(row: &TranscriptRow) -> String {
    let title = row.title.trim();
    if title == "!" {
        return String::new();
    }
    for prefix in ["! ", "Running ! ", "Ran ! ", "Running ", "Ran "] {
        if let Some(command) = title.strip_prefix(prefix) {
            return command.trim().to_string();
        }
    }
    title.to_string()
}

pub(crate) fn wrap_detail_text(text: &str, prefix: &str, width: u16) -> Vec<String> {
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

pub(crate) fn answer_lines(
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

pub(crate) fn raw_markdown_source_lines(input: &str, style: Style) -> Vec<Line<'static>> {
    input
        .lines()
        .map(|line| Line::from(Span::styled(line.to_string(), style)))
        .collect()
}

pub(crate) struct LedgerEvidenceRowView {
    pub(crate) marker: String,
    pub(crate) marker_style: Style,
    pub(crate) title: String,
    pub(crate) title_style: Style,
    pub(crate) elapsed: Option<String>,
    pub(crate) expand_hint: Option<String>,
    pub(crate) body_lines: Vec<String>,
    pub(crate) body_style: Style,
    pub(crate) compact_trailing: bool,
}

pub(crate) fn ledger_evidence_lines(view: LedgerEvidenceRowView, width: u16) -> Vec<Line<'static>> {
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

pub(crate) fn thinking_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    ledger_evidence_lines(thinking_ledger_view(row, selected, compact_trailing), width)
}

pub(crate) fn thinking_ledger_view(
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

pub(crate) fn thinking_body_lines(row: &TranscriptRow) -> Vec<String> {
    let mut lines = Vec::new();
    append_expandable_evidence_body(&mut lines, row);
    lines
}

pub(crate) fn tool_lines(
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

pub(crate) fn is_agent_tool_row(row: &TranscriptRow) -> bool {
    row.tool_name.as_deref() == Some("Agent")
}

pub(crate) fn tool_ledger_view(
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
pub(crate) enum ToolRowPhase {
    Active,
    Completed,
}

impl ToolRowPhase {
    pub(crate) fn from_row(row: &TranscriptRow) -> Self {
        if !row.failed && active_tool_elapsed(row).is_some() {
            Self::Active
        } else {
            Self::Completed
        }
    }
}

pub(crate) fn tool_display_title(row: &TranscriptRow, _phase: ToolRowPhase) -> String {
    let title = row.title.trim();
    tool_title_as_invocation(row.tool_name.as_deref(), row.kind, title, row.user_shell)
}

pub(crate) fn foldable_evidence_body(row: &TranscriptRow) -> bool {
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

pub(crate) fn foldable_tool_title(row: &TranscriptRow) -> bool {
    !row.user_shell
        && matches!(row.kind, TranscriptKind::Ran)
        && tool_title_detail(row, &row.title).is_some()
}

pub(crate) fn toggle_transcript_row_details(row: &mut TranscriptRow) {
    if row.details_collapsed {
        row.details_collapsed = false;
        return;
    }
    if row.full_text.as_ref().is_some_and(|full| full != &row.text) || foldable_tool_title(row) {
        if row.expanded && row.kind == TranscriptKind::Thinking && foldable_evidence_body(row) {
            row.expanded = false;
            row.details_collapsed = true;
            return;
        }
        row.expanded = !row.expanded;
        return;
    }
    if foldable_evidence_body(row) {
        row.details_collapsed = true;
    }
}

pub(crate) const TOOL_TITLE_DETAIL_WIDTH: usize = 80;

pub(crate) fn tool_title_detail(row: &TranscriptRow, title: &str) -> Option<String> {
    if row.kind != TranscriptKind::Ran || row.user_shell {
        return None;
    }
    let command = title
        .trim()
        .strip_prefix("exec_command ")
        .or_else(|| title.trim().strip_prefix("Running "))
        .or_else(|| title.trim().strip_prefix("Ran "))
        .unwrap_or_else(|| title.trim());
    if UnicodeWidthStr::width(command) <= TOOL_TITLE_DETAIL_WIDTH {
        return None;
    }
    Some(format!("command: {command}"))
}

pub(crate) fn suppressed_active_tool_body(row: &TranscriptRow, lines: &[&str]) -> bool {
    ToolRowPhase::from_row(row) == ToolRowPhase::Active
        && lines.len() == 1
        && matches!(lines[0].trim(), "running" | "preparing")
}

pub(crate) fn tool_body_lines(
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

pub(crate) fn append_expandable_evidence_body(lines: &mut Vec<String>, row: &TranscriptRow) {
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

pub(crate) fn row_expand_hint(
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

pub(crate) fn omitted_line_count(row: &TranscriptRow) -> Option<usize> {
    if let Some(count) = row.text.lines().find_map(collapsed_more_line_count) {
        return Some(count);
    }
    if row.text.trim_end().ends_with('…') {
        return None;
    }
    None
}

pub(crate) fn collapsed_more_line_count(line: &str) -> Option<usize> {
    line.trim()
        .strip_prefix("... ")
        .and_then(|value| value.strip_suffix(" more lines"))
        .and_then(|value| value.parse::<usize>().ok())
}

pub(crate) fn row_has_collapsed_body(row: &TranscriptRow) -> bool {
    row.full_text.as_ref().is_some_and(|full| full != &row.text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LedgerBodyCollapsePolicy {
    pub(crate) head_lines: usize,
    pub(crate) tail_lines: usize,
    pub(crate) max_tokens: usize,
    pub(crate) max_width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LedgerBodyCollapse {
    pub(crate) preview: String,
    pub(crate) full_text: Option<String>,
}

pub(crate) const LEDGER_BODY_COLLAPSE_HEAD_LINES: usize = 2;
pub(crate) const LEDGER_BODY_COLLAPSE_TAIL_LINES: usize = 4;
pub(crate) const LEDGER_BODY_COLLAPSE_TOKENS: usize = 200;
pub(crate) const LEDGER_BODY_COLLAPSE_WIDTH: usize = 1200;
pub(crate) const DISPLAY_TOKEN_LONG_RUN_FREE_CELLS: usize = 16;
pub(crate) const DISPLAY_TOKEN_CHUNK_CELLS: usize = 4;

pub(crate) fn ledger_body_collapse_policy() -> LedgerBodyCollapsePolicy {
    LedgerBodyCollapsePolicy {
        head_lines: LEDGER_BODY_COLLAPSE_HEAD_LINES,
        tail_lines: LEDGER_BODY_COLLAPSE_TAIL_LINES,
        max_tokens: LEDGER_BODY_COLLAPSE_TOKENS,
        max_width: LEDGER_BODY_COLLAPSE_WIDTH,
    }
}

impl LedgerBodyCollapsePolicy {
    pub(crate) fn should_collapse(self, text: &str) -> bool {
        text.lines().count() > self.head_lines.saturating_add(self.tail_lines)
            || display_token_count(text) > self.max_tokens
            || UnicodeWidthStr::width(text) > self.max_width
    }

    pub(crate) fn collapse(self, text: &str) -> LedgerBodyCollapse {
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

    pub(crate) fn collapse_by_token_or_width(self, text: &str) -> LedgerBodyCollapse {
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

pub(crate) fn middle_fold_lines(lines: &[&str], head_lines: usize, tail_lines: usize) -> String {
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

pub(crate) fn middle_fold_display_tokens(text: &str, max_tokens: usize) -> String {
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

pub(crate) fn middle_fold_display_width(text: &str, max_width: usize) -> String {
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

pub(crate) fn join_middle_fold_parts(head: &str, tail: &str) -> String {
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

pub(crate) fn trim_trailing_ellipsis(mut text: String) -> String {
    while text.ends_with('…') {
        text.pop();
    }
    text.trim_end().to_string()
}

pub(crate) fn display_token_count(text: &str) -> usize {
    text.split_whitespace()
        .map(display_token_count_segment)
        .sum()
}

pub(crate) fn display_token_count_segment(segment: &str) -> usize {
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

pub(crate) fn truncate_display_tokens(text: &str, max_tokens: usize) -> String {
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

pub(crate) fn prefix_display_width(value: &str, max_width: usize) -> String {
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

pub(crate) fn suffix_display_width(value: &str, max_width: usize) -> String {
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
