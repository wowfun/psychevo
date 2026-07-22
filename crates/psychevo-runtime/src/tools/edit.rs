#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) struct EditTool(CwdTool);

impl EditTool {
    pub(crate) fn new(cwd: PathBuf, context: ToolRuntimeContext) -> Self {
        Self(CwdTool::with_context(cwd, context))
    }
}

impl ToolBinding for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Replace text in one file or apply a V4A Update/Add/Delete patch. Delete requires a complete prior read; Move is unsupported, and patch failure may leave earlier operations committed."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["replace", "patch"],
                    "description": "Edit mode. 'replace' uses path + old_string + new_string. 'patch' uses V4A patch content.",
                    "default": "replace"
                },
                "path": {
                    "type": "string",
                    "description": "Required when mode='replace'. File path to edit; relative paths resolve from the working directory."
                },
                "old_string": {
                    "type": "string",
                    "description": "Required when mode='replace'. Text to find. Must match uniquely unless replace_all=true. Include surrounding context for safe matching."
                },
                "new_string": {
                    "type": "string",
                    "description": "Required when mode='replace'. Replacement text. Pass an empty string to delete the matched text."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every match instead of requiring a unique match.",
                    "default": false
                },
                "patch": {
                    "type": "string",
                    "description": "Required when mode='patch'. V4A patch content, for example:\n*** Begin Patch\n*** Update File: path/to/file\n@@ context hint @@\n context line\n-removed line\n+added line\n*** End Patch"
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
            result_output(edit_tool_impl_for_call(tool, Some(&tool_call_id), args))
        })
    }
}

#[cfg(test)]
pub(crate) fn edit_tool_impl(tool: CwdTool, args: Value) -> Result<Value> {
    edit_tool_impl_for_call(tool, None, args)
}

pub(crate) fn edit_tool_impl_for_call(
    tool: CwdTool,
    tool_call_id: Option<&str>,
    args: Value,
) -> Result<Value> {
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("replace");
    match mode {
        "replace" => edit_replace(tool, tool_call_id, args),
        "patch" => edit_patch(tool, tool_call_id, args),
        _ => Err(Error::Message(format!("unsupported edit mode: {mode}"))),
    }
}

pub(crate) fn edit_replace(
    tool: CwdTool,
    tool_call_id: Option<&str>,
    args: Value,
) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let old_string = required_string(&args, "old_string")?;
    let new_string = required_string(&args, "new_string")?;
    let replace_all = optional_bool(&args, "replace_all")?.unwrap_or(false);
    let target = tool.resolve_existing(path)?;
    tool.ensure_sandbox_write_allowed(&target, tool_call_id)?;
    let _locks = acquire_path_locks(std::slice::from_ref(&target));
    let (text, expected) = read_text_snapshot(&LOCAL_FILE_MUTATION, &target)?;
    let old = normalize_lf(old_string.trim_start_matches('\u{feff}'));
    let new = normalize_lf(new_string.trim_start_matches('\u{feff}'));
    let outcome = match fuzzy_find_and_replace(&text.normalized, &old, &new, replace_all) {
        Ok(outcome) => outcome,
        Err(err) => {
            return Ok(json!({
                "success": false,
                "error": err
            }));
        }
    };
    let rel = tool.relative(&target);
    let diff = git_patch_update(&rel, &text.normalized, &outcome.content);
    let restored = restore_text_file(&text, &outcome.content);
    let baseline = snapshot_lsp_baseline(&tool, &target, Some(&text.original));
    LOCAL_FILE_MUTATION
        .replace(tool.task_id(), &target, expected, restored.as_bytes())
        .map_err(Error::from)?;
    tool.observe_workspace_mutation(WorkspaceMutation::ExactUtf8 {
        path: rel.clone(),
        before: Some(text.original.clone()),
        after: Some(restored.clone()),
    });
    let (lint, lsp) =
        post_write_feedback(&tool, &target, &restored, Some(&text.original), baseline)?;
    Ok(edit_success_value(EditSuccess {
        diff,
        files_modified: vec![rel],
        files_created: Vec::new(),
        files_deleted: Vec::new(),
        lint,
        lsp_diagnostics: lsp,
    }))
}

