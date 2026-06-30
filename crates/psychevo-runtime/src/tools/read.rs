#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) struct ReadTool(CwdTool);

impl ReadTool {
    pub(crate) fn new(cwd: PathBuf, context: ToolRuntimeContext) -> Self {
        Self(CwdTool::with_context(cwd, context))
    }
}

impl ToolBinding for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read UTF-8 text files inside the working directory. Use read instead of shell cat/head/tail/sed for file contents. Output is bounded to 50KB or 2000 lines; use offset and limit to continue through large files."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the UTF-8 text file to read, relative to the working directory or absolute inside it"
                },
                "offset": {
                    "type": "integer",
                    "description": "1-based line number to start reading from; values below 1 are treated as 1",
                    "default": 1,
                    "minimum": 1
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read; values below 1 become 1 and values above 2000 become 2000",
                    "minimum": 1,
                    "maximum": READ_MAX_LINES
                }
            }
        })
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
            read_tool_output(tool, args)
        })
    }
}

pub(crate) fn read_tool_output(tool: CwdTool, args: Value) -> ToolOutput {
    match read_tool_impl(tool, args) {
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

pub(crate) fn value_reports_error(value: &Value) -> bool {
    value
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| !error.is_empty())
}

pub(crate) fn read_tool_impl(tool: CwdTool, args: Value) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let offset = optional_i64(&args, "offset")?.unwrap_or(1).max(1) as usize;
    let limit =
        optional_i64(&args, "limit")?.map(|limit| limit.clamp(1, READ_MAX_LINES as i64) as usize);
    let target = match tool.resolve_existing(path) {
        Ok(target) => target,
        Err(Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(missing_read_result(&tool, path));
        }
        Err(err) => return Err(err),
    };
    let bytes = fs::read(&target)?;
    let file_size = bytes.len();
    if bytes.contains(&0) {
        return Err(Error::Message("binary files are not supported".to_string()));
    }
    let content =
        String::from_utf8(bytes).map_err(|_| Error::Message("invalid UTF-8".to_string()))?;
    let lines = content.split('\n').collect::<Vec<_>>();
    let total_lines = lines.len();
    let start = offset.saturating_sub(1);
    if start >= total_lines {
        return Err(Error::Message(format!(
            "offset {offset} is beyond end of file ({total_lines} lines)"
        )));
    }
    let end = limit
        .map(|limit| start.saturating_add(limit).min(total_lines))
        .unwrap_or(total_lines);
    let selected = lines[start..end].join("\n");
    let truncated = truncate_head(&selected, READ_MAX_BYTES, READ_MAX_LINES);
    let truncated_by = if truncated.truncated {
        truncated.truncated_by
    } else if end < total_lines {
        Some("limit")
    } else {
        None
    };
    record_file_read(
        tool.task_id(),
        &target,
        offset > 1 || truncated_by.is_some(),
    );
    let next_offset = if truncated_by.is_some() {
        if truncated.lines > 0 {
            Some(offset + truncated.lines)
        } else if truncated.first_line_exceeds_limit {
            Some(offset + 1)
        } else {
            None
        }
    } else {
        None
    };
    let shown_start_line = (truncated.lines > 0).then_some(offset);
    let shown_end_line = (truncated.lines > 0).then_some(offset + truncated.lines - 1);
    let hint = read_hint(
        offset,
        total_lines,
        shown_start_line,
        shown_end_line,
        next_offset,
        truncated_by,
        truncated.first_line_exceeds_limit,
    );
    Ok(json!({
        "path": tool.relative(&target),
        "content": truncated.content,
        "total_lines": total_lines,
        "file_size": file_size,
        "truncated": truncated_by.is_some(),
        "hint": hint,
        "error": null,
        "similar_files": [],
        "shown_start_line": shown_start_line,
        "shown_end_line": shown_end_line,
        "next_offset": next_offset,
        "output_lines": truncated.lines,
        "output_bytes": truncated.bytes,
        "truncated_by": truncated_by,
        "first_line_exceeds_limit": truncated.first_line_exceeds_limit
    }))
}

