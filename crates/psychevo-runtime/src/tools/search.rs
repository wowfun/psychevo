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