pub(crate) fn edit_patch(tool: CwdTool, tool_call_id: Option<&str>, args: Value) -> Result<Value> {
    let patch = required_string(&args, "patch")?;
    let operations = match parse_v4a_patch(patch) {
        Ok(operations) => operations,
        Err(err) => return Ok(json!({ "success": false, "error": err.to_string() })),
    };
    let mut lock_paths = Vec::new();
    for op in &operations {
        match op.kind {
            V4aOperationKind::Add => {
                let (target, _) = tool.resolve_write_target(&op.file_path)?;
                tool.ensure_sandbox_write_allowed(&target, tool_call_id)?;
                lock_paths.push(target);
            }
            V4aOperationKind::Update | V4aOperationKind::Delete => {
                let target = tool.resolve_existing(&op.file_path)?;
                tool.ensure_sandbox_write_allowed(&target, tool_call_id)?;
                lock_paths.push(target);
            }
        }
    }
    if let Some((first, second)) = overlapping_patch_targets(&lock_paths) {
        return Ok(json!({
            "success": false,
            "error": format!(
                "Patch validation failed (no files were modified): overlapping targets {} and {}",
                first.display(),
                second.display()
            )
        }));
    }
    let _locks = acquire_path_locks(&lock_paths);
    let plan = match validate_v4a_operations(&tool, tool_call_id, &operations, &LOCAL_FILE_MUTATION)
    {
        Ok(plan) => plan,
        Err(err) => {
            return Ok(json!({
                "success": false,
                "error": format!("Patch validation failed (no files were modified):\n{err}")
            }));
        }
    };
    apply_v4a_plan_with_backend(&tool, plan, &LOCAL_FILE_MUTATION)
}

pub(crate) fn overlapping_patch_targets(paths: &[PathBuf]) -> Option<(PathBuf, PathBuf)> {
    for (index, path) in paths.iter().enumerate() {
        for other in &paths[index + 1..] {
            if path == other || path.starts_with(other) || other.starts_with(path) {
                return Some((path.clone(), other.clone()));
            }
        }
    }
    None
}

pub(crate) enum V4aApply {
    Add {
        target: PathBuf,
        rel: String,
        content: String,
    },
    Update {
        target: PathBuf,
        rel: String,
        text: TextFile,
        updated: String,
        version: FileVersion,
    },
    Delete {
        target: PathBuf,
        rel: String,
        text: TextFile,
        version: FileVersion,
    },
}

pub(crate) fn validate_v4a_operations(
    tool: &CwdTool,
    tool_call_id: Option<&str>,
    operations: &[V4aOperation],
    backend: &dyn FileMutationBackend,
) -> Result<Vec<V4aApply>> {
    let mut plan = Vec::new();
    for op in operations {
        match op.kind {
            V4aOperationKind::Add => {
                let (target, _) = tool.resolve_write_target(&op.file_path)?;
                tool.ensure_sandbox_write_allowed(&target, tool_call_id)?;
                if target.exists() {
                    return Err(Error::Message(format!(
                        "{}: add target already exists",
                        op.file_path
                    )));
                }
                plan.push(V4aApply::Add {
                    rel: tool.relative(&target),
                    target,
                    content: v4a_add_content(op),
                });
            }
            V4aOperationKind::Update => {
                let target = tool.resolve_existing(&op.file_path)?;
                tool.ensure_sandbox_write_allowed(&target, tool_call_id)?;
                let (text, version) = read_text_snapshot(backend, &target)?;
                let updated =
                    apply_v4a_update_hunks(&text.normalized, &op.hunks).map_err(Error::Message)?;
                if updated == text.normalized {
                    return Err(Error::Message(format!("{}: no-change patch", op.file_path)));
                }
                plan.push(V4aApply::Update {
                    rel: tool.relative(&target),
                    target,
                    text,
                    updated,
                    version,
                });
            }
            V4aOperationKind::Delete => {
                let target = tool.resolve_existing(&op.file_path)?;
                tool.ensure_sandbox_write_allowed(&target, tool_call_id)?;
                let expected = require_fresh_read(tool.task_id(), &target)?;
                let (text, version) = read_text_snapshot(backend, &target)?;
                if version != expected {
                    return Err(MutationConflict::Modified { path: target }.into());
                }
                plan.push(V4aApply::Delete {
                    rel: tool.relative(&target),
                    target,
                    text,
                    version,
                });
            }
        }
    }
    Ok(plan)
}

