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

use super::{ImAdapter, ImIdentity, ImInboundMessage, ImOutboundMessage};

const TELEGRAM_API_BASE: &str = "https://api.telegram.org";
pub const WECHAT_ILINK_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
const WECHAT_ILINK_APP_ID: &str = "bot";
const WECHAT_CHANNEL_VERSION: &str = "2.2.0";
const WECHAT_ILINK_CLIENT_VERSION: u32 = (2 << 16) | (2 << 8);
const WECHAT_ITEM_TEXT: i64 = 1;
const WECHAT_MSG_TYPE_BOT: i64 = 2;
const WECHAT_MSG_STATE_FINISH: i64 = 2;
const WECHAT_QR_BOT_TYPE: &str = "3";
const WECHAT_SESSION_EXPIRED_ERRCODE: i64 = -14;

#[derive(Debug, Clone)]
pub struct TelegramPollingConfig {
    pub connection_id: Option<String>,
    pub token: String,
    pub api_base: String,
    pub timeout_secs: u64,
}

impl TelegramPollingConfig {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            connection_id: None,
            token: token.into(),
            api_base: TELEGRAM_API_BASE.to_string(),
            timeout_secs: 25,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TelegramPollingAdapter {
    config: Arc<TelegramPollingConfig>,
    client: reqwest::Client,
    offset: Arc<Mutex<Option<i64>>>,
}

impl TelegramPollingAdapter {
    pub fn new(config: TelegramPollingConfig) -> Result<Self> {
        if config.token.trim().is_empty() {
            return Err(Error::Message(
                "Telegram adapter requires a bot token".to_string(),
            ));
        }
        Ok(Self {
            config: Arc::new(config),
            client: reqwest::Client::new(),
            offset: Arc::new(Mutex::new(None)),
        })
    }

    fn method_url(&self, method: &str) -> String {
        format!(
            "{}/bot{}/{}",
            self.config.api_base.trim_end_matches('/'),
            self.config.token,
            method
        )
    }
}

impl ImAdapter for TelegramPollingAdapter {
    fn platform(&self) -> &str {
        "telegram"
    }

    fn poll(&self) -> BoxFuture<'static, Result<Vec<ImInboundMessage>>> {
        let adapter = self.clone();
        Box::pin(async move {
            let offset = *adapter.offset.lock().await;
            let response = adapter
                .client
                .post(adapter.method_url("getUpdates"))
                .json(&TelegramGetUpdatesRequest {
                    allowed_updates: vec!["message".to_string()],
                    offset,
                    timeout: adapter.config.timeout_secs,
                })
                .send()
                .await?;
            let body: Value = response.error_for_status()?.json().await?;
            if !body.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                return Err(Error::Message(format!(
                    "Telegram getUpdates failed: {}",
                    body.get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                )));
            }
            let updates = body
                .get("result")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut next_offset = offset;
            let mut messages = Vec::new();
            for update in updates {
                if let Some(update_id) = update.get("update_id").and_then(Value::as_i64) {
                    next_offset =
                        Some(next_offset.map_or(update_id + 1, |old| old.max(update_id + 1)));
                }
                if let Some(message) =
                    telegram_update_to_message(&update, adapter.config.connection_id.as_deref())
                {
                    messages.push(message);
                }
            }
            *adapter.offset.lock().await = next_offset;
            Ok(messages)
        })
    }

    fn send(&self, message: ImOutboundMessage) -> BoxFuture<'static, Result<()>> {
        let adapter = self.clone();
        Box::pin(async move {
            if message.text.trim().is_empty() {
                return Err(Error::Message(
                    "Telegram outbound text cannot be empty".to_string(),
                ));
            }
            let mut body = json!({
                "chat_id": message.identity.chat_id,
                "text": message.text,
            });
            if let Some(reply_to) = message
                .identity
                .reply_to
                .as_deref()
                .and_then(|value| value.parse::<i64>().ok())
            {
                body["reply_parameters"] = json!({ "message_id": reply_to });
            }
            let response = adapter
                .client
                .post(adapter.method_url("sendMessage"))
                .json(&body)
                .send()
                .await?;
            let body: Value = response.error_for_status()?.json().await?;
            if body.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                Ok(())
            } else {
                Err(Error::Message(format!(
                    "Telegram sendMessage failed: {}",
                    body.get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                )))
            }
        })
    }
}

#[derive(Debug, Serialize)]
struct TelegramGetUpdatesRequest {
    allowed_updates: Vec<String>,
    offset: Option<i64>,
    timeout: u64,
}

