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
    let selector = thread_id
        .map(GatewayThreadSelector::thread_id)
        .unwrap_or_else(|| GatewayThreadSelector::source(scope.source.source_key()));
    let pending_permissions = prune_pending_permissions(state, &selector, thread_id)?;
    let pending_clarifies = prune_pending_clarifies(state, &selector, thread_id)?;
    let activity = state.activity(&scope.source, thread_id);
    let entries = match thread_id {
        Some(thread_id) => {
            let mut entries = state.inner.gateway.thread_transcript(thread_id)?;
            replay_running_live_transcript_overlay(state, thread_id, &activity, &mut entries)?;
            entries
        }
        None => Vec::new(),
    };
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

fn replay_running_live_transcript_overlay(
    state: &WebState,
    thread_id: &str,
    activity: &GatewayActivity,
    entries: &mut Vec<TranscriptEntry>,
) -> psychevo_runtime::Result<()> {
    if !activity.running {
        return Ok(());
    }
    let Some(active_turn_id) = activity.active_turn_id.as_deref() else {
        return Ok(());
    };

    let snapshots = state
        .inner
        .state
        .store()
        .list_gateway_live_snapshots_for_thread(thread_id, Some(active_turn_id), 1000)?;
    for snapshot in snapshots {
        let Ok(event) = serde_json::from_value::<GatewayEvent>(snapshot.event) else {
            continue;
        };
        apply_live_transcript_overlay(entries, thread_id, active_turn_id, event);
    }
    Ok(())
}

fn apply_live_transcript_overlay(
    entries: &mut Vec<TranscriptEntry>,
    thread_id: &str,
    active_turn_id: &str,
    event: GatewayEvent,
) {
    let (turn_id, mut entry) = match event {
        GatewayEvent::EntryStarted { turn_id, entry }
        | GatewayEvent::EntryUpdated { turn_id, entry }
        | GatewayEvent::EntryCompleted { turn_id, entry } => (turn_id, entry),
        _ => return,
    };
    if turn_id != active_turn_id || entry.turn_id.as_deref() != Some(active_turn_id) {
        return;
    }
    if !entry.thread_id.is_empty() && entry.thread_id != thread_id {
        return;
    }
    if entry.thread_id.is_empty() {
        entry.thread_id = thread_id.to_string();
    }
    if transcript_entry_hidden(&entry) {
        entries.retain(|candidate| candidate.id != entry.id);
        return;
    }

    let live_entry = entry.clone();
    let mut remaining_blocks = Vec::new();
    for block in std::mem::take(&mut entry.blocks) {
        if !anchor_live_tool_block(entries, &live_entry, &block) {
            remaining_blocks.push(block);
        }
    }
    entry.blocks = remaining_blocks;
    if !entry_has_visible_overlay(&entry) {
        entries.retain(|candidate| candidate.id != entry.id);
        return;
    }
    if let Some(existing) = entries.iter_mut().find(|candidate| candidate.id == entry.id) {
        *existing = entry;
    } else {
        entries.push(entry);
    }
}

fn anchor_live_tool_block(
    entries: &mut [TranscriptEntry],
    live_entry: &TranscriptEntry,
    live_block: &TranscriptBlock,
) -> bool {
    let live_signatures = tool_block_signatures(live_block);
    if live_signatures.is_empty() {
        return false;
    }
    for entry in entries {
        if entry.source != "runtime.message" || entry.message_seq.is_none() {
            continue;
        }
        for block in &mut entry.blocks {
            let block_signatures = tool_block_signatures(block);
            if block_signatures
                .iter()
                .any(|signature| live_signatures.contains(signature))
            {
                merge_live_tool_block(block, live_block);
                entry.status = entry_status_for_blocks(&entry.blocks, entry.status);
                entry.updated_at_ms = entry.updated_at_ms.max(live_entry.updated_at_ms);
                return true;
            }
        }
    }
    false
}

fn merge_live_tool_block(current: &mut TranscriptBlock, live: &TranscriptBlock) {
    current.status = live.status;
    if live.title.is_some() {
        current.title = live.title.clone();
    }
    if live.body.is_some() {
        current.body = live.body.clone();
    }
    if live.preview.is_some() {
        current.preview = live.preview.clone();
    }
    if live.detail.is_some() {
        current.detail = live.detail.clone();
    }
    if !live.artifact_ids.is_empty() {
        current.artifact_ids = live.artifact_ids.clone();
    }
    current.metadata = merge_metadata_values(current.metadata.take(), live.metadata.clone());
    if live.result.is_some() {
        current.result = live.result.clone();
    }
    current.updated_at_ms = current.updated_at_ms.max(live.updated_at_ms);
}