#[derive(Default)]
pub(crate) struct PatchCommitState {
    pub(crate) diffs: Vec<String>,
    pub(crate) files_modified: Vec<String>,
    pub(crate) files_created: Vec<String>,
    pub(crate) files_deleted: Vec<String>,
    pub(crate) lint_by_file: serde_json::Map<String, Value>,
    pub(crate) lsp_blocks: Vec<String>,
}

impl PatchCommitState {
    pub(crate) fn record_feedback(&mut self, path: &str, lint: Option<Value>, lsp: Option<String>) {
        if let Some(lint) = lint {
            self.lint_by_file.insert(path.to_string(), lint);
        }
        if let Some(lsp) = lsp {
            self.lsp_blocks.push(lsp);
        }
    }

    pub(crate) fn success_value(self) -> Value {
        let lint = (!self.lint_by_file.is_empty()).then_some(Value::Object(self.lint_by_file));
        let lsp = (!self.lsp_blocks.is_empty()).then(|| self.lsp_blocks.join("\n\n"));
        edit_success_value(EditSuccess {
            diff: self.diffs.concat(),
            files_modified: self.files_modified,
            files_created: self.files_created,
            files_deleted: self.files_deleted,
            lint,
            lsp_diagnostics: lsp,
        })
    }

    pub(crate) fn failure_value(
        self,
        index: usize,
        kind: &str,
        path: &str,
        error: &Error,
    ) -> Value {
        let committed =
            self.files_modified.len() + self.files_created.len() + self.files_deleted.len();
        let message = if committed == 0 {
            format!(
                "Patch failed at operation {index} ({kind} {path}); no files were modified: {error}"
            )
        } else {
            format!(
                "Patch partially applied before failing at operation {index} ({kind} {path}); {committed} earlier operation(s) remain committed: {error}"
            )
        };
        json!({
            "success": false,
            "error": message,
            "diff": self.diffs.concat(),
            "files_modified": self.files_modified,
            "files_created": self.files_created,
            "files_deleted": self.files_deleted,
            "failed_operation": {
                "index": index,
                "kind": kind,
                "path": path,
            }
        })
    }
}

