use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures::future::BoxFuture;
use psychevo_runtime::{Error, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};

use super::{ImAdapter, ImAttachment, ImIdentity, ImInboundMessage, ImOutboundMessage};

mod feishu_lark;
mod telegram;
mod util;
mod wechat;

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

#[cfg(test)]
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
