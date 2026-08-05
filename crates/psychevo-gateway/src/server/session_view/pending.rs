use serde_json::Value;

use psychevo::application::GatewayActivityState;

use crate::gateway_now_ms;
use psychevo_gateway_protocol::events_transcript::{GatewayActionKind, PendingActionView};
use psychevo_gateway_protocol::source::GatewayThreadSelector;

use super::super::binding::WebState;

pub(in super::super) async fn prune_pending_actions(
    state: &WebState,
    selector: &GatewayThreadSelector,
    thread_id: Option<&str>,
) -> psychevo::Result<Vec<PendingActionView>> {
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
        match pending_action_state(state, selector, thread_id, &action).await? {
            PendingInteractionState::Visible => visible.push(action),
            PendingInteractionState::Hidden => {}
            PendingInteractionState::Stale => {
                stale_action_ids.push(action.action_id);
            }
        }
    }
    if let Some(thread_id) = thread_id
        && let Ok(thread) = state.inner.framework.resume_thread(thread_id).await
    {
        for interaction in thread.pending_interactions().await? {
            if visible
                .iter()
                .any(|action| action.action_id == interaction.interaction_id)
            {
                continue;
            }
            let kind = match interaction.kind.as_str() {
                "permission" => GatewayActionKind::Permission,
                "clarify" | "user_input" => GatewayActionKind::Clarify,
                _ => continue,
            };
            let title = interaction
                .payload
                .get("toolName")
                .or_else(|| interaction.payload.get("title"))
                .or_else(|| interaction.payload.get("message"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let summary = interaction
                .payload
                .get("summary")
                .or_else(|| interaction.payload.get("reason"))
                .or_else(|| interaction.payload.get("message"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            visible.push(PendingActionView {
                action_id: interaction.interaction_id,
                kind,
                title,
                summary,
                payload: interaction.payload,
                thread_id: Some(interaction.thread_id),
                turn_id: Some(interaction.turn_id),
                activity_id: None,
                source_key: None,
                owner_id: None,
                lease_expires_at_ms: None,
            });
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

async fn pending_action_state(
    state: &WebState,
    selector: &GatewayThreadSelector,
    thread_id: Option<&str>,
    action: &PendingActionView,
) -> psychevo::Result<PendingInteractionState> {
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
    .await
}

#[derive(Debug, Clone, Copy)]
struct PendingInteractionRoute<'a> {
    thread_id: Option<&'a str>,
    source_key: Option<&'a str>,
    activity_id: Option<&'a str>,
    owner_id: Option<&'a str>,
    lease_expires_at_ms: Option<i64>,
}

async fn pending_interaction_context_state(
    state: &WebState,
    selector: &GatewayThreadSelector,
    thread_id: Option<&str>,
    request: PendingInteractionRoute<'_>,
) -> psychevo::Result<PendingInteractionState> {
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
    let Some(activity) = state.inner.durability.gateway_activity(activity_id).await? else {
        return Ok(PendingInteractionState::Stale);
    };
    if !matches!(
        activity.status,
        GatewayActivityState::Running | GatewayActivityState::Queued
    ) {
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
