use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

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
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

const MAX_REQUEST_FRAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_IN_FLIGHT_REQUESTS: usize = 64;
const REQUEST_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

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
    run_io_with_limits(
        extension_id,
        factory,
        reader,
        writer,
        MAX_REQUEST_FRAME_BYTES,
        MAX_IN_FLIGHT_REQUESTS,
    )
    .await
}

async fn run_io_with_limits<R, W>(
    extension_id: String,
    factory: Arc<dyn ChannelAdapterFactory>,
    mut reader: R,
    writer: W,
    max_frame_bytes: usize,
    max_in_flight: usize,
) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let stdout = Arc::new(Mutex::new(writer));
    let initialized = Arc::new(AtomicBool::new(false));
    let adapters = Arc::new(RwLock::new(BTreeMap::<String, ConnectionState>::new()));
    let next_start_attempt = Arc::new(AtomicU64::new(1));
    let max_in_flight = max_in_flight.max(1);
    let admission = Arc::new(Semaphore::new(max_in_flight));
    let poll_admission = Arc::new(Semaphore::new(long_poll_capacity(max_in_flight)));
    let cancellation = CancellationToken::new();
    let mut tasks = JoinSet::new();
    let owner_result = async {
        let mut shutdown_id = None;
        loop {
            let frame = tokio::select! {
                biased;
                completed = tasks.join_next(), if !tasks.is_empty() => {
                    observe_request_task(completed)?;
                    continue;
                }
                frame = read_bounded_frame(&mut reader, max_frame_bytes) => frame?,
            };
            let Some(frame) = frame else {
                break;
            };
            let Frame::Bytes(frame) = frame else {
                write_response(
                    &mut *stdout.lock().await,
                    error_response(
                        0,
                        -32700,
                        format!("request frame exceeds {max_frame_bytes} bytes"),
                    ),
                )
                .await?;
                continue;
            };
            let request = match serde_json::from_slice::<RpcRequest>(&frame) {
                Ok(request) => request,
                Err(err) => {
                    write_response(
                        &mut *stdout.lock().await,
                        error_response(0, -32700, format!("invalid request: {err}")),
                    )
                    .await?;
                    continue;
                }
            };
            let id = request.id;
            if request.method == "shutdown" {
                shutdown_id = Some(id);
                break;
            }
            let permit = match Arc::clone(&admission).try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    write_response(
                        &mut *stdout.lock().await,
                        error_response(
                            id,
                            -32001,
                            format!("sidecar has {max_in_flight} requests in flight"),
                        ),
                    )
                    .await?;
                    continue;
                }
            };
            let poll_permit = if request.method == "channel/poll" {
                match Arc::clone(&poll_admission).try_acquire_owned() {
                    Ok(permit) => Some(permit),
                    Err(_) => {
                        drop(permit);
                        write_response(
                            &mut *stdout.lock().await,
                            error_response(
                                id,
                                -32001,
                                "sidecar long-poll capacity is full".to_string(),
                            ),
                        )
                        .await?;
                        continue;
                    }
                }
            } else {
                None
            };
            let request_factory = Arc::clone(&factory);
            let request_adapters = Arc::clone(&adapters);
            let request_initialized = Arc::clone(&initialized);
            let request_stdout = Arc::clone(&stdout);
            let request_extension_id = extension_id.clone();
            let request_cancellation = cancellation.clone();
            let request_next_start_attempt = Arc::clone(&next_start_attempt);
            tasks.spawn(async move {
            let _permit = permit;
            let _poll_permit = poll_permit;
            let method = request.method.clone();
            let handled = tokio::select! {
                biased;
                _ = request_cancellation.cancelled() => Err(anyhow!("sidecar is shutting down")),
                result = handle_request(
                    &request_extension_id,
                    request_factory,
                    request_adapters,
                    request_initialized,
                    request_next_start_attempt,
                    request,
                ) => result,
            };
            let response = match handled {
                Ok(value) => success(id, value),
                Err(err) => error_response(
                    id,
                    if method == "initialize" {
                        -32602
                    } else {
                        -32000
                    },
                    format!("{err:#}"),
                ),
            };
            write_response(&mut *request_stdout.lock().await, response).await
        });
        }
        Result::<Option<u64>>::Ok(shutdown_id)
    }
    .await;
    cancellation.cancel();
    let drain_result = drain_request_tasks(&mut tasks).await;
    let shutdown_result = shutdown_connections(&adapters).await;
    let shutdown_id = owner_result?;
    drain_result?;
    shutdown_result?;
    if let Some(id) = shutdown_id {
        write_response(&mut *stdout.lock().await, success(id, json!({}))).await?;
    }
    Ok(())
}

