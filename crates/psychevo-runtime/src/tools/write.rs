#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) struct WriteTool(CwdTool);

impl WriteTool {
    pub(crate) fn new(cwd: PathBuf, context: ToolRuntimeContext) -> Self {
        Self(CwdTool::with_context(cwd, context))
    }
}

impl ToolBinding for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Create or completely replace a UTF-8 text file inside configured writer roots, normally the working directory. Use write instead of shell redirection when writing project files; shell-only temp roots are for exec_command artifacts. Creates missing parent directories when allowed, then returns lint and LSP diagnostics when available."
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
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            result_output(write_tool_impl_for_call(tool, Some(&tool_call_id), args))
        })
    }
}

#[cfg(test)]
pub(crate) fn write_tool_impl(tool: CwdTool, args: Value) -> Result<Value> {
    write_tool_impl_for_call(tool, None, args)
}

pub(crate) fn write_tool_impl_for_call(
    tool: CwdTool,
    tool_call_id: Option<&str>,
    args: Value,
) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let content = required_string(&args, "content")?;
    let (target, dirs_created) = tool.resolve_write_target(path)?;
    tool.ensure_sandbox_write_allowed(&target, tool_call_id)?;
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
pub(crate) mod write_tool_tests {
    pub(crate) use super::*;

    fn cwd_tool(path: &Path) -> CwdTool {
        CwdTool::with_context(
            path.canonicalize().expect("canonical cwd"),
            ToolRuntimeContext {
                task_id: uuid::Uuid::now_v7().to_string(),
                lsp: LspConfig {
                    enabled: false,
                    ..Default::default()
                },
                lsp_manager: default_lsp_manager(),
                allow_login_shell: false,
                stream_events: None,
                env: BTreeMap::new(),
                path_prefixes: Vec::new(),
                sandbox_policy: SandboxPolicy::disabled(),
                sandbox_grants: crate::sandbox::SandboxWriteGrants::default(),
            },
        )
    }

    fn cwd_tool_with_task(path: &Path, task_id: &str) -> CwdTool {
        CwdTool::with_context(
            path.canonicalize().expect("canonical cwd"),
            ToolRuntimeContext {
                task_id: task_id.to_string(),
                lsp: LspConfig {
                    enabled: false,
                    ..Default::default()
                },
                lsp_manager: default_lsp_manager(),
                allow_login_shell: false,
                stream_events: None,
                env: BTreeMap::new(),
                path_prefixes: Vec::new(),
                sandbox_policy: SandboxPolicy::disabled(),
                sandbox_grants: crate::sandbox::SandboxWriteGrants::default(),
            },
        )
    }

    fn cwd_tool_with_sandbox(path: &Path) -> CwdTool {
        let env = BTreeMap::new();
        let policy = crate::sandbox::SandboxPolicy::from_config(
            &crate::sandbox::SandboxConfig {
                enabled: true,
                mode: crate::sandbox::SandboxMode::WorkspaceWrite,
                writable_roots: Vec::new(),
                include_tmp: false,
                include_common_caches: false,
            },
            path,
            RunMode::Default,
            &env,
        )
        .expect("sandbox policy");
        CwdTool::with_context(
            path.canonicalize().expect("canonical cwd"),
            ToolRuntimeContext {
                sandbox_policy: policy,
                ..ToolRuntimeContext::default()
            },
        )
    }

    fn cwd_tool_with_shell_tmp(path: &Path, tmp: &Path) -> CwdTool {
        let env = BTreeMap::from([("TMPDIR".to_string(), tmp.display().to_string())]);
        let policy = crate::sandbox::SandboxPolicy::from_config(
            &crate::sandbox::SandboxConfig {
                enabled: true,
                mode: crate::sandbox::SandboxMode::WorkspaceWrite,
                writable_roots: Vec::new(),
                include_tmp: true,
                include_common_caches: false,
            },
            path,
            RunMode::Default,
            &env,
        )
        .expect("sandbox policy");
        CwdTool::with_context(
            path.canonicalize().expect("canonical cwd"),
            ToolRuntimeContext {
                sandbox_policy: policy,
                ..ToolRuntimeContext::default()
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
            cwd_tool(temp.path()),
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
            cwd_tool(temp.path()),
            json!({"path": "bad.txt", "content": "replacement\n"}),
        )
        .expect_err("invalid preexisting utf8");
        assert!(err.to_string().contains("invalid UTF-8"));
    }

    #[test]
    fn write_tool_warns_after_partial_read() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("partial.txt"), "one\ntwo\nthree\n").expect("seed");
        let tool = cwd_tool_with_task(temp.path(), "partial-read-test");
        read_tool_impl(
            tool.clone(),
            json!({"path": "partial.txt", "offset": 1, "limit": 1}),
        )
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
        let reader = cwd_tool_with_task(temp.path(), "reader-agent");
        let writer = cwd_tool_with_task(temp.path(), "writer-agent");
        read_tool_impl(reader.clone(), json!({"path": "shared.txt"})).expect("read");
        write_tool_impl(writer, json!({"path": "shared.txt", "content": "two\n"}))
            .expect("sibling write");
        let value = write_tool_impl(reader, json!({"path": "shared.txt", "content": "three\n"}))
            .expect("reader write");
        assert!(value["warning"].as_str().unwrap().contains("sibling agent"));
    }

    #[test]
    fn write_tool_rejects_sandbox_write_outside_workspace() {
        let temp = tempfile::tempdir().expect("temp");
        let outside = tempfile::tempdir().expect("outside");
        let err = write_tool_impl(
            cwd_tool_with_sandbox(temp.path()),
            json!({
                "path": outside.path().join("blocked.txt").display().to_string(),
                "content": "nope\n",
            }),
        )
        .expect_err("sandbox denial");

        assert!(err.to_string().contains("denied by sandbox policy"));
        assert!(!outside.path().join("blocked.txt").exists());
    }

    #[test]
    fn write_tool_explains_shell_only_tmp_root_denial() {
        let temp = tempfile::tempdir().expect("temp");
        let shell_tmp = tempfile::tempdir().expect("shell tmp");
        let target = shell_tmp.path().join("blocked.txt");
        let err = write_tool_impl(
            cwd_tool_with_shell_tmp(temp.path(), shell_tmp.path()),
            json!({
                "path": target.display().to_string(),
                "content": "nope\n",
            }),
        )
        .expect_err("sandbox denial");

        let message = err.to_string();
        assert!(message.contains("shell-only"), "{message}");
        assert!(message.contains("write/edit"), "{message}");
        assert!(!target.exists());
    }
}
