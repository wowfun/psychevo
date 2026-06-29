fn write_checked(path: &Path, content: &str, check: bool) -> Result<()> {
    if check {
        let existing = fs::read_to_string(path).with_context(|| {
            format!(
                "generated file is missing or unreadable: {}",
                path.display()
            )
        })?;
        if existing != content {
            bail!("generated file is out of date: {}", path.display());
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn ts_decl<T>() -> Result<String>
where
    T: TS,
{
    let decl = T::decl();
    Ok(export_ts_decl(camelize_ts_decl_numbers(decl)))
}

fn export_ts_decl(decl: String) -> String {
    if decl.starts_with("type ") || decl.starts_with("interface ") {
        format!("export {decl}")
    } else {
        decl
    }
}

fn camelize_ts_decl_numbers(mut decl: String) -> String {
    for (from, to) in [
        ("thread_id", "threadId"),
        ("source_key", "sourceKey"),
        ("visible_to_model", "visibleToModel"),
        ("turn_id", "turnId"),
        ("item_id", "itemId"),
        ("queue_position", "queuePosition"),
        ("request_id", "requestId"),
        ("tool_name", "toolName"),
        ("source_path", "sourcePath"),
        ("call_id", "callId"),
        ("native_id", "nativeId"),
        ("raw_id", "rawId"),
        ("raw_identity", "rawIdentity"),
        ("visible_name", "visibleName"),
        ("artifact_ids", "artifactIds"),
        ("created_at_ms", "createdAtMs"),
        ("updated_at_ms", "updatedAtMs"),
        ("event_type", "eventType"),
        ("active_turn_id", "activeTurnId"),
        ("queued_turns", "queuedTurns"),
        ("started_at_ms", "startedAtMs"),
        ("ended_at_ms", "endedAtMs"),
        ("archived_at_ms", "archivedAtMs"),
        ("message_count", "messageCount"),
        ("tool_call_count", "toolCallCount"),
        ("reasoning_effort", "reasoningEffort"),
        ("insert_text", "insertText"),
        ("sort_text", "sortText"),
        ("visible_text", "visibleText"),
        ("backend_ref", "backendRef"),
        ("relative_path", "relativePath"),
        ("target_kind", "targetKind"),
        ("permission_mode", "permissionMode"),
        ("permission_mode_options", "permissionModeOptions"),
        ("display_path", "displayPath"),
        ("model_status", "modelStatus"),
        ("model_error", "modelError"),
        ("model_options", "modelOptions"),
        ("mode_options", "modeOptions"),
        ("variant_options", "variantOptions"),
        ("used_tokens", "usedTokens"),
        ("context_limit", "contextLimit"),
        ("is_git_repo", "isGitRepo"),
        ("unified_diff", "unifiedDiff"),
        ("selected_path", "selectedPath"),
        ("max_bytes", "maxBytes"),
        ("max_lines", "maxLines"),
        ("omitted_bytes", "omittedBytes"),
        ("omitted_lines", "omittedLines"),
    ] {
        decl = decl.replace(from, to);
    }
    decl.replace("bigint", "number")
}

fn schema<T>() -> Result<Value>
where
    T: JsonSchema,
{
    serde_json::to_value(schemars::schema_for!(T)).map_err(Into::into)
}

macro_rules! exported_type {
    ($ty:ty) => {
        ExportedType {
            name: stringify!($ty),
            ts_decl: ts_decl::<$ty>,
            schema: schema::<$ty>,
        }
    };
}