fn long_poll_capacity(max_in_flight: usize) -> usize {
    let control_reserve = (max_in_flight / 4).max(1);
    max_in_flight.saturating_sub(control_reserve).max(1)
}

enum Frame {
    Bytes(Vec<u8>),
    TooLarge,
}

async fn read_bounded_frame(
    reader: &mut (impl AsyncBufRead + Unpin),
    max_bytes: usize,
) -> Result<Option<Frame>> {
    let mut frame = Vec::with_capacity(max_bytes.min(8 * 1024));
    let mut too_large = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if frame.is_empty() && !too_large {
                return Ok(None);
            }
            return Ok(Some(if too_large {
                Frame::TooLarge
            } else {
                Frame::Bytes(frame)
            }));
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        let content_len = newline.unwrap_or(take);
        if !too_large {
            if frame.len().saturating_add(content_len) > max_bytes {
                frame.clear();
                too_large = true;
            } else {
                frame.extend_from_slice(&available[..content_len]);
            }
        }
        reader.consume(take);
        if newline.is_some() {
            if frame.last() == Some(&b'\r') {
                frame.pop();
            }
            return Ok(Some(if too_large {
                Frame::TooLarge
            } else {
                Frame::Bytes(frame)
            }));
        }
    }
}

fn observe_request_task(
    completed: Option<std::result::Result<Result<()>, tokio::task::JoinError>>,
) -> Result<()> {
    match completed {
        Some(Ok(result)) => result,
        Some(Err(error)) => Err(anyhow!("sidecar request task failed: {error}")),
        None => Ok(()),
    }
}

async fn drain_request_tasks(tasks: &mut JoinSet<Result<()>>) -> Result<()> {
    let drain = async {
        while let Some(completed) = tasks.join_next().await {
            observe_request_task(Some(completed))?;
        }
        Result::<()>::Ok(())
    };
    match tokio::time::timeout(REQUEST_DRAIN_TIMEOUT, drain).await {
        Ok(result) => result,
        Err(_) => {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            Ok(())
        }
    }
}

async fn shutdown_connections(adapters: &RwLock<BTreeMap<String, ConnectionState>>) -> Result<()> {
    let active = std::mem::take(&mut *adapters.write().await);
    for state in active.into_values() {
        match state {
            ConnectionState::Starting { cancel, .. } => cancel.cancel(),
            ConnectionState::Running { adapter, cancel } => {
                cancel.cancel();
                adapter.shutdown().await?;
            }
        }
    }
    Ok(())
}

enum ConnectionState {
    Starting {
        attempt: u64,
        cancel: CancellationToken,
    },
    Running {
        adapter: Arc<dyn ImAdapter>,
        cancel: CancellationToken,
    },
}

