use super::*;
use crate::im::adapters::{
    FeishuLarkDomain, FeishuLarkLongConnectionAdapter, FeishuLarkLongConnectionConfig,
    TelegramPollingAdapter, TelegramPollingConfig, WECHAT_ILINK_BASE_URL, WechatIlinkAdapter,
    WechatIlinkConfig, is_wechat_ilink_session_expired_error, wechat_ilink_error_code_from_message,
};
use crate::im::{
    ChannelAdapterBinding, ChannelAllowlist, ChannelGateway, ImIdentity, ImInboundMessage,
    ImOutboundMessage, gateway_input_parts_for_im, gateway_source_for_im,
};
use psychevo_runtime::{ChannelRuntimeConnection, channel_runtime_connections};
use tokio_util::sync::CancellationToken;

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

pub(super) use paths::redact_channel_error;
pub(super) use reconcile::reconcile;
pub(super) use state::ChannelRuntimeState;

pub(super) fn channel_effective_runtime_ref(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
) -> psychevo_runtime::Result<String> {
    Ok(channel_bound_runtime_ref(state, source)?
        .or_else(|| connection.runtime_ref.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "native".to_string()))
}

pub(super) fn channel_bind_runtime_ref(
    state: &WebState,
    source: &GatewaySource,
    runtime_ref: &str,
) -> psychevo_runtime::Result<Option<String>> {
    let source_key = source.source_key();
    let lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&source_key.0)?;
    let Some(current_thread_id) = lane.as_ref().and_then(|lane| lane.thread_id.as_deref()) else {
        state
            .inner
            .state
            .store()
            .upsert_gateway_source_lane(GatewaySourceLaneInput {
                source_key: &source_key.0,
                source_kind: &source.kind,
                raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                visible_name: source.visible_name.as_deref(),
                thread_id: None,
                draft_runtime_ref: Some(runtime_ref),
                lineage: Some(json!({"reason": "channel_profile_draft"})),
            })?;
        state.inner.gateway.bump_source_generation_key(&source_key);
        return Ok(None);
    };

    let current = state
        .inner
        .state
        .store()
        .session_summary(current_thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {current_thread_id}")))?;
    let new_thread_id = state.inner.state.store().create_session_with_metadata(
        Path::new(&current.cwd),
        &source.kind,
        "pending",
        "pending",
        None,
    )?;
    let mut options = state.run_options(PathBuf::from(&current.cwd), Some(new_thread_id.clone()));
    options.runtime_ref = Some(runtime_ref.to_string());
    let (profile, revision, fingerprint) = crate::resolve_gateway_runtime_profile(&options)?;
    crate::ensure_gateway_runtime_binding(
        &state.inner.state,
        &new_thread_id,
        &profile,
        revision,
        &fingerprint,
    )?;
    state
        .inner
        .state
        .store()
        .upsert_gateway_source_lane(GatewaySourceLaneInput {
            source_key: &source_key.0,
            source_kind: &source.kind,
            raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
            visible_name: source.visible_name.as_deref(),
            thread_id: Some(&new_thread_id),
            draft_runtime_ref: None,
            lineage: Some(json!({
                "reason": "channel_profile_switch",
                "previousThreadId": current_thread_id,
            })),
        })?;
    state.inner.gateway.bump_source_generation_key(&source_key);
    Ok(Some(new_thread_id))
}

fn channel_bound_runtime_ref(
    state: &WebState,
    source: &GatewaySource,
) -> psychevo_runtime::Result<Option<String>> {
    let source_key = source.source_key();
    let lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&source_key.0)?;
    let bound = lane
        .as_ref()
        .and_then(|lane| lane.thread_id.as_deref())
        .map(|thread_id| state.inner.state.store().gateway_runtime_binding(thread_id))
        .transpose()?
        .flatten()
        .and_then(|binding| binding.runtime_ref);
    Ok(bound
        .or_else(|| {
            lane.as_ref()
                .and_then(|lane| lane.draft_runtime_ref.clone())
        })
        .or_else(|| {
            // Read legacy source lineage only when no immutable thread binding exists.
            lane.and_then(|lane| lane.lineage).and_then(|lineage| {
                lineage
                    .get("runtimeRef")
                    .or_else(|| lineage.get("runtime_ref"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
        }))
}

#[cfg(test)]
use events::channel_event_sink;
#[cfg(test)]
use runner::{handle_channel_message, run_channel_loop};

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
