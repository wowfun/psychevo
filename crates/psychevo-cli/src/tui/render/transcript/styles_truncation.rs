#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn suffix_display_tokens(text: &str, max_tokens: usize) -> String {
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

pub(crate) fn collapse_ledger_body(text: &str) -> (String, Option<String>) {
    let collapsed = ledger_body_collapse_policy().collapse(text);
    (collapsed.preview, collapsed.full_text)
}

pub(crate) fn ledger_title_line(
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

pub(crate) fn ledger_title_right_text(
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

pub(crate) fn fit_expand_hint(hint: &str, max_width: usize) -> String {
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

pub(crate) fn tool_elapsed_label(row: &TranscriptRow) -> Option<String> {
    row.tool_elapsed
        .or_else(|| active_tool_elapsed(row))
        .map(format_duration_compact)
}

pub(crate) fn active_tool_elapsed(row: &TranscriptRow) -> Option<Duration> {
    if row.failed || row.interrupted || row.tool_elapsed.is_some() {
        return None;
    }
    row.tool_started.map(|started| started.elapsed())
}

pub(crate) fn truncate_display_width(value: &str, max_width: usize) -> String {
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

pub(crate) fn label_style(kind: TranscriptKind, failed: bool) -> Style {
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

pub(crate) fn focus_marker_style(failed: bool) -> Style {
    if failed {
        tui_theme().error_style()
    } else {
        tui_theme().accent_style()
    }
}

pub(crate) fn interruption_style() -> Style {
    tui_theme().thinking_style()
}

pub(crate) fn style_for_body(kind: TranscriptKind, failed: bool) -> Style {
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