async fn handle_request(
    extension_id: &str,
    factory: Arc<dyn ChannelAdapterFactory>,
    adapters: Arc<RwLock<BTreeMap<String, ConnectionState>>>,
    initialized: Arc<AtomicBool>,
    next_start_attempt: Arc<AtomicU64>,
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
            let attempt = next_start_attempt.fetch_add(1, Ordering::Relaxed);
            let cancel = CancellationToken::new();
            {
                let mut adapters = adapters.write().await;
                if adapters.contains_key(&connection_id) {
                    return Err(anyhow!(
                        "Channel connection `{connection_id}` is already started"
                    ));
                }
                adapters.insert(
                    connection_id.clone(),
                    ConnectionState::Starting {
                        attempt,
                        cancel: cancel.clone(),
                    },
                );
            }
            let built = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(anyhow!(
                    "Channel connection `{connection_id}` start was cancelled"
                )),
                result = factory.build(
                    connection_id.clone(),
                    params.channel,
                    params.configuration,
                ) => result,
            };
            let adapter = match built {
                Ok(adapter) => adapter,
                Err(error) => {
                    remove_start_attempt(&adapters, &connection_id, attempt).await;
                    return Err(error);
                }
            };
            let published = {
                let mut adapters = adapters.write().await;
                if matches!(
                    adapters.get(&connection_id),
                    Some(ConnectionState::Starting {
                        attempt: current_attempt,
                        ..
                    }) if *current_attempt == attempt
                ) {
                    adapters.insert(
                        connection_id.clone(),
                        ConnectionState::Running {
                            adapter: adapter.clone(),
                            cancel: cancel.clone(),
                        },
                    );
                    true
                } else {
                    false
                }
            };
            if !published {
                adapter.shutdown().await?;
                return Err(anyhow!(
                    "Channel connection `{connection_id}` start became stale"
                ));
            }
            Ok(json!({}))
        }
        "channel/poll" => {
            let params: ChannelConnectionParams = serde_json::from_value(request.params)?;
            let (adapter, cancel) = adapter(&adapters, &params.connection_id).await?;
            Ok(serde_json::to_value(ChannelPollResult {
                messages: tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(anyhow!(
                        "Channel connection `{}` was stopped",
                        params.connection_id
                    )),
                    result = adapter.poll() => result?,
                },
            })?)
        }
        "channel/send" => {
            let params: ChannelSendParams = serde_json::from_value(request.params)?;
            let (adapter, cancel) = adapter(&adapters, &params.connection_id).await?;
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(anyhow!(
                    "Channel connection `{}` was stopped",
                    params.connection_id
                )),
                result = adapter.send(params.message) => result?,
            }
            Ok(json!({}))
        }
        "channel/stop" => {
            let params: ChannelConnectionParams = serde_json::from_value(request.params)?;
            let state = adapters.write().await.remove(&params.connection_id);
            if let Some(state) = state {
                match state {
                    ConnectionState::Starting { cancel, .. } => cancel.cancel(),
                    ConnectionState::Running { adapter, cancel } => {
                        cancel.cancel();
                        adapter.shutdown().await?;
                    }
                }
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
    adapters: &RwLock<BTreeMap<String, ConnectionState>>,
    connection_id: &str,
) -> Result<(Arc<dyn ImAdapter>, CancellationToken)> {
    match adapters.read().await.get(connection_id) {
        Some(ConnectionState::Running { adapter, cancel }) => Ok((adapter.clone(), cancel.clone())),
        Some(ConnectionState::Starting { .. }) => Err(anyhow!(
            "Channel connection `{connection_id}` is still starting"
        )),
        None => Err(anyhow!(
            "Channel connection `{connection_id}` is not started"
        )),
    }
}

async fn remove_start_attempt(
    adapters: &RwLock<BTreeMap<String, ConnectionState>>,
    connection_id: &str,
    attempt: u64,
) {
    let mut adapters = adapters.write().await;
    if matches!(
        adapters.get(connection_id),
        Some(ConnectionState::Starting {
            attempt: current_attempt,
            ..
        }) if *current_attempt == attempt
    ) {
        adapters.remove(connection_id);
    }
}

fn success(id: u64, result: Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(result),
        error: None,
    }
}

