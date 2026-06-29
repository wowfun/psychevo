use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use psychevo_runtime::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{GatewayImageInput, GatewayInputPart, GatewaySource, GatewaySourceLifetime};

#[path = "im_adapters/mod.rs"]
pub mod adapters;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImIdentity {
    pub connection_id: Option<String>,
    pub platform: String,
    pub domain: Option<String>,
    pub workspace_id: Option<String>,
    pub chat_type: Option<String>,
    pub chat_id: String,
    pub thread_id: Option<String>,
    pub user_id: Option<String>,
    pub operator_id: Option<String>,
    pub reply_to: Option<String>,
}

impl ImIdentity {
    pub fn route_material(&self) -> String {
        [
            self.connection_id.as_deref().unwrap_or(""),
            self.platform.as_str(),
            self.domain.as_deref().unwrap_or(""),
            self.workspace_id.as_deref().unwrap_or(""),
            self.chat_type.as_deref().unwrap_or(""),
            self.chat_id.as_str(),
            self.thread_id.as_deref().unwrap_or(""),
            self.user_id.as_deref().unwrap_or(""),
            self.operator_id.as_deref().unwrap_or(""),
        ]
        .join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImInboundMessage {
    pub identity: ImIdentity,
    pub message_id: String,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<ImAttachment>,
    pub task_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ImAttachment {
    Image {
        path: String,
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        mime_type: Option<String>,
    },
    File {
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        mime_type: Option<String>,
        #[serde(default)]
        size_bytes: Option<u64>,
        #[serde(default)]
        text: Option<String>,
    },
    MediaMetadata {
        media_kind: String,
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        mime_type: Option<String>,
        #[serde(default)]
        size_bytes: Option<u64>,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImOutboundMessage {
    pub identity: ImIdentity,
    pub thread_id: String,
    pub text: String,
}

pub trait ImAdapter: Send + Sync {
    fn platform(&self) -> &str;
    fn poll(&self) -> BoxFuture<'static, Result<Vec<ImInboundMessage>>>;
    fn send(&self, message: ImOutboundMessage) -> BoxFuture<'static, Result<()>>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelAllowlist {
    pub users: BTreeSet<String>,
    pub chats: BTreeSet<String>,
}

impl ChannelAllowlist {
    pub fn new(
        users: impl IntoIterator<Item = String>,
        chats: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            users: users.into_iter().collect(),
            chats: chats.into_iter().collect(),
        }
    }

    pub fn allows(&self, identity: &ImIdentity) -> bool {
        identity
            .user_id
            .as_ref()
            .is_some_and(|user| self.users.contains(user))
            || self.chats.contains(&identity.chat_id)
    }
}

#[derive(Clone)]
pub struct ChannelAdapterBinding {
    pub connection_id: String,
    pub allowlist: ChannelAllowlist,
    adapter: Arc<dyn ImAdapter>,
}

impl ChannelAdapterBinding {
    pub fn new(
        connection_id: impl Into<String>,
        adapter: Arc<dyn ImAdapter>,
        allowlist: ChannelAllowlist,
    ) -> Self {
        Self {
            connection_id: connection_id.into(),
            adapter,
            allowlist,
        }
    }
}

#[derive(Clone, Default)]
pub struct ChannelGateway {
    adapters: Vec<ChannelAdapterBinding>,
}

impl ChannelGateway {
    pub fn new(adapters: Vec<ChannelAdapterBinding>) -> Self {
        Self { adapters }
    }

    pub async fn poll_once(&self) -> Result<Vec<ImInboundMessage>> {
        let mut accepted = Vec::new();
        for binding in &self.adapters {
            for mut message in binding.adapter.poll().await? {
                match message.identity.connection_id.as_deref() {
                    Some(connection_id) if connection_id != binding.connection_id => continue,
                    Some(_) => {}
                    None => message.identity.connection_id = Some(binding.connection_id.clone()),
                }
                if binding.allowlist.allows(&message.identity) {
                    accepted.push(message);
                }
            }
        }
        Ok(accepted)
    }

    pub async fn send(&self, message: ImOutboundMessage) -> Result<()> {
        let connection_id = message.identity.connection_id.as_deref().ok_or_else(|| {
            Error::Message("IM outbound message is missing connection id".to_string())
        })?;
        let binding = self
            .adapters
            .iter()
            .find(|binding| binding.connection_id == connection_id)
            .ok_or_else(|| {
                Error::Message(format!("unknown channel connection `{connection_id}`"))
            })?;
        binding.adapter.send(message).await
    }
}

pub fn gateway_source_for_im(message: &ImInboundMessage) -> GatewaySource {
    let raw_id = stable_source_hash(&message.identity.route_material());
    GatewaySource {
        kind: format!("im.{}", normalize_source_part(&message.identity.platform)),
        raw_id,
        lifetime: GatewaySourceLifetime::Persistent,
        raw_identity: Some(json!({
            "connectionId": message.identity.connection_id,
            "platform": message.identity.platform,
            "domain": message.identity.domain,
            "workspaceId": message.identity.workspace_id,
            "chatType": message.identity.chat_type,
            "chatId": message.identity.chat_id,
            "threadId": message.identity.thread_id,
            "userId": message.identity.user_id,
            "operatorId": message.identity.operator_id,
            "replyTo": message.identity.reply_to,
            "messageId": message.message_id,
            "taskKey": message.task_key,
        })),
        visible_name: Some(im_visible_name(&message.identity)),
    }
}

pub fn gateway_input_parts_for_im(message: &ImInboundMessage) -> Vec<GatewayInputPart> {
    let mut input = Vec::new();
    let text = message.text.trim();
    if !text.is_empty() {
        input.push(GatewayInputPart::Text {
            text: text.to_string(),
        });
    }
    for attachment in &message.attachments {
        match attachment {
            ImAttachment::Image { path, .. } if !path.trim().is_empty() => {
                input.push(GatewayInputPart::Image {
                    input: GatewayImageInput::LocalPath {
                        path: path.to_string(),
                    },
                });
            }
            ImAttachment::Image { .. } => {}
            ImAttachment::File {
                filename,
                mime_type,
                size_bytes,
                text,
            } => {
                input.push(GatewayInputPart::Context {
                    label: channel_file_context_label(filename.as_deref()),
                    text: channel_file_context_text(
                        filename.as_deref(),
                        mime_type.as_deref(),
                        *size_bytes,
                        text.as_deref(),
                    ),
                    visible_to_model: true,
                });
            }
            ImAttachment::MediaMetadata {
                media_kind,
                filename,
                mime_type,
                size_bytes,
                reason,
            } => {
                input.push(GatewayInputPart::Context {
                    label: channel_media_context_label(media_kind, filename.as_deref()),
                    text: channel_media_context_text(
                        media_kind,
                        filename.as_deref(),
                        mime_type.as_deref(),
                        *size_bytes,
                        reason,
                    ),
                    visible_to_model: true,
                });
            }
        }
    }
    input
}

pub fn im_task_route_key(message: &ImInboundMessage) -> String {
    let task = message.task_key.as_deref().unwrap_or("default");
    format!(
        "im:{}:{}",
        normalize_source_part(&message.identity.platform),
        stable_source_hash(&format!("{}\n{task}", message.identity.route_material()))
    )
}

fn channel_file_context_label(filename: Option<&str>) -> String {
    filename
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("Attachment: {value}"))
        .unwrap_or_else(|| "Attachment".to_string())
}

fn channel_media_context_label(media_kind: &str, filename: Option<&str>) -> String {
    filename
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("Attachment: {value}"))
        .unwrap_or_else(|| format!("Attachment: {media_kind}"))
}

fn channel_file_context_text(
    filename: Option<&str>,
    mime_type: Option<&str>,
    size_bytes: Option<u64>,
    text: Option<&str>,
) -> String {
    let mut lines = Vec::new();
    lines.push("Attached file metadata:".to_string());
    if let Some(filename) = filename.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(format!("filename: {filename}"));
    }
    if let Some(mime_type) = mime_type.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(format!("mime_type: {mime_type}"));
    }
    if let Some(size_bytes) = size_bytes {
        lines.push(format!("size_bytes: {size_bytes}"));
    }
    if let Some(text) = text.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(String::new());
        lines.push("Extracted text:".to_string());
        lines.push(text.to_string());
    } else {
        lines.push("content: not extracted".to_string());
    }
    lines.join("\n")
}