fn telegram_update_to_message(
    update: &Value,
    connection_id: Option<&str>,
) -> Option<ImInboundMessage> {
    let message = update.get("message")?;
    let text = message
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| message.get("caption").and_then(Value::as_str))?
        .trim();
    if text.is_empty() {
        return None;
    }
    let chat = message.get("chat")?;
    let chat_id = value_id_to_string(chat.get("id")?)?;
    let chat_type = chat
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| Some("chat".to_string()));
    let from = message.get("from");
    let user_id = from
        .and_then(|from| from.get("id"))
        .and_then(value_id_to_string);
    let message_id = message
        .get("message_id")
        .and_then(value_id_to_string)
        .or_else(|| update.get("update_id").and_then(value_id_to_string))?;
    Some(ImInboundMessage {
        identity: ImIdentity {
            connection_id: connection_id.map(str::to_string),
            platform: "telegram".to_string(),
            domain: Some("telegram".to_string()),
            workspace_id: None,
            chat_type,
            chat_id,
            thread_id: message
                .get("message_thread_id")
                .and_then(value_id_to_string),
            user_id,
            operator_id: None,
            reply_to: message
                .get("reply_to_message")
                .and_then(|reply| reply.get("message_id"))
                .and_then(value_id_to_string),
        },
        message_id,
        text: text.to_string(),
        task_key: None,
    })
}

#[derive(Debug, Clone)]
pub struct WechatQrCode {
    pub qrcode: String,
    pub qr_url: String,
    pub qr_image: Option<String>,
    pub qr_svg: Option<String>,
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WechatQrPoll {
    Waiting {
        status: String,
        message: String,
        base_url: String,
    },
    Expired {
        message: String,
    },
    Confirmed {
        account_id: String,
        token: String,
        base_url: String,
        user_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatIlinkHealth {
    pub ok: bool,
    pub reason: Option<String>,
    pub errcode: Option<i64>,
    pub message: Option<String>,
    pub msg_count: usize,
}

#[derive(Debug, Deserialize)]
struct WechatQrResponse {
    qrcode: Option<String>,
    qrcode_img_content: Option<String>,
}

pub async fn fetch_wechat_qr_code(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<WechatQrCode> {
    let base_url = normalize_wechat_base_url(base_url);
    let response = client
        .get(format!(
            "{}/ilink/bot/get_bot_qrcode?bot_type={}",
            base_url.trim_end_matches('/'),
            WECHAT_QR_BOT_TYPE
        ))
        .timeout(Duration::from_secs(35))
        .send()
        .await?;
    let body: WechatQrResponse = response.error_for_status()?.json().await?;
    let qrcode = body
        .qrcode
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::Message("WeChat QR response did not include qrcode".to_string()))?;
    let qr_url = body
        .qrcode_img_content
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| qrcode.clone());
    let is_image = qr_url
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("data:image/");
    let qr_image = is_image.then(|| qr_url.clone());
    let qr_svg = if is_image {
        None
    } else {
        Some(render_wechat_qr_svg(&qr_url)?)
    };
    Ok(WechatQrCode {
        qrcode,
        qr_url,
        qr_image,
        qr_svg,
        base_url,
    })
}

pub async fn poll_wechat_qr_code(
    client: &reqwest::Client,
    base_url: &str,
    qrcode: &str,
) -> Result<WechatQrPoll> {
    let base_url = normalize_wechat_base_url(base_url);
    let response = client
        .get(format!(
            "{}/ilink/bot/get_qrcode_status?qrcode={}",
            base_url.trim_end_matches('/'),
            qrcode
        ))
        .timeout(Duration::from_secs(35))
        .send()
        .await?;
    let body: Value = response.error_for_status()?.json().await?;
    let status = body
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("wait")
        .to_string();
    match status.as_str() {
        "confirmed" => {
            let account_id = body
                .get("ilink_bot_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let token = body
                .get("bot_token")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if account_id.is_empty() || token.is_empty() {
                return Err(Error::Message(
                    "WeChat QR confirmation did not include account id and token".to_string(),
                ));
            }
            let confirmed_base_url = body
                .get("baseurl")
                .and_then(Value::as_str)
                .map(normalize_wechat_base_url)
                .unwrap_or(base_url);
            let user_id = body
                .get("ilink_user_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            Ok(WechatQrPoll::Confirmed {
                account_id,
                token,
                base_url: confirmed_base_url,
                user_id,
            })
        }
        "expired" => Ok(WechatQrPoll::Expired {
            message: "WeChat QR code expired; generate a new code".to_string(),
        }),
        "scaned" => Ok(WechatQrPoll::Waiting {
            status,
            message: "Scanned; confirm login in WeChat".to_string(),
            base_url,
        }),
        "scaned_but_redirect" => {
            let redirected = body
                .get("redirect_host")
                .and_then(Value::as_str)
                .map(|host| redirected_wechat_base_url(&base_url, host))
                .unwrap_or_else(|| base_url.clone());
            Ok(WechatQrPoll::Waiting {
                status,
                message: "Scanned; following redirected iLink host".to_string(),
                base_url: redirected,
            })
        }
        _ => Ok(WechatQrPoll::Waiting {
            status,
            message: "Waiting for WeChat scan".to_string(),
            base_url,
        }),
    }
}

pub async fn check_wechat_ilink_health(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
    timeout_secs: u64,
) -> Result<WechatIlinkHealth> {
    let base_url = normalize_wechat_base_url(base_url);
    let body = json!({
        "get_updates_buf": "",
        "base_info": { "channel_version": WECHAT_CHANNEL_VERSION },
    });
    let body_text = serde_json::to_string(&body)?;
    let response = client
        .post(format!(
            "{}/ilink/bot/getupdates",
            base_url.trim_end_matches('/')
        ))
        .headers(wechat_headers(token, &body_text)?)
        .body(body_text)
        .timeout(Duration::from_secs(timeout_secs.max(1)))
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(err) if err.is_timeout() => {
            return Ok(WechatIlinkHealth {
                ok: true,
                reason: Some("polling_empty".to_string()),
                errcode: None,
                message: None,
                msg_count: 0,
            });
        }
        Err(err) => return Err(err.into()),
    };
    let body: Value = response.error_for_status()?.json().await?;
    Ok(wechat_health_from_getupdates_body(&body))
}

pub fn render_wechat_qr_svg(value: &str) -> Result<String> {
    let code = qrcode::QrCode::new(value.as_bytes())
        .map_err(|err| Error::Message(format!("failed to render WeChat QR code: {err}")))?;
    Ok(code
        .render::<qrcode::render::svg::Color<'_>>()
        .min_dimensions(196, 196)
        .dark_color(qrcode::render::svg::Color("#111111"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build())
}

fn normalize_wechat_base_url(value: &str) -> String {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        WECHAT_ILINK_BASE_URL.to_string()
    } else {
        trimmed.to_string()
    }
}

fn redirected_wechat_base_url(current_base_url: &str, redirect_host: &str) -> String {
    let host = redirect_host.trim().trim_end_matches('/');
    if host.starts_with("http://") || host.starts_with("https://") {
        host.to_string()
    } else if current_base_url.starts_with("http://") {
        format!("http://{host}")
    } else {
        format!("https://{host}")
    }
}

#[derive(Debug, Clone)]
pub struct WechatIlinkConfig {
    pub connection_id: Option<String>,
    pub token: String,
    pub account_id: String,
    pub base_url: String,
    pub timeout_secs: u64,
    pub context_store_path: Option<PathBuf>,
}

impl WechatIlinkConfig {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            connection_id: None,
            token: token.into(),
            account_id: String::new(),
            base_url: WECHAT_ILINK_BASE_URL.to_string(),
            timeout_secs: 35,
            context_store_path: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WechatIlinkAdapter {
    config: Arc<WechatIlinkConfig>,
    client: reqwest::Client,
    sync_buf: Arc<Mutex<String>>,
    context_tokens: Arc<Mutex<BTreeMap<String, String>>>,
}

impl WechatIlinkAdapter {
    pub fn new(config: WechatIlinkConfig) -> Result<Self> {
        if config.token.trim().is_empty() {
            return Err(Error::Message(
                "WeChat iLink adapter requires a bot token".to_string(),
            ));
        }
        let context_tokens = config
            .context_store_path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<BTreeMap<String, String>>(&text).ok())
            .unwrap_or_default();
        Ok(Self {
            config: Arc::new(config),
            client: reqwest::Client::new(),
            sync_buf: Arc::new(Mutex::new(String::new())),
            context_tokens: Arc::new(Mutex::new(context_tokens)),
        })
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.config.base_url.trim_end_matches('/'), path)
    }

    async fn post_ilink(&self, endpoint: &str, payload: Value, timeout_secs: u64) -> Result<Value> {
        let mut body = payload;
        body["base_info"] = json!({ "channel_version": WECHAT_CHANNEL_VERSION });
        let body_text = serde_json::to_string(&body)?;
        let response = self
            .client
            .post(self.endpoint(endpoint))
            .headers(wechat_headers(&self.config.token, &body_text)?)
            .body(body_text)
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await?;
        Ok(response.error_for_status()?.json().await?)
    }

    async fn remember_context_token(&self, chat_id: &str, token: &str) -> Result<()> {
        let snapshot = {
            let mut tokens = self.context_tokens.lock().await;
            tokens.insert(chat_id.to_string(), token.to_string());
            tokens.clone()
        };
        if let Some(path) = &self.config.context_store_path {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, serde_json::to_vec_pretty(&snapshot)?)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            }
        }
        Ok(())
    }
}

