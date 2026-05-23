#[derive(Debug, Clone)]
struct TextFile {
    original: String,
    normalized: String,
    bom: bool,
    line_ending: &'static str,
}

#[derive(Debug)]
struct FuzzyOutcome {
    content: String,
}

#[derive(Debug, Clone)]
struct MatchRange {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
struct LspBaseline {
    diagnostics: Vec<Value>,
}

type FuzzyStrategy = fn(&str, &str) -> Vec<MatchRange>;
type LspAutoCommand<'a> = Option<(&'a str, &'a [&'a str])>;

struct EditSuccess {
    diff: String,
    files_modified: Vec<String>,
    files_created: Vec<String>,
    files_deleted: Vec<String>,
    files_moved: Vec<Value>,
    lint: Option<Value>,
    lsp_diagnostics: Option<String>,
    warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum V4aOperationKind {
    Add,
    Update,
    Delete,
    Move,
}

#[derive(Debug, Clone)]
struct V4aOperation {
    kind: V4aOperationKind,
    file_path: String,
    new_path: Option<String>,
    hunks: Vec<V4aHunk>,
}

#[derive(Debug, Clone)]
struct V4aHunk {
    context_hint: Option<String>,
    lines: Vec<V4aLine>,
}

#[derive(Debug, Clone)]
struct V4aLine {
    prefix: char,
    content: String,
}

fn read_text_file(path: &Path) -> Result<TextFile> {
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

fn restore_text_file(text: &TextFile, normalized: &str) -> String {
    let restored = restore_line_endings(normalized, text.line_ending);
    if text.bom {
        format!("\u{feff}{restored}")
    } else {
        restored
    }
}

fn result_output(result: Result<Value>) -> ToolOutput {
    match result {
        Ok(value) if value_reports_error(&value) => ToolOutput {
            json: value,
            model_content: None,
            is_error: true,
        },
        Ok(value) => ToolOutput::ok(value),
        Err(err) => ToolOutput::error(err.to_string()),
    }
}

fn write_text_to_target(
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

fn edit_success_value(result: EditSuccess) -> Value {
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

fn unified_diff_named(old_path: &str, new_path: &str, old: &str, new: &str) -> String {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .header(old_path, new_path)
        .to_string()
}

fn write_edit_text(
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

fn lint_allows_lsp(lint: &Option<Value>) -> bool {
    lint.as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .map(|status| status == "ok" || status == "skipped")
        .unwrap_or(true)
}

fn check_lint_delta(path: &Path, pre_content: Option<&str>, post_content: &str) -> Option<Value> {
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

fn check_lint(path: &Path, content: &str) -> Value {
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

fn lint_ok() -> Value {
    json!({ "status": "ok", "output": "" })
}

fn lint_error(output: impl Into<String>) -> Value {
    json!({ "status": "error", "output": output.into() })
}

fn check_shell_lint(path: &Path, content: &str, command: &[&str]) -> Value {
    let Some((program, args)) = command.split_first() else {
        return json!({ "status": "skipped", "message": "No linter command" });
    };
    let suffix = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    let Ok(temp) = tempfile::Builder::new()
        .suffix(&suffix)
        .tempfile()
    else {
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

fn linter_unusable(program: &str, output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("command not found")
        || lower.contains("no such file or directory")
        || lower.contains(&format!("{program}: not found"))
}

fn run_bounded_process(
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

fn truncate_lint_output(output: &str) -> String {
    let truncated = truncate_tail(output, 8 * 1024, 200);
    truncated.content
}

fn fuzzy_find_and_replace(
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

fn strategy_exact(content: &str, pattern: &str) -> Vec<MatchRange> {
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

fn strategy_line_trimmed(content: &str, pattern: &str) -> Vec<MatchRange> {
    let pattern_lines = pattern.lines().map(str::trim).collect::<Vec<_>>();
    normalized_line_matches(content, &pattern_lines, |line| line.trim().to_string())
}

fn strategy_whitespace_normalized(content: &str, pattern: &str) -> Vec<MatchRange> {
    let pattern = normalize_spaces(pattern);
    let content_norm = normalize_spaces(content);
    map_normalized_matches(content, &content_norm, &strategy_exact(&content_norm, &pattern))
}

fn strategy_indentation_flexible(content: &str, pattern: &str) -> Vec<MatchRange> {
    let pattern_lines = pattern.lines().map(str::trim_start).collect::<Vec<_>>();
    normalized_line_matches(content, &pattern_lines, |line| line.trim_start().to_string())
}

fn strategy_escape_normalized(content: &str, pattern: &str) -> Vec<MatchRange> {
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

fn strategy_trimmed_boundary(content: &str, pattern: &str) -> Vec<MatchRange> {
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
            matches.push(range_for_line_window(&content_lines, window_start, expected.len(), content));
        }
    }
    matches
}

fn strategy_unicode_normalized(content: &str, pattern: &str) -> Vec<MatchRange> {
    let norm_content = unicode_normalize(content);
    let norm_pattern = unicode_normalize(pattern);
    if norm_content == content && norm_pattern == pattern {
        return Vec::new();
    }
    let matches = strategy_exact(&norm_content, &norm_pattern);
    map_unicode_matches(content, &matches)
}

fn strategy_block_anchor(content: &str, pattern: &str) -> Vec<MatchRange> {
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

fn strategy_context_aware(content: &str, pattern: &str) -> Vec<MatchRange> {
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
            matches.push(range_for_line_window(&content_lines, start, pattern_lines.len(), content));
        }
    }
    matches
}

fn normalized_line_matches(
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
            matches.push(range_for_line_window(&content_lines, start, expected.len(), content));
        }
    }
    matches
}

fn split_lines_with_offsets(content: &str) -> Vec<(&str, usize)> {
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

fn range_for_line_window(
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

fn normalize_spaces(text: &str) -> String {
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

fn unicode_normalize(text: &str) -> String {
    text.replace(['\u{201c}', '\u{201d}'], "\"")
        .replace(['\u{2018}', '\u{2019}'], "'")
        .replace('\u{2014}', "--")
        .replace('\u{2013}', "-")
        .replace('\u{2026}', "...")
        .replace('\u{00a0}', " ")
}

fn map_normalized_matches(
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

fn map_unicode_matches(original: &str, matches: &[MatchRange]) -> Vec<MatchRange> {
    let mut orig_to_norm = Vec::new();
    let mut norm_idx = 0usize;
    for (orig_idx, ch) in original.char_indices() {
        orig_to_norm.push((orig_idx, norm_idx));
        norm_idx += unicode_normalize(&ch.to_string()).len();
    }
    orig_to_norm.push((original.len(), norm_idx));
    norm_matches_to_original(&orig_to_norm, matches)
}

fn norm_matches_to_original(
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

fn apply_replacements(content: &str, matches: &[MatchRange], new_string: &str) -> String {
    let mut result = content.to_string();
    let mut matches = matches.to_vec();
    matches.sort_by_key(|m| std::cmp::Reverse(m.start));
    for m in matches {
        result.replace_range(m.start..m.end, new_string);
    }
    result
}

fn detect_escape_drift(
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

fn format_no_match_hint(old_string: &str, content: &str) -> String {
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

fn similarity(a: &str, b: &str) -> f64 {
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

fn parse_v4a_patch(patch: &str) -> Result<Vec<V4aOperation>> {
    let lines = patch.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.trim() == "*** Begin Patch")
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let end = lines
        .iter()
        .position(|line| line.trim() == "*** End Patch")
        .unwrap_or(lines.len());
    let mut operations = Vec::new();
    let mut current: Option<V4aOperation> = None;
    let mut current_hunk: Option<V4aHunk> = None;
    for line in &lines[start..end] {
        if let Some(path) = marker_value(line, "*** Update File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            current = Some(V4aOperation {
                kind: V4aOperationKind::Update,
                file_path: path,
                new_path: None,
                hunks: Vec::new(),
            });
        } else if let Some(path) = marker_value(line, "*** Add File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            current = Some(V4aOperation {
                kind: V4aOperationKind::Add,
                file_path: path,
                new_path: None,
                hunks: Vec::new(),
            });
            current_hunk = Some(V4aHunk {
                context_hint: None,
                lines: Vec::new(),
            });
        } else if let Some(path) = marker_value(line, "*** Delete File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            operations.push(V4aOperation {
                kind: V4aOperationKind::Delete,
                file_path: path,
                new_path: None,
                hunks: Vec::new(),
            });
        } else if let Some(rest) = marker_value(line, "*** Move File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            let Some((src, dst)) = rest.split_once("->") else {
                return Err(Error::Message(format!(
                    "invalid move marker: expected '*** Move File: old -> new', got {line}"
                )));
            };
            operations.push(V4aOperation {
                kind: V4aOperationKind::Move,
                file_path: src.trim().to_string(),
                new_path: Some(dst.trim().to_string()),
                hunks: Vec::new(),
            });
        } else if let Some(path) = marker_value(line, "*** Move to:") {
            let Some(op) = current.as_mut() else {
                return Err(Error::Message("*** Move to without current file".to_string()));
            };
            op.new_path = Some(path);
            op.kind = V4aOperationKind::Move;
        } else if line.starts_with("@@") {
            if let Some(op) = current.as_mut()
                && let Some(hunk) = current_hunk.take()
                && !hunk.lines.is_empty()
            {
                op.hunks.push(hunk);
            }
            current_hunk = Some(V4aHunk {
                context_hint: parse_context_hint(line),
                lines: Vec::new(),
            });
        } else if let Some(op) = current.as_mut() {
            let hunk = current_hunk.get_or_insert_with(|| V4aHunk {
                context_hint: None,
                lines: Vec::new(),
            });
            if let Some(content) = line.strip_prefix('+') {
                hunk.lines.push(V4aLine {
                    prefix: '+',
                    content: content.to_string(),
                });
            } else if let Some(content) = line.strip_prefix('-') {
                hunk.lines.push(V4aLine {
                    prefix: '-',
                    content: content.to_string(),
                });
            } else if let Some(content) = line.strip_prefix(' ') {
                hunk.lines.push(V4aLine {
                    prefix: ' ',
                    content: content.to_string(),
                });
            } else if line.starts_with('\\') {
                continue;
            } else if !line.is_empty() || op.kind == V4aOperationKind::Add {
                hunk.lines.push(V4aLine {
                    prefix: ' ',
                    content: (*line).to_string(),
                });
            }
        }
    }
    push_v4a_current(&mut operations, &mut current, &mut current_hunk);
    if operations.is_empty() {
        return Err(Error::Message("patch contains no operations".to_string()));
    }
    for op in &operations {
        if op.file_path.trim().is_empty() {
            return Err(Error::Message("patch operation has empty path".to_string()));
        }
        if op.kind == V4aOperationKind::Update && op.hunks.is_empty() {
            return Err(Error::Message(format!(
                "update operation has no hunks: {}",
                op.file_path
            )));
        }
        if op.kind == V4aOperationKind::Move
            && op.new_path.as_deref().unwrap_or_default().trim().is_empty()
        {
            return Err(Error::Message(format!(
                "move operation missing destination: {}",
                op.file_path
            )));
        }
    }
    Ok(operations)
}

fn marker_value(line: &str, marker: &str) -> Option<String> {
    line.trim()
        .strip_prefix(marker)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn push_v4a_current(
    operations: &mut Vec<V4aOperation>,
    current: &mut Option<V4aOperation>,
    current_hunk: &mut Option<V4aHunk>,
) {
    if let Some(mut op) = current.take() {
        if let Some(hunk) = current_hunk.take()
            && !hunk.lines.is_empty()
        {
            op.hunks.push(hunk);
        }
        operations.push(op);
    }
}

fn parse_context_hint(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix("@@")?.strip_suffix("@@")?.trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

fn apply_v4a_update_hunks(content: &str, hunks: &[V4aHunk]) -> std::result::Result<String, String> {
    let mut updated = content.to_string();
    for hunk in hunks {
        let search_lines = hunk
            .lines
            .iter()
            .filter(|line| line.prefix == ' ' || line.prefix == '-')
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();
        let replacement_lines = hunk
            .lines
            .iter()
            .filter(|line| line.prefix == ' ' || line.prefix == '+')
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();
        if search_lines.is_empty() {
            let insert_text = replacement_lines.join("\n");
            updated = apply_addition_only_hunk(&updated, hunk.context_hint.as_deref(), &insert_text)?;
            continue;
        }
        let search = search_lines.join("\n");
        let replacement = replacement_lines.join("\n");
        match fuzzy_find_and_replace(&updated, &search, &replacement, false) {
            Ok(outcome) => updated = outcome.content,
            Err(err) => {
                return Err(format!(
                    "hunk {} not found: {err}",
                    hunk.context_hint
                        .as_ref()
                        .map(|hint| format!("{hint:?}"))
                        .unwrap_or_else(|| "(no hint)".to_string())
                ));
            }
        }
    }
    Ok(updated)
}

fn apply_addition_only_hunk(
    content: &str,
    context_hint: Option<&str>,
    insert_text: &str,
) -> std::result::Result<String, String> {
    if insert_text.is_empty() {
        return Ok(content.to_string());
    }
    let Some(hint) = context_hint else {
        return Ok(format!("{}\n{}\n", content.trim_end_matches('\n'), insert_text));
    };
    let matches = strategy_exact(content, hint);
    if matches.is_empty() {
        return Err(format!("addition-only hunk context hint {hint:?} not found"));
    }
    if matches.len() > 1 {
        return Err(format!(
            "addition-only hunk context hint {hint:?} is ambiguous ({} occurrences)",
            matches.len()
        ));
    }
    let insert_at = content[matches[0].end..]
        .find('\n')
        .map(|idx| matches[0].end + idx + 1)
        .unwrap_or(content.len());
    let mut out = String::new();
    out.push_str(&content[..insert_at]);
    out.push_str(insert_text);
    out.push('\n');
    out.push_str(&content[insert_at..]);
    Ok(out)
}

fn v4a_add_content(op: &V4aOperation) -> String {
    op.hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .filter(|line| line.prefix == '+')
        .map(|line| line.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn snapshot_lsp_baseline(
    tool: &WorkdirTool,
    path: &Path,
    pre_content: Option<&str>,
) -> Option<LspBaseline> {
    if !tool.lsp_config().enabled {
        return None;
    }
    let content = pre_content?;
    run_lsp_diagnostics(tool, path, content)
        .map(|diagnostics| LspBaseline { diagnostics })
        .ok()
}

fn lsp_diagnostics_after(
    tool: &WorkdirTool,
    path: &Path,
    _pre_content: Option<&str>,
    post_content: &str,
    baseline: Option<LspBaseline>,
) -> Option<String> {
    if !tool.lsp_config().enabled {
        return None;
    }
    let fresh = run_lsp_diagnostics(tool, path, post_content).ok()?;
    let baseline_keys = baseline
        .map(|baseline| {
            baseline
                .diagnostics
                .iter()
                .map(lsp_diag_key)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let introduced = fresh
        .into_iter()
        .filter(|diag| !baseline_keys.contains(&lsp_diag_key(diag)))
        .collect::<Vec<_>>();
    format_lsp_diagnostics(path, &introduced)
}

fn run_lsp_diagnostics(tool: &WorkdirTool, path: &Path, content: &str) -> Result<Vec<Value>> {
    let Some(server) = resolve_lsp_server(path, tool.lsp_config()) else {
        return Ok(Vec::new());
    };
    let timeout = Duration::from_secs_f64(tool.lsp_config().wait_timeout_secs.max(0.1)) + Duration::from_secs(2);
    lsp_diagnostics_with_command(&server, tool.workdir(), path, content, timeout)
}

#[derive(Clone)]
struct LspServerCommand {
    program: String,
    args: Vec<String>,
}

fn resolve_lsp_server(path: &Path, config: &LspConfig) -> Option<LspServerCommand> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let (bin, args, auto): (&str, &[&str], LspAutoCommand<'_>) = match ext.as_str() {
        "rs" => ("rust-analyzer", &[], None),
        "py" => ("pyright-langserver", &["--stdio"], Some(("npx", &["-y", "pyright-langserver", "--stdio"]))),
        "js" | "jsx" | "ts" | "tsx" => (
            "typescript-language-server",
            &["--stdio"],
            Some(("npx", &["-y", "typescript-language-server", "--stdio"])),
        ),
        "go" => ("gopls", &[], None),
        "yaml" | "yml" => (
            "yaml-language-server",
            &["--stdio"],
            Some(("npx", &["-y", "yaml-language-server", "--stdio"])),
        ),
        _ => return None,
    };
    if command_available(bin) {
        return Some(LspServerCommand {
            program: bin.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        });
    }
    if config.install_strategy == "auto"
        && let Some((program, args)) = auto
    {
        return Some(LspServerCommand {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        });
    }
    None
}

fn command_available(program: &str) -> bool {
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|path| path.join(program))
                .find(|path| path.exists())
        })
        .is_some()
}

fn lsp_diagnostics_with_command(
    server: &LspServerCommand,
    workdir: &Path,
    path: &Path,
    content: &str,
    timeout: Duration,
) -> Result<Vec<Value>> {
    let uri = file_uri(path);
    let mut child = std::process::Command::new(&server.program)
        .args(&server.args)
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| Error::Message(format!("LSP spawn failed: {err}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::Message("LSP stdin unavailable".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Message("LSP stdout unavailable".to_string()))?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout);
        while let Ok(Some(message)) = read_lsp_message(&mut reader) {
            if tx.send(message).is_err() {
                break;
            }
        }
    });
    send_lsp(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": file_uri(workdir),
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": { "relatedInformation": false },
                        "diagnostic": { "dynamicRegistration": true }
                    }
                },
                "workspaceFolders": [{ "uri": file_uri(workdir), "name": "workspace" }],
                "clientInfo": { "name": "psychevo", "version": "0" }
            }
        }),
    )?;
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        let remaining = timeout.saturating_sub(start.elapsed());
        let Ok(message) = rx.recv_timeout(remaining.min(Duration::from_millis(200))) else {
            continue;
        };
        if message.get("id").and_then(Value::as_i64) == Some(1) {
            break;
        }
    }
    send_lsp(
        &mut stdin,
        json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    )?;
    send_lsp(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": lsp_language_id(path),
                    "version": 1,
                    "text": content
                }
            }
        }),
    )?;
    let mut diagnostics = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        let remaining = timeout.saturating_sub(start.elapsed());
        let Ok(message) = rx.recv_timeout(remaining.min(Duration::from_millis(200))) else {
            continue;
        };
        if message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
            && message
                .pointer("/params/uri")
                .and_then(Value::as_str)
                .is_some_and(|value| value == uri)
        {
            diagnostics = message
                .pointer("/params/diagnostics")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            break;
        }
    }
    let _ = send_lsp(&mut stdin, json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" }));
    let _ = send_lsp(&mut stdin, json!({ "jsonrpc": "2.0", "method": "exit" }));
    let _ = child.kill();
    let _ = child.wait();
    Ok(diagnostics)
}

