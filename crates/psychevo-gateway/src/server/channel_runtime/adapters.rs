use std::sync::Arc;

use futures::future::BoxFuture;
use psychevo::Error;
use psychevo::config::ChannelRuntimeConnection;
use psychevo::extensions::protocol::{
    ChannelAttachment, ChannelConnectionParams, ChannelIdentity, ChannelOutboundMessage,
    ChannelSendParams, ChannelStartParams,
};
use psychevo::extensions::{ExtensionLease, ExtensionRuntime, ExtensionStore};
use serde_json::Value;
use serde_json::json;
use tokio::sync::RwLock;

use crate::im::{
    ChannelAdapterBinding, ChannelAllowlist, ChannelGateway, ImAdapter, ImAttachment, ImIdentity,
    ImInboundMessage, ImOutboundMessage,
};

use super::super::binding::WebState;
use super::super::extension_management::ExtensionActivityGuard;

pub(super) async fn build_channel_gateway(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
) -> psychevo::Result<ChannelGateway> {
    let registration = state.inner.extension_app_leases.begin_activity();
    let (record, manifest) = resolve_channel_extension(state, &connection.channel)?;
    let activity = registration.register(
        format!("{}@{}", record.id, record.scope.as_str()),
        "Channel",
    );
    let runtime = channel_runtime(state, record, manifest)?;
    let lease = runtime.acquire().await?;
    lease
        .channel_start(ChannelStartParams {
            connection_id: connection.id.clone(),
            channel: connection.channel.clone(),
            configuration: json!({
                "domain": connection.domain,
                "transport": connection.transport,
                "credential": connection.credential,
                "appId": connection.app_id,
                "appSecret": connection.app_secret,
                "accountId": connection.account_id,
                "baseUrl": connection.base_url,
            }),
        })
        .await?;
    let adapter: Arc<dyn ImAdapter> = Arc::new(ExtensionChannelAdapter {
        platform: connection.channel.clone(),
        connection_id: connection.id.clone(),
        lease: Arc::new(RwLock::new(Some(lease))),
        _activity: activity,
    });
    Ok(ChannelGateway::new(vec![ChannelAdapterBinding::new(
        connection.id.clone(),
        adapter,
        ChannelAllowlist::new(
            connection.allow_users.clone(),
            connection.allow_groups.clone(),
        ),
    )]))
}

pub(in crate::server) async fn channel_control(
    state: &WebState,
    channel: &str,
    method: &str,
    params: Value,
) -> psychevo::Result<Value> {
    let registration = state.inner.extension_app_leases.begin_activity();
    let (record, manifest) = resolve_channel_extension(state, channel)?;
    let _activity = registration.register(
        format!("{}@{}", record.id, record.scope.as_str()),
        "Channel control",
    );
    let runtime = channel_runtime(state, record, manifest)?;
    let lease = runtime.acquire().await?;
    let result = lease.channel_control(method, params).await;
    let release = lease.release().await;
    let value = result?;
    release?;
    Ok(value)
}

fn resolve_channel_extension(
    state: &WebState,
    channel: &str,
) -> psychevo::Result<(
    psychevo::extensions::ExtensionInstallRecord,
    psychevo::extensions::ExtensionManifest,
)> {
    let store = ExtensionStore::new(&state.inner.home, &state.inner.cwd);
    store.resolve_channel_extension(channel)
}

fn channel_runtime(
    state: &WebState,
    record: psychevo::extensions::ExtensionInstallRecord,
    manifest: psychevo::extensions::ExtensionManifest,
) -> psychevo::Result<Arc<ExtensionRuntime>> {
    state
        .inner
        .extension_app_leases
        .runtime_for(record, manifest, &state.inner.inherited_env)
}

struct ExtensionChannelAdapter {
    platform: String,
    connection_id: String,
    lease: Arc<RwLock<Option<ExtensionLease>>>,
    _activity: ExtensionActivityGuard,
}

impl ImAdapter for ExtensionChannelAdapter {
    fn platform(&self) -> &str {
        &self.platform
    }

    fn poll(&self) -> BoxFuture<'static, psychevo::Result<Vec<ImInboundMessage>>> {
        let lease = Arc::clone(&self.lease);
        let connection_id = self.connection_id.clone();
        Box::pin(async move {
            let lease = lease.read().await;
            let lease = lease
                .as_ref()
                .ok_or_else(|| Error::Message("Channel Extension lease is closed".to_string()))?;
            let result = lease
                .channel_poll(ChannelConnectionParams { connection_id })
                .await?;
            Ok(result.messages.into_iter().map(inbound_message).collect())
        })
    }

    fn send(&self, message: ImOutboundMessage) -> BoxFuture<'static, psychevo::Result<()>> {
        let lease = Arc::clone(&self.lease);
        let connection_id = self.connection_id.clone();
        Box::pin(async move {
            let lease = lease.read().await;
            let lease = lease
                .as_ref()
                .ok_or_else(|| Error::Message("Channel Extension lease is closed".to_string()))?;
            lease
                .channel_send(ChannelSendParams {
                    connection_id,
                    message: ChannelOutboundMessage {
                        identity: channel_identity(message.identity),
                        thread_id: message.thread_id,
                        text: message.text,
                    },
                })
                .await
        })
    }

    fn shutdown(&self) -> BoxFuture<'static, psychevo::Result<()>> {
        let lease = Arc::clone(&self.lease);
        let connection_id = self.connection_id.clone();
        Box::pin(async move {
            if let Some(lease) = lease.write().await.take() {
                let stop = lease
                    .channel_stop(ChannelConnectionParams { connection_id })
                    .await;
                let release = lease.release().await;
                stop?;
                release?;
            }
            Ok(())
        })
    }
}

