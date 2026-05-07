use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use psychevo_agent_core::{ToolBinding, ToolExecutionMode, ToolOutput};
use psychevo_ai::AbortSignal;
use serde::Deserialize;
use serde_json::{Value, json};
use similar::TextDiff;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time;

use crate::error::{Error, Result};
use crate::types::RunMode;

const READ_MAX_BYTES: usize = 50 * 1024;
const READ_MAX_LINES: usize = 2000;
const BASH_DEFAULT_TIMEOUT_SECS: u64 = 120;
const BASH_MAX_TIMEOUT_SECS: u64 = 300;

pub(crate) fn coding_core_tools(workdir: &Path) -> Vec<Arc<dyn ToolBinding>> {
    coding_core_tools_for_mode(workdir, RunMode::Build)
}

pub(crate) fn coding_core_tools_for_mode(
    workdir: &Path,
    mode: RunMode,
) -> Vec<Arc<dyn ToolBinding>> {
    match mode {
        RunMode::Plan => read_only_plan_tools(workdir),
        RunMode::Build => full_build_tools(workdir),
    }
}

fn full_build_tools(workdir: &Path) -> Vec<Arc<dyn ToolBinding>> {
    vec![
        Arc::new(ReadTool::new(workdir.to_path_buf())),
        Arc::new(WriteTool::new(workdir.to_path_buf())),
        Arc::new(EditTool::new(workdir.to_path_buf())),
        Arc::new(BashTool::new(workdir.to_path_buf())),
    ]
}

fn read_only_plan_tools(workdir: &Path) -> Vec<Arc<dyn ToolBinding>> {
    vec![
        Arc::new(ReadTool::new(workdir.to_path_buf())),
        Arc::new(ListTool::new(workdir.to_path_buf())),
        Arc::new(SearchTool::new(workdir.to_path_buf())),
    ]
}

pub fn tool_names_for_mode(mode: RunMode) -> Vec<&'static str> {
    match mode {
        RunMode::Plan => vec!["read", "list", "search"],
        RunMode::Build => vec!["read", "write", "edit", "bash"],
    }
}

pub(crate) fn mode_instruction(mode: RunMode) -> &'static str {
    match mode {
        RunMode::Build => {
            "Runtime mode: default. You may use the available coding tools to read, edit, write, and run commands under the selected workdir."
        }
        RunMode::Plan => {
            "Runtime mode: plan. This turn is hard read-only. Use only the available read, list, and search tools to inspect the workdir. Do not write files, edit files, run shell commands, or claim to have modified the workspace."
        }
    }
}

#[derive(Clone)]
pub(crate) struct WorkdirTool {
    workdir: PathBuf,
}

impl WorkdirTool {
    pub(crate) fn new(workdir: PathBuf) -> Self {
        Self { workdir }
    }

    fn resolve_existing(&self, raw: &str) -> Result<PathBuf> {
        let target = self.resolve_raw(raw);
        let canonical = target.canonicalize()?;
        self.ensure_contained(&canonical)?;
        Ok(canonical)
    }

    fn resolve_write_target(&self, raw: &str) -> Result<(PathBuf, bool)> {
        let target = self.resolve_raw(raw);
        if target.exists() {
            let canonical = target.canonicalize()?;
            self.ensure_contained(&canonical)?;
            return Ok((canonical, false));
        }
        let parent = target
            .parent()
            .ok_or_else(|| Error::Message("target has no parent".to_string()))?
            .to_path_buf();
        let mut existing = parent.as_path();
        while !existing.exists() {
            existing = existing
                .parent()
                .ok_or_else(|| Error::Message("no existing parent under workdir".to_string()))?;
        }
        let canonical_parent = existing.canonicalize()?;
        self.ensure_contained(&canonical_parent)?;
        let dirs_created = !parent.exists();
        Ok((target, dirs_created))
    }