fn send_lsp(stdin: &mut std::process::ChildStdin, message: Value) -> Result<()> {
    let body = serde_json::to_string(&message)?;
    std::io::Write::write_all(
        stdin,
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes(),
    )?;
    std::io::Write::flush(stdin)?;
    Ok(())
}

fn read_lsp_message(reader: &mut dyn std::io::BufRead) -> std::io::Result<Option<Value>> {
    let mut content_len = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_len = value.trim().parse::<usize>().ok();
        }
    }
    let Some(len) = content_len else {
        return Ok(None);
    };
    let mut body = vec![0u8; len];
    std::io::Read::read_exact(reader, &mut body)?;
    Ok(serde_json::from_slice(&body).ok())
}

fn file_uri(path: &Path) -> String {
    let raw = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    format!("file://{}", percent_encode_path(&raw))
}

fn percent_encode_path(path: &str) -> String {
    let mut out = String::new();
    for byte in path.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.' | '~' | ':') {
            out.push(ch);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn lsp_language_id(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "go" => "go",
        "yaml" | "yml" => "yaml",
        _ => "plaintext",
    }
}

fn lsp_diag_key(diag: &Value) -> String {
    format!(
        "{}|{}|{}|{}",
        diag.get("severity").and_then(Value::as_i64).unwrap_or(1),
        diag.get("code").map(Value::to_string).unwrap_or_default(),
        diag.get("source").and_then(Value::as_str).unwrap_or_default(),
        diag.get("message").and_then(Value::as_str).unwrap_or_default()
    )
}

