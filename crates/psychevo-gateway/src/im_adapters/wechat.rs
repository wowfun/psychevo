use super::util::{now_millis, value_id_to_string};
use super::*;

pub const WECHAT_ILINK_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
pub(super) const WECHAT_ILINK_APP_ID: &str = "bot";
pub(super) const WECHAT_CHANNEL_VERSION: &str = "2.2.0";
const WECHAT_ILINK_CLIENT_VERSION: u32 = (2 << 16) | (2 << 8);
const WECHAT_ITEM_TEXT: i64 = 1;
const WECHAT_ITEM_IMAGE: i64 = 2;
const WECHAT_ITEM_VOICE: i64 = 3;
const WECHAT_ITEM_FILE: i64 = 4;
const WECHAT_ITEM_VIDEO: i64 = 5;
const WECHAT_MSG_TYPE_BOT: i64 = 2;
const WECHAT_MSG_STATE_FINISH: i64 = 2;
const WECHAT_QR_BOT_TYPE: &str = "3";
pub(super) const WECHAT_SESSION_EXPIRED_ERRCODE: i64 = -14;

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

#[cfg(feature = "native-channels")]
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

#[cfg(not(feature = "native-channels"))]
pub fn render_wechat_qr_svg(_value: &str) -> Result<String> {
    Err(Error::Message(
        "native channel QR rendering is not compiled into this build".to_string(),
    ))
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

pub(super) fn wechat_message_to_inbound(
    raw: &Value,
    connection_id: Option<&str>,
    account_id: &str,
) -> Option<ImInboundMessage> {
    let sender = raw.get("from_user_id").and_then(Value::as_str)?.trim();
    if sender.is_empty() || (!account_id.is_empty() && sender == account_id) {
        return None;
    }
    let items = raw.get("item_list").and_then(Value::as_array)?;
    let text = extract_wechat_text(items).trim().to_string();
    let attachments = extract_wechat_media_metadata(items);
    if text.is_empty() && attachments.is_empty() {
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
        attachments,
        task_key: None,
    })
}

fn extract_wechat_text(items: &[Value]) -> String {
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
    chunks.join("\n")
}

fn extract_wechat_media_metadata(items: &[Value]) -> Vec<ImAttachment> {
    let mut attachments = Vec::new();
    for item in items {
        match item.get("type").and_then(Value::as_i64) {
            Some(WECHAT_ITEM_IMAGE) => attachments.push(ImAttachment::MediaMetadata {
                media_kind: "image".to_string(),
                filename: None,
                mime_type: Some("image/*".to_string()),
                size_bytes: wechat_image_size(item),
                reason: "WeChat media download is not enabled yet".to_string(),
            }),
            Some(WECHAT_ITEM_FILE) => {
                let file = item.get("file_item").unwrap_or(item);
                attachments.push(ImAttachment::MediaMetadata {
                    media_kind: "file".to_string(),
                    filename: file
                        .get("file_name")
                        .or_else(|| file.get("filename"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    mime_type: None,
                    size_bytes: file
                        .get("len")
                        .or_else(|| file.get("size"))
                        .and_then(value_to_u64),
                    reason: "WeChat media download is not enabled yet".to_string(),
                });
            }
            Some(WECHAT_ITEM_VOICE) => {
                let voice = item.get("voice_item").unwrap_or(item);
                let transcript = voice
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                attachments.push(ImAttachment::MediaMetadata {
                    media_kind: "voice".to_string(),
                    filename: None,
                    mime_type: Some("audio/*".to_string()),
                    size_bytes: None,
                    reason: transcript
                        .map(|text| format!("voice transcription: {text}"))
                        .unwrap_or_else(|| "WeChat voice download is not enabled yet".to_string()),
                });
            }
            Some(WECHAT_ITEM_VIDEO) => attachments.push(ImAttachment::MediaMetadata {
                media_kind: "video".to_string(),
                filename: None,
                mime_type: Some("video/*".to_string()),
                size_bytes: item
                    .get("video_item")
                    .and_then(|video| video.get("video_size"))
                    .and_then(value_to_u64),
                reason: "WeChat video download is not enabled yet".to_string(),
            }),
            _ => {}
        }
    }
    attachments
}

fn wechat_image_size(item: &Value) -> Option<u64> {
    let image = item.get("image_item").unwrap_or(item);
    [
        "hd_size",
        "mid_size",
        "thumb_size",
        "size",
        "raw_size",
        "rawsize",
    ]
    .into_iter()
    .find_map(|key| image.get(key).and_then(value_to_u64))
}

fn value_to_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
        .or_else(|| value.as_str()?.parse::<u64>().ok())
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
