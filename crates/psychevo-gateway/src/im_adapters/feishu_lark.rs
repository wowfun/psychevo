use super::*;

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

pub(super) fn feishu_event_to_inbound(
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
        attachments: Vec::new(),
        task_key: None,
    })
}

fn feishu_error(err: feishu_sdk::core::Error) -> Error {
    Error::Message(format!("Feishu/Lark adapter failed: {err}"))
}
