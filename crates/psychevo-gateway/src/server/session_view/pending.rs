fn prune_pending_actions(
    state: &WebState,
    selector: &GatewayThreadSelector,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Vec<PendingActionView>> {
    let pending = state
        .inner
        .pending_actions
        .lock()
        .expect("web pending actions poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut visible = Vec::new();
    let mut stale_action_ids = Vec::new();
    for action in pending {
        match pending_action_state(state, selector, thread_id, &action)? {
            PendingInteractionState::Visible => visible.push(action),
            PendingInteractionState::Hidden => {}
            PendingInteractionState::Stale => {
                stale_action_ids.push(action.action_id);
            }
        }
    }
    if !stale_action_ids.is_empty() {
        let mut pending = state
            .inner
            .pending_actions
            .lock()
            .expect("web pending actions poisoned");
        for action_id in stale_action_ids {
            pending.remove(&action_id);
        }
    }
    visible.sort_by(|left, right| {
        left.turn_id
            .cmp(&right.turn_id)
            .then_with(|| left.action_id.cmp(&right.action_id))
    });
    Ok(visible)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingInteractionState {
    Visible,
    Hidden,
    Stale,
}

fn pending_action_state(
    state: &WebState,
    selector: &GatewayThreadSelector,
    thread_id: Option<&str>,
    action: &PendingActionView,
) -> psychevo_runtime::Result<PendingInteractionState> {
    if action.kind == GatewayActionKind::Permission
        && state
            .inner
            .gateway
            .has_pending_permission_for_selector(selector, &action.action_id)
    {
        return Ok(PendingInteractionState::Visible);
    }
    if action.kind == GatewayActionKind::Permission
        && action.owner_id.as_deref() == Some(state.inner.gateway.owner_id())
    {
        return Ok(PendingInteractionState::Stale);
    }
    pending_interaction_context_state(
        state,
        selector,
        thread_id,
        PendingInteractionRoute {
            thread_id: action.thread_id.as_deref(),
            source_key: action.source_key.as_deref(),
            activity_id: action.activity_id.as_deref(),
            owner_id: action.owner_id.as_deref(),
            lease_expires_at_ms: action.lease_expires_at_ms,
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
    let Some(activity) = state.inner.state.gateway_activity(activity_id)? else {
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
