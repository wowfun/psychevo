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
        "Create or completely replace a text file. Existing files must be read completely and remain unchanged before replacement."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path", "content"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to create or replace; relative paths resolve from the working directory"
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
    let initially_existed = target.exists();
    let _locks = acquire_path_locks(std::slice::from_ref(&target));
    let (pre_content, persisted_content, baseline) = if initially_existed {
        if !target.exists() {
            return Err(MutationConflict::TargetMissing {
                path: target.clone(),
            }
            .into());
        }
        let expected = require_fresh_read(tool.task_id(), &target)?;
        let (text, snapshot_version) = read_text_snapshot(&LOCAL_FILE_MUTATION, &target)?;
        if snapshot_version != expected {
            return Err(MutationConflict::Modified {
                path: target.clone(),
            }
            .into());
        }
        let normalized = normalize_lf(content.trim_start_matches('\u{feff}'));
        let persisted = restore_text_file(&text, &normalized);
        let baseline = snapshot_lsp_baseline(&tool, &target, Some(&text.original));
        LOCAL_FILE_MUTATION
            .replace(tool.task_id(), &target, expected, persisted.as_bytes())
            .map_err(Error::from)?;
        (Some(text.original), persisted, baseline)
    } else {
        let baseline = snapshot_lsp_baseline(&tool, &target, None);
        LOCAL_FILE_MUTATION
            .create(tool.task_id(), &target, content.as_bytes())
            .map_err(Error::from)?;
        (None, content.to_string(), baseline)
    };
    tool.observe_workspace_mutation(WorkspaceMutation::ExactUtf8 {
        path: tool.relative(&target),
        before: pre_content.clone(),
        after: Some(persisted_content.clone()),
    });
    Ok(write_success_value(
        &tool,
        &target,
        &persisted_content,
        dirs_created,
        pre_content.as_deref(),
        baseline,
    ))
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
                ..ToolRuntimeContext::default()
            },
        )
    }

    fn cwd_tool_with_mutations(
        path: &Path,
        mutations: Arc<Mutex<Vec<WorkspaceMutation>>>,
    ) -> CwdTool {
        CwdTool::with_context(
            path.canonicalize().expect("canonical cwd"),
            ToolRuntimeContext {
                workspace_mutations: Some(WorkspaceMutationSink::new(move |mutation| {
                    mutations.lock().expect("mutations poisoned").push(mutation);
                })),
                ..ToolRuntimeContext::default()
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
                ..ToolRuntimeContext::default()
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
    fn write_emits_exact_workspace_mutation_after_success() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("notes.txt"), "before\n").expect("seed");
        let mutations = Arc::new(Mutex::new(Vec::new()));
        let tool = cwd_tool_with_mutations(temp.path(), mutations.clone());
        read_tool_impl(tool.clone(), json!({"path": "notes.txt"})).expect("read");

        write_tool_impl(tool, json!({"path": "notes.txt", "content": "after\n"})).expect("write");

        assert_eq!(
            *mutations.lock().expect("mutations poisoned"),
            vec![WorkspaceMutation::ExactUtf8 {
                path: "notes.txt".to_string(),
                before: Some("before\n".to_string()),
                after: Some("after\n".to_string()),
            }]
        );
    }

    #[test]
    fn write_tool_rejects_unread_existing_file_without_inspecting_or_changing_it() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("bad.txt"), [0xff, 0xfe]).expect("seed");
        let err = write_tool_impl(
            cwd_tool(temp.path()),
            json!({"path": "bad.txt", "content": "replacement\n"}),
        )
        .expect_err("unread existing file");
        assert!(
            err.to_string().starts_with(&format!(
                "{} already exists",
                temp.path().join("bad.txt").display()
            )),
            "unexpected conflict: {err}"
        );
        assert!(err.to_string().contains("Read the complete existing file"));
        assert_eq!(
            fs::read(temp.path().join("bad.txt")).expect("unchanged"),
            [0xff, 0xfe]
        );
    }

    #[test]
    fn write_tool_fails_after_partial_read_without_modifying_the_file() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("partial.txt"), "one\ntwo\nthree\n").expect("seed");
        let tool = cwd_tool_with_task(temp.path(), "partial-read-test");
        read_tool_impl(
            tool.clone(),
            json!({"path": "partial.txt", "offset": 1, "limit": 1}),
        )
        .expect("read");
        let err = write_tool_impl(
            tool,
            json!({"path": "partial.txt", "content": "replacement\n"}),
        )
        .expect_err("partial read conflict");
        assert!(err.to_string().contains("partial or truncated"));
        assert_eq!(
            fs::read_to_string(temp.path().join("partial.txt")).expect("unchanged"),
            "one\ntwo\nthree\n"
        );
    }

    #[test]
    fn write_tool_fails_after_sibling_write_even_when_mtime_is_restored() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("shared.txt"), "one\n").expect("seed");
        let path = temp.path().join("shared.txt");
        let original_mtime = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .expect("mtime");
        let reader = cwd_tool_with_task(temp.path(), "reader-agent");
        let writer = cwd_tool_with_task(temp.path(), "writer-agent");
        read_tool_impl(reader.clone(), json!({"path": "shared.txt"})).expect("read");
        read_tool_impl(writer.clone(), json!({"path": "shared.txt"})).expect("writer read");
        write_tool_impl(writer, json!({"path": "shared.txt", "content": "two\n"}))
            .expect("sibling write");
        fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(original_mtime)))
            .expect("restore mtime");
        let err = write_tool_impl(reader, json!({"path": "shared.txt", "content": "three\n"}))
            .expect_err("sibling conflict");
        assert!(err.to_string().contains("sibling agent"));
        assert_eq!(fs::read_to_string(path).expect("winner"), "two\n");
    }

    #[test]
    fn write_tool_accepts_full_read_and_refreshes_its_own_version() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("note.txt"), "one\n").expect("seed");
        let tool = cwd_tool_with_task(temp.path(), "owner-agent");
        read_tool_impl(tool.clone(), json!({"path": "note.txt"})).expect("read");
        let first = write_tool_impl(
            tool.clone(),
            json!({"path": "note.txt", "content": "two\n"}),
        )
        .expect("first write");
        assert!(first.get("warning").is_none());
        write_tool_impl(tool, json!({"path": "note.txt", "content": "three\n"}))
            .expect("own write refreshes version");
        assert_eq!(
            fs::read_to_string(temp.path().join("note.txt")).expect("file"),
            "three\n"
        );
    }

    #[test]
    fn write_tool_rejects_changed_mtime_after_full_read() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("note.txt");
        fs::write(&path, "one\n").expect("seed");
        let tool = cwd_tool_with_task(temp.path(), "mtime-agent");
        read_tool_impl(tool.clone(), json!({"path": "note.txt"})).expect("read");
        fs::write(&path, "external\n").expect("external write");
        let changed = SystemTime::now() + Duration::from_secs(2);
        fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(changed)))
            .expect("change mtime");
        let err = write_tool_impl(tool, json!({"path": "note.txt", "content": "agent\n"}))
            .expect_err("mtime conflict");
        assert!(err.to_string().contains("changed on disk"));
        assert_eq!(
            fs::read_to_string(path).expect("external wins"),
            "external\n"
        );
    }

    #[test]
    fn write_tool_preserves_existing_bom_and_crlf() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("win.txt");
        fs::write(&path, "\u{feff}one\r\ntwo\r\n").expect("seed");
        let tool = cwd_tool_with_task(temp.path(), "style-agent");
        read_tool_impl(tool.clone(), json!({"path": "win.txt"})).expect("read");
        write_tool_impl(tool, json!({"path": "win.txt", "content": "uno\ndos\n"})).expect("write");
        assert_eq!(
            fs::read_to_string(path).expect("styled"),
            "\u{feff}uno\r\ndos\r\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_tool_preserves_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("script.sh");
        fs::write(&path, "#!/bin/sh\nexit 0\n").expect("seed");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        let tool = cwd_tool_with_task(temp.path(), "mode-agent");
        read_tool_impl(tool.clone(), json!({"path": "script.sh"})).expect("read");
        write_tool_impl(
            tool,
            json!({"path": "script.sh", "content": "#!/bin/sh\nexit 1\n"}),
        )
        .expect("write");
        assert_eq!(
            fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
            0o755
        );
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
