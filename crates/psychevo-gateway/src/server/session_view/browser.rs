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
        .collect::<std::collections::BTreeSet<_>>();
    let store = state.inner.state.store();
    let sessions = if params.archived.unwrap_or(false) {
        match cwd.as_ref() {
            Some(cwd) => store.list_archived_sessions_for_cwd_with_sources(cwd, &[])?,
            None => store.list_archived_sessions_with_sources(&[])?,
        }
    } else {
        match cwd.as_ref() {
            Some(cwd) => store.list_sessions_for_cwd_with_sources(cwd, &[])?,
            None => store.list_sessions_with_sources(&[])?,
        }
    };
    let cursor_cwd = params.cursor.as_ref().map(|cursor| cursor.cwd.as_str());
    let cursor_offset = params
        .cursor
        .as_ref()
        .map(|cursor| cursor.offset)
        .unwrap_or(0);
    let mut groups: BTreeMap<String, Vec<SessionSummary>> = BTreeMap::new();
    for session in sessions
        .into_iter()
        .filter(|session| human_visible_session(state, session))
    {
        if cursor_cwd.is_some_and(|cwd| cwd != session.cwd) {
            continue;
        }
        groups
            .entry(session.cwd.clone())
            .or_default()
            .push(session);
    }

    let mut workspaces = Vec::new();
    for (cwd, mut sessions) in groups {
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
            for session in sessions
                .iter()
                .filter(|session| exception_ids.contains(&session.id))
            {
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
                "cwd": cwd,
                "offset": next_offset,
            })
        });
        workspaces.push(json!({
            "cwd": cwd,
            "project": session_project_value(&cwd),
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

fn human_visible_session(_state: &WebState, summary: &SessionSummary) -> bool {
    if summary.parent_session_id.is_some() {
        return false;
    }
    if INTERNAL_SESSION_SOURCES.contains(&summary.source.as_str()) {
        return false;
    }
    true
}