pub(crate) fn apply_v4a_plan_with_backend(
    tool: &CwdTool,
    plan: Vec<V4aApply>,
    backend: &dyn FileMutationBackend,
) -> Result<Value> {
    let mut committed = PatchCommitState::default();
    for (offset, op) in plan.into_iter().enumerate() {
        let index = offset + 1;
        let (kind, path) = match &op {
            V4aApply::Add { rel, .. } => (V4aOperationKind::Add.as_str(), rel.clone()),
            V4aApply::Update { rel, .. } => (V4aOperationKind::Update.as_str(), rel.clone()),
            V4aApply::Delete { rel, .. } => (V4aOperationKind::Delete.as_str(), rel.clone()),
        };
        let result = (|| -> Result<()> {
            match op {
                V4aApply::Add {
                    target,
                    rel,
                    content,
                } => {
                    backend
                        .create(tool.task_id(), &target, content.as_bytes())
                        .map_err(Error::from)?;
                    tool.observe_workspace_mutation(WorkspaceMutation::ExactUtf8 {
                        path: rel.clone(),
                        before: None,
                        after: Some(content.clone()),
                    });
                    committed.diffs.push(git_patch_add(&rel, &content));
                    committed.files_created.push(rel.clone());
                    let (lint, lsp) = post_write_feedback(tool, &target, &content, None, None)?;
                    committed.record_feedback(&rel, lint, lsp);
                }
                V4aApply::Update {
                    target,
                    rel,
                    text,
                    updated,
                    version,
                } => {
                    let restored = restore_text_file(&text, &updated);
                    let baseline = snapshot_lsp_baseline(tool, &target, Some(&text.original));
                    backend
                        .replace(tool.task_id(), &target, version, restored.as_bytes())
                        .map_err(Error::from)?;
                    tool.observe_workspace_mutation(WorkspaceMutation::ExactUtf8 {
                        path: rel.clone(),
                        before: Some(text.original.clone()),
                        after: Some(restored.clone()),
                    });
                    committed
                        .diffs
                        .push(git_patch_update(&rel, &text.normalized, &updated));
                    committed.files_modified.push(rel.clone());
                    let (lint, lsp) = post_write_feedback(
                        tool,
                        &target,
                        &restored,
                        Some(&text.original),
                        baseline,
                    )?;
                    committed.record_feedback(&rel, lint, lsp);
                }
                V4aApply::Delete {
                    target,
                    rel,
                    text,
                    version,
                } => {
                    backend
                        .delete(tool.task_id(), &target, version)
                        .map_err(Error::from)?;
                    tool.observe_workspace_mutation(WorkspaceMutation::ExactUtf8 {
                        path: rel.clone(),
                        before: Some(text.original.clone()),
                        after: None,
                    });
                    committed
                        .diffs
                        .push(git_patch_delete(&rel, &text.normalized));
                    committed.files_deleted.push(rel);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            return Ok(committed.failure_value(index, kind, &path, &error));
        }
    }
    Ok(committed.success_value())
}

#[cfg(test)]
pub(crate) mod edit_tool_tests {
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

    fn cwd_tool_read_only_sandbox(path: &Path) -> CwdTool {
        let env = BTreeMap::new();
        let policy = crate::sandbox::SandboxPolicy::from_config(
            &crate::sandbox::SandboxConfig {
                enabled: true,
                mode: crate::sandbox::SandboxMode::ReadOnly,
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

    #[test]
    fn edit_tool_schema_is_hermes_style_without_legacy_edits() {
        let tool = EditTool::new(PathBuf::from("/tmp/work"), ToolRuntimeContext::default());
        let schema = tool.parameters();
        assert_eq!(
            schema["properties"]["mode"]["enum"],
            json!(["replace", "patch"])
        );
        assert_eq!(schema["properties"]["mode"]["default"], "replace");
        assert!(schema["properties"].get("edits").is_none());
        assert!(
            schema["properties"]["patch"]["description"]
                .as_str()
                .unwrap()
                .contains("*** Begin Patch")
        );
    }

    #[test]
    fn edit_replace_uses_fuzzy_matching() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(
            temp.path().join("main.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .expect("seed");
        let value = edit_tool_impl(
            cwd_tool(temp.path()),
            json!({
                "mode": "replace",
                "path": "main.rs",
                "old_string": "println!(\"hi\");",
                "new_string": "println!(\"bye\");"
            }),
        )
        .expect("edit");
        assert_eq!(value["success"], true);
        let diff = value["diff"].as_str().expect("diff");
        assert!(diff.starts_with("diff --git a/main.rs b/main.rs"), "{diff}");
        assert!(diff.contains("--- a/main.rs\n+++ b/main.rs"), "{diff}");
        assert!(diff.contains("-    println!(\"hi\");"), "{diff}");
        assert!(diff.contains("+    println!(\"bye\");"), "{diff}");
        assert!(
            fs::read_to_string(temp.path().join("main.rs"))
                .expect("file")
                .contains("bye")
        );
    }

    #[test]
    fn edit_replace_reports_ambiguous_match_unless_replace_all() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("dup.txt"), "a\na\n").expect("seed");
        let ambiguous = edit_tool_impl(
            cwd_tool(temp.path()),
            json!({
                "mode": "replace",
                "path": "dup.txt",
                "old_string": "a",
                "new_string": "b"
            }),
        )
        .expect("ambiguous value");
        assert_eq!(ambiguous["success"], false);
        assert!(
            ambiguous["error"]
                .as_str()
                .unwrap()
                .contains("Found 2 matches")
        );

        let replaced = edit_tool_impl(
            cwd_tool(temp.path()),
            json!({
                "mode": "replace",
                "path": "dup.txt",
                "old_string": "a",
                "new_string": "b",
                "replace_all": true
            }),
        )
        .expect("replace all");
        assert_eq!(replaced["success"], true);
        assert_eq!(
            fs::read_to_string(temp.path().join("dup.txt")).expect("file"),
            "b\nb\n"
        );
    }

    #[test]
    fn edit_replace_preserves_bom_and_crlf() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("win.txt"), "\u{feff}one\r\ntwo\r\n").expect("seed");
        edit_tool_impl(
            cwd_tool(temp.path()),
            json!({
                "mode": "replace",
                "path": "win.txt",
                "old_string": "one",
                "new_string": "uno"
            }),
        )
        .expect("edit");
        let content = fs::read_to_string(temp.path().join("win.txt")).expect("file");
        assert!(content.starts_with('\u{feff}'));
        assert!(content.contains("uno\r\ntwo\r\n"));
    }

    #[test]
    fn edit_replace_rejects_read_only_sandbox_write() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("main.rs"), "fn main() {}\n").expect("seed");
        let err = edit_tool_impl(
            cwd_tool_read_only_sandbox(temp.path()),
            json!({
                "mode": "replace",
                "path": "main.rs",
                "old_string": "main",
                "new_string": "run"
            }),
        )
        .expect_err("sandbox denial");

        assert!(err.to_string().contains("read-only"));
        assert_eq!(
            fs::read_to_string(temp.path().join("main.rs")).expect("file"),
            "fn main() {}\n"
        );
    }

    #[test]
    fn edit_patch_applies_full_v4a_operations() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("update.txt"), "alpha\nbeta\n").expect("update");
        fs::write(temp.path().join("delete.txt"), "remove me\n").expect("delete");
        let patch = r#"*** Begin Patch
*** Update File: update.txt
@@ beta @@
 alpha
-beta
+bravo
*** Add File: add.txt
+created
+file
*** Delete File: delete.txt
*** End Patch"#;
        let mutations = Arc::new(Mutex::new(Vec::new()));
        let tool = cwd_tool_with_mutations(temp.path(), mutations.clone());
        read_tool_impl(tool.clone(), json!({"path": "delete.txt"})).expect("read delete");
        let value = edit_tool_impl(tool, json!({"mode": "patch", "patch": patch})).expect("patch");
        assert_eq!(value["success"], true);
        assert_eq!(
            fs::read_to_string(temp.path().join("update.txt")).expect("update"),
            "alpha\nbravo\n"
        );
        assert_eq!(
            fs::read_to_string(temp.path().join("add.txt")).expect("add"),
            "created\nfile"
        );
        assert!(!temp.path().join("delete.txt").exists());
        assert_eq!(value["files_created"], json!(["add.txt"]));
        assert_eq!(value["files_deleted"], json!(["delete.txt"]));
        assert!(value.get("files_moved").is_none());
        assert!(value.get("warning").is_none());
        let diff = value["diff"].as_str().expect("diff");
        assert_eq!(diff.matches("diff --git ").count(), 3, "{diff}");
        assert!(
            diff.contains("diff --git a/update.txt b/update.txt"),
            "{diff}"
        );
        assert!(
            diff.contains("--- a/update.txt\n+++ b/update.txt"),
            "{diff}"
        );
        assert!(diff.contains("-beta"), "{diff}");
        assert!(diff.contains("+bravo"), "{diff}");
        assert!(diff.contains("diff --git a/add.txt b/add.txt"), "{diff}");
        assert!(diff.contains("new file mode 100644"), "{diff}");
        assert!(diff.contains("--- /dev/null\n+++ b/add.txt"), "{diff}");
        assert!(
            diff.contains("diff --git a/delete.txt b/delete.txt"),
            "{diff}"
        );
        assert!(diff.contains("deleted file mode 100644"), "{diff}");
        assert!(diff.contains("--- a/delete.txt\n+++ /dev/null"), "{diff}");
        assert_eq!(
            *mutations.lock().expect("mutations poisoned"),
            vec![
                WorkspaceMutation::ExactUtf8 {
                    path: "update.txt".to_string(),
                    before: Some("alpha\nbeta\n".to_string()),
                    after: Some("alpha\nbravo\n".to_string()),
                },
                WorkspaceMutation::ExactUtf8 {
                    path: "add.txt".to_string(),
                    before: None,
                    after: Some("created\nfile".to_string()),
                },
                WorkspaceMutation::ExactUtf8 {
                    path: "delete.txt".to_string(),
                    before: Some("remove me\n".to_string()),
                    after: None,
                },
            ]
        );
    }

    #[test]
    fn edit_patch_validation_failure_does_not_partially_apply() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("update.txt"), "alpha\nbeta\n").expect("update");
        let patch = r#"*** Begin Patch
