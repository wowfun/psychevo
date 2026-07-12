fn thread_snapshot(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    let thread = thread_id
        .map(|thread_id| {
            Ok::<GatewayThread, Error>(GatewayThread {
                id: thread_id.to_string(),
                backend: gateway_backend_info_for_thread(state, thread_id)?,
                source_key: Some(scope.source.source_key()),
            })
        })
        .transpose()?;
    let selector = thread_id
        .map(GatewayThreadSelector::thread_id)
        .unwrap_or_else(|| GatewayThreadSelector::source(scope.source.source_key()));
    let pending_actions = prune_pending_actions(state, &selector, thread_id)?;
    let activity = snapshot_activity(state, &scope.source, thread_id)?;
    let entries = thread_id
        .map(|thread_id| authoritative_history_projection(state, scope, thread_id))
        .transpose()?
        .unwrap_or_default();
    let history = authoritative_history_view(state, thread_id)?;
    Ok(json!({
        "source": scope.source,
        "scope": scope.to_wire_scope(),
        "thread": thread,
        "history": history,
        "entries": entries,
        "activity": activity,
        "pendingActions": pending_actions,
    }))
}

async fn thread_snapshot_live(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    thread_snapshot(state, scope, thread_id)
}

fn snapshot_activity(
    state: &WebState,
    source: &GatewaySource,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<GatewayActivity> {
    let activity = state.activity(source, thread_id);
    let Some(thread_id) = thread_id else {
        return Ok(activity);
    };
    if activity.running || activity.active_turn_id.is_some() || activity.takeover_state.is_some() {
        return Ok(activity);
    }

    if state
        .inner
        .state
        .store()
        .gateway_runtime_binding(thread_id)?
        .is_some_and(|binding| binding.backend_kind.as_deref() == Some("acp"))
    {
        return Ok(activity);
    }

    let Some(edge) = state.inner.state.store().find_agent_edge(thread_id)? else {
        return Ok(activity);
    };
    if edge.child_session_id != thread_id || edge.status != psychevo_runtime::AgentEdgeStatus::Open
    {
        return Ok(activity);
    }

    let Some(parent_record) = state
        .inner
        .state
        .store()
        .active_gateway_activity_for_thread(&edge.parent_session_id)?
    else {
        return Ok(activity);
    };
    if parent_record.lease_expires_at_ms < gateway_now_ms() {
        return Ok(activity);
    }
    let parent_activity = state.activity(source, Some(&edge.parent_session_id));
    if parent_activity.running {
        return Ok(parent_activity);
    }
    Ok(activity)
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

fn active_turn_projection_window(
    state: &WebState,
    thread_id: &str,
    activity: &GatewayActivity,
) -> psychevo_runtime::Result<Option<(String, i64)>> {
    if !activity.running {
        return Ok(None);
    }
    let Some(active_turn_id) = activity.active_turn_id.as_deref() else {
        return Ok(None);
    };
    let Some(record) = state
        .inner
        .state
        .store()
        .active_gateway_activity_for_thread(thread_id)?
    else {
        return Ok(None);
    };
    if record.turn_id.as_deref() != Some(active_turn_id) {
        return Ok(None);
    }
    let Some(first_committed_seq) = first_committed_seq_from_activity_intent(&record) else {
        return Ok(None);
    };
    Ok(Some((active_turn_id.to_string(), first_committed_seq)))
}

fn first_committed_seq_from_activity_intent(
    record: &psychevo_runtime::GatewayActivityRecord,
) -> Option<i64> {
    record
        .intent
        .as_ref()
        .and_then(|intent| intent.get("firstCommittedSeq"))
        .and_then(Value::as_i64)
        .filter(|seq| *seq > 0)
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
    if committed_assistant_owner_exists(entries, &entry) {
        entries.retain(|candidate| candidate.id != entry.id);
        return;
    }
    if !entry_has_visible_overlay(&entry) {
        entries.retain(|candidate| candidate.id != entry.id);
        return;
    }
    if let Some(existing) = entries
        .iter_mut()
        .find(|candidate| candidate.id == entry.id)
    {
        *existing = entry;
    } else {
        entries.push(entry);
    }
}

fn committed_assistant_owner_exists(
    entries: &[TranscriptEntry],
    live_entry: &TranscriptEntry,
) -> bool {
    if live_entry.source != "runtime.stream" || live_entry.role != TranscriptEntryRole::Assistant {
        return false;
    }
    let Some(live_turn_id) = live_entry.turn_id.as_deref() else {
        return false;
    };
    let Some(live_order) = entry_live_order(live_entry) else {
        return false;
    };
    entries.iter().any(|entry| {
        entry.source == "runtime.message"
            && entry.role == TranscriptEntryRole::Assistant
            && entry.turn_id.as_deref() == Some(live_turn_id)
            && entry_live_order(entry) == Some(live_order)
    })
}

fn entry_live_order(entry: &TranscriptEntry) -> Option<i64> {
    entry
        .metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| {
            metadata
                .get("liveOrder")
                .or_else(|| metadata.get("live_order"))
        })
        .and_then(Value::as_i64)
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
    let original_status = current.status;
    let live_can_replace = live_overlay_can_replace(original_status, live.status);
    current.status = monotonic_tool_status(original_status, live.status);
    merge_optional_string(&mut current.title, &live.title, live_can_replace);
    merge_optional_string(&mut current.body, &live.body, live_can_replace);
    merge_optional_string(&mut current.preview, &live.preview, live_can_replace);
    merge_optional_string(&mut current.detail, &live.detail, live_can_replace);
    if !live.artifact_ids.is_empty() && (live_can_replace || current.artifact_ids.is_empty()) {
        current.artifact_ids = live.artifact_ids.clone();
    }
    current.metadata = if live_can_replace {
        merge_metadata_values(current.metadata.take(), live.metadata.clone())
    } else {
        merge_metadata_values(live.metadata.clone(), current.metadata.take())
    };
    if live.result.is_some() && (live_can_replace || current.result.is_none()) {
        current.result = live.result.clone();
    }
    current.updated_at_ms = current.updated_at_ms.max(live.updated_at_ms);
}

fn merge_optional_string(
    current: &mut Option<String>,
    live: &Option<String>,
    live_can_replace: bool,
) {
    let Some(live) = live.as_ref() else {
        return;
    };
    let current_missing = current
        .as_deref()
        .is_none_or(|value| value.trim().is_empty());
    if live_can_replace || current_missing {
        *current = Some(live.clone());
    }
}

fn live_overlay_can_replace(current: TranscriptBlockStatus, live: TranscriptBlockStatus) -> bool {
    !terminal_tool_status(current) && status_rank(live) >= status_rank(current)
}

fn monotonic_tool_status(
    current: TranscriptBlockStatus,
    live: TranscriptBlockStatus,
) -> TranscriptBlockStatus {
    if terminal_tool_status(current) {
        return current;
    }
    if status_rank(live) > status_rank(current) {
        return live;
    }
    current
}

fn terminal_tool_status(status: TranscriptBlockStatus) -> bool {
    matches!(
        status,
        TranscriptBlockStatus::Completed
            | TranscriptBlockStatus::Failed
            | TranscriptBlockStatus::Cancelled
            | TranscriptBlockStatus::Info
    )
}

fn status_rank(status: TranscriptBlockStatus) -> u8 {
    match status {
        TranscriptBlockStatus::Pending => 0,
        TranscriptBlockStatus::Running | TranscriptBlockStatus::NeedsInput => 1,
        TranscriptBlockStatus::Completed
        | TranscriptBlockStatus::Failed
        | TranscriptBlockStatus::Cancelled
        | TranscriptBlockStatus::Info => 2,
    }
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