fn inbound_message(
    message: psychevo::extensions::protocol::ChannelInboundMessage,
) -> ImInboundMessage {
    ImInboundMessage {
        identity: im_identity(message.identity),
        message_id: message.message_id,
        text: message.text,
        attachments: message.attachments.into_iter().map(im_attachment).collect(),
        task_key: message.task_key,
    }
}

fn im_identity(identity: ChannelIdentity) -> ImIdentity {
    ImIdentity {
        connection_id: identity.connection_id,
        platform: identity.platform,
        domain: identity.domain,
        workspace_id: identity.workspace_id,
        chat_type: identity.chat_type,
        chat_id: identity.chat_id,
        thread_id: identity.thread_id,
        user_id: identity.user_id,
        operator_id: identity.operator_id,
        reply_to: identity.reply_to,
    }
}

fn channel_identity(identity: ImIdentity) -> ChannelIdentity {
    ChannelIdentity {
        connection_id: identity.connection_id,
        platform: identity.platform,
        domain: identity.domain,
        workspace_id: identity.workspace_id,
        chat_type: identity.chat_type,
        chat_id: identity.chat_id,
        thread_id: identity.thread_id,
        user_id: identity.user_id,
        operator_id: identity.operator_id,
        reply_to: identity.reply_to,
    }
}

fn im_attachment(attachment: ChannelAttachment) -> ImAttachment {
    match attachment {
        ChannelAttachment::Image {
            path,
            filename,
            mime_type,
        } => ImAttachment::Image {
            path,
            filename,
            mime_type,
        },
        ChannelAttachment::File {
            filename,
            mime_type,
            size_bytes,
            text,
        } => ImAttachment::File {
            filename,
            mime_type,
            size_bytes,
            text,
        },
        ChannelAttachment::MediaMetadata {
            media_kind,
            filename,
            mime_type,
            size_bytes,
            reason,
        } => ImAttachment::MediaMetadata {
            media_kind,
            filename,
            mime_type,
            size_bytes,
            reason,
        },
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::time::Duration;

    use psychevo::extensions::protocol::{ChannelStartParams, HostCapabilities};
    use psychevo::extensions::{
        ExtensionHostMode, ExtensionRuntime, ExtensionScope, ExtensionStore,
        load_extension_manifest,
    };
    use serde_json::json;
    use tempfile::TempDir;
    use tokio::sync::RwLock;

    use super::{ExtensionChannelAdapter, ImAdapter, ImIdentity, ImOutboundMessage};

    #[tokio::test]
    async fn gateway_adapter_does_not_serialize_send_behind_poll() {
        let profile = TempDir::new().expect("profile");
        let source = TempDir::new().expect("source");
        write_channel_extension(source.path());
        let store = ExtensionStore::new(profile.path(), source.path());
        let record = store
            .install_local(source.path(), ExtensionScope::Profile)
            .expect("install");
        let manifest = load_extension_manifest(source.path()).expect("manifest");
        let runtime = ExtensionRuntime::with_capabilities(
            record,
            manifest,
            BTreeMap::new(),
            ExtensionHostMode::Leased {
                idle_timeout: Duration::from_secs(300),
            },
            HostCapabilities {
                channels: true,
                ..HostCapabilities::default()
            },
        )
        .expect("runtime");
        let lease = runtime.acquire().await.expect("lease");
        lease
            .channel_start(ChannelStartParams {
                connection_id: "test".to_string(),
                channel: "test".to_string(),
                configuration: json!({ "blockPollUntilSend": true }),
            })
            .await
            .expect("start");
        let adapter = ExtensionChannelAdapter {
            platform: "test".to_string(),
            connection_id: "test".to_string(),
            lease: Arc::new(RwLock::new(Some(lease))),
            _activity: test_activity_guard(),
        };

        let poll = adapter.poll();
        let send = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            adapter
                .send(ImOutboundMessage {
                    identity: ImIdentity {
                        connection_id: Some("test".to_string()),
                        platform: "test".to_string(),
                        domain: None,
                        workspace_id: None,
                        chat_type: None,
                        chat_id: "chat".to_string(),
                        thread_id: None,
                        user_id: None,
                        operator_id: None,
                        reply_to: None,
                    },
                    thread_id: "thread".to_string(),
                    text: "release poll".to_string(),
                })
                .await
        };
        let (polled, sent) =
            tokio::time::timeout(Duration::from_secs(1), async { tokio::join!(poll, send) })
                .await
                .expect("Gateway send must not queue behind Channel long poll");
        assert_eq!(polled.expect("poll").len(), 1);
        sent.expect("send");
        adapter.shutdown().await.expect("shutdown");
        runtime.shutdown().await.expect("runtime shutdown");
    }

    fn test_activity_guard() -> super::ExtensionActivityGuard {
        let store = super::super::super::extension_management::ExtensionAppLeaseStore::default();
        store
            .begin_activity()
            .register("example.channel@profile".to_string(), "test")
    }

    fn write_channel_extension(root: &std::path::Path) {
        fs::write(
            root.join("psychevo.extension.json"),
            r#"{
              "schemaVersion": 1,
              "id": "example.channel",
              "version": "local",
              "runtime": {
                "protocol": "psychevo-extension/1",
                "executable": "./sidecar.py"
              },
              "contributions": {
                "channels": [{
                  "channel": "test",
                  "deliveryCapabilities": ["poll", "text"]
                }]
              }
            }"#,
        )
        .expect("manifest");
        let sidecar = root.join("sidecar.py");
        fs::write(
            &sidecar,
            include_str!("../../../../psychevo/tests/fixtures/extension_echo_sidecar.py"),
        )
        .expect("sidecar");
        let mut permissions = fs::metadata(&sidecar).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(sidecar, permissions).expect("chmod");
    }
}
