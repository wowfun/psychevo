struct GitSnapshot {
    branch: String,
    changed_files: Vec<String>,
}

fn git_snapshot(workdir: &PathBuf) -> GitSnapshot {
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

fn tail_compact_status_line(line: &str) -> String {
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

fn tail_compact_path(path: &str, max_chars: usize) -> String {
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

