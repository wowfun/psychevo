#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) struct EditTool(WorkdirTool);

impl EditTool {
    pub(crate) fn new(workdir: PathBuf, context: ToolRuntimeContext) -> Self {
        Self(WorkdirTool::with_context(workdir, context))
    }
}

impl ToolBinding for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Apply targeted file edits inside the working directory. Use edit instead of sed/awk for replacements. Replace mode uses fuzzy matching and returns a Git-style patch diff. Patch mode accepts V4A multi-file patches with Update/Add/Delete/Move operations."
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
                    "description": "Required when mode='replace'. File path to edit, relative to the working directory or absolute inside it."
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
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            result_output(edit_tool_impl(tool, args))
        })
    }
}

pub(crate) fn edit_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
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

pub(crate) fn edit_replace(tool: WorkdirTool, args: Value) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let old_string = required_string(&args, "old_string")?;
    let new_string = required_string(&args, "new_string")?;
    let replace_all = optional_bool(&args, "replace_all")?.unwrap_or(false);
    let target = tool.resolve_existing(path)?;
    let _locks = acquire_path_locks(std::slice::from_ref(&target));
    let warning = stale_file_warning(tool.task_id(), &target);
    let text = read_text_file(&target)?;
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
    let (lint, lsp) = write_edit_text(&tool, &target, &restored, Some(&text.original))?;
    Ok(edit_success_value(EditSuccess {
        diff,
        files_modified: vec![rel],
        files_created: Vec::new(),
        files_deleted: Vec::new(),
        files_moved: Vec::new(),
        lint,
        lsp_diagnostics: lsp,
        warning,
    }))
}

pub(crate) fn edit_patch(tool: WorkdirTool, args: Value) -> Result<Value> {
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
                lock_paths.push(target);
            }
            V4aOperationKind::Update | V4aOperationKind::Delete => {
                lock_paths.push(tool.resolve_existing(&op.file_path)?);
            }
            V4aOperationKind::Move => {
                lock_paths.push(tool.resolve_existing(&op.file_path)?);
                let new_path = op
                    .new_path
                    .as_deref()
                    .ok_or_else(|| Error::Message("move destination required".to_string()))?;
                let (target, _) = tool.resolve_write_target(new_path)?;
                lock_paths.push(target);
            }
        }
    }
    let _locks = acquire_path_locks(&lock_paths);
    let plan = match validate_v4a_operations(&tool, &operations) {
        Ok(plan) => plan,
        Err(err) => {
            return Ok(json!({
                "success": false,
                "error": format!("Patch validation failed (no files were modified):\n{err}")
            }));
        }
    };
    apply_v4a_plan(&tool, plan)
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
    },
    Delete {
        target: PathBuf,
        rel: String,
        text: TextFile,
    },
    Move {
        source: PathBuf,
        dest: PathBuf,
        source_rel: String,
        dest_rel: String,
    },
}

pub(crate) fn validate_v4a_operations(
    tool: &WorkdirTool,
    operations: &[V4aOperation],
) -> Result<Vec<V4aApply>> {
    let mut plan = Vec::new();
    for op in operations {
        match op.kind {
            V4aOperationKind::Add => {
                let (target, _) = tool.resolve_write_target(&op.file_path)?;
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
                let text = read_text_file(&target)?;
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
                });
            }
            V4aOperationKind::Delete => {
                let target = tool.resolve_existing(&op.file_path)?;
                let text = read_text_file(&target)?;
                plan.push(V4aApply::Delete {
                    rel: tool.relative(&target),
                    target,
                    text,
                });
            }
            V4aOperationKind::Move => {
                let source = tool.resolve_existing(&op.file_path)?;
                let dest = op
                    .new_path
                    .as_deref()
                    .ok_or_else(|| Error::Message("move destination required".to_string()))?;
                let (dest, _) = tool.resolve_write_target(dest)?;
                if dest.exists() {
                    return Err(Error::Message(format!(
                        "{}: move destination already exists",
                        dest.display()
                    )));
                }
                plan.push(V4aApply::Move {
                    source_rel: tool.relative(&source),
                    dest_rel: tool.relative(&dest),
                    source,
                    dest,
                });
            }
        }
    }
    Ok(plan)
}

