fn thread_browser_value(
    state: &WebState,
    params: wire::ThreadBrowserParams,
    cwd: Option<PathBuf>,
) -> psychevo_runtime::Result<Value> {
    let limit = params.limit.unwrap_or(20).clamp(1, 50);
    let recent_days = params.recent_days.unwrap_or(7).clamp(1, 365);
    let recent_since_ms = gateway_now_ms().saturating_sub(recent_days * 86_400_000);
    let include_ids = params
        .include_session_ids
        .iter()
        .filter(|id| !id.trim().is_empty())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let activity_snapshot = state.inner.gateway.session_activity_snapshot()?;
    let active_ids = activity_snapshot
        .iter()
        .filter(|(_, activity)| activity.running || activity.takeover_state.is_some())
        .map(|(thread_id, _)| thread_id.clone())
        .collect::<Vec<_>>();
    let cwd = cwd.map(|cwd| cwd.to_string_lossy().into_owned());
    let cursor_cwd = params.cursor.as_ref().map(|cursor| cursor.cwd.as_str());
    let cursor_offset = params
        .cursor
        .as_ref()
        .map(|cursor| cursor.offset)
        .unwrap_or(0);
    let projections = state.inner.state.store().browse_human_sessions(
        psychevo_runtime::SessionBrowserRequest {
            cwd: cwd.as_deref(),
            archived: params.archived.unwrap_or(false),
            cursor_cwd,
            cursor_offset,
            limit,
            recent_since_ms,
            include_session_ids: &include_ids,
            active_session_ids: &active_ids,
        },
    )?;

    let mut workspaces = projections
        .into_iter()
        .map(|workspace| {
            let cwd = workspace.cwd;
            let sessions = workspace
                .sessions
                .into_iter()
                .map(|projection| {
                    let activity = activity_snapshot
                        .get(&projection.summary.id)
                        .cloned()
                        .unwrap_or_default();
                    session_summary_value(projection, activity)
                })
                .collect::<Vec<_>>();
            let next_cursor = workspace.next_offset.map(|offset| {
                json!({
                    "cwd": cwd,
                    "offset": offset,
                })
            });
            json!({
                "cwd": cwd,
                "project": session_project_value(&cwd),
                "sessions": sessions,
                "hiddenCount": workspace.hidden_count,
                "nextCursor": next_cursor,
            })
        })
        .collect::<Vec<_>>();
    workspaces.sort_by(|left, right| {
        let left_latest = browser_workspace_latest_at(left);
        let right_latest = browser_workspace_latest_at(right);
        right_latest.cmp(&left_latest).then_with(|| {
            left.get("cwd")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .cmp(
                    right
                        .get("cwd")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
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
