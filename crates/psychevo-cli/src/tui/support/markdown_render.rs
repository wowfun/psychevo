fn render_markdown_lines(input: &str, cwd: &Path) -> Vec<Line<'static>> {
    let mut options = pulldown_cmark::Options::empty();
    options.insert(pulldown_cmark::Options::ENABLE_STRIKETHROUGH);
    options.insert(pulldown_cmark::Options::ENABLE_TASKLISTS);
    let parser = pulldown_cmark::Parser::new_ext(input, options);
    let mut writer = MarkdownWriter::new(cwd);
    writer.render(parser);
    writer.finish()
}

struct MarkdownWriter<'a> {
    cwd: &'a Path,
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    list_stack: Vec<Option<u64>>,
    link_stack: Vec<MarkdownLink>,
    code_block: Option<String>,
    blockquote_depth: usize,
}

struct MarkdownLink {
    local_display: Option<String>,
    suppress_label: bool,
}

impl<'a> MarkdownWriter<'a> {
    fn new(cwd: &'a Path) -> Self {
        Self {
            cwd,
            lines: Vec::new(),
            current: Vec::new(),
            style_stack: Vec::new(),
            list_stack: Vec::new(),
            link_stack: Vec::new(),
            code_block: None,
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
            pulldown_cmark::Tag::CodeBlock(_) => {
                self.flush_current();
                self.code_block = Some(String::new());
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
                if !suppress_label {
                    self.style_stack.push(
                        tui_theme()
                            .accent_style()
                            .add_modifier(Modifier::UNDERLINED),
                    );
                }
                self.link_stack.push(MarkdownLink {
                    local_display,
                    suppress_label,
                });
            }
            pulldown_cmark::Tag::Image { dest_url, .. } => {
                self.text("[image: ");
                self.text(dest_url.as_ref());
                self.text("]");
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: pulldown_cmark::TagEnd) {
        match tag {
            pulldown_cmark::TagEnd::Paragraph => {
                self.flush_current();
                self.push_blank();
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
                        self.current.push(Span::styled(
                            display,
                            tui_theme()
                                .accent_style()
                                .add_modifier(Modifier::UNDERLINED),
                        ));
                    } else if !link.suppress_label {
                        self.style_stack.pop();
                    }
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if let Some(code) = self.code_block.as_mut() {
            code.push_str(text);
            return;
        }
        if self
            .link_stack
            .last()
            .is_some_and(|link| link.suppress_label)
        {
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

    fn push_code_block(&mut self, code: &str) {
        let theme = tui_theme();
        for line in code.trim_end_matches('\n').lines() {
            self.lines.push(Line::from(vec![
                Span::styled("  ".to_string(), theme.dim_style()),
                Span::styled(line.to_string(), theme.code_style()),
            ]));
        }
        if code.trim_end_matches('\n').is_empty() {
            self.lines.push(Line::from(vec![
                Span::styled("  ".to_string(), theme.dim_style()),
                Span::styled(" ".to_string(), theme.code_style()),
            ]));
        }
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
