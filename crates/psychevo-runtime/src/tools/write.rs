struct WriteTool(WorkdirTool);

impl WriteTool {
    fn new(workdir: PathBuf, context: ToolRuntimeContext) -> Self {
        Self(WorkdirTool::with_context(workdir, context))
    }
}

impl ToolBinding for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Create or completely replace a UTF-8 text file inside the working directory. Use write instead of shell redirection when writing complete file contents. Creates missing parent directories when allowed, then returns lint and LSP diagnostics when available."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path", "content"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to create or completely replace, relative to the working directory or absolute inside it"
                },
                "content": {
                    "type": "string",
                    "description": "Complete UTF-8 text content to write to the file"
                }
            }
        })
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
            result_output(write_tool_impl(tool, args))
        })
    }
}

fn write_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let content = required_string(&args, "content")?;
    let (target, dirs_created) = tool.resolve_write_target(path)?;
    let _locks = acquire_path_locks(std::slice::from_ref(&target));
    let warning = stale_file_warning(tool.task_id(), &target);
    let pre_content = if target.exists() {
        Some(
            String::from_utf8(fs::read(&target)?)
                .map_err(|_| Error::Message("invalid UTF-8".to_string()))?,
        )
    } else {
        None
    };
    write_text_to_target(
        &tool,
        &target,
        content,
        dirs_created,
        pre_content.as_deref(),
        warning,
    )
}

#[cfg(test)]
mod write_tool_tests {
    use super::*;

    fn workdir_tool(path: &Path) -> WorkdirTool {
        WorkdirTool::with_context(
            path.canonicalize().expect("canonical workdir"),
            ToolRuntimeContext {
                task_id: uuid::Uuid::now_v7().to_string(),
                lsp: LspConfig {
                    enabled: false,
                    ..Default::default()
                },
                allow_login_shell: false,
                stream_events: None,
                path_prefixes: Vec::new(),
            },
        )
    }

    fn workdir_tool_with_task(path: &Path, task_id: &str) -> WorkdirTool {
        WorkdirTool::with_context(
            path.canonicalize().expect("canonical workdir"),
            ToolRuntimeContext {
                task_id: task_id.to_string(),
                lsp: LspConfig {
                    enabled: false,
                    ..Default::default()
                },
                allow_login_shell: false,
                stream_events: None,
                path_prefixes: Vec::new(),
            },
        )
    }

    #[test]
    fn write_tool_schema_describes_parameters() {
        let tool = WriteTool::new(PathBuf::from("/tmp/work"), ToolRuntimeContext::default());
        let schema = tool.parameters();
        assert_eq!(schema["required"], json!(["path", "content"]));
        assert!(
            schema["properties"]["path"]["description"]
                .as_str()
                .unwrap()
                .contains("working directory")
        );
        assert!(
            schema["properties"]["content"]["description"]
                .as_str()
                .unwrap()
                .contains("Complete UTF-8")
        );
    }

    #[test]
    fn write_tool_creates_parent_and_returns_lint() {
        let temp = tempfile::tempdir().expect("temp");
        let value = write_tool_impl(
            workdir_tool(temp.path()),
            json!({"path": "nested/config.json", "content": "{\"ok\": true}\n"}),
        )
        .expect("write");
        assert_eq!(value["path"], "nested/config.json");
        assert_eq!(value["dirs_created"], true);
        assert_eq!(value["lint"]["status"], "ok");
        assert_eq!(
            fs::read_to_string(temp.path().join("nested/config.json")).expect("file"),
            "{\"ok\": true}\n"
        );
    }

    #[test]
    fn write_tool_rejects_invalid_utf8_existing_file() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("bad.txt"), [0xff, 0xfe]).expect("seed");
        let err = write_tool_impl(
            workdir_tool(temp.path()),
            json!({"path": "bad.txt", "content": "replacement\n"}),
        )
        .expect_err("invalid preexisting utf8");
        assert!(err.to_string().contains("invalid UTF-8"));
    }

    #[test]
    fn write_tool_warns_after_partial_read() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("partial.txt"), "one\ntwo\nthree\n").expect("seed");
        let tool = workdir_tool_with_task(temp.path(), "partial-read-test");
        read_tool_impl(tool.clone(), json!({"path": "partial.txt", "offset": 1, "limit": 1}))
            .expect("read");
        let value = write_tool_impl(
            tool,
            json!({"path": "partial.txt", "content": "replacement\n"}),
        )
        .expect("write");
        assert!(value["warning"].as_str().unwrap().contains("partial"));
    }

    #[test]
    fn write_tool_warns_after_sibling_write() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("shared.txt"), "one\n").expect("seed");
        let reader = workdir_tool_with_task(temp.path(), "reader-agent");
        let writer = workdir_tool_with_task(temp.path(), "writer-agent");
        read_tool_impl(reader.clone(), json!({"path": "shared.txt"})).expect("read");
        write_tool_impl(
            writer,
            json!({"path": "shared.txt", "content": "two\n"}),
        )
        .expect("sibling write");
        let value = write_tool_impl(
            reader,
            json!({"path": "shared.txt", "content": "three\n"}),
        )
        .expect("reader write");
        assert!(value["warning"].as_str().unwrap().contains("sibling agent"));
    }
}
