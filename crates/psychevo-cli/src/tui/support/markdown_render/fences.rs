#[allow(unused_imports)]
pub(crate) use super::*;
use std::borrow::Cow;

pub(crate) fn unwrap_markdown_table_fences(input: &str) -> Cow<'_, str> {
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

pub(crate) struct FenceOpen {
    pub(crate) marker: char,
    pub(crate) len: usize,
    pub(crate) info: String,
}

pub(crate) fn parse_fence_open(line: &str) -> Option<FenceOpen> {
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

pub(crate) fn is_fence_close(line: &str, marker: char, len: usize) -> bool {
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

pub(crate) fn is_markdown_fence_info(info: &str) -> bool {
    matches!(
        info.split_whitespace().next().unwrap_or_default(),
        "md" | "markdown"
    )
}

pub(crate) fn is_markdown_table_like(lines: &[&str]) -> bool {
    let mut non_empty = lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty());
    let Some(header) = non_empty.next() else {
        return false;
    };
    let Some(delimiter) = non_empty.next() else {
        return false;
    };
    header.contains('|') && is_table_delimiter_line(delimiter)
}

pub(crate) fn is_table_delimiter_line(line: &str) -> bool {
    let trimmed = line.trim().trim_matches('|');
    let cells = trimmed.split('|').map(str::trim).collect::<Vec<_>>();
    cells.len() >= 2
        && cells.iter().all(|cell| {
            let cell = cell.trim_matches(':').trim();
            cell.len() >= 3 && cell.chars().all(|ch| ch == '-')
        })
}

pub(crate) fn local_link_display(destination: &str, cwd: &Path) -> Option<String> {
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

pub(crate) fn split_location_suffix(value: &str) -> (&str, &str) {
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
