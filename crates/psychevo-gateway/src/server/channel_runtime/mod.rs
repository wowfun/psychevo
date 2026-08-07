use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::{Arc, Mutex};
#[cfg(test)]
use std::time::Duration;

use psychevo::application::{GatewaySourceLaneInput, StartThreadRequest, ThreadAgentBinding};
use psychevo::config::ChannelRuntimeConnection;
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};
#[cfg(test)]
use tokio_util::sync::CancellationToken;

#[cfg(test)]
use crate::gateway_now_ms;
#[cfg(test)]
use crate::im::{
    ChannelAdapterBinding, ChannelAllowlist, ChannelGateway, ImIdentity, ImInboundMessage,
    ImOutboundMessage, gateway_source_for_im,
};
#[cfg(test)]
use psychevo_gateway_protocol::events_transcript::{
    GatewayActionKind, GatewayEvent, PendingActionView,
};
use psychevo_gateway_protocol::source::GatewaySource;
#[cfg(test)]
use psychevo_gateway_protocol::source::SourceKey;

#[cfg(test)]
use super::binding::GatewayWebServerConfig;
use super::binding::WebState;
#[cfg(test)]
use super::voice::voice_policy_for_source;

const CHANNEL_POLL_BACKOFF_MS: u64 = 5_000;
const CHANNEL_IDLE_SLEEP_MS: u64 = 1_000;
const WECHAT_LOGIN_GRACE_MS: i64 = 60_000;

fn channel_multi_question_guidance(token: &str) -> String {
    format!(
        "This request has multiple questions. Answer it in Shared Attention in the Psychevo GUI, or reply /cancel {token}."
    )
}

mod adapters;
mod commands;
mod events;
mod paths;
mod reconcile;
mod runner;
mod state;

pub(super) use adapters::channel_control;
pub(super) use paths::redact_channel_error;
pub(super) use reconcile::reconcile;
pub(super) use state::ChannelRuntimeState;

pub(super) async fn channel_effective_profile_ref(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
) -> psychevo::Result<String> {
    Ok(channel_bound_profile_ref(state, source)
        .await?
        .or_else(|| connection.runtime_ref.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "native".to_string()))
}

pub(super) async fn channel_bind_target_draft(
    state: &WebState,
    source: &GatewaySource,
    target: &wire::agents_backend_rpc::RunnableTargetView,
) -> psychevo::Result<Option<String>> {
    let agent_ref = target.agent_ref.as_deref();
    let profile_ref = target.runtime_profile_ref.as_str();
    let source_key = source.source_key();
    let lane = state
        .inner
        .durability
        .gateway_source_lane(&source_key.0)
        .await?;
    let Some(current_thread_id) = lane.as_ref().and_then(|lane| lane.thread_id.as_deref()) else {
        state
            .inner
            .durability
            .upsert_gateway_source_lane(GatewaySourceLaneInput {
                source_key: &source_key.0,
                source_kind: &source.kind,
                raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                visible_name: source.visible_name.as_deref(),
                thread_id: None,
                draft_agent_ref: agent_ref,
                draft_profile_ref: Some(profile_ref),
                draft_control_values: &BTreeMap::new(),
                lineage: Some(json!({"reason": "channel_profile_draft"})),
            })
            .await?;
        state.inner.gateway.bump_source_generation_key(&source_key);
        return Ok(None);
    };

    let current_thread = state
        .inner
        .framework
        .resume_thread(current_thread_id.to_string())
        .await?;
    let current = current_thread.summary().await?;
    let mut start = StartThreadRequest::new(&current.cwd);
    start.source = source.kind.clone();
    let new_thread = state.inner.framework.start_thread(start).await?;
    let new_thread_id = new_thread.id().to_string();
    state
        .inner
        .durability
        .upsert_gateway_source_lane(GatewaySourceLaneInput {
            source_key: &source_key.0,
            source_kind: &source.kind,
            raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
            visible_name: source.visible_name.as_deref(),
            thread_id: Some(&new_thread_id),
            draft_agent_ref: agent_ref,
            draft_profile_ref: Some(profile_ref),
            draft_control_values: &BTreeMap::new(),
            lineage: Some(json!({
                "reason": "channel_target_switch",
                "previousThreadId": current_thread_id,
            })),
        })
        .await?;
    state.inner.gateway.bump_source_generation_key(&source_key);
    Ok(Some(new_thread_id))
}

pub(super) async fn channel_draft_agent_ref(
    state: &WebState,
    source: &GatewaySource,
) -> psychevo::Result<Option<String>> {
    Ok(state
        .inner
        .durability
        .gateway_source_lane(&source.source_key().0)
        .await?
        .and_then(|lane| lane.draft_agent_ref))
}

async fn channel_bound_profile_ref(
    state: &WebState,
    source: &GatewaySource,
) -> psychevo::Result<Option<String>> {
    let source_key = source.source_key();
    let lane = state
        .inner
        .durability
        .gateway_source_lane(&source_key.0)
        .await?;
    let bound = if let Some(thread_id) = lane.as_ref().and_then(|lane| lane.thread_id.as_deref()) {
        let thread = state
            .inner
            .framework
            .resume_thread(thread_id.to_string())
            .await?;
        match thread.agent_binding().await? {
            Some(ThreadAgentBinding::Resolved { binding, .. }) => Some(binding.runtime_ref),
            Some(ThreadAgentBinding::Unresolved { .. }) | None => None,
        }
    } else {
        None
    };
    Ok(bound.or_else(|| {
        lane.as_ref()
            .and_then(|lane| lane.draft_profile_ref.clone())
    }))
}

#[cfg(test)]
use events::channel_event_sink;
#[cfg(test)]
use runner::{handle_channel_message, run_channel_loop};

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