fn merge_metadata_values(left: Option<Value>, right: Option<Value>) -> Option<Value> {
    match (left, right) {
        (Some(Value::Object(mut left)), Some(Value::Object(right))) => {
            for (key, value) in right {
                left.insert(key, value);
            }
            Some(Value::Object(left))
        }
        (_, Some(right)) => Some(right),
        (Some(left), None) => Some(left),
        (None, None) => None,
    }
}

fn entry_status_for_blocks(
    blocks: &[TranscriptBlock],
    fallback: TranscriptBlockStatus,
) -> TranscriptBlockStatus {
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::Failed)
    {
        return TranscriptBlockStatus::Failed;
    }
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::Cancelled)
    {
        return TranscriptBlockStatus::Cancelled;
    }
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::NeedsInput)
    {
        return TranscriptBlockStatus::NeedsInput;
    }
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::Running)
    {
        return TranscriptBlockStatus::Running;
    }
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::Pending)
    {
        return TranscriptBlockStatus::Pending;
    }
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::Completed)
    {
        return TranscriptBlockStatus::Completed;
    }
    fallback
}

fn entry_has_visible_overlay(entry: &TranscriptEntry) -> bool {
    entry
        .blocks
        .iter()
        .any(|block| !block_hidden(block) && block_has_visible_overlay(block))
}

fn block_has_visible_overlay(block: &TranscriptBlock) -> bool {
    !tool_block_signatures(block).is_empty()
        || block
            .body
            .as_deref()
            .or(block.detail.as_deref())
            .or(block.preview.as_deref())
            .is_some_and(|text| !text.trim().is_empty())
}

