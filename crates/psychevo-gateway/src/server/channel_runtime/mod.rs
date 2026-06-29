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

#[cfg(test)]
use events::channel_event_sink;
#[cfg(test)]
use runner::{handle_channel_message, run_channel_loop};

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