fn format_lsp_diagnostics(path: &Path, diagnostics: &[Value]) -> Option<String> {
    if diagnostics.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for diag in diagnostics.iter().take(20) {
        let start = diag.pointer("/range/start").unwrap_or(&Value::Null);
        let line = start.get("line").and_then(Value::as_u64).unwrap_or(0) + 1;
        let col = start.get("character").and_then(Value::as_u64).unwrap_or(0) + 1;
        let severity = match diag.get("severity").and_then(Value::as_u64).unwrap_or(1) {
            1 => "error",
            2 => "warning",
            3 => "info",
            4 => "hint",
            _ => "diagnostic",
        };
        let message = diag
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .replace('\n', " ");
        let source = diag.get("source").and_then(Value::as_str).unwrap_or("");
        let code = diag
            .get("code")
            .map(Value::to_string)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let label = [source, &code]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(if label.is_empty() {
            format!("{line}:{col} {severity}: {message}")
        } else {
            format!("{line}:{col} {severity} [{label}]: {message}")
        });
    }
    if diagnostics.len() > 20 {
        lines.push(format!("... {} more diagnostics", diagnostics.len() - 20));
    }
    let body = lines.join("\n");
    let block = format!(
        "<diagnostics file=\"{}\">\n{}\n</diagnostics>",
        path.display(),
        body
    );
    Some(truncate_lint_output(&block))
}

