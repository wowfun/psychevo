#[derive(Debug)]
pub(crate) struct PatchFile {
    pub(crate) path: String,
    pub(crate) hunks: Vec<PatchHunk>,
}

#[derive(Debug)]
pub(crate) struct PatchHunk {
    pub(crate) old_lines: Vec<String>,
    pub(crate) new_lines: Vec<String>,
}

pub(crate) fn parse_unified_patch(patch: &str) -> Result<Vec<PatchFile>> {
    let mut files = Vec::new();
    let mut lines = patch.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with("--- ") {
            continue;
        }
        let old = line.trim_start_matches("--- ").trim();
        let new = lines
            .next()
            .ok_or_else(|| Error::Message("patch missing +++ header".to_string()))?;
        if !new.starts_with("+++ ") {
            return Err(Error::Message("patch missing +++ header".to_string()));
        }
        let new = new.trim_start_matches("+++ ").trim();
        if old == "/dev/null" || new == "/dev/null" {
            return Err(Error::Message(
                "patch add/delete is not supported".to_string(),
            ));
        }
        let path = strip_diff_prefix(new);
        let mut hunks = Vec::new();
        while let Some(next) = lines.peek().copied() {
            if next.starts_with("--- ") {
                break;
            }
            if !next.starts_with("@@") {
                let _ = lines.next();
                continue;
            }
            let _ = lines.next();
            let mut old_lines = Vec::new();
            let mut new_lines = Vec::new();
            while let Some(hunk_line) = lines.peek().copied() {
                if hunk_line.starts_with("@@") || hunk_line.starts_with("--- ") {
                    break;
                }
                let hunk_line = lines.next().expect("peeked line exists");
                if let Some(rest) = hunk_line.strip_prefix(' ') {
                    old_lines.push(rest.to_string());
                    new_lines.push(rest.to_string());
                } else if let Some(rest) = hunk_line.strip_prefix('-') {
                    old_lines.push(rest.to_string());
                } else if let Some(rest) = hunk_line.strip_prefix('+') {
                    new_lines.push(rest.to_string());
                } else if hunk_line.starts_with("\\ No newline") {
                } else {
                    return Err(Error::Message(format!(
                        "unsupported patch line: {hunk_line}"
                    )));
                }
            }
            if old_lines.is_empty() {
                return Err(Error::Message(
                    "empty patch hunks are not supported".to_string(),
                ));
            }
            hunks.push(PatchHunk {
                old_lines,
                new_lines,
            });
        }
        files.push(PatchFile { path, hunks });
    }
    Ok(files)
}

pub(crate) fn strip_diff_prefix(path: &str) -> String {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
        .to_string()
}

pub(crate) fn find_unique_subslice(lines: &[String], needle: &[String]) -> Option<usize> {
    let mut found = None;
    for idx in 0..=lines.len().saturating_sub(needle.len()) {
        if lines[idx..idx + needle.len()] == *needle {
            if found.is_some() {
                return None;
            }
            found = Some(idx);
        }
    }
    found
}

