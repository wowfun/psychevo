#[cfg(test)]
use std::collections::BTreeMap;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use serde_json::{Value, json};

#[cfg(test)]
use super::{ImAdapter, ImAttachment};

#[cfg(feature = "native-channels")]
mod feishu_lark;
mod telegram;
mod util;
mod wechat;

#[cfg(feature = "native-channels")]
pub use feishu_lark::{
    FeishuLarkDomain, FeishuLarkLongConnectionAdapter, FeishuLarkLongConnectionConfig,
};
pub use telegram::{TelegramPollingAdapter, TelegramPollingConfig};
pub use wechat::{
    WECHAT_ILINK_BASE_URL, WechatIlinkAdapter, WechatIlinkConfig, WechatIlinkHealth, WechatQrCode,
    WechatQrPoll, check_wechat_ilink_health, fetch_wechat_qr_code,
    is_wechat_ilink_session_expired_error, poll_wechat_qr_code, render_wechat_qr_svg,
    wechat_ilink_error_code_from_message,
};

#[cfg(all(test, feature = "native-channels"))]
use feishu_lark::feishu_event_to_inbound;
#[cfg(test)]
use telegram::telegram_update_to_message;
#[cfg(test)]
use wechat::{
    WECHAT_CHANNEL_VERSION, WECHAT_ILINK_APP_ID, WECHAT_SESSION_EXPIRED_ERRCODE,
    wechat_message_to_inbound,
};

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