#[cfg(test)]
mod lsp_tests {
    use super::*;

    #[test]
    fn lsp_fake_server_returns_diagnostics() {
        if !command_available("python3") {
            return;
        }
        let temp = tempfile::tempdir().expect("temp");
        let script = temp.path().join("fake_lsp.py");
        fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys

def read_msg():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode("ascii").strip()
        if not line:
            break
        key, value = line.split(":", 1)
        headers[key.lower()] = value.strip()
    length = int(headers.get("content-length", "0"))
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))

def send(msg):
    body = json.dumps(msg).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: " + str(len(body)).encode("ascii") + b"\r\n\r\n" + body)
    sys.stdout.buffer.flush()

while True:
    msg = read_msg()
    if msg is None:
        break
    method = msg.get("method")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":msg["id"],"result":{"capabilities":{"textDocumentSync":1}}})
    elif method == "textDocument/didOpen":
        doc = msg["params"]["textDocument"]
        diagnostics = []
        if "bad" in doc.get("text", ""):
            diagnostics.append({
                "range": {"start": {"line": 0, "character": 1}, "end": {"line": 0, "character": 4}},
                "severity": 1,
                "source": "fake",
                "code": "E001",
                "message": "bad token"
            })
        send({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":doc["uri"],"diagnostics":diagnostics}})
    elif method == "shutdown":
        send({"jsonrpc":"2.0","id":msg["id"],"result":None})
    elif method == "exit":
        break
"#,
        )
        .expect("script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        let file = temp.path().join("sample.fake");
        fs::write(&file, "bad\n").expect("file");
        let diagnostics = lsp_diagnostics_with_command(
            &LspServerCommand {
                program: "python3".to_string(),
                args: vec![script.to_string_lossy().to_string()],
            },
            temp.path(),
            &file,
            "bad\n",
            Duration::from_secs(2),
        )
        .expect("diagnostics");
        assert_eq!(diagnostics.len(), 1);
        let formatted = format_lsp_diagnostics(&file, &diagnostics).expect("formatted");
        assert!(formatted.contains("bad token"));
        assert!(formatted.contains("<diagnostics"));
    }
}
