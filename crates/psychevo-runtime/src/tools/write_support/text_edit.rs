#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

#[allow(unused_imports)]
use serde_json::json;

#[derive(Debug, Clone)]
pub(crate) struct TextFile {
    pub(crate) original: String,
    pub(crate) normalized: String,
    pub(crate) bom: bool,
    pub(crate) line_ending: &'static str,
}

#[derive(Debug)]
pub(crate) struct FuzzyOutcome {
    pub(crate) content: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MatchRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct LspBaseline {
    pub(crate) diagnostics: Vec<Value>,
}

pub(crate) type FuzzyStrategy = fn(&str, &str) -> Vec<MatchRange>;
pub(crate) type LspAutoCommand<'a> = Option<(&'a str, &'a [&'a str])>;

pub(crate) struct EditSuccess {
    pub(crate) diff: String,
    pub(crate) files_modified: Vec<String>,
    pub(crate) files_created: Vec<String>,
    pub(crate) files_deleted: Vec<String>,
    pub(crate) files_moved: Vec<Value>,
    pub(crate) lint: Option<Value>,
    pub(crate) lsp_diagnostics: Option<String>,
    pub(crate) warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum V4aOperationKind {
    Add,
    Update,
    Delete,
    Move,
}

#[derive(Debug, Clone)]
pub(crate) struct V4aOperation {
    pub(crate) kind: V4aOperationKind,
    pub(crate) file_path: String,
    pub(crate) new_path: Option<String>,
    pub(crate) hunks: Vec<V4aHunk>,
}

#[derive(Debug, Clone)]
pub(crate) struct V4aHunk {
    pub(crate) context_hint: Option<String>,
    pub(crate) lines: Vec<V4aLine>,
}

#[derive(Debug, Clone)]
pub(crate) struct V4aLine {
    pub(crate) prefix: char,
    pub(crate) content: String,
}

pub(crate) fn read_text_file(path: &Path) -> Result<TextFile> {
    let bytes = fs::read(path)?;
    if bytes.contains(&0) {
        return Err(Error::Message("binary files are not supported".to_string()));
    }
    let original =
        String::from_utf8(bytes).map_err(|_| Error::Message("invalid UTF-8".to_string()))?;
    let bom = original.starts_with('\u{feff}');
    let body = original.trim_start_matches('\u{feff}').to_string();
    let line_ending = dominant_line_ending(&body);
    let normalized = normalize_lf(&body);
    Ok(TextFile {
        original,
        normalized,
        bom,
        line_ending,
    })
}

pub(crate) fn restore_text_file(text: &TextFile, normalized: &str) -> String {
    let restored = restore_line_endings(normalized, text.line_ending);
    if text.bom {
        format!("\u{feff}{restored}")
    } else {
        restored
    }
}

pub(crate) fn result_output(result: Result<Value>) -> ToolOutput {
    match result {
        Ok(value) if value_reports_error(&value) => ToolOutput {
            json: value,
            model_content: None,
            attachments: Vec::new(),
            is_error: true,
        },
        Ok(value) => ToolOutput::ok(value),
        Err(err) => ToolOutput::error(err.to_string()),
    }
}

pub(crate) fn write_text_to_target(
    tool: &WorkdirTool,
    target: &Path,
    content: &str,
    dirs_created: bool,
    pre_content: Option<&str>,
    warning: Option<String>,
) -> Result<Value> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let baseline = snapshot_lsp_baseline(tool, target, pre_content);
    fs::write(target, content)?;
    let verify = String::from_utf8(fs::read(target)?)
        .map_err(|_| Error::Message("invalid UTF-8 after write".to_string()))?;
    if verify != content {
        return Err(Error::Message(format!(
            "post-write verification failed for {}",
            target.display()
        )));
    }
    note_file_write(tool.task_id(), target);
    let lint = check_lint_delta(target, pre_content, content);
    let lint_allows_lsp = lint_allows_lsp(&lint);
    let lsp_diagnostics = if lint_allows_lsp {
        lsp_diagnostics_after(tool, target, pre_content, content, baseline)
    } else {
        None
    };
    Ok(json!({
        "path": tool.relative(target),
        "bytes_written": content.len(),
        "dirs_created": dirs_created,
        "lint": lint,
        "lsp_diagnostics": lsp_diagnostics,
        "warning": warning,
        "error": null
    }))
}