fn channel_media_context_text(
    media_kind: &str,
    filename: Option<&str>,
    mime_type: Option<&str>,
    size_bytes: Option<u64>,
    reason: &str,
) -> String {
    let mut lines = Vec::new();
    lines.push("Attached media metadata:".to_string());
    lines.push(format!("kind: {media_kind}"));
    if let Some(filename) = filename.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(format!("filename: {filename}"));
    }
    if let Some(mime_type) = mime_type.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(format!("mime_type: {mime_type}"));
    }
    if let Some(size_bytes) = size_bytes {
        lines.push(format!("size_bytes: {size_bytes}"));
    }
    lines.push(format!("content: {reason}"));
    lines.join("\n")
}

#[derive(Debug, Default, Clone)]
pub struct FakeImAdapter {
    platform: String,
    inbound: Arc<Mutex<Vec<ImInboundMessage>>>,
    outbound: Arc<Mutex<Vec<ImOutboundMessage>>>,
}

impl FakeImAdapter {
    pub fn new(platform: impl Into<String>) -> Self {
        Self {
            platform: platform.into(),
            inbound: Arc::new(Mutex::new(Vec::new())),
            outbound: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn push(&self, message: ImInboundMessage) {
        self.inbound
            .lock()
            .expect("fake im inbound lock poisoned")
            .push(message);
    }

    pub fn sent(&self) -> Vec<ImOutboundMessage> {
        self.outbound
            .lock()
            .expect("fake im outbound lock poisoned")
            .clone()
    }
}

impl ImAdapter for FakeImAdapter {
    fn platform(&self) -> &str {
        &self.platform
    }

    fn poll(&self) -> BoxFuture<'static, Result<Vec<ImInboundMessage>>> {
        let inbound = Arc::clone(&self.inbound);
        Box::pin(async move {
            let mut guard = inbound.lock().expect("fake im inbound lock poisoned");
            Ok(std::mem::take(&mut *guard))
        })
    }

    fn send(&self, message: ImOutboundMessage) -> BoxFuture<'static, Result<()>> {
        let outbound = Arc::clone(&self.outbound);
        Box::pin(async move {
            if message.text.trim().is_empty() {
                return Err(Error::Message(
                    "IM outbound text cannot be empty".to_string(),
                ));
            }
            outbound
                .lock()
                .expect("fake im outbound lock poisoned")
                .push(message);
            Ok(())
        })
    }
}

