#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) struct GitSnapshot {
    pub(crate) branch: String,
    pub(crate) changed_files: Vec<String>,
}

pub(crate) fn git_snapshot(workdir: &PathBuf) -> GitSnapshot {
    let branch = StdCommand::new("git")
        .arg("-C")
        .arg(workdir)
        .args(["branch", "--show-current"])
        .output()
        .ok()
        .and_then(|output| {
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "(none)".to_string());
    let changed_files = StdCommand::new("git")
        .arg("-C")
        .arg(workdir)
        .args(["status", "--short"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .take(10)
                .map(tail_compact_status_line)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    GitSnapshot {
        branch,
        changed_files,
    }
}

pub(crate) fn tail_compact_status_line(line: &str) -> String {
    let mut parts = line.split_whitespace().collect::<Vec<_>>();
    let Some(path) = parts.pop() else {
        return line.to_string();
    };
    let prefix = line
        .strip_suffix(path)
        .map(str::trim_end)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    let compact = tail_compact_path(path, 32);
    if prefix.is_empty() {
        compact
    } else {
        format!("{prefix} {compact}")
    }
}

pub(crate) fn tail_compact_path(path: &str, max_chars: usize) -> String {
    if path.chars().count() <= max_chars {
        return path.to_string();
    }
    let tail = path
        .chars()
        .rev()
        .take(max_chars.saturating_sub(3))
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("...{tail}")
}

pub(crate) fn home_dir_for_display(app: &TuiApp) -> Option<PathBuf> {
    app.env_map
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

pub(crate) fn format_directory_display_with_home(
    directory: &Path,
    home: Option<&Path>,
    max_width: usize,
) -> String {
    center_truncate_path(&directory_display_value(directory, home), max_width)
}

pub(crate) fn directory_display_value(directory: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home
        && !home.as_os_str().is_empty()
        && let Ok(relative) = directory.strip_prefix(home)
    {
        if relative.as_os_str().is_empty() {
            return "~".to_string();
        }
        return format!("~/{}", relative.display());
    }
    directory.display().to_string()
}

pub(crate) fn center_truncate_path(path: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(path) <= max_width {
        return path.to_string();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let available = max_width.saturating_sub(1);
    let prefix_width = available / 2;
    let suffix_width = available.saturating_sub(prefix_width);
    let prefix = take_width_prefix(path, prefix_width);
    let suffix = take_width_suffix(path, suffix_width);
    format!("{prefix}…{suffix}")
}

pub(crate) fn take_width_prefix(text: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width.saturating_add(ch_width) > max_width {
            break;
        }
        out.push(ch);
        width = width.saturating_add(ch_width);
    }
    out
}

pub(crate) fn take_width_suffix(text: &str, max_width: usize) -> String {
    let mut chars = Vec::new();
    let mut width = 0usize;
    for ch in text.chars().rev() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width.saturating_add(ch_width) > max_width {
            break;
        }
        chars.push(ch);
        width = width.saturating_add(ch_width);
    }
    chars.into_iter().rev().collect()
}