pub(crate) fn edit_success_value(result: EditSuccess) -> Value {
    json!({
        "success": true,
        "diff": result.diff,
        "files_modified": result.files_modified,
        "files_created": result.files_created,
        "files_deleted": result.files_deleted,
        "files_moved": result.files_moved,
        "lint": result.lint,
        "lsp_diagnostics": result.lsp_diagnostics,
        "warning": result.warning,
        "error": null
    })
}

pub(crate) fn unified_diff_named(old_path: &str, new_path: &str, old: &str, new: &str) -> String {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .header(old_path, new_path)
        .to_string()
}

pub(crate) fn write_edit_text(
    tool: &WorkdirTool,
    target: &Path,
    content: &str,
    pre_content: Option<&str>,
) -> Result<(Option<Value>, Option<String>)> {
    let baseline = snapshot_lsp_baseline(tool, target, pre_content);
    fs::write(target, content)?;
    let verify = String::from_utf8(fs::read(target)?)
        .map_err(|_| Error::Message("invalid UTF-8 after write".to_string()))?;
    if normalize_lf(&verify) != normalize_lf(content) {
        return Err(Error::Message(format!(
            "post-write verification failed for {}",
            target.display()
        )));
    }
    note_file_write(tool.task_id(), target);
    let lint = check_lint_delta(target, pre_content, content);
    let lsp = if lint_allows_lsp(&lint) {
        lsp_diagnostics_after(tool, target, pre_content, content, baseline)
    } else {
        None
    };
    Ok((lint, lsp))
}

pub(crate) fn lint_allows_lsp(lint: &Option<Value>) -> bool {
    lint.as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .map(|status| status == "ok" || status == "skipped")
        .unwrap_or(true)
}

pub(crate) fn check_lint_delta(
    path: &Path,
    pre_content: Option<&str>,
    post_content: &str,
) -> Option<Value> {
    let post = check_lint(path, post_content);
    if post
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status == "ok" || status == "skipped")
    {
        return Some(post);
    }
    let Some(pre_content) = pre_content else {
        return Some(post);
    };
    let pre = check_lint(path, pre_content);
    if pre.get("status") == post.get("status") && pre.get("output") == post.get("output") {
        return Some(json!({
            "status": "skipped",
            "message": "Pre-existing lint errors; this write did not introduce new lint output."
        }));
    }
    Some(post)
}

pub(crate) fn check_lint(path: &Path, content: &str) -> Value {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "json" => match serde_json::from_str::<Value>(content) {
            Ok(_) => lint_ok(),
            Err(err) => lint_error(err.to_string()),
        },
        "toml" => match toml::from_str::<toml::Value>(content) {
            Ok(_) => lint_ok(),
            Err(err) => lint_error(err.to_string()),
        },
        "yaml" | "yml" => match serde_yaml::from_str::<Value>(content) {
            Ok(_) => lint_ok(),
            Err(err) => lint_error(err.to_string()),
        },
        "py" => check_shell_lint(path, content, &["python", "-m", "py_compile"]),
        "js" | "mjs" | "cjs" => check_shell_lint(path, content, &["node", "--check"]),
        "rs" => check_shell_lint(path, content, &["rustfmt", "--check"]),
        _ => json!({
            "status": "skipped",
            "message": format!("No linter for .{ext} files")
        }),
    }
}

pub(crate) fn lint_ok() -> Value {
    json!({ "status": "ok", "output": "" })
}

pub(crate) fn lint_error(output: impl Into<String>) -> Value {
    json!({ "status": "error", "output": output.into() })
}