*** Update File: update.txt
@@ missing @@
-missing
+changed
*** Add File: add.txt
+created
*** End Patch"#;
        let value = edit_tool_impl(
            cwd_tool(temp.path()),
            json!({"mode": "patch", "patch": patch}),
        )
        .expect("failed value");
        assert_eq!(value["success"], false);
        assert_eq!(
            fs::read_to_string(temp.path().join("update.txt")).expect("update"),
            "alpha\nbeta\n"
        );
        assert!(!temp.path().join("add.txt").exists());
    }

    #[test]
    fn edit_replace_returns_block_anchor_candidate_without_mutating() {
        let temp = tempfile::tempdir().expect("temp");
        let original = "fn target() {\n    let sibling = 20;\n    let preserved = 30;\n}\n";
        fs::write(temp.path().join("main.rs"), original).expect("seed");
        let value = edit_tool_impl(
            cwd_tool(temp.path()),
            json!({
                "mode": "replace",
                "path": "main.rs",
                "old_string": "fn target() {\n    let stale = 2;\n    let stale_too = 3;\n}",
                "new_string": "fn target() {\n    replacement();\n}"
            }),
        )
        .expect("candidate result");
        assert_eq!(value["success"], false);
        let error = value["error"].as_str().expect("error");
        assert!(error.contains("No changes were applied"), "{error}");
        assert!(error.contains("block_anchor"), "{error}");
        assert!(error.contains("1-4"), "{error}");
        assert_eq!(
            fs::read_to_string(temp.path().join("main.rs")).expect("unchanged"),
            original
        );
    }

    #[test]
    fn edit_replace_returns_context_candidate_without_mutating() {
        let temp = tempfile::tempdir().expect("temp");
        let original = "header current\nkeep one\nkeep two\nfooter current\n";
        fs::write(temp.path().join("note.txt"), original).expect("seed");
        let value = edit_tool_impl(
            cwd_tool(temp.path()),
            json!({
                "mode": "replace",
                "path": "note.txt",
                "old_string": "header stale\nkeep one\nkeep two\nfooter stale",
                "new_string": "replacement"
            }),
        )
        .expect("candidate result");
        let error = value["error"].as_str().expect("error");
        assert!(error.contains("context_aware"), "{error}");
        assert!(error.contains("1-4"), "{error}");
        assert_eq!(
            fs::read_to_string(temp.path().join("note.txt")).expect("unchanged"),
            original
        );
    }

    #[test]
    fn edit_replace_preserves_indentation_unicode_and_real_tabs_for_fuzzy_matches() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(
            temp.path().join("indent.txt"),
            "scope {\n        alpha();\n        beta();\n}\n",
        )
        .expect("indent seed");
        edit_tool_impl(
            cwd_tool(temp.path()),
            json!({
                "path": "indent.txt",
                "old_string": "    alpha();\n    beta();",
                "new_string": "    alpha();\n        nested();"
            }),
        )
        .expect("indent edit");
        assert_eq!(
            fs::read_to_string(temp.path().join("indent.txt")).expect("indent result"),
            "scope {\n        alpha();\n            nested();\n}\n"
        );

        fs::write(temp.path().join("unicode.txt"), "title — old\n").expect("unicode seed");
        edit_tool_impl(
            cwd_tool(temp.path()),
            json!({
                "path": "unicode.txt",
                "old_string": "title -- old",
                "new_string": "title -- new"
            }),
        )
        .expect("unicode edit");
        assert_eq!(
            fs::read_to_string(temp.path().join("unicode.txt")).expect("unicode result"),
            "title — new\n"
        );

        fs::write(temp.path().join("tabs.txt"), "\told\n").expect("tab seed");
        edit_tool_impl(
            cwd_tool(temp.path()),
            json!({
                "path": "tabs.txt",
                "old_string": "\\told",
                "new_string": "\\tnew"
            }),
        )
        .expect("tab edit");
        assert_eq!(
            fs::read_to_string(temp.path().join("tabs.txt")).expect("tab result"),
            "\tnew\n"
        );
    }

    #[test]
    fn edit_replace_does_not_restore_partially_retained_unicode_expansions() {
        let temp = tempfile::tempdir().expect("temp");
        for (path, original, old_string, new_string, expected) in [
            ("dash.txt", "—\n", "--", "-", "-\n"),
            ("ellipsis.txt", "…\n", "...", ".", ".\n"),
        ] {
            fs::write(temp.path().join(path), original).expect("unicode seed");
            let value = edit_tool_impl(
                cwd_tool(temp.path()),
                json!({
                    "path": path,
                    "old_string": old_string,
                    "new_string": new_string
                }),
            )
            .expect("unicode edit");
            assert_eq!(value["success"], true, "{path}: {value}");
            assert_eq!(
                fs::read_to_string(temp.path().join(path)).expect("unicode result"),
                expected,
                "{path}"
            );
        }
    }

    #[test]
    fn edit_patch_rejects_move_and_unread_delete_without_side_effects() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("source.txt"), "source\n").expect("source");
        let moved = edit_tool_impl(
            cwd_tool(temp.path()),
            json!({
                "mode": "patch",
                "patch": "*** Begin Patch\n*** Move File: source.txt -> target.txt\n*** End Patch"
            }),
        )
        .expect("move rejection");
        assert_eq!(moved["success"], false);
        assert!(
            moved["error"]
                .as_str()
                .unwrap()
                .contains("moves are not supported")
        );
        assert!(temp.path().join("source.txt").exists());
        assert!(!temp.path().join("target.txt").exists());

        let deleted = edit_tool_impl(
            cwd_tool(temp.path()),
            json!({
                "mode": "patch",
                "patch": "*** Begin Patch\n*** Delete File: source.txt\n*** End Patch"
            }),
        )
        .expect("delete rejection");
        assert_eq!(deleted["success"], false);
        assert!(deleted["error"].as_str().unwrap().contains("fully read"));
        assert!(temp.path().join("source.txt").exists());
    }

    #[test]
    fn edit_patch_rejects_duplicate_targets_during_validation() {
        let temp = tempfile::tempdir().expect("temp");
        let patch = r#"*** Begin Patch
*** Add File: same.txt
+one
*** Add File: same.txt
+two
*** End Patch"#;
        let value = edit_tool_impl(
            cwd_tool(temp.path()),
            json!({"mode": "patch", "patch": patch}),
        )
        .expect("duplicate rejection");
        assert_eq!(value["success"], false);
        assert!(
            value["error"]
                .as_str()
                .unwrap()
                .contains("overlapping targets")
        );
        assert!(!temp.path().join("same.txt").exists());
    }

    struct FailSecondMutation {
        calls: std::cell::Cell<usize>,
    }

    impl FailSecondMutation {
        fn before_commit(&self, path: &Path) -> MutationResult<()> {
            let next = self.calls.get() + 1;
            self.calls.set(next);
            if next == 2 {
                Err(MutationConflict::Modified {
                    path: path.to_path_buf(),
                }
                .into())
            } else {
                Ok(())
            }
        }
    }

    impl FileMutationBackend for FailSecondMutation {
        fn snapshot(&self, path: &Path) -> MutationResult<FileSnapshot> {
            LOCAL_FILE_MUTATION.snapshot(path)
        }

        fn create(&self, task_id: &str, path: &Path, content: &[u8]) -> MutationResult<()> {
            self.before_commit(path)?;
            LOCAL_FILE_MUTATION.create(task_id, path, content)
        }

        fn replace(
            &self,
            task_id: &str,
            path: &Path,
            expected: FileVersion,
            content: &[u8],
        ) -> MutationResult<()> {
            self.before_commit(path)?;
            LOCAL_FILE_MUTATION.replace(task_id, path, expected, content)
        }

        fn delete(&self, task_id: &str, path: &Path, expected: FileVersion) -> MutationResult<()> {
            self.before_commit(path)?;
            LOCAL_FILE_MUTATION.delete(task_id, path, expected)
        }
    }

    #[test]
    fn edit_patch_failure_reports_committed_prefix_and_compact_model_content() {
        let temp = tempfile::tempdir().expect("temp");
        let tool = cwd_tool(temp.path());
        fs::write(temp.path().join("second.txt"), "second\n").expect("update seed");
        let operations = parse_v4a_patch(
            "*** Begin Patch\n*** Add File: first.txt\n+first\n*** Update File: second.txt\n@@ second @@\n-second\n+updated\n*** End Patch",
        )
        .expect("parse");
        let backend = FailSecondMutation {
            calls: std::cell::Cell::new(0),
        };
        let plan = validate_v4a_operations(&tool, None, &operations, &backend).expect("validate");
        let value = apply_v4a_plan_with_backend(&tool, plan, &backend).expect("apply result");
        assert_eq!(value["success"], false);
        assert_eq!(value["files_created"], json!(["first.txt"]));
        assert_eq!(
            value["failed_operation"],
            json!({"index": 2, "kind": "update", "path": "second.txt"})
        );
        assert!(value["diff"].as_str().unwrap().contains("first.txt"));
        assert!(temp.path().join("first.txt").exists());
        assert_eq!(
            fs::read_to_string(temp.path().join("second.txt")).expect("unchanged update"),
            "second\n"
        );

        let output = result_output(Ok(value));
        assert!(output.is_error);
        let model_content = output.model_content.expect("compact model content");
        assert!(model_content.contains("operation 2"), "{model_content}");
        assert!(model_content.contains("first.txt"), "{model_content}");
        assert!(
            model_content.contains("Read the file again"),
            "{model_content}"
        );
    }

    #[test]
    fn partial_patch_failure_model_content_bounds_the_concrete_reason() {
        let long_reason = "conflict recovery ".repeat(200);
        let value = json!({
            "success": false,
            "error": long_reason,
            "diff": "",
            "files_modified": [],
            "files_created": [],
            "files_deleted": [],
            "failed_operation": {
                "index": 1,
                "kind": "add",
                "path": "target.txt"
            }
        });
        let summary = partial_patch_failure_model_content(&value).expect("failure summary");
        assert!(summary.contains("conflict recovery"), "{summary}");
        assert!(
            summary.chars().count() <= 640,
            "{}",
            summary.chars().count()
        );
    }
}
