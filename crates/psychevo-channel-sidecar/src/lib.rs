use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use psychevo_channel_adapters::im::ImAdapter;
use psychevo_extension_protocol::{
    ChannelConnectionParams, ChannelDescriptor, ChannelPollResult, ChannelSendParams,
    ChannelStartParams, ContributionDescriptors, InitializeParams, InitializeResult,
    PROTOCOL_VERSION, RpcError, RpcRequest, RpcResponse,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, RwLock};

pub trait ChannelAdapterFactory: Send + Sync + 'static {
    fn descriptors(&self) -> Vec<ChannelDescriptor>;

    fn build(
        &self,
        connection_id: String,
        channel: String,
        configuration: Value,
    ) -> BoxFuture<'static, Result<Arc<dyn ImAdapter>>>;

    fn control(&self, method: String, _params: Value) -> BoxFuture<'static, Result<Value>> {
        Box::pin(async move { Err(anyhow!("unsupported Channel control method `{method}`")) })
    }
}

pub async fn run(extension_id: &str, factory: Arc<dyn ChannelAdapterFactory>) -> Result<()> {
    run_io(
        extension_id.to_string(),
        factory,
        BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
    )
    .await
}

async fn run_io<R, W>(
    extension_id: String,
    factory: Arc<dyn ChannelAdapterFactory>,
    reader: R,
    writer: W,
) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut lines = reader.lines();
    let stdout = Arc::new(Mutex::new(writer));
    let initialized = Arc::new(AtomicBool::new(false));
    let adapters = Arc::new(RwLock::new(BTreeMap::<String, Arc<dyn ImAdapter>>::new()));
    while let Some(line) = lines.next_line().await? {
        let request = match serde_json::from_str::<RpcRequest>(&line) {
            Ok(request) => request,
            Err(err) => {
                write_response(
                    &mut *stdout.lock().await,
                    RpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: 0,
                        result: None,
                        error: Some(RpcError {
                            code: -32700,
                            message: format!("invalid request: {err}"),
                        }),
                    },
                )
                .await?;
                continue;
            }
        };
        let id = request.id;
        if request.method == "shutdown" {
            let active = std::mem::take(&mut *adapters.write().await);
            for adapter in active.into_values() {
                adapter.shutdown().await?;
            }
            write_response(&mut *stdout.lock().await, success(id, json!({}))).await?;
            break;
        }
        let request_factory = Arc::clone(&factory);
        let request_adapters = Arc::clone(&adapters);
        let request_initialized = Arc::clone(&initialized);
        let request_stdout = Arc::clone(&stdout);
        let request_extension_id = extension_id.clone();
        tokio::spawn(async move {
            let method = request.method.clone();
            let response = match handle_request(
                &request_extension_id,
                request_factory,
                request_adapters,
                request_initialized,
                request,
            )
            .await
            {
                Ok(value) => success(id, value),
                Err(err) => RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(RpcError {
                        code: if method == "initialize" {
                            -32602
                        } else {
                            -32000
                        },
                        message: format!("{err:#}"),
                    }),
                },
            };
            let _ = write_response(&mut *request_stdout.lock().await, response).await;
        });
    }
    Ok(())
}

