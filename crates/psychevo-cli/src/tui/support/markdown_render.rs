#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn render_markdown_lines(
    input: &str,
    cwd: &Path,
    width: Option<u16>,
) -> Vec<Line<'static>> {
    let mut options = pulldown_cmark::Options::empty();
    options.insert(pulldown_cmark::Options::ENABLE_TABLES);
    options.insert(pulldown_cmark::Options::ENABLE_STRIKETHROUGH);
    options.insert(pulldown_cmark::Options::ENABLE_TASKLISTS);
    let input = unwrap_markdown_table_fences(input);
    let parser = pulldown_cmark::Parser::new_ext(input.as_ref(), options);
    let mut writer = MarkdownWriter::new(cwd, width);
    writer.render(parser);
    writer.finish()
}

pub(crate) struct MarkdownWriter<'a> {
    pub(crate) cwd: &'a Path,
    pub(crate) width: Option<u16>,
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) current: Vec<Span<'static>>,
    pub(crate) style_stack: Vec<Style>,
    pub(crate) list_stack: Vec<Option<u64>>,
    pub(crate) link_stack: Vec<MarkdownLink>,
    pub(crate) code_block: Option<MarkdownCodeBlock>,
    pub(crate) table: Option<MarkdownTable>,
    pub(crate) blockquote_depth: usize,
}

pub(crate) struct MarkdownLink {
    pub(crate) destination: String,
    pub(crate) local_display: Option<String>,
    pub(crate) suppress_label: bool,
}

pub(crate) struct MarkdownCodeBlock {
    pub(crate) lang: String,
    pub(crate) code: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MarkdownTable {
    pub(crate) alignments: Vec<pulldown_cmark::Alignment>,
    pub(crate) header: Option<Vec<String>>,
    pub(crate) rows: Vec<Vec<String>>,
    pub(crate) current_row: Option<Vec<String>>,
    pub(crate) current_cell: Option<String>,
    pub(crate) in_header: bool,
}

impl<'a> MarkdownWriter<'a> {
    pub(crate) fn new(cwd: &'a Path, width: Option<u16>) -> Self {
        Self {
            cwd,
            width,
            lines: Vec::new(),
            current: Vec::new(),
            style_stack: Vec::new(),
            list_stack: Vec::new(),
            link_stack: Vec::new(),
            code_block: None,
            table: None,
            blockquote_depth: 0,
        }
    }

    pub(crate) fn render<'b>(&mut self, parser: pulldown_cmark::Parser<'b>) {
        for event in parser {
            self.event(event);
        }
    }