impl ImAdapter for WechatIlinkAdapter {
    fn platform(&self) -> &str {
        "wechat"
    }

    fn poll(&self) -> BoxFuture<'static, Result<Vec<ImInboundMessage>>> {
        let adapter = self.clone();
        Box::pin(async move {
            let sync_buf = adapter.sync_buf.lock().await.clone();
            let body = match adapter
                .post_ilink(
                    "ilink/bot/getupdates",
                    json!({ "get_updates_buf": sync_buf }),
                    adapter.config.timeout_secs,
                )
                .await
            {
                Ok(body) => body,
                Err(Error::Http(err)) if err.is_timeout() => return Ok(Vec::new()),
                Err(err) => return Err(err),
            };
            if wechat_ilink_session_expired(&body) {
                return Err(Error::Message(wechat_expired_session_message(&body)));
            }
            if !wechat_success(&body) {
                return Err(Error::Message(format!(
                    "WeChat iLink getupdates failed: {}",
                    wechat_error_message(&body)
                )));
            }
            if let Some(next) = body.get("get_updates_buf").and_then(Value::as_str) {
                *adapter.sync_buf.lock().await = next.to_string();
            }
            let mut messages = Vec::new();
            for raw in body
                .get("msgs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                if let Some(message) = wechat_message_to_inbound(
                    &raw,
                    adapter.config.connection_id.as_deref(),
                    adapter.config.account_id.as_str(),
                ) {
                    if let Some(token) = raw.get("context_token").and_then(Value::as_str) {
                        adapter
                            .remember_context_token(&message.identity.chat_id, token)
                            .await?;
                    }
                    messages.push(message);
                }
            }
            Ok(messages)
        })
    }

    fn send(&self, message: ImOutboundMessage) -> BoxFuture<'static, Result<()>> {
        let adapter = self.clone();
        Box::pin(async move {
            if message.text.trim().is_empty() {
                return Err(Error::Message(
                    "WeChat iLink outbound text cannot be empty".to_string(),
                ));
            }
            let context_token = adapter
                .context_tokens
                .lock()
                .await
                .get(&message.identity.chat_id)
                .cloned();
            let mut msg = json!({
                "from_user_id": "",
                "to_user_id": message.identity.chat_id,
                "client_id": format!("psychevo-{}", now_millis()),
                "message_type": WECHAT_MSG_TYPE_BOT,
                "message_state": WECHAT_MSG_STATE_FINISH,
                "item_list": [{
                    "type": WECHAT_ITEM_TEXT,
                    "text_item": { "text": message.text },
                }],
            });
            if let Some(token) = context_token {
                msg["context_token"] = json!(token);
            }
            let body = adapter
                .post_ilink("ilink/bot/sendmessage", json!({ "msg": msg }), 15)
                .await?;
            if wechat_success(&body) {
                Ok(())
            } else {
                Err(Error::Message(format!(
                    "WeChat iLink sendmessage failed: {}",
                    wechat_error_message(&body)
                )))
            }
        })
    }
}