pub(crate) fn check_shell_lint(path: &Path, content: &str, command: &[&str]) -> Value {
    let Some((program, args)) = command.split_first() else {
        return json!({ "status": "skipped", "message": "No linter command" });
    };
    let suffix = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    let Ok(temp) = tempfile::Builder::new().suffix(&suffix).tempfile() else {
        return json!({ "status": "skipped", "message": "Could not create lint temp file" });
    };
    if fs::write(temp.path(), content).is_err() {
        return json!({ "status": "skipped", "message": "Could not write lint temp file" });
    }
    let mut process = std::process::Command::new(program);
    for arg in args {
        process.arg(arg);
    }
    process.arg(temp.path());
    let output = run_bounded_process(process, Duration::from_secs(5));
    match output {
        Ok((0, _)) => lint_ok(),
        Ok((_code, output)) if linter_unusable(program, &output) => json!({
            "status": "skipped",
            "message": format!("{program} is unavailable or unusable")
        }),
        Ok((_code, output)) => lint_error(output),
        Err(err) => json!({ "status": "skipped", "message": err }),
    }
}

pub(crate) fn linter_unusable(program: &str, output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("command not found")
        || lower.contains("no such file or directory")
        || lower.contains(&format!("{program}: not found"))
}

pub(crate) fn run_bounded_process(
    mut command: std::process::Command,
    timeout: Duration,
) -> std::result::Result<(i32, String), String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|err| err.to_string())?;
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            let output = child.wait_with_output().map_err(|err| err.to_string())?;
            let mut combined = String::new();
            combined.push_str(&String::from_utf8_lossy(&output.stdout));
            combined.push_str(&String::from_utf8_lossy(&output.stderr));
            return Ok((status.code().unwrap_or(1), truncate_lint_output(&combined)));
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err("lint timed out".to_string());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub(crate) fn truncate_lint_output(output: &str) -> String {
    let truncated = truncate_tail(output, 8 * 1024, 200);
    truncated.content
}

