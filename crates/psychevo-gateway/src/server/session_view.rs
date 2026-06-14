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
    let selector = thread_id
        .map(GatewayThreadSelector::thread_id)
        .unwrap_or_else(|| GatewayThreadSelector::source(scope.source.source_key()));
    let pending_permissions = prune_pending_permissions(state, &selector, thread_id)?;
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

fn prune_pending_permissions(
    state: &WebState,
    selector: &GatewayThreadSelector,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Vec<PendingPermissionView>> {
    let pending = state
        .inner
        .pending_permissions
        .lock()
        .expect("web pending permissions poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut visible = Vec::new();
    let mut stale_request_ids = Vec::new();
    for permission in pending {
        if pending_permission_visible(state, selector, thread_id, &permission)? {
            visible.push(permission);
        } else {
            stale_request_ids.push(permission.request_id);
        }
    }
    if !stale_request_ids.is_empty() {
        let mut pending = state
            .inner
            .pending_permissions
            .lock()
            .expect("web pending permissions poisoned");
        for request_id in stale_request_ids {
            pending.remove(&request_id);
        }
    }
    Ok(visible)
}

fn pending_permission_visible(
    state: &WebState,
    selector: &GatewayThreadSelector,
    thread_id: Option<&str>,
    permission: &PendingPermissionView,
) -> psychevo_runtime::Result<bool> {
    if let (Some(current_thread_id), Some(permission_thread_id)) =
        (thread_id, permission.thread_id.as_deref())
        && current_thread_id != permission_thread_id
    {
        return Ok(false);
    }
    if state
        .inner
        .gateway
        .has_pending_permission_for_selector(selector, &permission.request_id)
    {
        return Ok(true);
    }
    if permission.owner_id.as_deref() == Some(state.inner.gateway.owner_id()) {
        return Ok(false);
    }
    if let Some(lease_expires_at_ms) = permission.lease_expires_at_ms
        && lease_expires_at_ms < gateway_now_ms()
    {
        return Ok(false);
    }
    let Some(activity_id) = permission.activity_id.as_deref() else {
        return Ok(false);
    };
    let Some(activity) = state.inner.state.store().gateway_activity(activity_id)? else {
        return Ok(false);
    };
    if !matches!(activity.status.as_str(), "running" | "queued") {
        return Ok(false);
    }
    if activity.lease_expires_at_ms < gateway_now_ms() {
        return Ok(false);
    }
    if let Some(owner_id) = permission.owner_id.as_deref()
        && activity.owner_id != owner_id
    {
        return Ok(false);
    }
    if let Some(current_thread_id) = thread_id
        && activity.thread_id.as_deref() != Some(current_thread_id)
        && permission.thread_id.as_deref() != Some(current_thread_id)
    {
        return Ok(false);
    }
    Ok(true)
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

fn thread_browser_value(
    state: &WebState,
    params: wire::ThreadBrowserParams,
    workdir: Option<PathBuf>,
) -> psychevo_runtime::Result<Value> {
    let limit = params.limit.unwrap_or(20).clamp(1, 50);
    let recent_days = params.recent_days.unwrap_or(7).clamp(1, 365);
    let recent_since_ms = gateway_now_ms().saturating_sub(recent_days * 86_400_000);
    let include_ids = params
        .include_session_ids
        .iter()
        .filter(|id| !id.trim().is_empty())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let store = state.inner.state.store();
    let sessions = if params.archived.unwrap_or(false) {
        match workdir.as_ref() {
            Some(workdir) => store.list_archived_sessions_for_workdir_with_sources(workdir, &[])?,
            None => store.list_archived_sessions_with_sources(&[])?,
        }
    } else {
        match workdir.as_ref() {
            Some(workdir) => store.list_sessions_for_workdir_with_sources(workdir, &[])?,
            None => store.list_sessions_with_sources(&[])?,
        }
    };
    let cursor_workdir = params.cursor.as_ref().map(|cursor| cursor.workdir.as_str());
    let cursor_offset = params.cursor.as_ref().map(|cursor| cursor.offset).unwrap_or(0);
    let mut groups: BTreeMap<String, Vec<SessionSummary>> = BTreeMap::new();
    for session in sessions
        .into_iter()
        .filter(|session| human_visible_session(state, session))
    {
        if cursor_workdir.is_some_and(|workdir| workdir != session.workdir) {
            continue;
        }
        groups
            .entry(session.workdir.clone())
            .or_default()
            .push(session);
    }

    let mut workspaces = Vec::new();
    for (workdir, mut sessions) in groups {
        sessions.sort_by(|left, right| {
            right
                .updated_at_ms
                .cmp(&left.updated_at_ms)
                .then_with(|| left.id.cmp(&right.id))
        });
        let mut exception_ids = std::collections::BTreeSet::new();
        if params.cursor.is_none() {
            for session in &sessions {
                let activity = state
                    .inner
                    .gateway
                    .activity_for_selector(GatewayThreadSelector::thread_id(&session.id));
                if include_ids.contains(&session.id)
                    || activity.running
                    || activity.takeover_state.is_some()
                {
                    exception_ids.insert(session.id.clone());
                }
            }
        }
        let normal_sessions = sessions
            .iter()
            .filter(|session| !exception_ids.contains(&session.id))
            .collect::<Vec<_>>();
        let (page_sessions, next_offset) = if params.cursor.is_some() {
            let page = normal_sessions
                .iter()
                .skip(cursor_offset)
                .take(limit)
                .copied()
                .cloned()
                .collect::<Vec<_>>();
            let next_offset = cursor_offset.saturating_add(page.len());
            (page, next_offset)
        } else {
            let page = normal_sessions
                .iter()
                .take_while(|session| session.updated_at_ms >= recent_since_ms)
                .take(limit)
                .copied()
                .cloned()
                .collect::<Vec<_>>();
            let next_offset = page.len();
            let mut page_ids = page
                .iter()
                .map(|session| session.id.clone())
                .collect::<std::collections::BTreeSet<_>>();
            let mut with_exceptions = page;
            for session in sessions.iter().filter(|session| exception_ids.contains(&session.id)) {
                if page_ids.insert(session.id.clone()) {
                    with_exceptions.push(session.clone());
                }
            }
            with_exceptions.sort_by(|left, right| {
                right
                    .updated_at_ms
                    .cmp(&left.updated_at_ms)
                    .then_with(|| left.id.cmp(&right.id))
            });
            (with_exceptions, next_offset)
        };
        let hidden_count = normal_sessions.len().saturating_sub(next_offset);
        let next_cursor = (hidden_count > 0).then(|| {
            json!({
                "workdir": workdir,
                "offset": next_offset,
            })
        });
        workspaces.push(json!({
            "workdir": workdir,
            "project": session_project_value(&workdir),
            "sessions": page_sessions
                .into_iter()
                .map(|session| session_summary_value(state, session))
                .collect::<psychevo_runtime::Result<Vec<_>>>()?,
            "hiddenCount": hidden_count,
            "nextCursor": next_cursor,
        }));
    }
    workspaces.sort_by(|left, right| {
        let left_latest = browser_workspace_latest_at(left);
        let right_latest = browser_workspace_latest_at(right);
        right_latest.cmp(&left_latest).then_with(|| {
            left.get("workdir")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .cmp(right.get("workdir").and_then(Value::as_str).unwrap_or_default())
        })
    });
    Ok(json!({ "workspaces": workspaces }))
}

fn browser_workspace_latest_at(workspace: &Value) -> i64 {
    workspace
        .get("sessions")
        .and_then(Value::as_array)
        .and_then(|sessions| {
            sessions
                .iter()
                .filter_map(|session| session.get("updatedAtMs").and_then(Value::as_i64))
                .max()
        })
        .unwrap_or_default()
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
