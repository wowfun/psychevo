#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn diff_overlay_from_workspace_diff(diff: &WorkspaceDiff) -> DiffOverlay {
    DiffOverlay::from_lines(workspace_diff_lines(diff))
}

pub(crate) fn workspace_diff_plain_text(diff: &WorkspaceDiff) -> String {
    if !diff.is_git_repo {
        return "Not a git repository.".to_string();
    }
    if diff.is_empty() {
        return "No changes detected.".to_string();
    }
    diff.unified_diff.clone()
}

pub(crate) fn workspace_diff_lines(diff: &WorkspaceDiff) -> Vec<Line<'static>> {
    if !diff.is_git_repo {
        return vec![Line::from("Not a git repository.")];
    }
    if diff.is_empty() {
        return vec![Line::from("No changes detected.")];
    }
    render_unified_diff_lines(&diff.unified_diff)
}

pub(crate) fn render_unified_diff_lines(input: &str) -> Vec<Line<'static>> {
    let theme = tui_theme();
    let mut old_line: Option<u64> = None;
    let mut new_line: Option<u64> = None;
    let mut lang = String::new();
    let mut lines = Vec::new();
    for line in input.lines() {
        if line.starts_with("diff --git ") {
            lang = diff_header_lang(line).unwrap_or_default();
            old_line = None;
            new_line = None;
            lines.push(Line::from(Span::styled(
                line.to_string(),
                theme.accent_style().add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if line.starts_with("+++ ") || line.starts_with("--- ") || line.starts_with("index ") {
            if let Some(path) = line.strip_prefix("+++ b/") {
                lang = lang_for_path(path);
            }
            lines.push(Line::from(Span::styled(
                line.to_string(),
                theme.dim_style(),
            )));
            continue;
        }
        if line.starts_with("@@") {
            if let Some((old_start, new_start)) = parse_hunk_header(line) {
                old_line = Some(old_start);
                new_line = Some(new_start);
            }
            lines.push(Line::from(vec![
                Span::styled("           | ".to_string(), theme.dim_style()),
                Span::styled(line.to_string(), theme.identity_style()),
            ]));
            continue;
        }
        if line.starts_with("[diff truncated:")
            || line.starts_with("[binary ")
            || line.starts_with("[unreadable ")
        {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                theme.error_style().add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        let (prefix, content, style) = if let Some(content) = line.strip_prefix('+') {
            let prefix = number_prefix(None, new_line);
            new_line = new_line.map(|value| value + 1);
            (prefix, format!("+{content}"), theme.success_style())
        } else if let Some(content) = line.strip_prefix('-') {
            let prefix = number_prefix(old_line, None);
            old_line = old_line.map(|value| value + 1);
            (prefix, format!("-{content}"), theme.error_style())
        } else {
            let content = line.strip_prefix(' ').unwrap_or(line);
            let prefix = number_prefix(old_line, new_line);
            old_line = old_line.map(|value| value + 1);
            new_line = new_line.map(|value| value + 1);
            (prefix, format!(" {content}"), Style::default())
        };
        let mut spans = vec![Span::styled(prefix, theme.dim_style())];
        if style == Style::default() {
            spans.extend(highlight_code_line(&content, &lang));
        } else {
            spans.push(Span::styled(content, style));
        }
        lines.push(Line::from(spans));
    }
    lines
}

#[derive(Debug)]
pub(crate) struct InlineEditDiffRender {
    pub(crate) title: String,
    pub(crate) lines: Vec<Line<'static>>,
}

#[derive(Debug)]
struct ParsedGitDiffFile {
    old_path: String,
    new_path: String,
    rename_from: Option<String>,
    rename_to: Option<String>,
    lang: String,
    rows: Vec<InlineDiffRow>,
    additions: usize,
    deletions: usize,
}

#[derive(Debug)]
enum InlineDiffRow {
    HunkGap,
    Line {
        number: u64,
        kind: InlineDiffKind,
        content: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineDiffKind {
    Add,
    Delete,
    Context,
}

pub(crate) fn render_inline_edit_diff(input: &str, width: u16) -> Option<InlineEditDiffRender> {
    let files = parse_git_patch_blocks(input)?;
    let title = inline_edit_diff_title(&files);
    let lines = render_inline_git_patch_files(&files, width);
    Some(InlineEditDiffRender { title, lines })
}

fn parse_git_patch_blocks(input: &str) -> Option<Vec<ParsedGitDiffFile>> {
    let mut files = Vec::new();
    let mut current: Option<ParsedGitDiffFile> = None;
    let mut old_line: Option<u64> = None;
    let mut new_line: Option<u64> = None;
    let mut saw_hunk = false;

    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(file) = current.take() {
                files.push(file);
            }
            let (old_path, new_path) = parse_diff_git_paths(rest)?;
            current = Some(ParsedGitDiffFile {
                lang: lang_for_path(&new_path),
                old_path,
                new_path,
                rename_from: None,
                rename_to: None,
                rows: Vec::new(),
                additions: 0,
                deletions: 0,
            });
            old_line = None;
            new_line = None;
            saw_hunk = false;
            continue;
        }

        let Some(file) = current.as_mut() else {
            if line.trim().is_empty() {
                continue;
            }
            return None;
        };

        if let Some(path) = line.strip_prefix("rename from ") {
            file.rename_from = Some(path.to_string());
            continue;
        }
        if let Some(path) = line.strip_prefix("rename to ") {
            file.rename_to = Some(path.to_string());
            file.lang = lang_for_path(path);
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ b/") {
            file.lang = lang_for_path(path);
            continue;
        }
        if line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("index ")
            || line.starts_with("new file mode ")
            || line.starts_with("deleted file mode ")
            || line.starts_with("similarity index ")
        {
            continue;
        }
        if line.starts_with("@@") {
            let (old_start, new_start) = parse_hunk_header(line)?;
            old_line = Some(old_start);
            new_line = Some(new_start);
            if saw_hunk {
                file.rows.push(InlineDiffRow::HunkGap);
            }
            saw_hunk = true;
            continue;
        }
        if line.starts_with("\\ No newline") {
            continue;
        }

        let (Some(old), Some(new)) = (old_line.as_mut(), new_line.as_mut()) else {
            if line.trim().is_empty() {
                continue;
            }
            return None;
        };
        if let Some(content) = line.strip_prefix('+') {
            file.rows.push(InlineDiffRow::Line {
                number: *new,
                kind: InlineDiffKind::Add,
                content: content.to_string(),
            });
            *new += 1;
            file.additions += 1;
        } else if let Some(content) = line.strip_prefix('-') {
            file.rows.push(InlineDiffRow::Line {
                number: *old,
                kind: InlineDiffKind::Delete,
                content: content.to_string(),
            });
            *old += 1;
            file.deletions += 1;
        } else {
            let content = line.strip_prefix(' ').unwrap_or(line);
            file.rows.push(InlineDiffRow::Line {
                number: *new,
                kind: InlineDiffKind::Context,
                content: content.to_string(),
            });
            *old += 1;
            *new += 1;
        }
    }

    if let Some(file) = current {
        files.push(file);
    }
    (!files.is_empty()
        && files.iter().all(|file| {
            !file.rows.is_empty() || file.rename_from.is_some() || file.rename_to.is_some()
        }))
    .then_some(files)
}

fn parse_diff_git_paths(input: &str) -> Option<(String, String)> {
    let mut parts = input.split_whitespace();
    let old = parts.next()?;
    let new = parts.next()?;
    Some((strip_diff_path_prefix(old), strip_diff_path_prefix(new)))
}

fn strip_diff_path_prefix(path: &str) -> String {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
        .to_string()
}

fn inline_edit_diff_title(files: &[ParsedGitDiffFile]) -> String {
    let additions = files.iter().map(|file| file.additions).sum::<usize>();
    let deletions = files.iter().map(|file| file.deletions).sum::<usize>();
    if let [file] = files {
        return format!(
            "Edited {} (+{} -{})",
            inline_file_display_path(file),
            additions,
            deletions
        );
    }
    format!(
        "Edited {} files (+{} -{})",
        files.len(),
        additions,
        deletions
    )
}

fn inline_file_display_path(file: &ParsedGitDiffFile) -> String {
    match (&file.rename_from, &file.rename_to) {
        (Some(from), Some(to)) => format!("{from} -> {to}"),
        _ if file.new_path == "/dev/null" => file.old_path.clone(),
        _ => file.new_path.clone(),
    }
}

fn render_inline_git_patch_files(files: &[ParsedGitDiffFile], width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let max_line_number = files
        .iter()
        .flat_map(|file| file.rows.iter())
        .filter_map(|row| match row {
            InlineDiffRow::Line { number, .. } => Some(*number),
            InlineDiffRow::HunkGap => None,
        })
        .max()
        .unwrap_or(1);
    let gutter_width = max_line_number.to_string().len().max(1);
    let multi_file = files.len() > 1;
    let mut lines = Vec::new();
    for file in files {
        if multi_file {
            lines.push(padded_plain_line(
                inline_file_display_path(file),
                tui_theme().accent_style().add_modifier(Modifier::BOLD),
                width,
            ));
        }
        if let (Some(from), Some(to)) = (&file.rename_from, &file.rename_to) {
            lines.push(padded_plain_line(
                format!("rename {from} -> {to}"),
                tui_theme().dim_style(),
                width,
            ));
        }
        for row in &file.rows {
            match row {
                InlineDiffRow::HunkGap => lines.push(render_inline_hunk_gap(gutter_width, width)),
                InlineDiffRow::Line {
                    number,
                    kind,
                    content,
                } => lines.push(render_inline_diff_row(
                    *number,
                    *kind,
                    content,
                    &file.lang,
                    gutter_width,
                    width,
                )),
            }
        }
    }
    lines
}

fn render_inline_hunk_gap(gutter_width: usize, width: usize) -> Line<'static> {
    padded_plain_line(
        format!("{:>gutter_width$}  ⋮", ""),
        tui_theme().dim_style(),
        width,
    )
}

fn render_inline_diff_row(
    number: u64,
    kind: InlineDiffKind,
    content: &str,
    lang: &str,
    gutter_width: usize,
    width: usize,
) -> Line<'static> {
    let theme = tui_theme();
    let (sign, line_style, content_style) = match kind {
        InlineDiffKind::Add => (
            "+",
            Style::default().bg(Color::Rgb(0, 84, 0)),
            theme.success_style().bg(Color::Rgb(0, 84, 0)),
        ),
        InlineDiffKind::Delete => (
            "-",
            Style::default().bg(Color::Rgb(92, 0, 0)),
            theme.error_style().bg(Color::Rgb(92, 0, 0)),
        ),
        InlineDiffKind::Context => (" ", Style::default(), Style::default()),
    };
    let mut spans = vec![
        Span::styled(
            format!("{number:>gutter_width$} "),
            theme.dim_style().patch(line_style),
        ),
        Span::styled(sign.to_string(), content_style),
    ];
    if kind == InlineDiffKind::Context {
        spans.extend(highlight_code_line(content, lang));
    } else {
        spans.push(Span::styled(content.to_string(), content_style));
    }
    pad_spans_to_width(&mut spans, width, line_style);
    Line::from(spans).style(line_style)
}

fn padded_plain_line(text: String, style: Style, width: usize) -> Line<'static> {
    let mut spans = vec![Span::styled(text, style)];
    pad_spans_to_width(&mut spans, width, Style::default());
    Line::from(spans)
}

fn pad_spans_to_width(spans: &mut Vec<Span<'static>>, width: usize, style: Style) {
    let used = spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum::<usize>();
    let padding = width.saturating_sub(used);
    if padding > 0 {
        spans.push(Span::styled(" ".repeat(padding), style));
    }
}

fn number_prefix(old_line: Option<u64>, new_line: Option<u64>) -> String {
    let old = old_line.map(|value| value.to_string()).unwrap_or_default();
    let new = new_line.map(|value| value.to_string()).unwrap_or_default();
    format!("{old:>5} {new:>5} | ")
}

fn parse_hunk_header(line: &str) -> Option<(u64, u64)> {
    let header = line.strip_prefix("@@ ")?;
    let end = header.find(" @@")?;
    let mut parts = header[..end].split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    Some((parse_hunk_start(old)?, parse_hunk_start(new)?))
}

fn parse_hunk_start(value: &str) -> Option<u64> {
    value.split(',').next()?.parse().ok()
}

fn diff_header_lang(line: &str) -> Option<String> {
    let path = line.split_whitespace().last()?.strip_prefix("b/")?;
    Some(lang_for_path(path))
}

fn lang_for_path(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_diff_with_line_numbers_and_hunks() {
        let lines = render_unified_diff_lines(
            "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,3 @@\n fn old() {}\n+fn new() {}\n",
        );
        let text = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("@@ -1,2 +1,3 @@"));
        assert!(text.contains("    1     1 |  fn old() {}"));
        assert!(text.contains("          2 | +fn new() {}"));
    }

    #[test]
    fn renders_inline_edit_diff_with_codex_style_single_gutter() {
        let rendered = render_inline_edit_diff(
            "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n fn old() {}\n-fn remove() {}\n+fn add() {}\n",
            80,
        )
        .expect("inline diff");
        assert_eq!(rendered.title, "Edited src/lib.rs (+1 -1)");
        let text = rendered
            .lines
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("1  fn old() {}"), "{text}");
        assert!(text.contains("2 -fn remove() {}"), "{text}");
        assert!(text.contains("2 +fn add() {}"), "{text}");
        assert!(!text.contains("1     1 |"), "{text}");
    }

    #[test]
    fn renders_inline_edit_rename_header() {
        let rendered = render_inline_edit_diff(
            "diff --git a/src/old.rs b/src/new.rs\nsimilarity index 100%\nrename from src/old.rs\nrename to src/new.rs\n",
            80,
        )
        .expect("rename diff");
        assert_eq!(rendered.title, "Edited src/old.rs -> src/new.rs (+0 -0)");
        let text = rendered
            .lines
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("rename src/old.rs -> src/new.rs"), "{text}");
    }

    #[test]
    fn inline_edit_diff_falls_back_on_malformed_diff() {
        assert!(render_inline_edit_diff("not a patch", 80).is_none());
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }
}