fn im_visible_name(identity: &ImIdentity) -> String {
    match (
        &identity.workspace_id,
        &identity.thread_id,
        &identity.user_id,
    ) {
        (Some(workspace), Some(thread), Some(user)) => {
            format!(
                "{} {workspace}/{}/{}",
                identity.platform,
                identity.chat_id,
                thread_or_user(thread, user)
            )
        }
        (Some(workspace), _, Some(user)) => {
            format!(
                "{} {workspace}/{}/{}",
                identity.platform, identity.chat_id, user
            )
        }
        (_, Some(thread), _) => format!("{} {}/{}", identity.platform, identity.chat_id, thread),
        _ => format!("{} {}", identity.platform, identity.chat_id),
    }
}

fn thread_or_user(thread: &str, user: &str) -> String {
    if thread == user {
        thread.to_string()
    } else {
        format!("{thread}/{user}")
    }
}

fn normalize_source_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '.'
            }
        })
        .collect()
}

fn stable_source_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inbound(chat_id: &str, user_id: &str, task_key: Option<&str>) -> ImInboundMessage {
        ImInboundMessage {
            identity: ImIdentity {
                connection_id: None,
                platform: "Fake Chat".to_string(),
                domain: Some("fake".to_string()),
                workspace_id: Some("team-raw".to_string()),
                chat_type: Some("group".to_string()),
                chat_id: chat_id.to_string(),
                thread_id: Some("thread-raw".to_string()),
                user_id: Some(user_id.to_string()),
                operator_id: None,
                reply_to: None,
            },
            message_id: "message-raw".to_string(),
            text: "hello".to_string(),
            attachments: Vec::new(),
            task_key: task_key.map(ToString::to_string),
        }
    }

    #[test]
    fn im_source_is_persistent_and_does_not_expose_raw_ids_as_key() {
        let message = inbound("chat-raw", "user-raw", Some("task"));
        let source = gateway_source_for_im(&message);

        assert_eq!(source.kind, "im.fake.chat");
        assert_eq!(source.lifetime, GatewaySourceLifetime::Persistent);
        assert!(!source.source_key().0.contains("chat-raw"));
        assert!(!source.source_key().0.contains("user-raw"));
        assert_eq!(source.raw_identity.as_ref().unwrap()["chatId"], "chat-raw");
    }

    #[test]
    fn im_route_key_is_task_scoped_and_deterministic() {
        let first = inbound("chat-raw", "user-raw", Some("task-a"));
        let second = inbound("chat-raw", "user-raw", Some("task-a"));
        let other_task = inbound("chat-raw", "user-raw", Some("task-b"));

        assert_eq!(im_task_route_key(&first), im_task_route_key(&second));
        assert_ne!(im_task_route_key(&first), im_task_route_key(&other_task));
    }

    #[test]
    fn im_input_parts_include_text_images_and_file_context() {
        let mut message = inbound("chat-raw", "user-raw", None);
        message.text = "look at these".to_string();
        message.attachments = vec![
            ImAttachment::Image {
                path: "/tmp/screenshot.png".to_string(),
                filename: Some("screenshot.png".to_string()),
                mime_type: Some("image/png".to_string()),
            },
            ImAttachment::File {
                filename: Some("notes.txt".to_string()),
                mime_type: Some("text/plain".to_string()),
                size_bytes: Some(12),
                text: Some("hello notes".to_string()),
            },
        ];

        let parts = gateway_input_parts_for_im(&message);

        assert_eq!(parts.len(), 3);
        match &parts[0] {
            GatewayInputPart::Text { text } => assert_eq!(text, "look at these"),
            other => panic!("expected text input, got {other:?}"),
        }
        match &parts[1] {
            GatewayInputPart::Image {
                input: GatewayImageInput::LocalPath { path },
            } => assert_eq!(path, "/tmp/screenshot.png"),
            other => panic!("expected local image input, got {other:?}"),
        }
        match &parts[2] {
            GatewayInputPart::Context {
                label,
                text,
                visible_to_model,
            } => {
                assert_eq!(label, "Attachment: notes.txt");
                assert_eq!(
                    text,
                    "Attached file metadata:\nfilename: notes.txt\nmime_type: text/plain\nsize_bytes: 12\n\nExtracted text:\nhello notes"
                );
                assert!(*visible_to_model);
            }
            other => panic!("expected context input, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fake_adapter_drains_inbound_and_records_outbound() {
        let adapter = FakeImAdapter::new("fake");
        let message = inbound("chat", "user", None);
        adapter.push(message.clone());

        assert_eq!(adapter.poll().await.unwrap(), vec![message.clone()]);
        assert!(adapter.poll().await.unwrap().is_empty());

        adapter
            .send(ImOutboundMessage {
                identity: message.identity,
                thread_id: "thread".to_string(),
                text: "done".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(adapter.sent().len(), 1);
    }

    #[tokio::test]
    async fn channel_gateway_filters_inbound_fail_closed_and_stamps_connection() {
        let adapter = FakeImAdapter::new("telegram");
        let allowed = inbound("chat", "allowed-user", None);
        let denied = inbound("chat", "denied-user", None);
        adapter.push(allowed.clone());
        adapter.push(denied);
        let gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
            "release",
            Arc::new(adapter),
            ChannelAllowlist::new(["allowed-user".to_string()], Vec::<String>::new()),
        )]);

        let messages = gateway.poll_once().await.unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].identity.connection_id.as_deref(),
            Some("release")
        );
        assert_eq!(
            messages[0].identity.user_id.as_deref(),
            Some("allowed-user")
        );
        let source = gateway_source_for_im(&messages[0]);
        assert!(!source.source_key().0.contains("allowed-user"));
    }

    #[tokio::test]
    async fn channel_gateway_routes_outbound_by_connection() {
        let release = FakeImAdapter::new("telegram");
        let alerts = FakeImAdapter::new("lark");
        let gateway = ChannelGateway::new(vec![
            ChannelAdapterBinding::new(
                "release",
                Arc::new(release.clone()),
                ChannelAllowlist::new(Vec::<String>::new(), ["chat".to_string()]),
            ),
            ChannelAdapterBinding::new(
                "alerts",
                Arc::new(alerts.clone()),
                ChannelAllowlist::new(Vec::<String>::new(), ["chat".to_string()]),
            ),
        ]);
        let mut message = inbound("chat", "user", None);
        message.identity.connection_id = Some("alerts".to_string());

        gateway
            .send(ImOutboundMessage {
                identity: message.identity,
                thread_id: "thread".to_string(),
                text: "done".to_string(),
            })
            .await
            .unwrap();

        assert!(release.sent().is_empty());
        assert_eq!(alerts.sent().len(), 1);
    }
}
