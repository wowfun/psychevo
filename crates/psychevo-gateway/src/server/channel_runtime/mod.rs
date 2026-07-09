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
    let Some(binding) = state
        .inner
        .state
        .store()
        .gateway_source_binding(&source_key.0)?
    else {
        return Ok(None);
    };
    let backend = GatewayBackendInfo {
        kind: if binding.backend_kind == BackendKind::PeerAgent.as_str() {
            BackendKind::PeerAgent
        } else {
            BackendKind::Psychevo
        },
        runtime_ref: Some(runtime_ref.to_string()),
        native_id: binding.backend_native_id.clone(),
    };
    state.inner.gateway.bind_source_thread(
        source,
        &binding.thread_id,
        &backend,
        binding.lineage,
    )?;
    Ok(Some(binding.thread_id))
}

fn channel_bound_runtime_ref(
    state: &WebState,
    source: &GatewaySource,
) -> psychevo_runtime::Result<Option<String>> {
    let source_key = source.source_key();
    Ok(state
        .inner
        .state
        .store()
        .gateway_source_binding(&source_key.0)?
        .and_then(|binding| binding.lineage)
        .and_then(|lineage| {
            lineage
                .get("runtimeRef")
                .or_else(|| lineage.get("runtime_ref"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        }))
}

#[cfg(test)]
use events::channel_event_sink;
#[cfg(test)]
use runner::{handle_channel_message, run_channel_loop};

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
