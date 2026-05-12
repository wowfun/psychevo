use std::borrow::Cow;

fn render_markdown_lines(input: &str, cwd: &Path, width: Option<u16>) -> Vec<Line<'static>> {
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

struct MarkdownWriter<'a> {
    cwd: &'a Path,
    width: Option<u16>,
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    list_stack: Vec<Option<u64>>,
    link_stack: Vec<MarkdownLink>,
    code_block: Option<MarkdownCodeBlock>,
    table: Option<MarkdownTable>,
    blockquote_depth: usize,
}

struct MarkdownLink {
    destination: String,
    local_display: Option<String>,
    suppress_label: bool,
}

struct MarkdownCodeBlock {
    lang: String,
    code: String,
}

#[derive(Debug, Clone)]
struct MarkdownTable {
    alignments: Vec<pulldown_cmark::Alignment>,
    header: Option<Vec<String>>,
    rows: Vec<Vec<String>>,
    current_row: Option<Vec<String>>,
    current_cell: Option<String>,
    in_header: bool,
}

impl<'a> MarkdownWriter<'a> {
    fn new(cwd: &'a Path, width: Option<u16>) -> Self {
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

    fn render<'b>(&mut self, parser: pulldown_cmark::Parser<'b>) {
        for event in parser {
            self.event(event);
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_current();
        while self.lines.last().is_some_and(is_blank_line) {
            self.lines.pop();
        }
        self.lines
    }

    fn event<'b>(&mut self, event: pulldown_cmark::Event<'b>) {
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

    fn start_tag<'b>(&mut self, tag: pulldown_cmark::Tag<'b>) {
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

    fn end_tag(&mut self, tag: pulldown_cmark::TagEnd) {
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
                    table.current_row.get_or_insert_with(Vec::new).push(clean_table_cell(cell));
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

    fn text(&mut self, text: &str) {
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

    fn inline_code(&mut self, code: &str) {
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

    fn current_style(&self) -> Style {
        let mut style = Style::default();
        for overlay in &self.style_stack {
            style = style.patch(*overlay);
        }
        style
    }

    fn flush_current(&mut self) {
        if self.current.is_empty() {
            return;
        }
        if self.blockquote_depth > 0 {
            let mut spans = vec![Span::styled(
                "│ ".to_string(),
                tui_theme().dim_style(),
            )];
            spans.append(&mut self.current);
            self.lines.push(Line::from(spans));
        } else {
            self.lines.push(Line::from(std::mem::take(&mut self.current)));
        }
    }

    fn push_blank(&mut self) {
        if !self.lines.is_empty() && !self.lines.last().is_some_and(is_blank_line) {
            self.lines.push(Line::from(""));
        }
    }

    fn push_code_block(&mut self, block: &MarkdownCodeBlock) {
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

    fn list_item_marker(&mut self) -> String {
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

    fn append_table_text(&mut self, text: &str) {
        if let Some(table) = &mut self.table
            && let Some(cell) = &mut table.current_cell
        {
            cell.push_str(text);
        }
    }

    fn push_link_target(&mut self, target: String) {
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
    fn new(alignments: Vec<pulldown_cmark::Alignment>) -> Self {
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

fn heading_style(level: pulldown_cmark::HeadingLevel) -> Style {
    let mut style = Style::default().add_modifier(Modifier::BOLD);
    if level == pulldown_cmark::HeadingLevel::H1 {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

fn heading_prefix(level: pulldown_cmark::HeadingLevel) -> &'static str {
    match level {
        pulldown_cmark::HeadingLevel::H1 => "# ",
        pulldown_cmark::HeadingLevel::H2 => "## ",
        pulldown_cmark::HeadingLevel::H3 => "### ",
        pulldown_cmark::HeadingLevel::H4 => "#### ",
        pulldown_cmark::HeadingLevel::H5 => "##### ",
        pulldown_cmark::HeadingLevel::H6 => "###### ",
    }
}

fn is_blank_line(line: &Line<'_>) -> bool {
    line.spans
        .iter()
        .all(|span| span.content.chars().all(char::is_whitespace))
}

fn code_block_language(kind: pulldown_cmark::CodeBlockKind<'_>) -> String {
    match kind {
        pulldown_cmark::CodeBlockKind::Fenced(info) => info
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string(),
        pulldown_cmark::CodeBlockKind::Indented => String::new(),
    }
}

fn clean_table_cell(value: String) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_table(table: MarkdownTable, width: Option<u16>) -> Vec<Line<'static>> {
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

fn table_column_count(
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

fn normalize_table_row(row: &[String], columns: usize) -> Vec<String> {
    (0..columns)
        .map(|index| row.get(index).cloned().unwrap_or_default())
        .collect()
}

fn table_widths(header: &[String], rows: &[Vec<String>], columns: usize) -> Vec<usize> {
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

fn table_box_width(widths: &[usize]) -> usize {
    1 + widths.iter().map(|width| width + 3).sum::<usize>()
}

fn render_box_table(
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

fn table_border(
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

fn table_box_row(
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

fn align_cell(value: &str, width: usize, alignment: pulldown_cmark::Alignment) -> String {
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

fn render_pipe_table(
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

fn pipe_table_row(row: &[String]) -> String {
    format!("| {} |", row.join(" | "))
}

fn pipe_table_delimiter(alignments: &[pulldown_cmark::Alignment], columns: usize) -> String {
    let cells = (0..columns)
        .map(|index| match alignments
            .get(index)
            .copied()
            .unwrap_or(pulldown_cmark::Alignment::None)
        {
            pulldown_cmark::Alignment::Left => ":---",
            pulldown_cmark::Alignment::Right => "---:",
            pulldown_cmark::Alignment::Center => ":---:",
            pulldown_cmark::Alignment::None => "---",
        })
        .collect::<Vec<_>>();
    format!("| {} |", cells.join(" | "))
}

fn highlight_code_line(line: &str, lang: &str) -> Vec<Span<'static>> {
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

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch == '-' || ch.is_ascii_alphanumeric()
}

fn normalized_lang(lang: &str) -> &str {
    match lang {
        "rs" => "rust",
        "js" | "jsx" | "ts" | "tsx" => "javascript",
        "sh" | "bash" | "zsh" => "shell",
        other => other,
    }
}

fn is_comment_line(line: &str, lang: &str) -> bool {
    let trimmed = line.trim_start();
    match normalized_lang(lang) {
        "shell" | "python" | "ruby" | "yaml" | "toml" => trimmed.starts_with('#'),
        "sql" => trimmed.starts_with("--"),
        _ => trimmed.starts_with("//"),
    }
}

fn is_code_keyword(token: &str, lang: &str) -> bool {
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
            "case" | "do" | "done" | "elif" | "else" | "esac" | "fi" | "for" | "function"
                | "if" | "in" | "then" | "while"
        ),
        _ => matches!(token, "true" | "false" | "null"),
    }
}

fn unwrap_markdown_table_fences(input: &str) -> Cow<'_, str> {
    let lines = input.lines().collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut index = 0usize;
    let mut changed = false;
    while index < lines.len() {
        let Some(open) = parse_fence_open(lines[index]) else {
            output.push(lines[index].to_string());
            index += 1;
            continue;
        };
        if !is_markdown_fence_info(&open.info) {
            output.push(lines[index].to_string());
            index += 1;
            continue;
        }
        let body_start = index + 1;
        let mut close_index = None;
        let mut cursor = body_start;
        while cursor < lines.len() {
            if is_fence_close(lines[cursor], open.marker, open.len) {
                close_index = Some(cursor);
                break;
            }
            cursor += 1;
        }
        let Some(close_index) = close_index else {
            output.push(lines[index].to_string());
            index += 1;
            continue;
        };
        let body = &lines[body_start..close_index];
        if is_markdown_table_like(body) {
            output.extend(body.iter().map(|line| (*line).to_string()));
            changed = true;
        } else {
            output.push(lines[index].to_string());
            output.extend(body.iter().map(|line| (*line).to_string()));
            output.push(lines[close_index].to_string());
        }
        index = close_index + 1;
    }
    if !changed {
        return Cow::Borrowed(input);
    }
    let mut text = output.join("\n");
    if input.ends_with('\n') {
        text.push('\n');
    }
    Cow::Owned(text)
}

struct FenceOpen {
    marker: char,
    len: usize,
    info: String,
}

fn parse_fence_open(line: &str) -> Option<FenceOpen> {
    let indent = line.len().saturating_sub(line.trim_start().len());
    if indent > 3 {
        return None;
    }
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|ch| *ch == marker).count();
    if len < 3 {
        return None;
    }
    let info = trimmed.chars().skip(len).collect::<String>();
    if marker == '`' && info.contains('`') {
        return None;
    }
    Some(FenceOpen {
        marker,
        len,
        info: info.trim().to_string(),
    })
}

fn is_fence_close(line: &str, marker: char, len: usize) -> bool {
    let indent = line.len().saturating_sub(line.trim_start().len());
    if indent > 3 {
        return false;
    }
    let trimmed = line.trim_start();
    if !trimmed.starts_with(marker) {
        return false;
    }
    let count = trimmed.chars().take_while(|ch| *ch == marker).count();
    count >= len && trimmed.chars().skip(count).all(char::is_whitespace)
}

fn is_markdown_fence_info(info: &str) -> bool {
    matches!(
        info.split_whitespace().next().unwrap_or_default(),
        "md" | "markdown"
    )
}

fn is_markdown_table_like(lines: &[&str]) -> bool {
    let mut non_empty = lines.iter().map(|line| line.trim()).filter(|line| !line.is_empty());
    let Some(header) = non_empty.next() else {
        return false;
    };
    let Some(delimiter) = non_empty.next() else {
        return false;
    };
    header.contains('|') && is_table_delimiter_line(delimiter)
}

fn is_table_delimiter_line(line: &str) -> bool {
    let trimmed = line.trim().trim_matches('|');
    let cells = trimmed.split('|').map(str::trim).collect::<Vec<_>>();
    cells.len() >= 2
        && cells.iter().all(|cell| {
            let cell = cell.trim_matches(':').trim();
            cell.len() >= 3 && cell.chars().all(|ch| ch == '-')
        })
}

fn local_link_display(destination: &str, cwd: &Path) -> Option<String> {
    let destination = destination.trim();
    if destination.contains("://") && !destination.starts_with("file://") {
        return None;
    }
    let destination = destination.strip_prefix("file://").unwrap_or(destination);
    let (path_text, suffix) = split_location_suffix(destination);
    let path = Path::new(path_text);
    if !path.is_absolute() && destination.starts_with("file:") {
        return None;
    }
    let display_path = if path.is_absolute() {
        path.strip_prefix(cwd)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| path.to_path_buf())
    } else {
        path.to_path_buf()
    };
    let display = display_path.to_string_lossy().replace('\\', "/");
    (!display.is_empty()).then(|| format!("{display}{suffix}"))
}

fn split_location_suffix(value: &str) -> (&str, &str) {
    static LOCATION_SUFFIX: std::sync::LazyLock<regex_lite::Regex> =
        std::sync::LazyLock::new(|| {
            regex_lite::Regex::new(r":\d+(?::\d+)?(?:[-–]\d+(?::\d+)?)?$")
                .expect("valid location suffix regex")
        });
    if let Some(found) = LOCATION_SUFFIX.find(value)
        && found.end() == value.len()
    {
        return (&value[..found.start()], &value[found.start()..]);
    }
    (value, "")
}
