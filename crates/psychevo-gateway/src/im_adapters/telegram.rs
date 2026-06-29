use super::util::value_id_to_string;
use super::*;

const TELEGRAM_API_BASE: &str = "https://api.telegram.org";

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

pub(super) fn telegram_update_to_message(
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
        attachments: Vec::new(),
        task_key: None,
    })
}