pub(crate) fn fuzzy_find_and_replace(
    content: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> std::result::Result<FuzzyOutcome, String> {
    if old_string.is_empty() {
        return Err("old_string cannot be empty".to_string());
    }
    if old_string == new_string {
        return Err("old_string and new_string are identical".to_string());
    }
    let strategies: [(&str, FuzzyStrategy); 9] = [
        ("exact", strategy_exact),
        ("line_trimmed", strategy_line_trimmed),
        ("whitespace_normalized", strategy_whitespace_normalized),
        ("indentation_flexible", strategy_indentation_flexible),
        ("escape_normalized", strategy_escape_normalized),
        ("trimmed_boundary", strategy_trimmed_boundary),
        ("unicode_normalized", strategy_unicode_normalized),
        ("block_anchor", strategy_block_anchor),
        ("context_aware", strategy_context_aware),
    ];
    for (strategy, matcher) in strategies {
        let matches = matcher(content, old_string);
        if matches.is_empty() {
            continue;
        }
        if matches.len() > 1 && !replace_all {
            return Err(format!(
                "Found {} matches for old_string. Provide more context to make it unique, or use replace_all=true.",
                matches.len()
            ));
        }
        if strategy != "exact"
            && let Some(err) = detect_escape_drift(content, &matches, old_string, new_string)
        {
            return Err(err);
        }
        return Ok(FuzzyOutcome {
            content: apply_replacements(content, &matches, new_string),
        });
    }
    Err(format!(
        "Could not find a match for old_string in the file{}",
        format_no_match_hint(old_string, content)
    ))
}

pub(crate) fn strategy_exact(content: &str, pattern: &str) -> Vec<MatchRange> {
    let mut matches = Vec::new();
    let mut start = 0;
    while let Some(pos) = content[start..].find(pattern) {
        let abs = start + pos;
        matches.push(MatchRange {
            start: abs,
            end: abs + pattern.len(),
        });
        start = abs.saturating_add(1);
        if start > content.len() {
            break;
        }
    }
    matches
}

pub(crate) fn strategy_line_trimmed(content: &str, pattern: &str) -> Vec<MatchRange> {
    let pattern_lines = pattern.lines().map(str::trim).collect::<Vec<_>>();
    normalized_line_matches(content, &pattern_lines, |line| line.trim().to_string())
}

pub(crate) fn strategy_whitespace_normalized(content: &str, pattern: &str) -> Vec<MatchRange> {
    let pattern = normalize_spaces(pattern);
    let content_norm = normalize_spaces(content);
    map_normalized_matches(
        content,
        &content_norm,
        &strategy_exact(&content_norm, &pattern),
    )
}

pub(crate) fn strategy_indentation_flexible(content: &str, pattern: &str) -> Vec<MatchRange> {
    let pattern_lines = pattern.lines().map(str::trim_start).collect::<Vec<_>>();
    normalized_line_matches(content, &pattern_lines, |line| {
        line.trim_start().to_string()
    })
}

pub(crate) fn strategy_escape_normalized(content: &str, pattern: &str) -> Vec<MatchRange> {
    let unescaped = pattern
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\r", "\r");
    if unescaped == pattern {
        Vec::new()
    } else {
        strategy_exact(content, &unescaped)
    }
}

pub(crate) fn strategy_trimmed_boundary(content: &str, pattern: &str) -> Vec<MatchRange> {
    let mut pattern_lines = pattern.lines().map(str::to_string).collect::<Vec<_>>();
    if pattern_lines.is_empty() {
        return Vec::new();
    }
    if let Some(first) = pattern_lines.first_mut() {
        *first = first.trim().to_string();
    }
    if pattern_lines.len() > 1
        && let Some(last) = pattern_lines.last_mut()
    {
        *last = last.trim().to_string();
    }
    let expected = pattern_lines;
    let content_lines = split_lines_with_offsets(content);
    let mut matches = Vec::new();
    for window_start in 0..=content_lines.len().saturating_sub(expected.len()) {
        let mut actual = Vec::new();
        for idx in 0..expected.len() {
            let mut line = content_lines[window_start + idx].0.to_string();
            if idx == 0 || idx + 1 == expected.len() {
                line = line.trim().to_string();
            }
            actual.push(line);
        }
        if actual == expected {
            matches.push(range_for_line_window(
                &content_lines,
                window_start,
                expected.len(),
                content,
            ));
        }
    }
    matches
}

pub(crate) fn strategy_unicode_normalized(content: &str, pattern: &str) -> Vec<MatchRange> {
    let norm_content = unicode_normalize(content);
    let norm_pattern = unicode_normalize(pattern);
    if norm_content == content && norm_pattern == pattern {
        return Vec::new();
    }
    let matches = strategy_exact(&norm_content, &norm_pattern);
    map_unicode_matches(content, &matches)
}

pub(crate) fn strategy_block_anchor(content: &str, pattern: &str) -> Vec<MatchRange> {
    let pattern_lines = pattern.lines().collect::<Vec<_>>();
    if pattern_lines.len() < 2 {
        return Vec::new();
    }
    let first = unicode_normalize(pattern_lines[0].trim());
    let last = unicode_normalize(pattern_lines[pattern_lines.len() - 1].trim());
    let content_lines = split_lines_with_offsets(content);
    let mut candidates = Vec::new();
    for start in 0..=content_lines.len().saturating_sub(pattern_lines.len()) {
        if unicode_normalize(content_lines[start].0.trim()) == first
            && unicode_normalize(content_lines[start + pattern_lines.len() - 1].0.trim()) == last
        {
            candidates.push(start);
        }
    }
    let threshold = if candidates.len() == 1 { 0.50 } else { 0.70 };
    candidates
        .into_iter()
        .filter_map(|start| {
            let actual = content_lines[start..start + pattern_lines.len()]
                .iter()
                .map(|(line, _)| unicode_normalize(line))
                .collect::<Vec<_>>()
                .join("\n");
            let expected = pattern_lines
                .iter()
                .map(|line| unicode_normalize(line))
                .collect::<Vec<_>>()
                .join("\n");
            (similarity(&actual, &expected) >= threshold)
                .then(|| range_for_line_window(&content_lines, start, pattern_lines.len(), content))
        })
        .collect()
}

pub(crate) fn strategy_context_aware(content: &str, pattern: &str) -> Vec<MatchRange> {
    let pattern_lines = pattern.lines().collect::<Vec<_>>();
    if pattern_lines.is_empty() {
        return Vec::new();
    }
    let content_lines = split_lines_with_offsets(content);
    let mut matches = Vec::new();
    for start in 0..=content_lines.len().saturating_sub(pattern_lines.len()) {
        let similar_lines = pattern_lines
            .iter()
            .zip(&content_lines[start..start + pattern_lines.len()])
            .filter(|(pattern, (actual, _))| similarity(pattern.trim(), actual.trim()) >= 0.80)
            .count();
        if similar_lines * 2 >= pattern_lines.len() {
            matches.push(range_for_line_window(
                &content_lines,
                start,
                pattern_lines.len(),
                content,
            ));
        }
    }
    matches
}

pub(crate) fn normalized_line_matches(
    content: &str,
    pattern_lines: &[&str],
    normalize: impl Fn(&str) -> String,
) -> Vec<MatchRange> {
    if pattern_lines.is_empty() {
        return Vec::new();
    }
    let expected = pattern_lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    let content_lines = split_lines_with_offsets(content);
    let mut matches = Vec::new();
    for start in 0..=content_lines.len().saturating_sub(expected.len()) {
        let actual = content_lines[start..start + expected.len()]
            .iter()
            .map(|(line, _)| normalize(line))
            .collect::<Vec<_>>();
        if actual == expected {
            matches.push(range_for_line_window(
                &content_lines,
                start,
                expected.len(),
                content,
            ));
        }
    }
    matches
}

pub(crate) fn split_lines_with_offsets(content: &str) -> Vec<(&str, usize)> {
    let mut lines = Vec::new();
    let mut start = 0;
    for segment in content.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        lines.push((line, start));
        start += segment.len();
    }
    if content.is_empty() || content.ends_with('\n') {
        lines.push(("", start));
    }
    lines
}