pub(crate) fn read_hint(
    offset: usize,
    total_lines: usize,
    shown_start_line: Option<usize>,
    shown_end_line: Option<usize>,
    next_offset: Option<usize>,
    truncated_by: Option<&str>,
    first_line_exceeds_limit: bool,
) -> Option<String> {
    let next_offset = next_offset?;
    if first_line_exceeds_limit {
        return Some(format!(
            "Line {offset} exceeds the 50KB read limit. Use offset={next_offset} to continue after it."
        ));
    }
    match truncated_by {
        Some("limit") => Some(format!("Use offset={next_offset} to continue.")),
        Some("lines") | Some("bytes") => {
            let start = shown_start_line.unwrap_or(offset);
            let end = shown_end_line.unwrap_or(start);
            Some(format!(
                "Showing lines {start}-{end} of {total_lines}. Use offset={next_offset} to continue."
            ))
        }
        _ => None,
    }
}

pub(crate) fn missing_read_result(tool: &CwdTool, path: &str) -> Value {
    json!({
        "path": path,
        "error": format!("file not found: {path}"),
        "similar_files": similar_files(tool, path)
    })
}

pub(crate) fn similar_files(tool: &CwdTool, path: &str) -> Vec<String> {
    let Ok(target) = tool.resolve_raw(path) else {
        return Vec::new();
    };
    let Some(parent) = target.parent() else {
        return Vec::new();
    };
    let Ok(parent) = parent.canonicalize() else {
        return Vec::new();
    };
    if tool.ensure_contained(&parent).is_err() {
        return Vec::new();
    }
    let Some(filename) = target
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
    else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };

    let mut scored = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let candidate = entry.file_name().to_string_lossy().to_string();
        let score = similar_file_score(&filename, &candidate);
        if score > 0 {
            scored.push((score, tool.relative(&entry.path())));
        }
    }
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    scored.into_iter().take(5).map(|(_, path)| path).collect()
}

pub(crate) fn similar_file_score(query: &str, candidate: &str) -> usize {
    if query.is_empty() || candidate.is_empty() {
        return 0;
    }
    let query_lower = query.to_lowercase();
    let candidate_lower = candidate.to_lowercase();
    if candidate_lower == query_lower {
        return 100;
    }
    let query_stem = Path::new(query)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let candidate_stem = Path::new(candidate)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !query_stem.is_empty() && query_stem == candidate_stem {
        return 90;
    }
    if candidate_lower.starts_with(&query_lower) || query_lower.starts_with(&candidate_lower) {
        return 70;
    }
    if candidate_lower.contains(&query_lower) {
        return 60;
    }
    if query_lower.contains(&candidate_lower) && candidate_lower.chars().count() > 2 {
        return 40;
    }
    let query_ext = Path::new(query)
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase());
    let candidate_ext = Path::new(candidate)
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase());
    if query_ext.is_some() && query_ext == candidate_ext {
        let query_chars = query_lower.chars().count();
        let candidate_chars = candidate_lower.chars().count();
        let common = query_lower
            .chars()
            .filter(|ch| candidate_lower.contains(*ch))
            .count();
        if common * 10 >= query_chars.max(candidate_chars) * 4 {
            return 30;
        }
    }
    0
}

#[cfg(test)]
pub(crate) mod read_tool_tests {
    pub(crate) use super::*;
    use psychevo_agent_core::ToolBinding;
    use serde_json::{Value, json};
    use std::fs;
    use tempfile::tempdir;

    fn cwd_tool(path: &std::path::Path) -> CwdTool {
        CwdTool::new(path.canonicalize().expect("canonical cwd"))
    }