async fn handle_request(
    extension_id: &str,
    factory: Arc<dyn ChannelAdapterFactory>,
    adapters: Arc<RwLock<BTreeMap<String, Arc<dyn ImAdapter>>>>,
    initialized: Arc<AtomicBool>,
    request: RpcRequest,
) -> Result<Value> {
    match request.method.as_str() {
        "initialize" => {
            let params: InitializeParams = serde_json::from_value(request.params)?;
            if params.protocol != PROTOCOL_VERSION || params.extension_id != extension_id {
                Err(anyhow!("Extension identity or protocol mismatch"))
            } else if !params.capabilities.channels {
                Err(anyhow!("host did not negotiate Channel capability"))
            } else {
                initialized.store(true, Ordering::Release);
                Ok(serde_json::to_value(InitializeResult {
                    protocol: PROTOCOL_VERSION.to_string(),
                    extension_id: extension_id.to_string(),
                    capabilities: ContributionDescriptors {
                        channels: factory.descriptors(),
                        ..ContributionDescriptors::default()
                    },
                })?)
            }
        }
        _ if !initialized.load(Ordering::Acquire) => Err(anyhow!("Extension is not initialized")),
        "contributions/list" => Ok(serde_json::to_value(ContributionDescriptors {
            channels: factory.descriptors(),
            ..ContributionDescriptors::default()
        })?),
        "channel/start" => {
            let params: ChannelStartParams = serde_json::from_value(request.params)?;
            if !factory
                .descriptors()
                .iter()
                .any(|descriptor| descriptor.channel == params.channel)
            {
                return Err(anyhow!("Channel `{}` is not declared", params.channel));
            }
            let connection_id = params.connection_id;
            let adapter = factory
                .build(connection_id.clone(), params.channel, params.configuration)
                .await?;
            match adapters.write().await.entry(connection_id) {
                Entry::Occupied(entry) => Err(anyhow!(
                    "Channel connection `{}` is already started",
                    entry.key()
                )),
                Entry::Vacant(entry) => {
                    entry.insert(adapter);
                    Ok(json!({}))
                }
            }
        }
        "channel/poll" => {
            let params: ChannelConnectionParams = serde_json::from_value(request.params)?;
            let adapter = adapter(&adapters, &params.connection_id).await?;
            Ok(serde_json::to_value(ChannelPollResult {
                messages: adapter.poll().await?,
            })?)
        }
        "channel/send" => {
            let params: ChannelSendParams = serde_json::from_value(request.params)?;
            let adapter = adapter(&adapters, &params.connection_id).await?;
            adapter.send(params.message).await?;
            Ok(json!({}))
        }
        "channel/stop" => {
            let params: ChannelConnectionParams = serde_json::from_value(request.params)?;
            let adapter = adapters.write().await.remove(&params.connection_id);
            if let Some(adapter) = adapter {
                adapter.shutdown().await?;
            }
            Ok(json!({}))
        }
        method if method.starts_with("channel/") => {
            factory.control(method.to_string(), request.params).await
        }
        _ => Err(anyhow!("method not found")),
    }
}

async fn adapter(
    adapters: &RwLock<BTreeMap<String, Arc<dyn ImAdapter>>>,
    connection_id: &str,
) -> Result<Arc<dyn ImAdapter>> {
    adapters
        .read()
        .await
        .get(connection_id)
        .cloned()
        .ok_or_else(|| anyhow!("Channel connection `{connection_id}` is not started"))
}

fn success(id: u64, result: Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(result),
        error: None,
    }
}