    fn resolve_raw(&self, raw: &str) -> PathBuf {
        let path = Path::new(raw);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workdir.join(path)
        }
    }

    fn ensure_contained(&self, path: &Path) -> Result<()> {
        if path == self.workdir || path.starts_with(&self.workdir) {
            Ok(())
        } else {
            Err(Error::Message(format!(
                "path escapes workdir: {}",
                path.display()
            )))
        }
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.workdir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

struct ReadTool(WorkdirTool);

impl ReadTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read UTF-8 text from the working directory."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"},"offset":{"type":"integer"},"limit":{"type":"integer"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match read_tool_impl(tool, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn read_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let offset = optional_i64(&args, "offset")?.unwrap_or(1);
    let limit = optional_i64(&args, "limit")?;
    if offset < 1 {
        return Err(Error::Message("offset must be >= 1".to_string()));
    }
    if let Some(limit) = limit
        && limit < 1
    {
        return Err(Error::Message("limit must be >= 1".to_string()));
    }
    let target = tool.resolve_existing(path)?;
    let bytes = fs::read(&target)?;
    if bytes.contains(&0) {
        return Err(Error::Message("binary files are not supported".to_string()));
    }
    let content =
        String::from_utf8(bytes).map_err(|_| Error::Message("invalid UTF-8".to_string()))?;
    let file_size = content.len();
    let lines = content.split('\n').collect::<Vec<_>>();
    let total_lines = lines.len();
    let start = (offset as usize).saturating_sub(1);
    if start >= total_lines {
        return Err(Error::Message(format!(
            "offset {offset} is beyond end of file ({total_lines} lines)"
        )));
    }
    let end = limit
        .map(|limit| start.saturating_add(limit as usize).min(total_lines))
        .unwrap_or(total_lines);
    let selected = lines[start..end].join("\n");
    let truncated = truncate_head(&selected, READ_MAX_BYTES, READ_MAX_LINES);
    let mut hint = Value::Null;
    if truncated.truncated || end < total_lines {
        let next = start + truncated.lines + 1;
        hint = json!(format!("Use offset={next} to continue."));
    }
    Ok(json!({
        "path": tool.relative(&target),
        "content": truncated.content,
        "total_lines": total_lines,
        "file_size": file_size,
        "truncated": truncated.truncated || end < total_lines,
        "hint": hint,
        "error": null,
        "similar_files": []
    }))
}

struct ListTool(WorkdirTool);

impl ListTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for ListTool {
    fn name(&self) -> &str {
        "list"
    }