fn error_response(id: u64, code: i64, message: String) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(RpcError { code, message }),
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
    use std::io;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};
    use std::time::Duration;

    use futures::future::BoxFuture;
    use psychevo_channel_adapters::im::{ImAdapter, ImInboundMessage, ImOutboundMessage};
    use psychevo_extension_protocol::ChannelDescriptor;
    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::sync::Notify;

    use super::{ChannelAdapterFactory, run_io, run_io_with_limits};

    struct ConcurrentAdapter {
        poll_started: Arc<Notify>,
        release_poll: Arc<Notify>,
        poll_count: Arc<AtomicUsize>,
        shutdown_count: Arc<AtomicUsize>,
    }

    impl ImAdapter for ConcurrentAdapter {
        fn platform(&self) -> &str {
            "test"
        }

        fn poll(&self) -> BoxFuture<'static, anyhow::Result<Vec<ImInboundMessage>>> {
            let started = Arc::clone(&self.poll_started);
            let release = Arc::clone(&self.release_poll);
            let count = Arc::clone(&self.poll_count);
            Box::pin(async move {
                count.fetch_add(1, Ordering::Relaxed);
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

        fn shutdown(&self) -> BoxFuture<'static, anyhow::Result<()>> {
            let count = Arc::clone(&self.shutdown_count);
            Box::pin(async move {
                count.fetch_add(1, Ordering::Relaxed);
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

    struct BlockingFactory {
        adapter: Arc<ConcurrentAdapter>,
        build_started: Arc<Notify>,
        release_build: Arc<Notify>,
        build_count: Arc<AtomicUsize>,
    }

    struct FailingWriter;

    impl tokio::io::AsyncWrite for FailingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "test writer failed",
            )))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl ChannelAdapterFactory for BlockingFactory {
        fn descriptors(&self) -> Vec<ChannelDescriptor> {
            vec![ChannelDescriptor {
                channel: "test".to_string(),
                domains: Vec::new(),
                delivery_capabilities: vec!["poll".to_string()],
            }]
        }

        fn build(
            &self,
            _connection_id: String,
            _channel: String,
            _configuration: Value,
        ) -> BoxFuture<'static, anyhow::Result<Arc<dyn ImAdapter>>> {
            let adapter = Arc::clone(&self.adapter);
            let started = Arc::clone(&self.build_started);
            let release = Arc::clone(&self.release_build);
            let count = Arc::clone(&self.build_count);
            Box::pin(async move {
                count.fetch_add(1, Ordering::Relaxed);
                started.notify_one();
                release.notified().await;
                Ok(adapter as Arc<dyn ImAdapter>)
            })
        }
    }

    fn concurrent_adapter() -> Arc<ConcurrentAdapter> {
        Arc::new(ConcurrentAdapter {
            poll_started: Arc::new(Notify::new()),
            release_poll: Arc::new(Notify::new()),
            poll_count: Arc::new(AtomicUsize::new(0)),
            shutdown_count: Arc::new(AtomicUsize::new(0)),
        })
    }

    #[tokio::test]
    async fn send_is_processed_while_poll_is_pending() {
        let adapter = concurrent_adapter();
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
        assert_eq!(adapter.shutdown_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected_without_losing_the_next_frame() {
        let adapter = concurrent_adapter();
        let factory = Arc::new(Factory { adapter });
        let (client, server) = tokio::io::duplex(4 * 1024);
        let (server_reader, server_writer) = tokio::io::split(server);
        let sidecar = tokio::spawn(run_io_with_limits(
            "example.channel".to_string(),
            factory,
            BufReader::new(server_reader),
            server_writer,
            256,
            4,
        ));
        let (client_reader, mut client_writer) = tokio::io::split(client);
        let mut client_reader = BufReader::new(client_reader);

        client_writer
            .write_all(&vec![b'x'; 257])
            .await
            .expect("oversized body");
        client_writer
            .write_all(b"\n")
            .await
            .expect("oversized delimiter");
        let oversized = read_response(&mut client_reader).await;
        assert_eq!(oversized["error"]["code"], -32700);
        assert!(
            oversized["error"]["message"]
                .as_str()
                .unwrap()
                .contains("256")
        );

        initialize(&mut client_writer, &mut client_reader).await;
        write_request(&mut client_writer, 9, "shutdown", json!({})).await;
        assert_eq!(read_response(&mut client_reader).await["id"], 9);
        drop(client_writer);
        sidecar.await.expect("sidecar task").expect("sidecar run");
    }

    #[tokio::test]
    async fn long_poll_admission_reserves_control_capacity_and_stop_cancels_poll() {
        let adapter = concurrent_adapter();
        let factory = Arc::new(Factory {
            adapter: Arc::clone(&adapter),
        });
        let (client, server) = tokio::io::duplex(16 * 1024);
        let (server_reader, server_writer) = tokio::io::split(server);
        let sidecar = tokio::spawn(run_io_with_limits(
            "example.channel".to_string(),
            factory,
            BufReader::new(server_reader),
            server_writer,
            16 * 1024,
            2,
        ));
        let (client_reader, mut client_writer) = tokio::io::split(client);
        let mut client_reader = BufReader::new(client_reader);
        initialize(&mut client_writer, &mut client_reader).await;
        start_connection(&mut client_writer, &mut client_reader, 2).await;

        for id in [3, 4] {
            write_request(
                &mut client_writer,
                id,
                "channel/poll",
                json!({ "connectionId": "test" }),
            )
            .await;
        }
        while adapter.poll_count.load(Ordering::Relaxed) < 1 {
            adapter.poll_started.notified().await;
        }
        let rejected = read_response(&mut client_reader).await;
        assert_eq!(rejected["id"], 4);
        assert_eq!(rejected["error"]["code"], -32001);

        write_request(
            &mut client_writer,
            5,
            "channel/stop",
            json!({ "connectionId": "test" }),
        )
        .await;
        let first = tokio::time::timeout(
            Duration::from_millis(500),
            read_response(&mut client_reader),
        )
        .await
        .expect("stop and its cancelled poll must respond promptly");
        let second = read_response(&mut client_reader).await;
        let responses = [first, second];
        let stop = responses
            .iter()
            .find(|response| response["id"] == 5)
            .unwrap();
        assert!(stop.get("result").is_some(), "{stop}");
        let poll = responses
            .iter()
            .find(|response| response["id"] == 3)
            .unwrap();
        assert_eq!(poll["error"]["code"], -32000);

        write_request(&mut client_writer, 6, "shutdown", json!({})).await;
        assert_eq!(read_response(&mut client_reader).await["id"], 6);
        drop(client_writer);
        sidecar.await.expect("sidecar task").expect("sidecar run");
        assert_eq!(adapter.shutdown_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn duplicate_start_reserves_before_build_and_shuts_down_once() {
        let adapter = concurrent_adapter();
        let build_started = Arc::new(Notify::new());
        let release_build = Arc::new(Notify::new());
        let build_count = Arc::new(AtomicUsize::new(0));
        let factory = Arc::new(BlockingFactory {
            adapter: Arc::clone(&adapter),
            build_started: Arc::clone(&build_started),
            release_build: Arc::clone(&release_build),
            build_count: Arc::clone(&build_count),
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
        initialize(&mut client_writer, &mut client_reader).await;

        write_start(&mut client_writer, 2).await;
        build_started.notified().await;
        write_start(&mut client_writer, 3).await;
        let duplicate = read_response(&mut client_reader).await;
        assert_eq!(duplicate["id"], 3);
        assert!(
            duplicate["error"]["message"]
                .as_str()
                .unwrap()
                .contains("already started")
        );
        assert_eq!(build_count.load(Ordering::Relaxed), 1);
        release_build.notify_one();
        assert_eq!(read_response(&mut client_reader).await["id"], 2);

        write_request(&mut client_writer, 4, "shutdown", json!({})).await;
        assert_eq!(read_response(&mut client_reader).await["id"], 4);
        drop(client_writer);
        sidecar.await.expect("sidecar task").expect("sidecar run");
        assert_eq!(adapter.shutdown_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn shutdown_during_build_cancels_the_start_before_replying() {
        let adapter = concurrent_adapter();
        let build_started = Arc::new(Notify::new());
        let factory = Arc::new(BlockingFactory {
            adapter: Arc::clone(&adapter),
            build_started: Arc::clone(&build_started),
            release_build: Arc::new(Notify::new()),
            build_count: Arc::new(AtomicUsize::new(0)),
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
        initialize(&mut client_writer, &mut client_reader).await;
        write_start(&mut client_writer, 2).await;
        build_started.notified().await;
        write_request(&mut client_writer, 3, "shutdown", json!({})).await;
        assert_eq!(read_response(&mut client_reader).await["id"], 2);
        assert_eq!(read_response(&mut client_reader).await["id"], 3);
        drop(client_writer);
        sidecar.await.expect("sidecar task").expect("sidecar run");
        assert_eq!(adapter.shutdown_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn response_writer_failure_is_returned_to_the_process_owner() {
        let adapter = concurrent_adapter();
        let factory = Arc::new(Factory { adapter });
        let (mut client, server) = tokio::io::duplex(4 * 1024);
        let (server_reader, _server_writer) = tokio::io::split(server);
        let sidecar = tokio::spawn(run_io(
            "example.channel".to_string(),
            factory,
            BufReader::new(server_reader),
            FailingWriter,
        ));
        write_request(
            &mut client,
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
        let error = tokio::time::timeout(Duration::from_secs(1), sidecar)
            .await
            .expect("owner observes writer failure")
            .expect("sidecar task")
            .expect_err("writer failure must fail run")
            .to_string();
        assert!(error.contains("test writer failed"), "{error}");
    }

    async fn initialize(
        writer: &mut (impl tokio::io::AsyncWrite + Unpin),
        reader: &mut (impl tokio::io::AsyncBufRead + Unpin),
    ) {
        write_request(
            writer,
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
        assert_eq!(read_response(reader).await["id"], 1);
    }

    async fn write_start(writer: &mut (impl tokio::io::AsyncWrite + Unpin), id: u64) {
        write_request(
            writer,
            id,
            "channel/start",
            json!({
                "connectionId": "test",
                "channel": "test",
                "configuration": {}
            }),
        )
        .await;
    }

    async fn start_connection(
        writer: &mut (impl tokio::io::AsyncWrite + Unpin),
        reader: &mut (impl tokio::io::AsyncBufRead + Unpin),
        id: u64,
    ) {
        write_start(writer, id).await;
        assert_eq!(read_response(reader).await["id"], id);
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