pub(crate) fn range_for_line_window(
    lines: &[(&str, usize)],
    start: usize,
    len: usize,
    content: &str,
) -> MatchRange {
    let start_pos = lines[start].1;
    let end_pos = if start + len < lines.len() {
        lines[start + len].1.saturating_sub(1)
    } else {
        content.len()
    };
    MatchRange {
        start: start_pos,
        end: end_pos,
    }
}

pub(crate) fn normalize_spaces(text: &str) -> String {
    let mut out = String::new();
    let mut in_space = false;
    for ch in text.chars() {
        if ch == ' ' || ch == '\t' {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(ch);
            in_space = false;
        }
    }
    out
}

pub(crate) fn unicode_normalize(text: &str) -> String {
    text.replace(['\u{201c}', '\u{201d}'], "\"")
        .replace(['\u{2018}', '\u{2019}'], "'")
        .replace('\u{2014}', "--")
        .replace('\u{2013}', "-")
        .replace('\u{2026}', "...")
        .replace('\u{00a0}', " ")
}

pub(crate) fn map_normalized_matches(
    original: &str,
    normalized: &str,
    matches: &[MatchRange],
) -> Vec<MatchRange> {
    let mut orig_to_norm = Vec::new();
    let mut norm_idx = 0usize;
    let mut in_space = false;
    for (orig_idx, ch) in original.char_indices() {
        orig_to_norm.push((orig_idx, norm_idx));
        if ch == ' ' || ch == '\t' {
            if !in_space {
                norm_idx += 1;
                in_space = true;
            }
        } else {
            norm_idx += ch.len_utf8();
            in_space = false;
        }
    }
    orig_to_norm.push((original.len(), normalized.len()));
    norm_matches_to_original(&orig_to_norm, matches)
}