pub(crate) fn apply_v4a_plan(tool: &WorkdirTool, plan: Vec<V4aApply>) -> Result<Value> {
    let mut diffs = Vec::new();
    let mut files_modified = Vec::new();
    let mut files_created = Vec::new();
    let mut files_deleted = Vec::new();
    let mut files_moved = Vec::new();
    let mut lint_by_file = serde_json::Map::new();
    let mut lsp_blocks = Vec::new();
    let warnings = plan
        .iter()
        .filter_map(|op| match op {
            V4aApply::Add { target, .. } => stale_file_warning(tool.task_id(), target),
            V4aApply::Update { target, .. } | V4aApply::Delete { target, .. } => {
                stale_file_warning(tool.task_id(), target)
            }
            V4aApply::Move { source, dest, .. } => stale_file_warning(tool.task_id(), source)
                .or_else(|| stale_file_warning(tool.task_id(), dest)),
        })
        .collect::<Vec<_>>();
    for op in plan {
        match op {
            V4aApply::Add {
                target,
                rel,
                content,
            } => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let (lint, lsp) = write_edit_text(tool, &target, &content, None)?;
                diffs.push(git_patch_add(&rel, &content));
                if let Some(lint) = lint {
                    lint_by_file.insert(rel.clone(), lint);
                }
                if let Some(lsp) = lsp {
                    lsp_blocks.push(lsp);
                }
                files_created.push(rel);
            }
            V4aApply::Update {
                target,
                rel,
                text,
                updated,
            } => {
                let restored = restore_text_file(&text, &updated);
                let (lint, lsp) = write_edit_text(tool, &target, &restored, Some(&text.original))?;
                diffs.push(git_patch_update(&rel, &text.normalized, &updated));
                if let Some(lint) = lint {
                    lint_by_file.insert(rel.clone(), lint);
                }
                if let Some(lsp) = lsp {
                    lsp_blocks.push(lsp);
                }
                files_modified.push(rel);
            }
            V4aApply::Delete { target, rel, text } => {
                fs::remove_file(&target)?;
                diffs.push(git_patch_delete(&rel, &text.normalized));
                files_deleted.push(rel);
            }
            V4aApply::Move {
                source,
                dest,
                source_rel,
                dest_rel,
            } => {
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(&source, &dest)?;
                note_file_write(tool.task_id(), &dest);
                diffs.push(git_patch_move(&source_rel, &dest_rel));
                files_moved.push(json!({ "from": source_rel, "to": dest_rel }));
            }
        }
    }
    let lint = (!lint_by_file.is_empty()).then_some(Value::Object(lint_by_file));
    let lsp = (!lsp_blocks.is_empty()).then(|| lsp_blocks.join("\n\n"));
    Ok(edit_success_value(EditSuccess {
        diff: diffs.concat(),
        files_modified,
        files_created,
        files_deleted,
        files_moved,
        lint,
        lsp_diagnostics: lsp,
        warning: if warnings.is_empty() {
            None
        } else {
            Some(warnings.join(" | "))
        },
    }))
}

#[cfg(test)]
pub(crate) mod edit_tool_tests {
    pub(crate) use super::*;

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
            workdir_tool(temp.path()),
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
            workdir_tool(temp.path()),
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
            workdir_tool(temp.path()),
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
            workdir_tool(temp.path()),
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
    fn edit_patch_applies_full_v4a_operations() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("update.txt"), "alpha\nbeta\n").expect("update");
        fs::write(temp.path().join("delete.txt"), "remove me\n").expect("delete");
        fs::write(temp.path().join("move.txt"), "move me\n").expect("move");
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
*** Move File: move.txt -> moved.txt
*** End Patch"#;
        let value = edit_tool_impl(
            workdir_tool(temp.path()),
            json!({"mode": "patch", "patch": patch}),
        )
        .expect("patch");
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
        assert!(!temp.path().join("move.txt").exists());
        assert!(temp.path().join("moved.txt").exists());
        assert_eq!(value["files_created"], json!(["add.txt"]));
        assert_eq!(value["files_deleted"], json!(["delete.txt"]));
        assert_eq!(
            value["files_moved"][0],
            json!({"from": "move.txt", "to": "moved.txt"})
        );
        let diff = value["diff"].as_str().expect("diff");
        assert_eq!(diff.matches("diff --git ").count(), 4, "{diff}");
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
        assert!(diff.contains("diff --git a/move.txt b/moved.txt"), "{diff}");
        assert!(diff.contains("similarity index 100%"), "{diff}");
        assert!(
            diff.contains("rename from move.txt\nrename to moved.txt"),
            "{diff}"
        );
        assert!(!diff.contains("# Moved"), "{diff}");
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
            workdir_tool(temp.path()),
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
}
