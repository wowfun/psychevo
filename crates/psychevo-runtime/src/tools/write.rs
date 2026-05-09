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

