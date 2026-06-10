#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) fn new_textarea<'a>() -> TextArea<'a> {
    let mut textarea = TextArea::default();
    apply_composer_textarea_style(&mut textarea);
    textarea
}

pub(crate) fn textarea_with_text<'a>(text: &str) -> TextArea<'a> {
    let mut textarea = TextArea::new(text.split('\n').map(ToString::to_string).collect());
    apply_composer_textarea_style(&mut textarea);
    textarea.move_cursor(CursorMove::Bottom);
    textarea.move_cursor(CursorMove::End);
    textarea
}

pub(crate) fn apply_composer_textarea_style(textarea: &mut TextArea<'_>) {
    let theme = tui_theme();
    let style = theme.surface_style();
    textarea.set_block(Block::default().style(style));
    textarea.set_style(style);
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
    textarea.set_cursor_line_style(Style::default());
    textarea.set_selection_style(text_selection_style());
}

pub(crate) fn composer_marker_width(
    area_width: u16,
    shell_mode: bool,
    textarea_empty: bool,
) -> u16 {
    if shell_mode {
        area_width.min(2)
    } else if textarea_empty {
        area_width.min(1)
    } else {
        area_width.min(2)
    }
}

pub(crate) fn composer_input_width(area_width: u16, shell_mode: bool, textarea_empty: bool) -> u16 {
    area_width.saturating_sub(composer_marker_width(
        area_width,
        shell_mode,
        textarea_empty,
    ))
}

pub(crate) fn composer_cursor_from_point(
    textarea: &TextArea<'_>,
    area: Rect,
    top_row: u16,
    column: u16,
    row: u16,
) -> Option<(usize, usize)> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let lines = textarea.lines();
    if lines.is_empty() {
        return Some((0, 0));
    }
    let row_offset = row
        .saturating_sub(area.y)
        .min(area.height.saturating_sub(1)) as usize;
    let display_col = column.saturating_sub(area.x).min(area.width) as usize;
    let target_screen_row = usize::from(top_row).saturating_add(row_offset);
    composer_text_position_at_screen_row(
        textarea,
        usize::from(area.width.max(1)),
        target_screen_row,
        display_col,
    )
}

pub(crate) fn composer_terminal_cursor_position(
    textarea: &TextArea<'_>,
    area: Rect,
    top_row: &mut u16,
) -> Option<(u16, u16)> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let cursor = textarea.screen_cursor();
    let cursor_row = cursor.row.min(usize::from(u16::MAX)) as u16;
    let cursor_col = cursor.col.min(usize::from(u16::MAX)) as u16;
    *top_row = next_composer_cursor_top_row(*top_row, cursor_row, area.height);
    let visible_row = cursor_row
        .saturating_sub(*top_row)
        .min(area.height.saturating_sub(1));
    let visible_col = cursor_col.min(area.width.saturating_sub(1));
    Some((
        area.x.saturating_add(visible_col),
        area.y.saturating_add(visible_row),
    ))
}

pub(crate) fn next_composer_cursor_top_row(prev_top: u16, cursor_row: u16, height: u16) -> u16 {
    if height == 0 {
        return 0;
    }
    if cursor_row < prev_top {
        cursor_row
    } else if prev_top.saturating_add(height) <= cursor_row {
        cursor_row.saturating_add(1).saturating_sub(height)
    } else {
        prev_top
    }
}

pub(crate) fn composer_char_col_at_display_col(
    line: &str,
    display_col: usize,
    tab_len: u8,
) -> usize {
    let mut width = 0usize;
    for (index, ch) in line.chars().enumerate() {
        width = width.saturating_add(composer_char_display_width(ch, width, tab_len));
        if width > display_col {
            return index;
        }
    }
    line.chars().count()
}

pub(crate) fn textarea_text(textarea: &TextArea<'_>) -> String {
    textarea.lines().join("\n")
}

pub(crate) fn textarea_with_lines_and_cursor<'a>(
    lines: Vec<String>,
    row: usize,
    col: usize,
) -> TextArea<'a> {
    let mut textarea = TextArea::new(lines);
    apply_composer_textarea_style(&mut textarea);
    textarea.move_cursor(CursorMove::Jump(
        row.min(u16::MAX as usize) as u16,
        col.min(u16::MAX as usize) as u16,
    ));
    textarea
}

pub(crate) fn current_file_token(textarea: &TextArea<'_>) -> Option<FileToken> {
    let cursor = textarea.cursor();
    let row = cursor.0;
    let col = cursor.1;
    let line = textarea.lines().get(row)?;
    file_token_on_line(row, line, col)
}

