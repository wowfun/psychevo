fn new_textarea<'a>() -> TextArea<'a> {
    let mut textarea = TextArea::default();
    let style = tui_theme().surface_style();
    textarea.set_block(Block::default().style(style));
    textarea.set_style(style);
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
    textarea.set_cursor_line_style(Style::default());
    textarea
}

fn textarea_with_text<'a>(text: &str) -> TextArea<'a> {
    let mut textarea = TextArea::new(text.split('\n').map(ToString::to_string).collect());
    let style = tui_theme().surface_style();
    textarea.set_block(Block::default().style(style));
    textarea.set_style(style);
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
    textarea.set_cursor_line_style(Style::default());
    textarea.move_cursor(CursorMove::Bottom);
    textarea.move_cursor(CursorMove::End);
    textarea
}

fn textarea_text(textarea: &TextArea<'_>) -> String {
    textarea.lines().join("\n")
}

fn textarea_with_lines_and_cursor<'a>(lines: Vec<String>, row: usize, col: usize) -> TextArea<'a> {
    let mut textarea = TextArea::new(lines);
    let style = tui_theme().surface_style();
    textarea.set_block(Block::default().style(style));
    textarea.set_style(style);
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
    textarea.set_cursor_line_style(Style::default());
    textarea.move_cursor(CursorMove::Jump(
        row.min(u16::MAX as usize) as u16,
        col.min(u16::MAX as usize) as u16,
    ));
    textarea
}

fn current_file_token(textarea: &TextArea<'_>) -> Option<FileToken> {
    let cursor = textarea.cursor();
    let row = cursor.0;
    let col = cursor.1;
    let line = textarea.lines().get(row)?;
    file_token_on_line(row, line, col)
}

fn current_skill_token(textarea: &TextArea<'_>) -> Option<SkillToken> {
    let cursor = textarea.cursor();
    let row = cursor.0;
    let col = cursor.1;
    let line = textarea.lines().get(row)?;
    skill_token_on_line(row, line, col)
}

fn current_agent_token(textarea: &TextArea<'_>) -> Option<AgentToken> {
    let cursor = textarea.cursor();
    let row = cursor.0;
    let col = cursor.1;
    let line = textarea.lines().get(row)?;
    agent_token_on_line(row, line, col)
}

fn skill_token_on_line(row: usize, line: &str, cursor_col: usize) -> Option<SkillToken> {
    let chars = line.chars().collect::<Vec<_>>();
    let cursor_col = cursor_col.min(chars.len());
    let mut start = 0usize;
    while start < chars.len() {
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
        if start >= chars.len() {
            break;
        }
        let mut end = start;
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        if cursor_col >= start
            && cursor_col <= end
            && chars[start] == '$'
            && chars
                .get(start + 1)
                .is_none_or(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        {
            let query = chars[start + 1..end].iter().collect::<String>();
            return Some(SkillToken {
                row,
                start_col: start,
                end_col: end,
                query,
            });
        }
        start = end.saturating_add(1);
    }
    None
}

fn file_token_on_line(row: usize, line: &str, cursor_col: usize) -> Option<FileToken> {
    let chars = line.chars().collect::<Vec<_>>();
    let cursor_col = cursor_col.min(chars.len());
    let mut start = 0usize;
    while start < chars.len() {
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
        if start >= chars.len() {
            break;
        }
        let mut end = start;
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        if cursor_col >= start && cursor_col <= end && chars[start] == '@' {
            let query = chars[start + 1..end].iter().collect::<String>();
            return Some(FileToken {
                row,
                start_col: start,
                end_col: end,
                query,
            });
        }
        start = end.saturating_add(1);
    }
    None
}

fn agent_token_on_line(row: usize, line: &str, cursor_col: usize) -> Option<AgentToken> {
    let chars = line.chars().collect::<Vec<_>>();
    let cursor_col = cursor_col.min(chars.len());
    let mut start = 0usize;
    while start < chars.len() {
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
        if start >= chars.len() {
            break;
        }
        let mut end = start;
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        if cursor_col >= start
            && cursor_col <= end
            && chars[start] == '@'
            && chars[start + 1..end]
                .iter()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || *ch == '-')
        {
            let query = chars[start + 1..end].iter().collect::<String>();
            return Some(AgentToken {
                row,
                start_col: start,
                end_col: end,
                query,
            });
        }
        start = end.saturating_add(1);
    }
    None
}

fn replace_current_file_token(textarea: &mut TextArea<'_>, path: &str) -> bool {
    let Some(token) = current_file_token(textarea) else {
        return false;
    };
    let inserted = prompt_file_path(path);
    let mut lines = textarea.lines().to_vec();
    let Some(line) = lines.get(token.row) else {
        return false;
    };
    let chars = line.chars().collect::<Vec<_>>();
    if token.start_col > token.end_col || token.end_col > chars.len() {
        return false;
    }
    let mut next = String::new();
    next.extend(chars[..token.start_col].iter().copied());
    next.push_str(&inserted);
    next.push(' ');
    next.extend(chars[token.end_col..].iter().copied());
    lines[token.row] = next;
    let next_col = token
        .start_col
        .saturating_add(inserted.chars().count())
        .saturating_add(1);
    *textarea = textarea_with_lines_and_cursor(lines, token.row, next_col);
    true
}