fn wechat_headers(token: &str, body: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "AuthorizationType",
        HeaderValue::from_static("ilink_bot_token"),
    );
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|err| Error::Message(format!("invalid WeChat token header: {err}")))?,
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&body.len().to_string())
            .map_err(|err| Error::Message(format!("invalid WeChat content length: {err}")))?,
    );
    headers.insert(
        "X-WECHAT-UIN",
        HeaderValue::from_str(&BASE64_STANDARD.encode(now_millis().to_string()))
            .map_err(|err| Error::Message(format!("invalid WeChat uin header: {err}")))?,
    );
    headers.insert(
        "iLink-App-Id",
        HeaderValue::from_static(WECHAT_ILINK_APP_ID),
    );
    headers.insert(
        "iLink-App-ClientVersion",
        HeaderValue::from_str(&WECHAT_ILINK_CLIENT_VERSION.to_string()).map_err(|err| {
            Error::Message(format!("invalid WeChat client version header: {err}"))
        })?,
    );
    Ok(headers)
}

fn wechat_message_to_inbound(
    raw: &Value,
    connection_id: Option<&str>,
    account_id: &str,
) -> Option<ImInboundMessage> {
    let sender = raw.get("from_user_id").and_then(Value::as_str)?.trim();
    if sender.is_empty() || (!account_id.is_empty() && sender == account_id) {
        return None;
    }
    let items = raw.get("item_list").and_then(Value::as_array)?;
    let text = extract_wechat_text(items)?.trim().to_string();
    if text.is_empty() {
        return None;
    }
    let (chat_type, chat_id) = wechat_chat_identity(raw, account_id, sender);
    let message_id = raw
        .get("message_id")
        .or_else(|| raw.get("msg_id"))
        .or_else(|| raw.get("client_id"))
        .and_then(value_id_to_string)
        .unwrap_or_else(|| format!("wechat-{}", now_millis()));
    Some(ImInboundMessage {
        identity: ImIdentity {
            connection_id: connection_id.map(str::to_string),
            platform: "wechat".to_string(),
            domain: Some("wechat".to_string()),
            workspace_id: None,
            chat_type: Some(chat_type),
            chat_id,
            thread_id: None,
            user_id: Some(sender.to_string()),
            operator_id: None,
            reply_to: None,
        },
        message_id,
        text,
        task_key: None,
    })
}

fn extract_wechat_text(items: &[Value]) -> Option<String> {
    let mut chunks = Vec::new();
    for item in items {
        if item.get("type").and_then(Value::as_i64) != Some(WECHAT_ITEM_TEXT) {
            continue;
        }
        if let Some(text) = item
            .get("text_item")
            .and_then(|text_item| text_item.get("text"))
            .and_then(Value::as_str)
            .or_else(|| item.get("text").and_then(Value::as_str))
        {
            chunks.push(text.to_string());
        }
    }
    (!chunks.is_empty()).then(|| chunks.join("\n"))
}