async fn write_response(
    stdout: &mut (impl AsyncWrite + Unpin),
    response: RpcResponse,
) -> Result<()> {
    stdout.write_all(&serde_json::to_vec(&response)?).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use futures::future::BoxFuture;
    use psychevo_channel_adapters::im::{ImAdapter, ImInboundMessage, ImOutboundMessage};
    use psychevo_extension_protocol::ChannelDescriptor;
    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::sync::Notify;

    use super::{ChannelAdapterFactory, run_io};

    struct ConcurrentAdapter {
        poll_started: Arc<Notify>,
        release_poll: Arc<Notify>,
    }

    impl ImAdapter for ConcurrentAdapter {
        fn platform(&self) -> &str {
            "test"
        }

        fn poll(&self) -> BoxFuture<'static, anyhow::Result<Vec<ImInboundMessage>>> {
            let started = Arc::clone(&self.poll_started);
            let release = Arc::clone(&self.release_poll);
            Box::pin(async move {
                started.notify_one();
                release.notified().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(Vec::new())
            })
        }

        fn send(&self, _message: ImOutboundMessage) -> BoxFuture<'static, anyhow::Result<()>> {
            let release = Arc::clone(&self.release_poll);
            Box::pin(async move {
                release.notify_one();
                Ok(())
            })
        }
    }

    struct Factory {
        adapter: Arc<ConcurrentAdapter>,
    }

    impl ChannelAdapterFactory for Factory {
        fn descriptors(&self) -> Vec<ChannelDescriptor> {
            vec![ChannelDescriptor {
                channel: "test".to_string(),
                domains: Vec::new(),
                delivery_capabilities: vec!["poll".to_string(), "text".to_string()],
            }]
        }

        fn build(
            &self,
            _connection_id: String,
            _channel: String,
            _configuration: Value,
        ) -> BoxFuture<'static, anyhow::Result<Arc<dyn ImAdapter>>> {
            let adapter = Arc::clone(&self.adapter);
            Box::pin(async move { Ok(adapter as Arc<dyn ImAdapter>) })
        }
    }

    #[tokio::test]
    async fn send_is_processed_while_poll_is_pending() {
        let adapter = Arc::new(ConcurrentAdapter {
            poll_started: Arc::new(Notify::new()),
            release_poll: Arc::new(Notify::new()),
        });
        let factory = Arc::new(Factory {
            adapter: Arc::clone(&adapter),
        });
        let (client, server) = tokio::io::duplex(16 * 1024);
        let (server_reader, server_writer) = tokio::io::split(server);
        let sidecar = tokio::spawn(run_io(
            "example.channel".to_string(),
            factory,
            BufReader::new(server_reader),
            server_writer,
        ));
        let (client_reader, mut client_writer) = tokio::io::split(client);
        let mut client_reader = BufReader::new(client_reader);

        write_request(
            &mut client_writer,
            1,
            "initialize",
            json!({
                "protocol": "psychevo-extension/1",
                "extensionId": "example.channel",
                "extensionVersion": "local",
                "scope": "profile",
                "packageRoot": "/tmp/package",
                "dataRoot": "/tmp/data",
                "capabilities": { "channels": true }
            }),
        )
        .await;
        assert_eq!(read_response(&mut client_reader).await["id"], 1);
        write_request(
            &mut client_writer,
            2,
            "channel/start",
            json!({
                "connectionId": "test",
                "channel": "test",
                "configuration": {}
            }),
        )
        .await;
        assert_eq!(read_response(&mut client_reader).await["id"], 2);

        write_request(
            &mut client_writer,
            3,
            "channel/poll",
            json!({
                "connectionId": "test"
            }),
        )
        .await;
        adapter.poll_started.notified().await;
        write_request(
            &mut client_writer,
            4,
            "channel/send",
            json!({
                "connectionId": "test",
                "message": {
                    "identity": { "platform": "test", "chatId": "chat" },
                    "threadId": "thread",
                    "text": "outbound"
                }
            }),
        )
        .await;

        let send = tokio::time::timeout(
            Duration::from_millis(500),
            read_response(&mut client_reader),
        )
        .await
        .expect("send response must not wait for poll");
        assert_eq!(send["id"], 4);
        assert_eq!(read_response(&mut client_reader).await["id"], 3);
        write_request(&mut client_writer, 5, "shutdown", json!({})).await;
        assert_eq!(read_response(&mut client_reader).await["id"], 5);
        drop(client_writer);
        sidecar.await.expect("sidecar task").expect("sidecar run");
    }

    async fn write_request(
        writer: &mut (impl tokio::io::AsyncWrite + Unpin),
        id: u64,
        method: &str,
        params: Value,
    ) {
        writer
            .write_all(
                format!(
                    "{}\n",
                    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
                )
                .as_bytes(),
            )
            .await
            .expect("write request");
        writer.flush().await.expect("flush request");
    }

    async fn read_response(reader: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> Value {
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read response");
        serde_json::from_str(&line).expect("response JSON")
    }
}