    fn numbered_lines(count: usize) -> String {
        (1..=count)
            .map(|line| format!("Line {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn read_tool_schema_describes_pagination_bounds() {
        let temp = tempdir().expect("temp");
        let tool = ReadTool::new(temp.path().to_path_buf(), ToolRuntimeContext::default());

        assert!(tool.description().contains("50KB"));
        assert!(tool.description().contains("2000 lines"));
        assert!(tool.description().contains("cat/head/tail/sed"));

        let parameters = tool.parameters();
        assert_eq!(parameters["required"], json!(["path"]));
        assert!(
            parameters["properties"]["path"]["description"]
                .as_str()
                .expect("path description")
                .contains("UTF-8")
        );
        assert_eq!(parameters["properties"]["offset"]["default"], 1);
        assert_eq!(parameters["properties"]["offset"]["minimum"], 1);
        assert_eq!(parameters["properties"]["limit"]["minimum"], 1);
        assert_eq!(
            parameters["properties"]["limit"]["maximum"],
            json!(READ_MAX_LINES)
        );
    }

    #[test]
    fn read_tool_normalizes_low_offset_and_limit() {
        let temp = tempdir().expect("temp");
        fs::write(temp.path().join("sample.txt"), numbered_lines(5)).expect("write");

        let value = read_tool_impl(
            cwd_tool(temp.path()),
            json!({"path": "sample.txt", "offset": 0, "limit": 0}),
        )
        .expect("read");

        assert_eq!(value["content"], "Line 1");
        assert_eq!(value["shown_start_line"], 1);
        assert_eq!(value["shown_end_line"], 1);
        assert_eq!(value["next_offset"], 2);
        assert_eq!(value["truncated_by"], "limit");
    }

    #[test]
    fn read_tool_clamps_high_limit() {
        let temp = tempdir().expect("temp");
        fs::write(
            temp.path().join("large.txt"),
            numbered_lines(READ_MAX_LINES + 5),
        )
        .expect("write");

        let value = read_tool_impl(
            cwd_tool(temp.path()),
            json!({"path": "large.txt", "limit": READ_MAX_LINES + 100}),
        )
        .expect("read");

        assert_eq!(value["output_lines"], json!(READ_MAX_LINES));
        assert_eq!(value["shown_end_line"], json!(READ_MAX_LINES));
        assert_eq!(value["next_offset"], json!(READ_MAX_LINES + 1));
        assert_eq!(value["truncated_by"], "limit");
    }

    #[test]
    fn read_tool_reports_line_safety_truncation() {
        let temp = tempdir().expect("temp");
        fs::write(
            temp.path().join("large.txt"),
            numbered_lines(READ_MAX_LINES + 5),
        )
        .expect("write");

        let value =
            read_tool_impl(cwd_tool(temp.path()), json!({"path": "large.txt"})).expect("read");

        assert_eq!(value["output_lines"], json!(READ_MAX_LINES));
        assert_eq!(value["shown_start_line"], 1);
        assert_eq!(value["shown_end_line"], json!(READ_MAX_LINES));
        assert_eq!(value["next_offset"], json!(READ_MAX_LINES + 1));
        assert_eq!(value["truncated_by"], "lines");
    }

    #[test]
    fn read_tool_reports_byte_safety_truncation() {
        let temp = tempdir().expect("temp");
        let content = (1..=500)
            .map(|line| format!("Line {line}: {}", "x".repeat(200)))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(temp.path().join("large-bytes.txt"), content).expect("write");

        let value = read_tool_impl(cwd_tool(temp.path()), json!({"path": "large-bytes.txt"}))
            .expect("read");

        assert_eq!(value["truncated_by"], "bytes");
        assert_eq!(value["first_line_exceeds_limit"], false);
        assert!(value["output_bytes"].as_u64().expect("output bytes") <= READ_MAX_BYTES as u64);
        assert_eq!(
            value["next_offset"].as_u64().expect("next offset"),
            value["output_lines"].as_u64().expect("output lines") + 1
        );
    }

    #[test]
    fn read_tool_marks_first_line_exceeding_limit_without_self_looping() {
        let temp = tempdir().expect("temp");
        let content = format!("{}\nsmall", "x".repeat(READ_MAX_BYTES + 1));
        fs::write(temp.path().join("long-line.txt"), content).expect("write");

        let value =
            read_tool_impl(cwd_tool(temp.path()), json!({"path": "long-line.txt"})).expect("read");

        assert_eq!(value["content"], "");
        assert_eq!(value["output_lines"], 0);
        assert_eq!(value["shown_start_line"], Value::Null);
        assert_eq!(value["shown_end_line"], Value::Null);
        assert_eq!(value["truncated_by"], "bytes");
        assert_eq!(value["first_line_exceeds_limit"], true);
        assert_eq!(value["next_offset"], 2);
    }

    #[test]
    fn missing_read_returns_failed_result_with_similar_files() {
        let temp = tempdir().expect("temp");
        fs::write(temp.path().join("config.yaml"), "name: test\n").expect("write");
        fs::write(temp.path().join("config.toml"), "name = \"test\"\n").expect("write");
        fs::write(temp.path().join("notes.txt"), "notes\n").expect("write");

        let output = read_tool_output(cwd_tool(temp.path()), json!({"path": "config.yml"}));

        assert!(output.is_error);
        assert!(
            output.json["error"]
                .as_str()
                .expect("error")
                .contains("config.yml")
        );
        let similar = output.json["similar_files"]
            .as_array()
            .expect("similar files");
        assert!(similar.len() <= 5);
        assert!(similar.iter().any(|path| path == "config.yaml"));
        assert!(similar.iter().any(|path| path == "config.toml"));
    }
}