    fn description(&self) -> &str {
        "List files and directories under the working directory."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"limit":{"type":"integer"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match list_tool_impl(tool, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

pub(crate) fn list_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
    let path = optional_string(&args, "path")?.unwrap_or(".");
    let limit = bounded_limit(optional_i64(&args, "limit")?, 200, 1000)?;
    let target = tool.resolve_existing(path)?;
    let mut entries = Vec::new();
    if target.is_file() {
        entries.push(json!({
            "path": tool.relative(&target),
            "type": "file",
        }));
        return Ok(json!({
            "path": tool.relative(&target),
            "entries": entries,
            "truncated": false,
            "error": null
        }));
    }

    let mut raw_entries = fs::read_dir(&target)?.collect::<std::result::Result<Vec<_>, _>>()?;
    raw_entries.sort_by_key(|entry| entry.path());
    let truncated = raw_entries.len() > limit;
    for entry in raw_entries.into_iter().take(limit) {
        let file_type = entry.file_type()?;
        let kind = if file_type.is_dir() {
            "dir"
        } else if file_type.is_file() {
            "file"
        } else if file_type.is_symlink() {
            "symlink"
        } else {
            "other"
        };
        entries.push(json!({
            "path": tool.relative(&entry.path()),
            "type": kind,
        }));
    }

    Ok(json!({
        "path": tool.relative(&target),
        "entries": entries,
        "truncated": truncated,
        "error": null
    }))
}

struct SearchTool(WorkdirTool);

impl SearchTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search UTF-8 text files under the working directory for a literal string."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["query"],"properties":{"query":{"type":"string"},"path":{"type":"string"},"limit":{"type":"integer"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match search_tool_impl(tool, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

pub(crate) fn search_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
    let query = required_string(&args, "query")?;
    if query.is_empty() {
        return Err(Error::Message("query must not be empty".to_string()));
    }
    let path = optional_string(&args, "path")?.unwrap_or(".");
    let limit = bounded_limit(optional_i64(&args, "limit")?, 100, 1000)?;
    let target = tool.resolve_existing(path)?;
    let mut queue = VecDeque::from([target.clone()]);
    let mut matches = Vec::new();
    let mut skipped_files = 0usize;
    let mut truncated = false;

    while let Some(path) = queue.pop_front() {
        if matches.len() >= limit {
            truncated = true;
            break;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            skipped_files += 1;
            continue;
        }
        if metadata.is_dir() {
            let mut entries = fs::read_dir(&path)?.collect::<std::result::Result<Vec<_>, _>>()?;
            entries.sort_by_key(|entry| entry.path());
            for entry in entries {
                let name = entry.file_name();
                if name == ".git" {
                    continue;
                }
                queue.push_back(entry.path());
            }
            continue;
        }
        if !metadata.is_file() {
            skipped_files += 1;
            continue;
        }
        let bytes = fs::read(&path)?;
        if bytes.contains(&0) {
            skipped_files += 1;
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            skipped_files += 1;
            continue;
        };
        for (idx, line) in content.lines().enumerate() {
            if line.contains(query) {
                matches.push(json!({
                    "path": tool.relative(&path),
                    "line_number": idx + 1,
                    "line": truncate_match_line(line),
                }));
                if matches.len() >= limit {
                    truncated = true;
                    break;
                }
            }
        }
    }

    Ok(json!({
        "path": tool.relative(&target),
        "query": query,
        "matches": matches,
        "truncated": truncated,
        "skipped_files": skipped_files,
        "error": null
    }))
}

struct WriteTool(WorkdirTool);

impl WriteTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Create or completely replace a UTF-8 text file."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["path","content"],"properties":{"path":{"type":"string"},"content":{"type":"string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match write_tool_impl(tool, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn write_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let content = required_string(&args, "content")?;
    let (target, dirs_created) = tool.resolve_write_target(path)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&target, content)?;
    Ok(json!({
        "path": tool.relative(&target),
        "bytes_written": content.len(),
        "dirs_created": dirs_created,
        "error": null
    }))
}

struct EditTool(WorkdirTool);

impl EditTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Apply targeted replacements or a unified diff to existing text files."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"mode":{"type":"string"},"path":{"type":"string"},"edits":{"type":"array"},"patch":{"type":"string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match edit_tool_impl(tool, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn edit_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("replace");
    match mode {
        "replace" => edit_replace(tool, args),
        "patch" => edit_patch(tool, args),
        _ => Err(Error::Message(format!("unsupported edit mode: {mode}"))),
    }
}

#[derive(Debug, Deserialize)]
struct ReplaceEdit {
    #[serde(rename = "oldText")]
    old_text: String,
    #[serde(rename = "newText")]
    new_text: String,
}

fn edit_replace(tool: WorkdirTool, args: Value) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let edits_value = args
        .get("edits")
        .ok_or_else(|| Error::Message("edits is required".to_string()))?;
    let edits: Vec<ReplaceEdit> = serde_json::from_value(edits_value.clone())?;
    if edits.is_empty() {
        return Err(Error::Message("edits must not be empty".to_string()));
    }
    let target = tool.resolve_existing(path)?;
    let original_bytes = fs::read(&target)?;
    let original_text = String::from_utf8(original_bytes)
        .map_err(|_| Error::Message("invalid UTF-8".to_string()))?;
    let bom = original_text.starts_with('\u{feff}');
    let body = original_text.trim_start_matches('\u{feff}');
    let line_ending = dominant_line_ending(body);
    let normalized = normalize_lf(body);
    let mut ranges = Vec::new();
    for edit in &edits {
        if edit.old_text == edit.new_text {
            return Err(Error::Message("no-change edit".to_string()));
        }
        let old = normalize_lf(edit.old_text.trim_start_matches('\u{feff}'));
        let matches = normalized.match_indices(&old).collect::<Vec<_>>();
        if matches.is_empty() {
            return Err(Error::Message(format!(
                "oldText not found: {}",
                edit.old_text
            )));
        }
        if matches.len() > 1 {
            return Err(Error::Message(format!(
                "oldText is ambiguous: {}",
                edit.old_text
            )));
        }
        let start = matches[0].0;
        let end = start + old.len();
        ranges.push((start, end, normalize_lf(&edit.new_text)));
    }
    ranges.sort_by_key(|(start, _, _)| *start);
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(Error::Message("edits overlap".to_string()));
        }
    }
    let mut updated = String::new();
    let mut cursor = 0usize;
    for (start, end, replacement) in ranges {
        updated.push_str(&normalized[cursor..start]);
        updated.push_str(&replacement);
        cursor = end;
    }
    updated.push_str(&normalized[cursor..]);
    let diff = unified_diff(&tool.relative(&target), &normalized, &updated);
    let restored = restore_line_endings(&updated, line_ending);
    fs::write(
        &target,
        if bom {
            format!("\u{feff}{restored}")
        } else {
            restored
        },
    )?;
    Ok(json!({
        "success": true,
        "diff": diff,
        "files_modified": [tool.relative(&target)],
        "error": null
    }))
}