fn wechat_chat_identity(raw: &Value, account_id: &str, sender: &str) -> (String, String) {
    let room_id = raw
        .get("room_id")
        .or_else(|| raw.get("chat_room_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let to_user_id = raw
        .get("to_user_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let group_like = !room_id.is_empty()
        || (!account_id.is_empty() && !to_user_id.is_empty() && to_user_id != account_id);
    if group_like {
        let chat_id = if room_id.is_empty() {
            to_user_id
        } else {
            room_id
        };
        ("group".to_string(), chat_id.to_string())
    } else {
        ("dm".to_string(), sender.to_string())
    }
}

fn wechat_success(body: &Value) -> bool {
    body.get("ret").and_then(Value::as_i64).unwrap_or(0) == 0
        && body.get("errcode").and_then(Value::as_i64).unwrap_or(0) == 0
}

pub fn wechat_ilink_error_code_from_message(message: &str) -> Option<i64> {
    if message.contains("errcode=-14") || message.contains("ret=-14") {
        Some(WECHAT_SESSION_EXPIRED_ERRCODE)
    } else {
        None
    }
}

pub fn is_wechat_ilink_session_expired_error(message: &str) -> bool {
    wechat_ilink_error_code_from_message(message) == Some(WECHAT_SESSION_EXPIRED_ERRCODE)
        || (message.contains("needs_qr_login") && message.contains("WeChat iLink"))
}

fn wechat_health_from_getupdates_body(body: &Value) -> WechatIlinkHealth {
    let errcode = wechat_ilink_error_code(body);
    if wechat_ilink_session_expired(body) {
        return WechatIlinkHealth {
            ok: false,
            reason: Some("needs_qr_login".to_string()),
            errcode,
            message: Some(wechat_error_message(body)),
            msg_count: 0,
        };
    }
    let msg_count = body
        .get("msgs")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    if wechat_success(body) {
        WechatIlinkHealth {
            ok: true,
            reason: Some(if msg_count == 0 {
                "polling_empty".to_string()
            } else {
                "running".to_string()
            }),
            errcode: None,
            message: None,
            msg_count,
        }
    } else {
        WechatIlinkHealth {
            ok: false,
            reason: Some("error".to_string()),
            errcode,
            message: Some(wechat_error_message(body)),
            msg_count,
        }
    }
}

fn wechat_ilink_session_expired(body: &Value) -> bool {
    wechat_ilink_error_code(body) == Some(WECHAT_SESSION_EXPIRED_ERRCODE)
}

fn wechat_ilink_error_code(body: &Value) -> Option<i64> {
    body.get("errcode")
        .and_then(Value::as_i64)
        .or_else(|| body.get("ret").and_then(Value::as_i64))
}

fn wechat_expired_session_message(body: &Value) -> String {
    let code = wechat_ilink_error_code(body).unwrap_or(WECHAT_SESSION_EXPIRED_ERRCODE);
    format!(
        "WeChat iLink getupdates failed: needs_qr_login errcode={code}: {}",
        wechat_error_message(body)
    )
}

fn wechat_error_message(body: &Value) -> String {
    body.get("errmsg")
        .or_else(|| body.get("msg"))
        .and_then(Value::as_str)
        .unwrap_or("unknown error")
        .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeishuLarkDomain {
    Feishu,
    Lark,
}

impl FeishuLarkDomain {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "feishu" => Some(Self::Feishu),
            "lark" => Some(Self::Lark),
            _ => None,
        }
    }

    pub fn platform(self) -> &'static str {
        match self {
            Self::Feishu => "feishu",
            Self::Lark => "lark",
        }
    }

    pub fn base_url(self) -> &'static str {
        match self {
            Self::Feishu => feishu_sdk::core::FEISHU_BASE_URL,
            Self::Lark => feishu_sdk::core::LARK_BASE_URL,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FeishuLarkLongConnectionConfig {
    pub connection_id: Option<String>,
    pub app_id: String,
    pub app_secret: String,
    pub domain: FeishuLarkDomain,
    pub base_url: Option<String>,
}

impl FeishuLarkLongConnectionConfig {
    pub fn new(
        domain: FeishuLarkDomain,
        app_id: impl Into<String>,
        app_secret: impl Into<String>,
    ) -> Self {
        Self {
            connection_id: None,
            app_id: app_id.into(),
            app_secret: app_secret.into(),
            domain,
            base_url: None,
        }
    }
}

#[derive(Clone)]
pub struct FeishuLarkLongConnectionAdapter {
    config: Arc<FeishuLarkLongConnectionConfig>,
    client: feishu_sdk::Client,
    inbound_tx: mpsc::UnboundedSender<ImInboundMessage>,
    inbound_rx: Arc<Mutex<mpsc::UnboundedReceiver<ImInboundMessage>>>,
    stream_task: Arc<Mutex<Option<FeishuStreamTask>>>,
}

type FeishuStreamTask = tokio::task::JoinHandle<std::result::Result<(), feishu_sdk::core::Error>>;

impl FeishuLarkLongConnectionAdapter {
    pub fn new(config: FeishuLarkLongConnectionConfig) -> Result<Self> {
        if config.app_id.trim().is_empty() || config.app_secret.trim().is_empty() {
            return Err(Error::Message(
                "Feishu/Lark adapter requires app id and app secret".to_string(),
            ));
        }
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| config.domain.base_url().to_string());
        let sdk_config = feishu_sdk::core::Config::builder(&config.app_id, &config.app_secret)
            .base_url(base_url)
            .log_level(feishu_sdk::core::LogLevel::Error)
            .build();
        let client = feishu_sdk::Client::new(sdk_config).map_err(feishu_error)?;
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        Ok(Self {
            config: Arc::new(config),
            client,
            inbound_tx,
            inbound_rx: Arc::new(Mutex::new(inbound_rx)),
            stream_task: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn connect(config: FeishuLarkLongConnectionConfig) -> Result<Self> {
        let adapter = Self::new(config)?;
        adapter.start_long_connection().await?;
        Ok(adapter)
    }

    pub async fn start_long_connection(&self) -> Result<()> {
        let mut task = self.stream_task.lock().await;
        if task.is_some() {
            return Ok(());
        }
        let dispatcher = feishu_sdk::event::EventDispatcher::new(
            feishu_sdk::event::EventDispatcherConfig::new(),
            feishu_sdk::core::noop_logger(),
        );
        dispatcher
            .register_handler(Box::new(FeishuQueueHandler::new(
                "im.message.receive_v1",
                self.config.clone(),
                self.inbound_tx.clone(),
            )))
            .await;
        dispatcher
            .register_handler(Box::new(FeishuQueueHandler::new(
                "im.message.receive",
                self.config.clone(),
                self.inbound_tx.clone(),
            )))
            .await;
        let stream = self
            .client
            .stream()
            .event_dispatcher(dispatcher)
            .build()
            .map_err(feishu_error)?;
        *task = Some(stream.spawn());
        Ok(())
    }

    async fn drain_inbound(&self) -> Vec<ImInboundMessage> {
        let mut receiver = self.inbound_rx.lock().await;
        let mut messages = Vec::new();
        while let Ok(message) = receiver.try_recv() {
            messages.push(message);
        }
        messages
    }
}

impl ImAdapter for FeishuLarkLongConnectionAdapter {
    fn platform(&self) -> &str {
        self.config.domain.platform()
    }

    fn poll(&self) -> BoxFuture<'static, Result<Vec<ImInboundMessage>>> {
        let adapter = self.clone();
        Box::pin(async move { Ok(adapter.drain_inbound().await) })
    }

    fn send(&self, message: ImOutboundMessage) -> BoxFuture<'static, Result<()>> {
        let adapter = self.clone();
        Box::pin(async move {
            if message.text.trim().is_empty() {
                return Err(Error::Message(
                    "Feishu/Lark outbound text cannot be empty".to_string(),
                ));
            }
            let receive_id_type = match message.identity.chat_type.as_deref() {
                Some("dm") | Some("private") => "open_id",
                _ => "chat_id",
            };
            let body = json!({
                "receive_id": message.identity.chat_id,
                "msg_type": "text",
                "content": json!({ "text": message.text }).to_string(),
            });
            let response = adapter
                .client
                .operation("im.v1.message.create")
                .query_param("receive_id_type", receive_id_type)
                .body_value(body)
                .send()
                .await
                .map_err(feishu_error)?;
            let value = response.json_value().unwrap_or_default();
            let code = value.get("code").and_then(Value::as_i64).unwrap_or(0);
            if code == 0 {
                Ok(())
            } else {
                Err(Error::Message(format!(
                    "Feishu/Lark message create failed: {}",
                    value
                        .get("msg")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                )))
            }
        })
    }
}

struct FeishuQueueHandler {
    event_type: String,
    config: Arc<FeishuLarkLongConnectionConfig>,
    tx: mpsc::UnboundedSender<ImInboundMessage>,
}

impl FeishuQueueHandler {
    fn new(
        event_type: impl Into<String>,
        config: Arc<FeishuLarkLongConnectionConfig>,
        tx: mpsc::UnboundedSender<ImInboundMessage>,
    ) -> Self {
        Self {
            event_type: event_type.into(),
            config,
            tx,
        }
    }
}

impl feishu_sdk::event::EventHandler for FeishuQueueHandler {
    fn event_type(&self) -> &str {
        &self.event_type
    }

    fn handle(
        &self,
        event: feishu_sdk::event::Event,
    ) -> Pin<Box<dyn Future<Output = feishu_sdk::event::EventHandlerResult> + Send + '_>> {
        let config = self.config.clone();
        let tx = self.tx.clone();
        Box::pin(async move {
            if let Some(message) = feishu_event_to_inbound(&event, &config) {
                let _ = tx.send(message);
            }
            Ok(None)
        })
    }
}

fn feishu_event_to_inbound(
    event: &feishu_sdk::event::Event,
    config: &FeishuLarkLongConnectionConfig,
) -> Option<ImInboundMessage> {
    let payload = event.event.clone()?;
    let message_event: feishu_sdk::event::models::im::MessageEvent =
        serde_json::from_value(payload).ok()?;
    if message_event.message.message_type.as_deref() != Some("text") {
        return None;
    }
    let content = message_event.message.content.as_deref()?;
    let text = serde_json::from_str::<Value>(content)
        .ok()
        .and_then(|value| {
            value
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| content.to_string());
    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }
    let sender_id = message_event.sender.sender_id.as_ref().and_then(|id| {
        id.open_id
            .clone()
            .or_else(|| id.user_id.clone())
            .or_else(|| id.union_id.clone())
    });
    let chat_id = message_event
        .message
        .chat_id
        .clone()
        .or_else(|| sender_id.clone())?;
    let message_id = message_event
        .message
        .message_id
        .clone()
        .or_else(|| event.event_id().map(str::to_string))?;
    Some(ImInboundMessage {
        identity: ImIdentity {
            connection_id: config.connection_id.clone(),
            platform: config.domain.platform().to_string(),
            domain: Some(config.domain.platform().to_string()),
            workspace_id: event.tenant_key().map(str::to_string),
            chat_type: Some(
                message_event
                    .message
                    .chat_id
                    .as_deref()
                    .map(|_| "group")
                    .unwrap_or("dm")
                    .to_string(),
            ),
            chat_id,
            thread_id: message_event
                .message
                .root_id
                .or(message_event.message.parent_id),
            user_id: sender_id,
            operator_id: None,
            reply_to: None,
        },
        message_id,
        text,
        task_key: None,
    })
}

fn feishu_error(err: feishu_sdk::core::Error) -> Error {
    Error::Message(format!("Feishu/Lark adapter failed: {err}"))
}

fn value_id_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use axum::extract::State;
    use axum::http::HeaderMap as AxumHeaderMap;
    use axum::routing::post;
    use axum::{Json, Router};
    use tokio::net::TcpListener;

    use super::*;

    #[derive(Clone, Default)]
    struct HttpTestState {
        bodies: Arc<StdMutex<Vec<Value>>>,
        headers: Arc<StdMutex<Vec<BTreeMap<String, String>>>>,
    }

    #[test]
    fn telegram_update_maps_text_message() {
        let update = json!({
            "update_id": 91,
            "message": {
                "message_id": 10,
                "message_thread_id": 5,
                "chat": { "id": -1001, "type": "supergroup" },
                "from": { "id": 42 },
                "text": "hello"
            }
        });

        let message = telegram_update_to_message(&update, Some("release")).expect("message");
        assert_eq!(message.identity.connection_id.as_deref(), Some("release"));
        assert_eq!(message.identity.platform, "telegram");
        assert_eq!(message.identity.chat_id, "-1001");
        assert_eq!(message.identity.user_id.as_deref(), Some("42"));
        assert_eq!(message.identity.thread_id.as_deref(), Some("5"));
        assert_eq!(message.message_id, "10");
        assert_eq!(message.text, "hello");
    }

    #[tokio::test]
    async fn telegram_polling_adapter_calls_bot_api_and_advances_offset() {
        async fn get_updates(
            State(state): State<HttpTestState>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            let mut bodies = state.bodies.lock().expect("bodies");
            bodies.push(body);
            let result = if bodies.len() == 1 {
                vec![json!({
                    "update_id": 91,
                    "message": {
                        "message_id": 10,
                        "chat": { "id": 123, "type": "private" },
                        "from": { "id": 42 },
                        "text": "hello"
                    }
                })]
            } else {
                Vec::new()
            };
            Json(json!({ "ok": true, "result": result }))
        }

        let state = HttpTestState::default();
        let base_url = spawn_router(
            Router::new()
                .route("/{bot}/getUpdates", post(get_updates))
                .with_state(state.clone()),
        )
        .await;
        let adapter = TelegramPollingAdapter::new(TelegramPollingConfig {
            connection_id: Some("telegram".to_string()),
            token: "test-token".to_string(),
            api_base: base_url,
            timeout_secs: 1,
        })
        .expect("adapter");

        let messages = adapter.poll().await.expect("first poll");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].identity.chat_id, "123");
        assert_eq!(messages[0].text, "hello");
        let _ = adapter.poll().await.expect("second poll");

        let bodies = state.bodies.lock().expect("bodies");
        assert_eq!(bodies[0]["timeout"], 1);
        assert_eq!(bodies[0]["allowed_updates"], json!(["message"]));
        assert!(bodies[0].get("offset").is_none_or(Value::is_null));
        assert_eq!(bodies[1]["offset"], 92);
    }

    #[test]
    fn wechat_message_maps_text_and_context_identity() {
        let raw = json!({
            "message_id": "wx_msg_1",
            "from_user_id": "wx_user",
            "to_user_id": "account",
            "item_list": [
                { "type": 1, "text_item": { "text": "ping" } }
            ]
        });

        let message = wechat_message_to_inbound(&raw, Some("wechat"), "account").expect("message");
        assert_eq!(message.identity.connection_id.as_deref(), Some("wechat"));
        assert_eq!(message.identity.platform, "wechat");
        assert_eq!(message.identity.chat_type.as_deref(), Some("dm"));
        assert_eq!(message.identity.chat_id, "wx_user");
        assert_eq!(message.text, "ping");
    }

    #[tokio::test]
    async fn wechat_ilink_adapter_posts_getupdates_and_persists_context_token() {
        async fn get_updates(
            State(state): State<HttpTestState>,
            headers: AxumHeaderMap,
            body: String,
        ) -> Json<Value> {
            let parsed: Value = serde_json::from_str(&body).expect("json body");
            state.bodies.lock().expect("bodies").push(parsed);
            state.headers.lock().expect("headers").push(
                headers
                    .iter()
                    .filter_map(|(name, value)| {
                        value
                            .to_str()
                            .ok()
                            .map(|value| (name.as_str().to_string(), value.to_string()))
                    })
                    .collect(),
            );
            Json(json!({
                "ret": 0,
                "get_updates_buf": "next",
                "msgs": [{
                    "message_id": "wx_msg_1",
                    "from_user_id": "wx_user",
                    "to_user_id": "account",
                    "context_token": "ctx-token",
                    "item_list": [
                        { "type": 1, "text_item": { "text": "ping" } }
                    ]
                }]
            }))
        }

        let state = HttpTestState::default();
        let base_url = spawn_router(
            Router::new()
                .route("/ilink/bot/getupdates", post(get_updates))
                .with_state(state.clone()),
        )
        .await;
        let temp = tempfile::tempdir().expect("tempdir");
        let context_store_path = temp.path().join("wechat-context.json");
        let adapter = WechatIlinkAdapter::new(WechatIlinkConfig {
            connection_id: Some("wechat".to_string()),
            token: "token".to_string(),
            account_id: "account".to_string(),
            base_url,
            timeout_secs: 1,
            context_store_path: Some(context_store_path.clone()),
        })
        .expect("adapter");

        let messages = adapter.poll().await.expect("poll");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].identity.chat_id, "wx_user");
        assert_eq!(messages[0].text, "ping");

        let bodies = state.bodies.lock().expect("bodies");
        assert_eq!(bodies[0]["get_updates_buf"], "");
        assert_eq!(
            bodies[0]["base_info"]["channel_version"],
            WECHAT_CHANNEL_VERSION
        );
        let headers = state.headers.lock().expect("headers");
        assert_eq!(
            headers[0].get("authorizationtype").map(String::as_str),
            Some("ilink_bot_token")
        );
        assert_eq!(
            headers[0].get("ilink-app-id").map(String::as_str),
            Some(WECHAT_ILINK_APP_ID)
        );
        let persisted = fs::read_to_string(context_store_path).expect("context tokens");
        assert!(persisted.contains("ctx-token"));
    }

    #[tokio::test]
    async fn wechat_ilink_adapter_classifies_session_timeout_as_qr_login_needed() {
        async fn get_updates() -> Json<Value> {
            Json(json!({
                "errcode": -14,
                "errmsg": "session timeout"
            }))
        }

        let base_url =
            spawn_router(Router::new().route("/ilink/bot/getupdates", post(get_updates))).await;
        let adapter = WechatIlinkAdapter::new(WechatIlinkConfig {
            connection_id: Some("wechat".to_string()),
            token: "token".to_string(),
            account_id: "account".to_string(),
            base_url,
            timeout_secs: 1,
            context_store_path: None,
        })
        .expect("adapter");

        let err = adapter.poll().await.expect_err("expired session");
        let message = err.to_string();
        assert!(is_wechat_ilink_session_expired_error(&message));
        assert_eq!(
            wechat_ilink_error_code_from_message(&message),
            Some(WECHAT_SESSION_EXPIRED_ERRCODE)
        );
    }

    #[tokio::test]
    async fn wechat_ilink_health_treats_local_longpoll_timeout_as_empty_poll() {
        async fn get_updates() -> Json<Value> {
            tokio::time::sleep(Duration::from_secs(2)).await;
            Json(json!({ "ret": 0, "errcode": 0, "msgs": [] }))
        }

        let base_url =
            spawn_router(Router::new().route("/ilink/bot/getupdates", post(get_updates))).await;
        let health = check_wechat_ilink_health(&reqwest::Client::new(), &base_url, "token", 1)
            .await
            .expect("health");
        assert!(health.ok);
        assert_eq!(health.reason.as_deref(), Some("polling_empty"));
    }

    #[test]
    fn feishu_event_maps_text_payload() {
        let event: feishu_sdk::event::Event = serde_json::from_value(json!({
            "schema": "2.0",
            "header": {
                "event_id": "evt_1",
                "event_type": "im.message.receive_v1",
                "tenant_key": "tenant"
            },
            "event": {
                "sender": {
                    "sender_id": { "open_id": "ou_user" },
                    "sender_type": "user",
                    "tenant_key": "tenant"
                },
                "message": {
                    "message_id": "om_msg",
                    "chat_id": "oc_chat",
                    "message_type": "text",
                    "content": "{\"text\":\"hello from lark\"}"
                }
            }
        }))
        .expect("event");
        let config = FeishuLarkLongConnectionConfig {
            connection_id: Some("lark".to_string()),
            app_id: "cli_test".to_string(),
            app_secret: "secret".to_string(),
            domain: FeishuLarkDomain::Lark,
            base_url: None,
        };

        let message = feishu_event_to_inbound(&event, &config).expect("message");
        assert_eq!(message.identity.connection_id.as_deref(), Some("lark"));
        assert_eq!(message.identity.platform, "lark");
        assert_eq!(message.identity.workspace_id.as_deref(), Some("tenant"));
        assert_eq!(message.identity.chat_id, "oc_chat");
        assert_eq!(message.identity.user_id.as_deref(), Some("ou_user"));
        assert_eq!(message.text, "hello from lark");
    }

    async fn spawn_router(router: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve");
        });
        format!("http://{addr}")
    }
}
