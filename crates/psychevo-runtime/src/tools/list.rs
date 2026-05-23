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
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory or file path to list, relative to the working directory or absolute inside it. Defaults to the working directory."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of directory entries to return; defaults to 200 and is capped at 1000."
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
