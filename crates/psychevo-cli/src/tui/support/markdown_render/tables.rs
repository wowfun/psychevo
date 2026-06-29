#[allow(unused_imports)]
pub(crate) use super::*;

impl MarkdownTable {
    pub(crate) fn new(alignments: Vec<pulldown_cmark::Alignment>) -> Self {
        Self {
            alignments,
            header: None,
            rows: Vec::new(),
            current_row: None,
            current_cell: None,
            in_header: false,
        }
    }
}

pub(crate) fn heading_style(level: pulldown_cmark::HeadingLevel) -> Style {
    let mut style = Style::default().add_modifier(Modifier::BOLD);
    if level == pulldown_cmark::HeadingLevel::H1 {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

pub(crate) fn heading_prefix(level: pulldown_cmark::HeadingLevel) -> &'static str {
    match level {
        pulldown_cmark::HeadingLevel::H1 => "# ",
        pulldown_cmark::HeadingLevel::H2 => "## ",
        pulldown_cmark::HeadingLevel::H3 => "### ",
        pulldown_cmark::HeadingLevel::H4 => "#### ",
        pulldown_cmark::HeadingLevel::H5 => "##### ",
        pulldown_cmark::HeadingLevel::H6 => "###### ",
    }
}

pub(crate) fn is_blank_line(line: &Line<'_>) -> bool {
    line.spans
        .iter()
        .all(|span| span.content.chars().all(char::is_whitespace))
}

pub(crate) fn code_block_language(kind: pulldown_cmark::CodeBlockKind<'_>) -> String {
    match kind {
        pulldown_cmark::CodeBlockKind::Fenced(info) => info
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string(),
        pulldown_cmark::CodeBlockKind::Indented => String::new(),
    }
}

pub(crate) fn clean_table_cell(value: String) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn render_table(table: MarkdownTable, width: Option<u16>) -> Vec<Line<'static>> {
    let Some(header) = table.header else {
        return Vec::new();
    };
    let col_count = table_column_count(&header, &table.rows, &table.alignments);
    if col_count == 0 {
        return Vec::new();
    }
    let header = normalize_table_row(&header, col_count);
    let rows = table
        .rows
        .iter()
        .map(|row| normalize_table_row(row, col_count))
        .collect::<Vec<_>>();
    let widths = table_widths(&header, &rows, col_count);
    let box_width = table_box_width(&widths);
    if width.is_some_and(|available| box_width > usize::from(available)) {
        return render_wrapped_table(&header, &rows, width.unwrap_or_default());
    }
    render_box_table(&header, &rows, &table.alignments, &widths)
}

pub(crate) fn table_column_count(
    header: &[String],
    rows: &[Vec<String>],
    alignments: &[pulldown_cmark::Alignment],
) -> usize {
    rows.iter()
        .map(Vec::len)
        .chain([header.len(), alignments.len()])
        .max()
        .unwrap_or(0)
}

pub(crate) fn normalize_table_row(row: &[String], columns: usize) -> Vec<String> {
    (0..columns)
        .map(|index| row.get(index).cloned().unwrap_or_default())
        .collect()
}

pub(crate) fn table_widths(header: &[String], rows: &[Vec<String>], columns: usize) -> Vec<usize> {
    (0..columns)
        .map(|column| {
            std::iter::once(header)
                .chain(rows.iter().map(Vec::as_slice))
                .map(|row| UnicodeWidthStr::width(row[column].as_str()))
                .max()
                .unwrap_or(1)
                .max(1)
        })
        .collect()
}

pub(crate) fn table_box_width(widths: &[usize]) -> usize {
    1 + widths.iter().map(|width| width + 3).sum::<usize>()
}

pub(crate) fn render_wrapped_table(
    header: &[String],
    rows: &[Vec<String>],
    width: u16,
) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let mut out = Vec::new();

    if rows.is_empty() {
        let text = header
            .iter()
            .map(|cell| cell.trim())
            .filter(|cell| !cell.is_empty())
            .collect::<Vec<_>>()
            .join(" / ");
        for line in wrap_table_text(&text, width) {
            out.push(Line::from(Span::styled(line, Style::default())));
        }
        return out;
    }

    for (row_index, row) in rows.iter().enumerate() {
        if row_index > 0 && !out.is_empty() {
            out.push(Line::from(""));
        }
        for (column_index, cell) in row.iter().enumerate() {
            let header = header
                .get(column_index)
                .map(|value| value.trim())
                .unwrap_or_default();
            let value = cell.trim();
            if header.is_empty() && value.is_empty() {
                continue;
            }
            let label = if header.is_empty() {
                format!("Column {}", column_index + 1)
            } else {
                header.to_string()
            };
            push_wrapped_table_cell(&mut out, &label, value, width);
        }
    }

    out
}