    pub(crate) fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_current();
        while self.lines.last().is_some_and(is_blank_line) {
            self.lines.pop();
        }
        self.lines
    }

    pub(crate) fn event<'b>(&mut self, event: pulldown_cmark::Event<'b>) {
        match event {
            pulldown_cmark::Event::Start(tag) => self.start_tag(tag),
            pulldown_cmark::Event::End(tag) => self.end_tag(tag),
            pulldown_cmark::Event::Text(text) => self.text(text.as_ref()),
            pulldown_cmark::Event::Code(code) => self.inline_code(code.as_ref()),
            pulldown_cmark::Event::SoftBreak | pulldown_cmark::Event::HardBreak => {
                self.flush_current();
                self.append_table_text("\n");
            }
            pulldown_cmark::Event::Rule => {
                self.flush_current();
                self.lines.push(Line::from(Span::styled(
                    "------".to_string(),
                    tui_theme().dim_style(),
                )));
                self.push_blank();
            }
            pulldown_cmark::Event::Html(html) | pulldown_cmark::Event::InlineHtml(html) => {
                self.text(html.as_ref());
            }
            pulldown_cmark::Event::FootnoteReference(_) => {}
            pulldown_cmark::Event::TaskListMarker(checked) => {
                self.text(if checked { "[x] " } else { "[ ] " });
            }
        }
    }

    pub(crate) fn start_tag<'b>(&mut self, tag: pulldown_cmark::Tag<'b>) {
        match tag {
            pulldown_cmark::Tag::Paragraph => {}
            pulldown_cmark::Tag::Heading { level, .. } => {
                self.flush_current();
                self.style_stack.push(heading_style(level));
                self.text(heading_prefix(level));
            }
            pulldown_cmark::Tag::BlockQuote => {
                self.flush_current();
                self.blockquote_depth += 1;
                self.style_stack.push(tui_theme().dim_style());
            }
            pulldown_cmark::Tag::CodeBlock(kind) => {
                self.flush_current();
                self.code_block = Some(MarkdownCodeBlock {
                    lang: code_block_language(kind),
                    code: String::new(),
                });
            }
            pulldown_cmark::Tag::List(start) => {
                self.flush_current();
                self.list_stack.push(start);
            }
            pulldown_cmark::Tag::Item => {
                self.flush_current();
                let marker = self.list_item_marker();
                self.current
                    .push(Span::styled(marker, tui_theme().dim_style()));
            }
            pulldown_cmark::Tag::Emphasis => {
                self.style_stack
                    .push(Style::default().add_modifier(Modifier::ITALIC));
            }
            pulldown_cmark::Tag::Strong => {
                self.style_stack
                    .push(Style::default().add_modifier(Modifier::BOLD));
            }
            pulldown_cmark::Tag::Strikethrough => {
                self.style_stack
                    .push(Style::default().add_modifier(Modifier::CROSSED_OUT));
            }
            pulldown_cmark::Tag::Link { dest_url, .. } => {
                let local_display = local_link_display(dest_url.as_ref(), self.cwd);
                let suppress_label = local_display.is_some();
                if !suppress_label && self.table.is_none() {
                    self.style_stack.push(
                        tui_theme()
                            .accent_style()
                            .add_modifier(Modifier::UNDERLINED),
                    );
                }
                self.link_stack.push(MarkdownLink {
                    destination: dest_url.to_string(),
                    local_display,
                    suppress_label,
                });
            }
            pulldown_cmark::Tag::Image { dest_url, .. } => {
                self.text("[image: ");
                self.text(dest_url.as_ref());
                self.text("]");
            }
            pulldown_cmark::Tag::Table(alignments) => {
                self.flush_current();
                self.table = Some(MarkdownTable::new(alignments));
            }
            pulldown_cmark::Tag::TableHead => {
                if let Some(table) = &mut self.table {
                    table.in_header = true;
                }
            }
            pulldown_cmark::Tag::TableRow => {
                if let Some(table) = &mut self.table {
                    table.current_row = Some(Vec::new());
                }
            }
            pulldown_cmark::Tag::TableCell => {
                if let Some(table) = &mut self.table {
                    table.current_cell = Some(String::new());
                }
            }
            _ => {}
        }
    }

    pub(crate) fn end_tag(&mut self, tag: pulldown_cmark::TagEnd) {
        match tag {
            pulldown_cmark::TagEnd::Paragraph => {
                if self.table.is_none() {
                    self.flush_current();
                    self.push_blank();
                } else {
                    self.append_table_text(" ");
                }
            }
            pulldown_cmark::TagEnd::Heading(_) => {
                self.flush_current();
                self.style_stack.pop();
                self.push_blank();
            }
            pulldown_cmark::TagEnd::BlockQuote => {
                self.flush_current();
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.style_stack.pop();
                self.push_blank();
            }
            pulldown_cmark::TagEnd::CodeBlock => {
                if let Some(code) = self.code_block.take() {
                    self.push_code_block(&code);
                }
                self.push_blank();
            }
            pulldown_cmark::TagEnd::List(_) => {
                self.flush_current();
                self.list_stack.pop();
                self.push_blank();
            }
            pulldown_cmark::TagEnd::Item => {
                self.flush_current();
            }
            pulldown_cmark::TagEnd::Emphasis
            | pulldown_cmark::TagEnd::Strong
            | pulldown_cmark::TagEnd::Strikethrough => {
                self.style_stack.pop();
            }
            pulldown_cmark::TagEnd::Link => {
                if let Some(link) = self.link_stack.pop() {
                    if let Some(display) = link.local_display {
                        self.push_link_target(display);
                    } else {
                        if !link.suppress_label && self.table.is_none() {
                            self.style_stack.pop();
                        }
                        self.push_link_target(format!(" ({})", link.destination));
                    }
                }
            }
            pulldown_cmark::TagEnd::TableCell => {
                if let Some(table) = &mut self.table {
                    let cell = table.current_cell.take().unwrap_or_default();
                    table
                        .current_row
                        .get_or_insert_with(Vec::new)
                        .push(clean_table_cell(cell));
                }
            }
            pulldown_cmark::TagEnd::TableRow => {
                if let Some(table) = &mut self.table
                    && let Some(row) = table.current_row.take()
                {
                    if table.in_header && table.header.is_none() {
                        table.header = Some(row);
                    } else {
                        table.rows.push(row);
                    }
                }
            }
            pulldown_cmark::TagEnd::TableHead => {
                if let Some(table) = &mut self.table {
                    if table.header.is_none()
                        && let Some(row) = table.current_row.take()
                    {
                        table.header = Some(row);
                    }
                    table.in_header = false;
                }
            }
            pulldown_cmark::TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.lines.extend(render_table(table, self.width));
                }
                self.push_blank();
            }
            _ => {}
        }
    }

    pub(crate) fn text(&mut self, text: &str) {
        if let Some(code) = self.code_block.as_mut() {
            code.code.push_str(text);
            return;
        }
        if self
            .link_stack
            .last()
            .is_some_and(|link| link.suppress_label)
        {
            return;
        }
        if self.table.is_some() {
            self.append_table_text(text);
            return;
        }
        for (index, part) in text.split('\n').enumerate() {
            if index > 0 {
                self.flush_current();
            }
            if !part.is_empty() {
                self.current
                    .push(Span::styled(part.to_string(), self.current_style()));
            }
        }
    }

    pub(crate) fn inline_code(&mut self, code: &str) {
        if self
            .link_stack
            .last()
            .is_some_and(|link| link.suppress_label)
        {
            return;
        }
        if self.table.is_some() {
            self.append_table_text(code);
            return;
        }
        self.current
            .push(Span::styled(code.to_string(), tui_theme().code_style()));
    }

    pub(crate) fn current_style(&self) -> Style {
        let mut style = Style::default();
        for overlay in &self.style_stack {
            style = style.patch(*overlay);
        }
        style
    }

    pub(crate) fn flush_current(&mut self) {
        if self.current.is_empty() {
            return;
        }
        if self.blockquote_depth > 0 {
            let mut spans = vec![Span::styled("│ ".to_string(), tui_theme().dim_style())];
            spans.append(&mut self.current);
            self.lines.push(Line::from(spans));
        } else {
            self.lines
                .push(Line::from(std::mem::take(&mut self.current)));
        }
    }

    pub(crate) fn push_blank(&mut self) {
        if !self.lines.is_empty() && !self.lines.last().is_some_and(is_blank_line) {
            self.lines.push(Line::from(""));
        }
    }

    pub(crate) fn push_code_block(&mut self, block: &MarkdownCodeBlock) {
        let theme = tui_theme();
        let lang = block.lang.trim();
        let label = if lang.is_empty() {
            "code".to_string()
        } else {
            format!("code {lang}")
        };
        self.lines.push(Line::from(vec![
            Span::styled("╭─ ".to_string(), theme.dim_style()),
            Span::styled(label, theme.dim_style().add_modifier(Modifier::BOLD)),
        ]));

        let source = block.code.trim_end_matches('\n');
        let collapsed = ledger_body_collapse_policy().collapse(source);
        let lines = if collapsed.preview.is_empty() {
            vec![" ".to_string()]
        } else {
            collapsed.preview.lines().map(ToOwned::to_owned).collect()
        };
        for line in lines {
            let mut spans = vec![Span::styled("│ ".to_string(), theme.dim_style())];
            if collapsed_more_line_count(&line).is_some() {
                spans.push(Span::styled(line, theme.dim_style()));
            } else {
                spans.extend(highlight_code_line(&line, lang));
            }
            self.lines.push(Line::from(spans));
        }
        self.lines.push(Line::from(Span::styled(
            "╰─".to_string(),
            theme.dim_style(),
        )));
    }

    pub(crate) fn list_item_marker(&mut self) -> String {
        let depth = self.list_stack.len().saturating_sub(1);
        let indent = "  ".repeat(depth);
        match self.list_stack.last_mut() {
            Some(Some(next)) => {
                let marker = format!("{indent}{next}. ");
                *next += 1;
                marker
            }
            _ => format!("{indent}• "),
        }
    }

    pub(crate) fn append_table_text(&mut self, text: &str) {
        if let Some(table) = &mut self.table
            && let Some(cell) = &mut table.current_cell
        {
            cell.push_str(text);
        }
    }

    pub(crate) fn push_link_target(&mut self, target: String) {
        if self.table.is_some() {
            self.append_table_text(&target);
            return;
        }
        self.current.push(Span::styled(
            target,
            tui_theme()
                .accent_style()
                .add_modifier(Modifier::UNDERLINED),
        ));
    }
}

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
        return render_pipe_table(&header, &rows, &table.alignments, col_count);
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