fn edit_patch(tool: WorkdirTool, args: Value) -> Result<Value> {
    let patch = required_string(&args, "patch")?;
    let files = parse_unified_patch(patch)?;
    if files.is_empty() {
        return Err(Error::Message("patch contains no file updates".to_string()));
    }
    let mut diffs = Vec::new();
    let mut modified = Vec::new();
    for file in files {
        let target = tool.resolve_existing(&file.path)?;
        let original_text = String::from_utf8(fs::read(&target)?)
            .map_err(|_| Error::Message("invalid UTF-8".to_string()))?;
        let bom = original_text.starts_with('\u{feff}');
        let body = original_text.trim_start_matches('\u{feff}');
        let line_ending = dominant_line_ending(body);
        let mut lines = normalize_lf(body)
            .split('\n')
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        for hunk in file.hunks {
            let idx = find_unique_subslice(&lines, &hunk.old_lines).ok_or_else(|| {
                Error::Message(format!("patch hunk did not match uniquely: {}", file.path))
            })?;
            lines.splice(idx..idx + hunk.old_lines.len(), hunk.new_lines);
        }
        let updated = lines.join("\n");
        let original_norm = normalize_lf(body);
        let rel = tool.relative(&target);
        diffs.push(unified_diff(&rel, &original_norm, &updated));
        let restored = restore_line_endings(&updated, line_ending);
        fs::write(
            &target,
            if bom {
                format!("\u{feff}{restored}")
            } else {
                restored
            },
        )?;
        modified.push(rel);
    }
    Ok(json!({
        "success": true,
        "diff": diffs.join("\n"),
        "files_modified": modified,
        "error": null
    }))
}

#[derive(Debug)]
struct PatchFile {
    path: String,
    hunks: Vec<PatchHunk>,
}

#[derive(Debug)]
struct PatchHunk {
    old_lines: Vec<String>,
    new_lines: Vec<String>,
}

fn parse_unified_patch(patch: &str) -> Result<Vec<PatchFile>> {
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

fn strip_diff_prefix(path: &str) -> String {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
        .to_string()
}

fn find_unique_subslice(lines: &[String], needle: &[String]) -> Option<usize> {
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

struct BashTool(WorkdirTool);

impl BashTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a bounded foreground bash command in the working directory."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["command"],"properties":{"command":{"type":"string"},"timeout":{"type":"number"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let workdir = self.0.workdir.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match bash_tool_impl(workdir, args, abort).await {
                Ok((value, is_error)) => ToolOutput {
                    json: value,
                    is_error,
                },
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

async fn bash_tool_impl(
    workdir: PathBuf,
    args: Value,
    abort: AbortSignal,
) -> Result<(Value, bool)> {
    let command = required_string(&args, "command")?.to_string();
    let timeout_secs = optional_u64(&args, "timeout")?
        .unwrap_or(BASH_DEFAULT_TIMEOUT_SECS)
        .min(BASH_MAX_TIMEOUT_SECS);
    let mut child = Command::new("bash")
        .arg("-lc")
        .arg(&command)
        .current_dir(&workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf).await;
        buf
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        buf
    });
    let status = time::timeout(Duration::from_secs(timeout_secs), child.wait()).await;
    let (exit_code, mut error) = match status {
        Ok(Ok(status)) => (status.code(), None),
        Ok(Err(err)) => return Err(err.into()),
        Err(_) => {
            let _ = child.kill().await;
            (
                None,
                Some(format!("command timed out after {timeout_secs} seconds")),
            )
        }
    };
    if abort.aborted() {
        let _ = child.kill().await;
        error = Some("aborted".to_string());
    }
    let mut output = stdout_task.await.unwrap_or_default();
    output.extend(stderr_task.await.unwrap_or_default());
    let output = String::from_utf8_lossy(&output).to_string();
    let truncated = truncate_tail(&output, READ_MAX_BYTES, READ_MAX_LINES);
    if exit_code.is_some_and(|code| code != 0) && error.is_none() {
        error = Some(format!(
            "command exited with code {}",
            exit_code.unwrap_or_default()
        ));
    }
    let meaning = exit_code.and_then(|code| exit_code_meaning(&command, code));
    let is_error = error.is_some() || exit_code.is_some_and(|code| code != 0);
    let output_text = if truncated.content.is_empty() {
        "(no output)".to_string()
    } else {
        truncated.content
    };
    Ok((
        json!({
            "output": output_text,
            "exit_code": exit_code,
            "error": error,
            "exit_code_meaning": meaning,
            "truncated": truncated.truncated
        }),
        is_error,
    ))
}

