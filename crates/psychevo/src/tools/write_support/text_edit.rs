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

pub(crate) struct EditSuccess {
    pub(crate) diff: String,
    pub(crate) files_modified: Vec<String>,
    pub(crate) files_created: Vec<String>,
    pub(crate) files_deleted: Vec<String>,
    pub(crate) lint: Option<Value>,
    pub(crate) lsp_diagnostics: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum V4aOperationKind {
    Add,
    Update,
    Delete,
}

impl V4aOperationKind {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct V4aOperation {
    pub(crate) kind: V4aOperationKind,
    pub(crate) file_path: String,
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

pub(crate) fn text_file_from_bytes(bytes: Vec<u8>) -> Result<TextFile> {
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

pub(crate) fn read_text_snapshot(
    backend: &dyn FileMutationBackend,
    path: &Path,
) -> Result<(TextFile, FileVersion)> {
    let snapshot = backend.snapshot(path).map_err(Error::from)?;
    let text = text_file_from_bytes(snapshot.bytes)?;
    Ok((text, snapshot.version))
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
            model_content: partial_patch_failure_model_content(&value),
            json: value,
            attachments: Vec::new(),
            is_error: true,
        },
        Ok(value) => ToolOutput::ok(value),
        Err(err) => ToolOutput::error(err.to_string()),
    }
}

pub(crate) fn partial_patch_failure_model_content(value: &Value) -> Option<String> {
    let failed = value.get("failed_operation")?;
    let index = failed.get("index")?.as_u64()?;
    let kind = failed.get("kind")?.as_str()?;
    let path = failed.get("path")?.as_str()?;
    let committed = ["files_modified", "files_created", "files_deleted"]
        .into_iter()
        .filter_map(|field| value.get(field).and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let summary = if committed.is_empty() {
        "No earlier operations were committed.".to_string()
    } else {
        format!("Committed before failure: {}.", committed.join(", "))
    };
    let reason = value
        .get("error")
        .and_then(Value::as_str)
        .map(|error| bounded_model_error(error, 400))?;
    Some(format!(
        "Patch failed at operation {index} ({kind} {path}). Reason: {reason}. {summary}"
    ))
}

fn bounded_model_error(error: &str, max_chars: usize) -> String {
    let compact = error.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let mut bounded = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        bounded.push('…');
    }
    bounded
}

pub(crate) fn write_success_value(
    tool: &CwdTool,
    target: &Path,
    content: &str,
    dirs_created: bool,
    pre_content: Option<&str>,
    baseline: Option<LspBaseline>,
) -> Value {
    let lint = check_lint_delta(target, pre_content, content);
    let lint_allows_lsp = lint_allows_lsp(&lint);
    let lsp_diagnostics = if lint_allows_lsp {
        lsp_diagnostics_after(tool, target, pre_content, content, baseline)
    } else {
        None
    };
    json!({
        "path": tool.relative(target),
        "bytes_written": content.len(),
        "dirs_created": dirs_created,
        "lint": lint,
        "lsp_diagnostics": lsp_diagnostics,
        "error": null
    })
}

pub(crate) fn edit_success_value(result: EditSuccess) -> Value {
    json!({
        "success": true,
        "diff": result.diff,
        "files_modified": result.files_modified,
        "files_created": result.files_created,
        "files_deleted": result.files_deleted,
        "lint": result.lint,
        "lsp_diagnostics": result.lsp_diagnostics,
        "error": null
    })
}

pub(crate) fn unified_diff_named(old_path: &str, new_path: &str, old: &str, new: &str) -> String {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .header(old_path, new_path)
        .to_string()
}

pub(crate) fn git_patch_update(path: &str, old: &str, new: &str) -> String {
    let mut patch = format!("diff --git a/{path} b/{path}\n");
    patch.push_str(&unified_diff_named(
        &format!("a/{path}"),
        &format!("b/{path}"),
        old,
        new,
    ));
    patch
}

pub(crate) fn git_patch_add(path: &str, content: &str) -> String {
    let mut patch = format!("diff --git a/{path} b/{path}\nnew file mode 100644\n");
    patch.push_str(&unified_diff_named(
        "/dev/null",
        &format!("b/{path}"),
        "",
        content,
    ));
    patch
}

pub(crate) fn git_patch_delete(path: &str, content: &str) -> String {
    let mut patch = format!("diff --git a/{path} b/{path}\ndeleted file mode 100644\n");
    patch.push_str(&unified_diff_named(
        &format!("a/{path}"),
        "/dev/null",
        content,
        "",
    ));
    patch
}

pub(crate) fn post_write_feedback(
    tool: &CwdTool,
    target: &Path,
    content: &str,
    pre_content: Option<&str>,
    baseline: Option<LspBaseline>,
) -> Result<(Option<Value>, Option<String>)> {
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
            combined.push_str(&crate::process_env::decode_process_output(&output.stdout));
            combined.push_str(&crate::process_env::decode_process_output(&output.stderr));
            return Ok((status.code().unwrap_or(1), truncate_lint_output(&combined)));
        }
        if start.elapsed() > timeout {
            crate::process_env::terminate_std_child_tree(&mut child);
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
        if matches!(strategy, "block_anchor" | "context_aware") {
            return Err(candidate_only_error(content, strategy, &matches));
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
            content: apply_fuzzy_replacements(content, &matches, old_string, new_string, strategy),
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
        start = abs.saturating_add(pattern.len().max(1));
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

pub(crate) fn apply_fuzzy_replacements(
    content: &str,
    matches: &[MatchRange],
    old_string: &str,
    new_string: &str,
    strategy: &str,
) -> String {
    let mut result = content.to_string();
    let mut matches = matches.to_vec();
    matches.sort_by_key(|m| std::cmp::Reverse(m.start));
    for m in matches {
        let region = &content[m.start..m.end];
        let mut replacement = maybe_unescape_new_string(new_string, region);
        if strategy == "unicode_normalized" {
            replacement = preserve_unicode_in_replacement(region, old_string, &replacement);
        }
        if !matches!(strategy, "exact" | "escape_normalized") {
            replacement = reindent_replacement(region, old_string, &replacement);
        }
        result.replace_range(m.start..m.end, &replacement);
    }
    result
}

pub(crate) fn candidate_only_error(
    content: &str,
    strategy: &str,
    matches: &[MatchRange],
) -> String {
    let shown = matches
        .iter()
        .take(3)
        .map(|range| {
            let start = content[..range.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            let end = start
                + content[range.start..range.end]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count();
            if start == end {
                start.to_string()
            } else {
                format!("{start}-{end}")
            }
        })
        .collect::<Vec<_>>();
    let remaining = matches.len().saturating_sub(shown.len());
    let suffix = if remaining == 0 {
        String::new()
    } else {
        format!(" (+{remaining} more)")
    };
    format!(
        "No changes were applied: {strategy} found candidate line range(s) {}{suffix}. Read the file and retry with a more precise old_string.",
        shown.join(", ")
    )
}

pub(crate) fn maybe_unescape_new_string(new_string: &str, file_region: &str) -> String {
    let mut out = new_string.to_string();
    if out.contains("\\t") && file_region.contains('\t') {
        out = out.replace("\\t", "\t");
    }
    if out.contains("\\r") && file_region.contains('\r') {
        out = out.replace("\\r", "\r");
    }
    out
}

pub(crate) fn reindent_replacement(
    file_region: &str,
    old_string: &str,
    new_string: &str,
) -> String {
    let Some(old_first) = old_string.lines().find(|line| !line.trim().is_empty()) else {
        return new_string.to_string();
    };
    let Some(file_first) = file_region.lines().find(|line| !line.trim().is_empty()) else {
        return new_string.to_string();
    };
    let old_indent = leading_whitespace(old_first);
    let file_indent = leading_whitespace(file_first);
    if old_indent == file_indent || new_string.is_empty() {
        return new_string.to_string();
    }
    new_string
        .split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                return line.to_string();
            }
            if let Some(remainder) = line.strip_prefix(old_indent) {
                format!("{file_indent}{remainder}")
            } else {
                format!("{file_indent}{}", line.trim_start_matches([' ', '\t']))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn leading_whitespace(line: &str) -> &str {
    let end = line
        .char_indices()
        .find_map(|(idx, ch)| (!matches!(ch, ' ' | '\t')).then_some(idx))
        .unwrap_or(line.len());
    &line[..end]
}

pub(crate) fn preserve_unicode_in_replacement(
    file_region: &str,
    old_string: &str,
    new_string: &str,
) -> String {
    let normalized_old = unicode_normalize(old_string);
    if normalized_old != unicode_normalize(file_region) {
        return new_string.to_string();
    }
    let new_string = new_string.to_string();
    let diff = TextDiff::from_chars(&normalized_old, &new_string);
    let new_chars = new_string.chars().collect::<Vec<_>>();
    let boundaries = normalized_char_boundaries(file_region);
    let mut out = String::new();
    for operation in diff.ops() {
        let (tag, old_range, new_range) = operation.as_tag_tuple();
        match tag {
            similar::DiffTag::Equal => {
                push_unicode_equal_segment(
                    &mut out,
                    file_region,
                    &boundaries,
                    old_range,
                    new_range,
                    &new_chars,
                );
            }
            similar::DiffTag::Insert | similar::DiffTag::Replace => {
                out.extend(&new_chars[new_range]);
            }
            similar::DiffTag::Delete => {}
        }
    }
    out
}

fn push_unicode_equal_segment(
    out: &mut String,
    original: &str,
    boundaries: &[(usize, usize)],
    old_range: std::ops::Range<usize>,
    new_range: std::ops::Range<usize>,
    new_chars: &[char],
) {
    for boundary in boundaries.windows(2) {
        let [
            (original_start, normalized_start),
            (original_end, normalized_end),
        ] = boundary
        else {
            continue;
        };
        let retained_start = (*normalized_start).max(old_range.start);
        let retained_end = (*normalized_end).min(old_range.end);
        if retained_start >= retained_end {
            continue;
        }
        if retained_start == *normalized_start && retained_end == *normalized_end {
            out.push_str(&original[*original_start..*original_end]);
        } else {
            let new_start = new_range.start + retained_start - old_range.start;
            let new_end = new_range.start + retained_end - old_range.start;
            out.extend(&new_chars[new_start..new_end]);
        }
    }
}

pub(crate) fn normalized_char_boundaries(text: &str) -> Vec<(usize, usize)> {
    let mut boundaries = Vec::new();
    let mut normalized = 0usize;
    for (byte, ch) in text.char_indices() {
        boundaries.push((byte, normalized));
        normalized += unicode_normalize(&ch.to_string()).chars().count();
    }
    boundaries.push((text.len(), normalized));
    boundaries
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