pub(crate) fn render_pipe_table(
    header: &[String],
    rows: &[Vec<String>],
    alignments: &[pulldown_cmark::Alignment],
    columns: usize,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    out.push(Line::from(Span::styled(
        pipe_table_row(header),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    out.push(Line::from(Span::styled(
        pipe_table_delimiter(alignments, columns),
        tui_theme().dim_style(),
    )));
    out.extend(
        rows.iter()
            .map(|row| Line::from(Span::styled(pipe_table_row(row), Style::default()))),
    );
    out
}

pub(crate) fn pipe_table_row(row: &[String]) -> String {
    format!("| {} |", row.join(" | "))
}

pub(crate) fn pipe_table_delimiter(
    alignments: &[pulldown_cmark::Alignment],
    columns: usize,
) -> String {
    let cells = (0..columns)
        .map(|index| {
            match alignments
                .get(index)
                .copied()
                .unwrap_or(pulldown_cmark::Alignment::None)
            {
                pulldown_cmark::Alignment::Left => ":---",
                pulldown_cmark::Alignment::Right => "---:",
                pulldown_cmark::Alignment::Center => ":---:",
                pulldown_cmark::Alignment::None => "---",
            }
        })
        .collect::<Vec<_>>();
    format!("| {} |", cells.join(" | "))
}

pub(crate) fn highlight_code_line(line: &str, lang: &str) -> Vec<Span<'static>> {
    let theme = tui_theme();
    if is_comment_line(line, lang) {
        return vec![Span::styled(line.to_string(), theme.dim_style())];
    }

    let mut spans = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if matches!(ch, '"' | '\'' | '`') {
            let quote = ch;
            let mut end = start + ch.len_utf8();
            let mut escaped = false;
            for (index, next) in chars.by_ref() {
                end = index + next.len_utf8();
                if escaped {
                    escaped = false;
                    continue;
                }
                if next == '\\' {
                    escaped = true;
                    continue;
                }
                if next == quote {
                    break;
                }
            }
            spans.push(Span::styled(
                line[start..end].to_string(),
                theme.success_style(),
            ));
            continue;
        }
        if is_identifier_start(ch) {
            let mut end = start + ch.len_utf8();
            while let Some((index, next)) = chars.peek().copied() {
                if !is_identifier_continue(next) {
                    break;
                }
                chars.next();
                end = index + next.len_utf8();
            }
            let token = &line[start..end];
            let style = if is_code_keyword(token, lang) {
                theme.accent_style().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            spans.push(Span::styled(token.to_string(), style));
            continue;
        }
        if ch.is_ascii_digit() {
            let mut end = start + ch.len_utf8();
            while let Some((index, next)) = chars.peek().copied() {
                if !next.is_ascii_alphanumeric() && next != '_' && next != '.' {
                    break;
                }
                chars.next();
                end = index + next.len_utf8();
            }
            spans.push(Span::styled(
                line[start..end].to_string(),
                theme.identity_style(),
            ));
            continue;
        }
        if matches!(
            ch,
            '{' | '}' | '[' | ']' | '(' | ')' | ':' | ';' | ',' | '.'
        ) {
            spans.push(Span::styled(ch.to_string(), theme.dim_style()));
        } else {
            spans.push(Span::raw(ch.to_string()));
        }
    }
    spans
}

pub(crate) fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

pub(crate) fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch == '-' || ch.is_ascii_alphanumeric()
}

pub(crate) fn normalized_lang(lang: &str) -> &str {
    match lang {
        "rs" => "rust",
        "js" | "jsx" | "ts" | "tsx" => "javascript",
        "sh" | "bash" | "zsh" => "shell",
        other => other,
    }
}

pub(crate) fn is_comment_line(line: &str, lang: &str) -> bool {
    let trimmed = line.trim_start();
    match normalized_lang(lang) {
        "shell" | "python" | "ruby" | "yaml" | "toml" => trimmed.starts_with('#'),
        "sql" => trimmed.starts_with("--"),
        _ => trimmed.starts_with("//"),
    }
}

pub(crate) fn is_code_keyword(token: &str, lang: &str) -> bool {
    match normalized_lang(lang) {
        "rust" => matches!(
            token,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "else"
                | "enum"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "struct"
                | "trait"
                | "true"
                | "type"
                | "use"
                | "where"
                | "while"
        ),
        "javascript" => matches!(
            token,
            "async"
                | "await"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "else"
                | "export"
                | "false"
                | "for"
                | "from"
                | "function"
                | "if"
                | "import"
                | "let"
                | "new"
                | "return"
                | "switch"
                | "true"
                | "try"
                | "type"
                | "while"
        ),
        "json" | "yaml" | "toml" => matches!(token, "true" | "false" | "null"),
        "shell" => matches!(
            token,
            "case"
                | "do"
                | "done"
                | "elif"
                | "else"
                | "esac"
                | "fi"
                | "for"
                | "function"
                | "if"
                | "in"
                | "then"
                | "while"
        ),
        _ => matches!(token, "true" | "false" | "null"),
    }
}

#[path = "markdown_render/fences.rs"]
pub(crate) mod fences;
#[allow(unused_imports)]
pub use fences::*;
