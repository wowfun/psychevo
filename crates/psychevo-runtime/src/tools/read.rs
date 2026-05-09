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