fn transcript_entry_hidden(entry: &TranscriptEntry) -> bool {
    entry
        .metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("hidden"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn block_hidden(block: &TranscriptBlock) -> bool {
    block
        .metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("hidden"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn tool_block_signatures(block: &TranscriptBlock) -> Vec<String> {
    if matches!(
        block.kind,
        TranscriptBlockKind::Text | TranscriptBlockKind::Reasoning
    ) {
        return Vec::new();
    }
    let Some(tool_name) = tool_block_name(block) else {
        return Vec::new();
    };
    let metadata = block.metadata.as_ref().and_then(Value::as_object);
    let mut signatures = Vec::new();
    if let Some(tool_call_id) = metadata
        .and_then(|metadata| metadata.get("tool_call_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tool_call_id| !tool_call_id.is_empty())
    {
        signatures.push(format!("{tool_name}:id:{tool_call_id}"));
    }
    if tool_name != "spawn_agent" {
        let args = metadata.and_then(|metadata| {
            metadata
                .get("args")
                .filter(|value| !value.is_null())
                .or_else(|| metadata.get("arguments").filter(|value| !value.is_null()))
        });
        if let Some(args) = args
            && let Ok(args_json) = serde_json::to_string(args)
        {
            signatures.push(format!("{tool_name}:args:{args_json}"));
        }
    }
    signatures
}

fn tool_block_name(block: &TranscriptBlock) -> Option<String> {
    block
        .metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("tool_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tool_name| !tool_name.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            block
                .title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(ToString::to_string)
        })
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
        match pending_permission_state(state, selector, thread_id, &permission)? {
            PendingInteractionState::Visible => visible.push(permission),
            PendingInteractionState::Hidden => {}
            PendingInteractionState::Stale => {
                stale_request_ids.push(permission.request_id);
            }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingInteractionState {
    Visible,
    Hidden,
    Stale,
}

fn prune_pending_clarifies(
    state: &WebState,
    selector: &GatewayThreadSelector,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Vec<PendingClarifyView>> {
    let pending = state
        .inner
        .pending_clarifies
        .lock()
        .expect("web pending clarifies poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut visible = Vec::new();
    let mut stale_request_ids = Vec::new();
    for clarify in pending {
        match pending_interaction_context_state(
            state,
            selector,
            thread_id,
            PendingInteractionRoute {
                thread_id: clarify.thread_id.as_deref(),
                source_key: clarify.source_key.as_deref(),
                activity_id: clarify.activity_id.as_deref(),
                owner_id: clarify.owner_id.as_deref(),
                lease_expires_at_ms: clarify.lease_expires_at_ms,
            },
        )? {
            PendingInteractionState::Visible => visible.push(clarify),
            PendingInteractionState::Hidden => {}
            PendingInteractionState::Stale => stale_request_ids.push(clarify.request_id),
        }
    }
    if !stale_request_ids.is_empty() {
        let mut pending = state
            .inner
            .pending_clarifies
            .lock()
            .expect("web pending clarifies poisoned");
        for request_id in stale_request_ids {
            pending.remove(&request_id);
        }
    }
    Ok(visible)
}

fn pending_permission_state(
    state: &WebState,
    selector: &GatewayThreadSelector,
    thread_id: Option<&str>,
    permission: &PendingPermissionView,
) -> psychevo_runtime::Result<PendingInteractionState> {
    if let (Some(current_thread_id), Some(permission_thread_id)) =
        (thread_id, permission.thread_id.as_deref())
        && current_thread_id != permission_thread_id
    {
        return Ok(PendingInteractionState::Hidden);
    }
    if source_selector_mismatch(selector, permission.source_key.as_deref()) {
        return Ok(PendingInteractionState::Hidden);
    }
    if state
        .inner
        .gateway
        .has_pending_permission_for_selector(selector, &permission.request_id)
    {
        return Ok(PendingInteractionState::Visible);
    }
    if permission.owner_id.as_deref() == Some(state.inner.gateway.owner_id()) {
        return Ok(PendingInteractionState::Stale);
    }
    pending_interaction_context_state(
        state,
        selector,
        thread_id,
        PendingInteractionRoute {
            thread_id: permission.thread_id.as_deref(),
            source_key: permission.source_key.as_deref(),
            activity_id: permission.activity_id.as_deref(),
            owner_id: permission.owner_id.as_deref(),
            lease_expires_at_ms: permission.lease_expires_at_ms,
        },
    )
}

#[derive(Debug, Clone, Copy)]
struct PendingInteractionRoute<'a> {
    thread_id: Option<&'a str>,
    source_key: Option<&'a str>,
    activity_id: Option<&'a str>,
    owner_id: Option<&'a str>,
    lease_expires_at_ms: Option<i64>,
}

fn pending_interaction_context_state(
    state: &WebState,
    selector: &GatewayThreadSelector,
    thread_id: Option<&str>,
    request: PendingInteractionRoute<'_>,
) -> psychevo_runtime::Result<PendingInteractionState> {
    if let (Some(current_thread_id), Some(request_thread_id)) = (thread_id, request.thread_id)
        && current_thread_id != request_thread_id
    {
        return Ok(PendingInteractionState::Hidden);
    }
    if source_selector_mismatch(selector, request.source_key) {
        return Ok(PendingInteractionState::Hidden);
    }
    if let Some(lease_expires_at_ms) = request.lease_expires_at_ms
        && lease_expires_at_ms < gateway_now_ms()
    {
        return Ok(PendingInteractionState::Stale);
    }
    let Some(activity_id) = request.activity_id else {
        return Ok(PendingInteractionState::Visible);
    };
    let Some(activity) = state.inner.state.store().gateway_activity(activity_id)? else {
        return Ok(PendingInteractionState::Stale);
    };
    if !matches!(activity.status.as_str(), "running" | "queued") {
        return Ok(PendingInteractionState::Stale);
    }
    if activity.lease_expires_at_ms < gateway_now_ms() {
        return Ok(PendingInteractionState::Stale);
    }
    if let Some(owner_id) = request.owner_id
        && activity.owner_id != owner_id
    {
        return Ok(PendingInteractionState::Stale);
    }
    if let Some(current_thread_id) = thread_id
        && activity.thread_id.as_deref() != Some(current_thread_id)
        && request.thread_id != Some(current_thread_id)
    {
        return Ok(PendingInteractionState::Hidden);
    }
    if let GatewayThreadSelector::Source { source_key } = selector
        && activity
            .source_key
            .as_deref()
            .or(request.source_key)
            .is_some_and(|activity_source| activity_source != source_key.0)
    {
        return Ok(PendingInteractionState::Hidden);
    }
    Ok(PendingInteractionState::Visible)
}

fn source_selector_mismatch(
    selector: &GatewayThreadSelector,
    request_source_key: Option<&str>,
) -> bool {
    matches!(
        (selector, request_source_key),
        (
            GatewayThreadSelector::Source { source_key },
            Some(request_source_key)
        ) if request_source_key != source_key.0
    )
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
                .cmp(
                    right
                        .get("workdir")
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
