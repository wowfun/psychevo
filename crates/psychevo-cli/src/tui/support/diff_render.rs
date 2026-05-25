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
}