pub(crate) fn map_unicode_matches(original: &str, matches: &[MatchRange]) -> Vec<MatchRange> {
    let mut orig_to_norm = Vec::new();
    let mut norm_idx = 0usize;
    for (orig_idx, ch) in original.char_indices() {
        orig_to_norm.push((orig_idx, norm_idx));
        norm_idx += unicode_normalize(&ch.to_string()).len();
    }
    orig_to_norm.push((original.len(), norm_idx));
    norm_matches_to_original(&orig_to_norm, matches)
}

pub(crate) fn norm_matches_to_original(
    orig_to_norm: &[(usize, usize)],
    matches: &[MatchRange],
) -> Vec<MatchRange> {
    let mut out = Vec::new();
    for m in matches {
        let Some((start, _)) = orig_to_norm.iter().find(|(_, norm)| *norm >= m.start) else {
            continue;
        };
        let end = orig_to_norm
            .iter()
            .find(|(_, norm)| *norm >= m.end)
            .map(|(orig, _)| *orig)
            .unwrap_or_else(|| orig_to_norm.last().map(|(orig, _)| *orig).unwrap_or(0));
        out.push(MatchRange { start: *start, end });
    }
    out
}

pub(crate) fn apply_replacements(
    content: &str,
    matches: &[MatchRange],
    new_string: &str,
) -> String {
    let mut result = content.to_string();
    let mut matches = matches.to_vec();
    matches.sort_by_key(|m| std::cmp::Reverse(m.start));
    for m in matches {
        result.replace_range(m.start..m.end, new_string);
    }
    result
}

pub(crate) fn detect_escape_drift(
    content: &str,
    matches: &[MatchRange],
    old_string: &str,
    new_string: &str,
) -> Option<String> {
    if !new_string.contains("\\'") && !new_string.contains("\\\"") {
        return None;
    }
    let matched = matches
        .iter()
        .map(|m| &content[m.start..m.end])
        .collect::<String>();
    for suspect in ["\\'", "\\\""] {
        if new_string.contains(suspect)
            && old_string.contains(suspect)
            && !matched.contains(suspect)
        {
            return Some(format!(
                "Escape-drift detected: old_string and new_string contain literal {suspect:?}, but the matched file text does not. Re-read the file and retry without spurious backslash escaping."
            ));
        }
    }
    None
}

pub(crate) fn format_no_match_hint(old_string: &str, content: &str) -> String {
    let old_lines = old_string.lines().collect::<Vec<_>>();
    let Some(anchor) = old_lines.iter().find(|line| !line.trim().is_empty()) else {
        return String::new();
    };
    let content_lines = content.lines().collect::<Vec<_>>();
    let mut scored = content_lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            let score = similarity(anchor.trim(), line.trim());
            (score > 0.30).then_some((score, idx))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut parts = Vec::new();
    let mut seen = HashSet::new();
    for (_, idx) in scored.into_iter().take(3) {
        let start = idx.saturating_sub(2);
        let end = (idx + old_lines.len() + 2).min(content_lines.len());
        if !seen.insert((start, end)) {
            continue;
        }
        let snippet = (start..end)
            .map(|line_idx| format!("{:4}| {}", line_idx + 1, content_lines[line_idx]))
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(snippet);
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nDid you mean one of these sections?\n{}",
            parts.join("\n---\n")
        )
    }
}

pub(crate) fn similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let a = a.chars().collect::<Vec<_>>();
    let b = b.chars().collect::<Vec<_>>();
    let mut prev = vec![0usize; b.len() + 1];
    let mut curr = vec![0usize; b.len() + 1];
    for (i, a_ch) in a.iter().enumerate() {
        for (j, b_ch) in b.iter().enumerate() {
            curr[j + 1] = if a_ch == b_ch {
                prev[j] + 1
            } else {
                prev[j + 1].max(curr[j])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.fill(0);
        if i > 200 && b.len() > 200 {
            break;
        }
    }
    let lcs = prev[b.len()] as f64;
    (2.0 * lcs) / (a.len() + b.len()) as f64
}