pub(crate) fn current_skill_token(textarea: &TextArea<'_>) -> Option<SkillToken> {
    let cursor = textarea.cursor();
    let row = cursor.0;
    let col = cursor.1;
    let line = textarea.lines().get(row)?;
    skill_token_on_line(row, line, col)
}

pub(crate) fn current_agent_token(textarea: &TextArea<'_>) -> Option<AgentToken> {
    let cursor = textarea.cursor();
    let row = cursor.0;
    let col = cursor.1;
    let line = textarea.lines().get(row)?;
    agent_token_on_line(row, line, col)
}

pub(crate) fn skill_token_on_line(row: usize, line: &str, cursor_col: usize) -> Option<SkillToken> {
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

pub(crate) fn file_token_on_line(row: usize, line: &str, cursor_col: usize) -> Option<FileToken> {
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

pub(crate) fn agent_token_on_line(row: usize, line: &str, cursor_col: usize) -> Option<AgentToken> {
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

pub(crate) fn replace_current_file_token(textarea: &mut TextArea<'_>, path: &str) -> bool {
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

pub(crate) fn replace_current_skill_token(textarea: &mut TextArea<'_>, name: &str) -> bool {
    let Some(token) = current_skill_token(textarea) else {
        return false;
    };
    replace_token_range(
        textarea,
        token.row,
        token.start_col,
        token.end_col,
        &format!("${name}"),
    )
}

pub(crate) fn replace_current_agent_token(textarea: &mut TextArea<'_>, name: &str) -> bool {
    let Some(token) = current_agent_token(textarea) else {
        return false;
    };
    replace_token_range(
        textarea,
        token.row,
        token.start_col,
        token.end_col,
        &format!("@{name}"),
    )
}

pub(crate) fn replace_token_range(
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

pub(crate) fn prompt_file_path(path: &str) -> String {
    if path.chars().any(char::is_whitespace) && !path.contains('"') {
        format!("\"{path}\"")
    } else {
        path.to_string()
    }
}

pub(crate) fn search_workdir_files(
    root: &Path,
    query: &str,
    cancel: &AtomicBool,
) -> Vec<FileSearchMatch> {
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

pub(crate) fn is_vcs_dir_entry(entry: &ignore::DirEntry) -> bool {
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

pub(crate) fn relative_path_text(path: &Path) -> Option<String> {
    let text = path.to_string_lossy().replace('\\', "/");
    (!text.is_empty()).then_some(text)
}

pub(crate) fn file_kind_rank(kind: FileSearchMatchKind) -> u8 {
    match kind {
        FileSearchMatchKind::Directory => 0,
        FileSearchMatchKind::File => 1,
    }
}

pub(crate) fn file_match_rank(path: &str, query: &str) -> Option<(u8, usize)> {
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

pub(crate) fn fuzzy_subsequence_score(haystack: &str, query: &str) -> Option<usize> {
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

pub(crate) fn is_newline_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT)
}

pub(crate) fn composer_height(textarea: &TextArea<'_>, input_width: u16) -> u16 {
    let rows = composer_screen_row_count(textarea, input_width);
    rows.min(usize::from(COMPOSER_MAX_VISIBLE_ROWS)) as u16
}

pub(crate) const COMPOSER_MAX_VISIBLE_ROWS: u16 = 6;

fn composer_screen_row_count(textarea: &TextArea<'_>, input_width: u16) -> usize {
    let width = usize::from(input_width.max(1));
    textarea
        .lines()
        .iter()
        .map(|line| composer_wrapped_line_segments(line, width, textarea.tab_length()).len())
        .sum::<usize>()
        .max(1)
}

fn composer_text_position_at_screen_row(
    textarea: &TextArea<'_>,
    width: usize,
    target_screen_row: usize,
    display_col: usize,
) -> Option<(usize, usize)> {
    let tab_len = textarea.tab_length();
    let mut screen_row = 0usize;
    for (text_row, line) in textarea.lines().iter().enumerate() {
        let segments = composer_wrapped_line_segments(line, width, tab_len);
        for segment in segments {
            if screen_row == target_screen_row {
                let fragment = &line[segment.start_byte..segment.end_byte];
                let relative_col = composer_char_col_at_display_col(fragment, display_col, tab_len);
                return Some((
                    text_row,
                    segment
                        .start_col
                        .saturating_add(relative_col)
                        .min(segment.end_col),
                ));
            }
            screen_row = screen_row.saturating_add(1);
        }
    }
    textarea
        .lines()
        .iter()
        .enumerate()
        .next_back()
        .map(|(row, line)| (row, line.chars().count()))
}

#[derive(Clone, Copy)]
struct ComposerWrappedSegment {
    start_byte: usize,
    end_byte: usize,
    start_col: usize,
    end_col: usize,
}

fn composer_wrapped_line_segments(
    line: &str,
    width: usize,
    tab_len: u8,
) -> Vec<ComposerWrappedSegment> {
    let ranges = composer_wrapped_line_ranges(line, width, tab_len);
    let mut start_col = 0usize;
    ranges
        .into_iter()
        .map(|(start_byte, end_byte)| {
            let char_count = line[start_byte..end_byte].chars().count();
            let segment = ComposerWrappedSegment {
                start_byte,
                end_byte,
                start_col,
                end_col: start_col.saturating_add(char_count),
            };
            start_col = segment.end_col;
            segment
        })
        .collect()
}

fn composer_wrapped_line_ranges(line: &str, width: usize, tab_len: u8) -> Vec<(usize, usize)> {
    let width = width.max(1);
    if line.is_empty() {
        return vec![(0, 0)];
    }

    let chunks = composer_word_like_chunks(line);
    let mut ranges = Vec::new();
    let mut index = 0usize;
    let mut segment_start = chunks[0].0;
    let mut segment_end = segment_start;
    let mut segment_width = 0usize;

    while index < chunks.len() {
        let (chunk_start, chunk_end) = chunks[index];
        if segment_end == segment_start {
            segment_start = chunk_start;
        }
        let chunk_width =
            composer_display_width_from(&line[chunk_start..chunk_end], segment_width, tab_len);
        if segment_width.saturating_add(chunk_width) <= width {
            segment_end = chunk_end;
            segment_width = segment_width.saturating_add(chunk_width);
            index = index.saturating_add(1);
            continue;
        }
        if segment_end > segment_start {
            ranges.push((segment_start, segment_end));
            segment_start = segment_end;
            segment_width = 0;
            continue;
        }
        composer_split_range_by_width(line, chunk_start, chunk_end, width, tab_len, &mut ranges);
        index = index.saturating_add(1);
        segment_start = chunk_end;
        segment_end = chunk_end;
        segment_width = 0;
    }
    if segment_end > segment_start {
        ranges.push((segment_start, segment_end));
    }
    if ranges.is_empty() {
        ranges.push((0, 0));
    }
    ranges
}

fn composer_word_like_chunks(line: &str) -> Vec<(usize, usize)> {
    let mut chunks = Vec::new();
    let mut start = None::<usize>;
    let mut whitespace = None::<bool>;
    for (index, ch) in line.char_indices() {
        let is_whitespace = ch.is_whitespace();
        match (start, whitespace) {
            (None, _) => {
                start = Some(index);
                whitespace = Some(is_whitespace);
            }
            (Some(chunk_start), Some(current)) if current != is_whitespace => {
                chunks.push((chunk_start, index));
                start = Some(index);
                whitespace = Some(is_whitespace);
            }
            _ => {}
        }
    }
    if let Some(chunk_start) = start {
        chunks.push((chunk_start, line.len()));
    }
    chunks
}

fn composer_split_range_by_width(
    line: &str,
    start: usize,
    end: usize,
    width: usize,
    tab_len: u8,
    ranges: &mut Vec<(usize, usize)>,
) {
    let mut segment_start = start;
    while segment_start < end {
        let mut segment_end = segment_start;
        let mut segment_width = 0usize;
        for (offset, ch) in line[segment_start..end].char_indices() {
            let char_start = segment_start.saturating_add(offset);
            let char_end = char_start.saturating_add(ch.len_utf8());
            let char_width = composer_char_display_width(ch, segment_width, tab_len);
            if segment_end != segment_start && segment_width.saturating_add(char_width) > width {
                break;
            }
            segment_end = char_end;
            segment_width = segment_width.saturating_add(char_width);
            if segment_width > width {
                break;
            }
        }
        if segment_end == segment_start {
            if let Some(ch) = line[segment_start..end].chars().next() {
                segment_end = segment_start.saturating_add(ch.len_utf8());
            } else {
                break;
            }
        }
        ranges.push((segment_start, segment_end));
        segment_start = segment_end;
    }
}

fn composer_display_width_from(text: &str, start_width: usize, tab_len: u8) -> usize {
    let mut width = start_width;
    for ch in text.chars() {
        width = width.saturating_add(composer_char_display_width(ch, width, tab_len));
    }
    width.saturating_sub(start_width)
}

fn composer_char_display_width(ch: char, current_width: usize, tab_len: u8) -> usize {
    if ch == '\t' {
        let tab = usize::from(tab_len.max(1));
        tab.saturating_sub(current_width % tab).max(1)
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}
