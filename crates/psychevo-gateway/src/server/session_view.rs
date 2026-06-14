fn thread_snapshot(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    let thread = thread_id.map(|thread_id| GatewayThread {
        id: thread_id.to_string(),
        backend: crate::GatewayBackendInfo {
            kind: crate::BackendKind::Psychevo,
            native_id: Some(thread_id.to_string()),
        },
        source_key: Some(scope.source.source_key()),
    });
    let entries = match thread_id {
        Some(thread_id) => state.inner.gateway.thread_transcript(thread_id)?,
        None => Vec::new(),
    };
    let pending_permissions = state
        .inner
        .pending_permissions
        .lock()
        .expect("web pending permissions poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let pending_clarifies = state
        .inner
        .pending_clarifies
        .lock()
        .expect("web pending clarifies poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let activity = state.activity(&scope.source, thread_id);
    Ok(json!({
        "source": scope.source,
        "scope": scope.to_wire_scope(),
        "thread": thread,
        "entries": entries,
        "activity": activity,
        "pendingPermissions": pending_permissions,
        "pendingClarifies": pending_clarifies,
    }))
}

fn guard_session_mutation(
    state: &WebState,
    auth: &AuthContext,
    session_id: &str,
    allow_current_idle: bool,
) -> psychevo_runtime::Result<()> {
    let scope = default_resolved_scope(state, auth)?;
    let activity = state.activity(&scope.source, Some(session_id));
    if activity.running {
        return Err(Error::Message(
            "running session cannot be archived, restored, or deleted".to_string(),
        ));
    }
    if !allow_current_idle
        && let Some(bound) = state.inner.gateway.resolve_source_thread(&scope.source)?
        && bound == session_id
    {
        return Err(Error::Message(
            "current bound session cannot be deleted; reset the source first".to_string(),
        ));
    }
    Ok(())
}

fn session_summary_by_id(state: &WebState, session_id: &str) -> psychevo_runtime::Result<Value> {
    state
        .inner
        .state
        .store()
        .session_summary(session_id)?
        .map(|summary| session_summary_value(state, summary))
        .transpose()?
        .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))
}

fn human_visible_session(_state: &WebState, summary: &SessionSummary) -> bool {
    if summary.parent_session_id.is_some() {
        return false;
    }
    if INTERNAL_SESSION_SOURCES.contains(&summary.source.as_str()) {
        return false;
    }
    true
}

fn session_summary_value(
    state: &WebState,
    summary: SessionSummary,
) -> psychevo_runtime::Result<Value> {
    let activity = state
        .inner
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(&summary.id));
    let entries = state.inner.gateway.thread_transcript(&summary.id)?;
    let preview = session_preview(&entries);
    let display_title = summary
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| preview.clone())
        .unwrap_or_else(|| short_thread_id(&summary.id));
    let project = session_project_value(&summary.workdir);
    Ok(json!({
        "id": summary.id,
        "workdir": summary.workdir,
        "project": project,
        "model": summary.model,
        "provider": summary.provider,
        "startedAtMs": summary.started_at_ms,
        "updatedAtMs": summary.updated_at_ms,
        "endedAtMs": summary.ended_at_ms,
        "endReason": summary.end_reason,
        "archivedAtMs": summary.archived_at_ms,
        "messageCount": summary.message_count,
        "toolCallCount": summary.tool_call_count,
        "visibleEntryCount": entries.len(),
        "activity": activity,
        "title": summary.title,
        "displayTitle": display_title,
        "preview": preview,
    }))
}

fn session_project_value(workdir: &str) -> Value {
    let path = PathBuf::from(workdir);
    json!({
        "workdir": workdir,
        "label": project_label(&path),
        "displayPath": display_workdir(&path),
    })
}

fn project_label(workdir: &Path) -> String {
    workdir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("workdir")
        .to_string()
}

fn session_preview(entries: &[TranscriptEntry]) -> Option<String> {
    entries
        .iter()
        .find(|entry| entry.role == TranscriptEntryRole::User)
        .and_then(entry_preview)
        .or_else(|| entries.iter().find_map(entry_preview))
}

fn entry_preview(entry: &TranscriptEntry) -> Option<String> {
    entry
        .blocks
        .iter()
        .filter_map(|block| block.preview.as_deref().or(block.body.as_deref()))
        .map(compact_display_text)
        .find(|text| !text.is_empty())
}

fn compact_display_text(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 120;
    if collapsed.chars().count() <= MAX_CHARS {
        return collapsed;
    }
    let mut out = collapsed.chars().take(MAX_CHARS - 1).collect::<String>();
    out.push('…');
    out
}

fn short_thread_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn gateway_turn_result_value(result: GatewayTurnResult) -> Value {
    json!({
        "thread": result.thread,
        "turn": result.turn,
        "result": {
            "sessionId": result.result.session_id,
            "outcome": result.result.outcome.as_str(),
            "finalAnswer": result.result.final_answer,
            "toolFailures": result.result.tool_failures,
            "provider": result.result.provider,
            "model": result.result.model,
        },
        "committedEntries": result.committed_entries,
    })
}

fn gateway_shell_result_value(result: GatewayShellResult) -> Value {
    json!({
        "thread": result.thread,
        "command": result.result.command,
        "outcome": result.result.outcome.as_str(),
        "toolFailures": result.result.tool_failures,
        "committedEntries": result.committed_entries,
    })
}
