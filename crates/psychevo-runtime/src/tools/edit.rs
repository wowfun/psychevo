struct EditTool(WorkdirTool);

impl EditTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Apply targeted replacements or a unified diff to existing text files."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"mode":{"type":"string"},"path":{"type":"string"},"edits":{"type":"array"},"patch":{"type":"string"}}})
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
            match edit_tool_impl(tool, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn edit_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
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

#[derive(Debug, Deserialize)]
struct ReplaceEdit {
    #[serde(rename = "oldText")]
    old_text: String,
    #[serde(rename = "newText")]
    new_text: String,
}

fn edit_replace(tool: WorkdirTool, args: Value) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let edits_value = args
        .get("edits")
        .ok_or_else(|| Error::Message("edits is required".to_string()))?;
    let edits: Vec<ReplaceEdit> = serde_json::from_value(edits_value.clone())?;
    if edits.is_empty() {
        return Err(Error::Message("edits must not be empty".to_string()));
    }
    let target = tool.resolve_existing(path)?;
    let original_bytes = fs::read(&target)?;
    let original_text = String::from_utf8(original_bytes)
        .map_err(|_| Error::Message("invalid UTF-8".to_string()))?;
    let bom = original_text.starts_with('\u{feff}');
    let body = original_text.trim_start_matches('\u{feff}');
    let line_ending = dominant_line_ending(body);
    let normalized = normalize_lf(body);
    let mut ranges = Vec::new();
    for edit in &edits {
        if edit.old_text == edit.new_text {
            return Err(Error::Message("no-change edit".to_string()));
        }
        let old = normalize_lf(edit.old_text.trim_start_matches('\u{feff}'));
        let matches = normalized.match_indices(&old).collect::<Vec<_>>();
        if matches.is_empty() {
            return Err(Error::Message(format!(
                "oldText not found: {}",
                edit.old_text
            )));
        }
        if matches.len() > 1 {
            return Err(Error::Message(format!(
                "oldText is ambiguous: {}",
                edit.old_text
            )));
        }
        let start = matches[0].0;
        let end = start + old.len();
        ranges.push((start, end, normalize_lf(&edit.new_text)));
    }
    ranges.sort_by_key(|(start, _, _)| *start);
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(Error::Message("edits overlap".to_string()));
        }
    }
    let mut updated = String::new();
    let mut cursor = 0usize;
    for (start, end, replacement) in ranges {
        updated.push_str(&normalized[cursor..start]);
        updated.push_str(&replacement);
        cursor = end;
    }
    updated.push_str(&normalized[cursor..]);
    let diff = unified_diff(&tool.relative(&target), &normalized, &updated);
    let restored = restore_line_endings(&updated, line_ending);
    fs::write(
        &target,
        if bom {
            format!("\u{feff}{restored}")
        } else {
            restored
        },
    )?;
    Ok(json!({
        "success": true,
        "diff": diff,
        "files_modified": [tool.relative(&target)],
        "error": null
    }))
}

fn edit_patch(tool: WorkdirTool, args: Value) -> Result<Value> {
    let patch = required_string(&args, "patch")?;
    let files = parse_unified_patch(patch)?;
    if files.is_empty() {
        return Err(Error::Message("patch contains no file updates".to_string()));
    }
    let mut diffs = Vec::new();
    let mut modified = Vec::new();
    for file in files {
        let target = tool.resolve_existing(&file.path)?;
        let original_text = String::from_utf8(fs::read(&target)?)
            .map_err(|_| Error::Message("invalid UTF-8".to_string()))?;
        let bom = original_text.starts_with('\u{feff}');
        let body = original_text.trim_start_matches('\u{feff}');
        let line_ending = dominant_line_ending(body);
        let mut lines = normalize_lf(body)
            .split('\n')
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        for hunk in file.hunks {
            let idx = find_unique_subslice(&lines, &hunk.old_lines).ok_or_else(|| {
                Error::Message(format!("patch hunk did not match uniquely: {}", file.path))
            })?;
            lines.splice(idx..idx + hunk.old_lines.len(), hunk.new_lines);
        }
        let updated = lines.join("\n");
        let original_norm = normalize_lf(body);
        let rel = tool.relative(&target);
        diffs.push(unified_diff(&rel, &original_norm, &updated));
        let restored = restore_line_endings(&updated, line_ending);
        fs::write(
            &target,
            if bom {
                format!("\u{feff}{restored}")
            } else {
                restored
            },
        )?;
        modified.push(rel);
    }
    Ok(json!({
        "success": true,
        "diff": diffs.join("\n"),
        "files_modified": modified,
        "error": null
    }))
}