fn replace_current_skill_token(textarea: &mut TextArea<'_>, name: &str) -> bool {
    let Some(token) = current_skill_token(textarea) else {
        return false;
    };
    replace_token_range(textarea, token.row, token.start_col, token.end_col, &format!("${name}"))
}

fn replace_current_agent_token(textarea: &mut TextArea<'_>, name: &str) -> bool {
    let Some(token) = current_agent_token(textarea) else {
        return false;
    };
    replace_token_range(textarea, token.row, token.start_col, token.end_col, &format!("@{name}"))
}

fn replace_token_range(
    textarea: &mut TextArea<'_>,
    row: usize,
    start_col: usize,
    end_col: usize,
    inserted: &str,
) -> bool {
    let mut lines = textarea.lines().to_vec();
    let Some(line) = lines.get(row) else {
        return false;
    };
    let chars = line.chars().collect::<Vec<_>>();
    if start_col > end_col || end_col > chars.len() {
        return false;
    }
    let mut next = String::new();
    next.extend(chars[..start_col].iter().copied());
    next.push_str(inserted);
    next.push(' ');
    next.extend(chars[end_col..].iter().copied());
    lines[row] = next;
    let next_col = start_col
        .saturating_add(inserted.chars().count())
        .saturating_add(1);
    *textarea = textarea_with_lines_and_cursor(lines, row, next_col);
    true
}

fn prompt_file_path(path: &str) -> String {
    if path.chars().any(char::is_whitespace) && !path.contains('"') {
        format!("\"{path}\"")
    } else {
        path.to_string()
    }
}

fn search_workdir_files(root: &Path, query: &str, cancel: &AtomicBool) -> Vec<FileSearchMatch> {
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true);
    builder.filter_entry(|entry| entry.depth() == 0 || !is_vcs_dir_entry(entry));
    let mut matches = Vec::new();
    for entry in builder.build() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if entry.depth() == 0 || is_vcs_dir_entry(&entry) {
            continue;
        }
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let Some(path_text) = relative_path_text(relative) else {
            continue;
        };
        let kind = if entry
            .file_type()
            .is_some_and(|file_type| file_type.is_dir())
        {
            FileSearchMatchKind::Directory
        } else {
            FileSearchMatchKind::File
        };
        let Some(rank) = file_match_rank(&path_text, query) else {
            continue;
        };
        matches.push((
            rank,
            FileSearchMatch {
                path: path_text,
                kind,
            },
        ));
    }
    matches.sort_by(|(left_rank, left), (right_rank, right)| {
        left_rank
            .cmp(right_rank)
            .then_with(|| file_kind_rank(left.kind).cmp(&file_kind_rank(right.kind)))
            .then_with(|| left.path.cmp(&right.path))
    });
    matches
        .into_iter()
        .take(FILE_POPUP_MAX_ROWS)
        .map(|(_, file_match)| file_match)
        .collect()
}

fn is_vcs_dir_entry(entry: &ignore::DirEntry) -> bool {
    if !entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir())
    {
        return false;
    }
    matches!(
        entry.file_name().to_string_lossy().as_ref(),
        ".git" | ".hg" | ".svn"
    )
}

fn relative_path_text(path: &Path) -> Option<String> {
    let text = path.to_string_lossy().replace('\\', "/");
    (!text.is_empty()).then_some(text)
}

fn file_kind_rank(kind: FileSearchMatchKind) -> u8 {
    match kind {
        FileSearchMatchKind::Directory => 0,
        FileSearchMatchKind::File => 1,
    }
}

fn file_match_rank(path: &str, query: &str) -> Option<(u8, usize)> {
    let query = query.trim();
    if query.is_empty() {
        return Some((3, 0));
    }
    let path_lower = path.to_lowercase();
    let query_lower = query.to_lowercase();
    let basename = path_lower
        .rsplit_once('/')
        .map(|(_, basename)| basename)
        .unwrap_or(path_lower.as_str());
    if basename.starts_with(&query_lower) {
        return Some((0, basename.len().saturating_sub(query_lower.len())));
    }
    if let Some(index) = basename.find(&query_lower) {
        return Some((1, index));
    }
    fuzzy_subsequence_score(&path_lower, &query_lower).map(|score| (2, score))
}

fn fuzzy_subsequence_score(haystack: &str, query: &str) -> Option<usize> {
    let haystack = haystack.chars().collect::<Vec<_>>();
    let mut start = 0usize;
    let mut last = 0usize;
    let mut score = 0usize;
    for needle in query.chars() {
        let relative = haystack[start..]
            .iter()
            .position(|candidate| *candidate == needle)?;
        let index = start + relative;
        score = score.saturating_add(index.saturating_sub(last));
        last = index;
        start = index.saturating_add(1);
    }
    Some(score)
}

fn is_newline_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT)
}

fn composer_height(textarea: &TextArea<'_>) -> u16 {
    let lines = textarea.lines().len() as u16;
    lines.clamp(1, 6)
}