pub(crate) fn push_wrapped_table_cell(
    out: &mut Vec<Line<'static>>,
    label: &str,
    value: &str,
    width: usize,
) {
    let theme = tui_theme();
    let label_style = Style::default().add_modifier(Modifier::BOLD);
    let prefix = format!("{label}: ");
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());

    if prefix_width < width {
        let mut wrapped = wrap_table_text(value, width.saturating_sub(prefix_width).max(1));
        let first = wrapped.first().cloned().unwrap_or_default();
        out.push(Line::from(vec![
            Span::styled(prefix, label_style),
            Span::styled(first, Style::default()),
        ]));
        for line in wrapped.drain(1..) {
            out.push(table_continuation_line(line, &theme, width));
        }
        return;
    }

    for label_line in wrap_table_text(&format!("{label}:"), width) {
        out.push(Line::from(Span::styled(label_line, label_style)));
    }
    let continuation_width = continuation_text_width(width);
    for line in wrap_table_text(value, continuation_width) {
        out.push(table_continuation_line(line, &theme, width));
    }
}

pub(crate) fn continuation_text_width(width: usize) -> usize {
    width
        .saturating_sub(table_continuation_prefix_width(width))
        .max(1)
}

pub(crate) fn table_continuation_prefix_width(width: usize) -> usize {
    if width >= 3 {
        UnicodeWidthStr::width("  ")
    } else {
        0
    }
}

pub(crate) fn table_continuation_line(
    line: String,
    theme: &TuiTheme,
    width: usize,
) -> Line<'static> {
    if table_continuation_prefix_width(width) == 0 {
        return Line::from(Span::styled(line, Style::default()));
    }
    Line::from(vec![
        Span::styled("  ".to_string(), theme.dim_style()),
        Span::styled(line, Style::default()),
    ])
}

pub(crate) fn wrap_table_text(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for word in value.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if current.is_empty() {
            start_table_wrap_line(&mut lines, &mut current, &mut current_width, word, width);
        } else if current_width + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
            start_table_wrap_line(&mut lines, &mut current, &mut current_width, word, width);
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub(crate) fn start_table_wrap_line(
    lines: &mut Vec<String>,
    current: &mut String,
    current_width: &mut usize,
    word: &str,
    width: usize,
) {
    let word_width = UnicodeWidthStr::width(word);
    if word_width <= width {
        current.push_str(word);
        *current_width = word_width;
        return;
    }

    let chunks = split_table_word(word, width);
    let last_index = chunks.len().saturating_sub(1);
    for (index, chunk) in chunks.into_iter().enumerate() {
        if index == last_index {
            *current_width = UnicodeWidthStr::width(chunk.as_str());
            *current = chunk;
        } else {
            lines.push(chunk);
        }
    }
}

pub(crate) fn split_table_word(word: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for ch in word.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if !current.is_empty() && current_width + ch_width > width {
            chunks.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
        if current_width >= width {
            chunks.push(std::mem::take(&mut current));
            current_width = 0;
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

pub(crate) fn render_box_table(
    header: &[String],
    rows: &[Vec<String>],
    alignments: &[pulldown_cmark::Alignment],
    widths: &[usize],
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    out.push(table_border("┌", "┬", "┐", widths));
    out.push(table_box_row(header, alignments, widths, true));
    out.push(table_border("├", "┼", "┤", widths));
    for row in rows {
        out.push(table_box_row(row, alignments, widths, false));
    }
    out.push(table_border("└", "┴", "┘", widths));
    out
}

pub(crate) fn table_border(
    left: &'static str,
    mid: &'static str,
    right: &'static str,
    widths: &[usize],
) -> Line<'static> {
    let mut text = String::new();
    text.push_str(left);
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            text.push_str(mid);
        }
        text.push_str(&"─".repeat(width + 2));
    }
    text.push_str(right);
    Line::from(Span::styled(text, tui_theme().dim_style()))
}

pub(crate) fn table_box_row(
    row: &[String],
    alignments: &[pulldown_cmark::Alignment],
    widths: &[usize],
    header: bool,
) -> Line<'static> {
    let theme = tui_theme();
    let mut spans = Vec::new();
    spans.push(Span::styled("│".to_string(), theme.dim_style()));
    for (index, cell) in row.iter().enumerate() {
        let alignment = alignments
            .get(index)
            .copied()
            .unwrap_or(pulldown_cmark::Alignment::None);
        let cell = align_cell(cell, widths[index], alignment);
        spans.push(Span::styled(" ".to_string(), theme.dim_style()));
        let style = if header {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        spans.push(Span::styled(cell, style));
        spans.push(Span::styled(" │".to_string(), theme.dim_style()));
    }
    Line::from(spans)
}

pub(crate) fn align_cell(
    value: &str,
    width: usize,
    alignment: pulldown_cmark::Alignment,
) -> String {
    let value_width = UnicodeWidthStr::width(value);
    let padding = width.saturating_sub(value_width);
    match alignment {
        pulldown_cmark::Alignment::Right => format!("{}{value}", " ".repeat(padding)),
        pulldown_cmark::Alignment::Center => {
            let left = padding / 2;
            let right = padding.saturating_sub(left);
            format!("{}{value}{}", " ".repeat(left), " ".repeat(right))
        }
        _ => format!("{value}{}", " ".repeat(padding)),
    }
}
