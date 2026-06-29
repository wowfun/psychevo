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