fn exit_code_meaning(command: &str, code: i32) -> Option<String> {
    if code != 1 {
        return None;
    }
    let first = command.split_whitespace().next().unwrap_or_default();
    match first {
        "grep" | "rg" | "ag" | "ack" => Some("no matches found".to_string()),
        "diff" => Some("files differ".to_string()),
        "test" | "[" => Some("condition evaluated false".to_string()),
        _ => None,
    }
}

fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Message(format!("{key} must be a string")))
}

fn optional_string<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>> {
    args.get(key)
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| Error::Message(format!("{key} must be a string")))
        })
        .transpose()
}

fn optional_i64(args: &Value, key: &str) -> Result<Option<i64>> {
    args.get(key)
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| Error::Message(format!("{key} must be an integer")))
        })
        .transpose()
}

fn bounded_limit(value: Option<i64>, default: usize, max: usize) -> Result<usize> {
    let limit = value.unwrap_or(default as i64);
    if limit < 1 {
        return Err(Error::Message("limit must be >= 1".to_string()));
    }
    Ok((limit as usize).min(max))
}

fn truncate_match_line(line: &str) -> String {
    const MAX_LINE_CHARS: usize = 240;
    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_string();
    }
    let mut value = line.chars().take(MAX_LINE_CHARS).collect::<String>();
    value.push_str("...");
    value
}

fn optional_u64(args: &Value, key: &str) -> Result<Option<u64>> {
    args.get(key)
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| Error::Message(format!("{key} must be an integer")))
        })
        .transpose()
}

#[derive(Debug)]
struct Truncated {
    content: String,
    truncated: bool,
    lines: usize,
}

fn truncate_head(input: &str, max_bytes: usize, max_lines: usize) -> Truncated {
    let mut out = String::new();
    let mut lines = 0usize;
    let mut bytes = 0usize;
    let mut truncated = false;
    for (idx, line) in input.split('\n').enumerate() {
        let addition = if idx == 0 {
            line.to_string()
        } else {
            format!("\n{line}")
        };
        if lines >= max_lines || bytes + addition.len() > max_bytes {
            truncated = true;
            break;
        }
        bytes += addition.len();
        out.push_str(&addition);
        lines += 1;
    }
    Truncated {
        content: out,
        truncated,
        lines,
    }
}

fn truncate_tail(input: &str, max_bytes: usize, max_lines: usize) -> Truncated {
    let all = input.split('\n').collect::<Vec<_>>();
    let mut selected = Vec::new();
    let mut bytes = 0usize;
    for line in all.iter().rev() {
        let addition = line.len() + usize::from(!selected.is_empty());
        if selected.len() >= max_lines || bytes + addition > max_bytes {
            break;
        }
        bytes += addition;
        selected.push(*line);
    }
    selected.reverse();
    Truncated {
        content: selected.join("\n"),
        truncated: selected.len() < all.len(),
        lines: selected.len(),
    }
}

fn dominant_line_ending(text: &str) -> &'static str {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count();
    if crlf > 0 && crlf >= lf.saturating_sub(crlf) {
        "\r\n"
    } else {
        "\n"
    }
}

fn normalize_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn restore_line_endings(text: &str, line_ending: &str) -> String {
    if line_ending == "\n" {
        text.to_string()
    } else {
        text.replace('\n', line_ending)
    }
}

fn unified_diff(path: &str, old: &str, new: &str) -> String {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string()
}
