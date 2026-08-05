use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use futures::Stream;
use tokio::sync::{mpsc, oneshot};

use crate::provider::{
    UnaryInvocationTarget, guarded_adapter_call, invocation_total_deadline,
    prepare_adapter_context, spawn_invocation_with_pair,
};
use crate::{
    AbortHandle, AbortSignal, AdapterCall, ErrorKind, ErrorPhase, Invocation, ProviderError,
    RealtimeAdapter, RealtimeAdapterEvent, RealtimeAdapterTransport, RealtimeCloseReason,
    RealtimeCommand, RealtimeCommandSink, RealtimeConnectRequest, RealtimeEvent,
};

const DEFAULT_REALTIME_COMMAND_CAPACITY: usize = 32;

#[derive(Clone)]
pub struct RealtimeSender {
    commands: mpsc::Sender<CommandEnvelope>,
    closed: Arc<AtomicBool>,
    command_timeout: Option<std::time::Duration>,
}

impl std::fmt::Debug for RealtimeSender {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RealtimeSender")
            .field("closed", &self.closed.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl RealtimeSender {
    pub async fn send_audio(&self, audio: crate::Media) -> Result<(), ProviderError> {
        self.send(RealtimeCommand::InputAudio { audio }).await
    }

    pub async fn send_text(&self, text: impl Into<String>) -> Result<(), ProviderError> {
        self.send(RealtimeCommand::InputText { text: text.into() })
            .await
    }

    pub async fn commit(&self) -> Result<(), ProviderError> {
        self.send(RealtimeCommand::Commit).await
    }

    pub async fn close(&self) -> Result<(), ProviderError> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Err(session_closed());
        }
        self.enqueue(CommandKind::Close).await
    }

    async fn send(&self, command: RealtimeCommand) -> Result<(), ProviderError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(session_closed());
        }
        self.enqueue(CommandKind::Send(command)).await
    }

    async fn enqueue(&self, kind: CommandKind) -> Result<(), ProviderError> {
        let (result_tx, result_rx) = oneshot::channel();
        wait_with_command_deadline(
            self.commands.send(CommandEnvelope { kind, result_tx }),
            self.command_timeout,
        )
        .await
        .map_err(|_| command_timeout())?
        .map_err(|_| session_closed())?;
        wait_with_command_deadline(result_rx, self.command_timeout)
            .await
            .map_err(|_| command_timeout())?
            .map_err(|_| session_closed())?
    }
}

pub struct RealtimeSession {
    sender: RealtimeSender,
    events: crate::AdapterStream<RealtimeAdapterEvent>,
    abort: AbortHandle,
    abort_wait: Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    command_worker: tokio::task::JoinHandle<()>,
    terminal: bool,
    emitted_eof_error: bool,
}

impl std::fmt::Debug for RealtimeSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RealtimeSession")
            .field("terminal", &self.terminal)
            .finish_non_exhaustive()
    }
}

impl RealtimeSession {
    pub fn sender(&self) -> RealtimeSender {
        self.sender.clone()
    }

    pub async fn next_event(&mut self) -> Option<Result<RealtimeEvent, ProviderError>> {
        futures::future::poll_fn(|context| Pin::new(&mut *self).poll_next(context)).await
    }

    pub async fn close(&self) -> Result<(), ProviderError> {
        self.sender.close().await
    }

    pub fn abort(&self) -> bool {
        self.abort.abort()
    }
}

impl Stream for RealtimeSession {
    type Item = Result<RealtimeEvent, ProviderError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminal {
            return Poll::Ready(None);
        }
        if self.abort_wait.as_mut().poll(context).is_ready() {
            self.terminal = true;
            self.sender.closed.store(true, Ordering::Release);
            return Poll::Ready(Some(Ok(RealtimeEvent::Closed {
                reason: RealtimeCloseReason::Aborted,
            })));
        }
        match self.events.as_mut().poll_next(context) {
            Poll::Ready(Some(Ok(event))) => {
                let event = map_realtime_event(event, &self.sender.closed);
                if matches!(event, RealtimeEvent::Closed { .. }) {
                    self.terminal = true;
                    self.sender.closed.store(true, Ordering::Release);
                }
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(Some(Err(error))) => {
                self.terminal = true;
                self.sender.closed.store(true, Ordering::Release);
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) if !self.emitted_eof_error => {
                self.emitted_eof_error = true;
                self.terminal = true;
                self.sender.closed.store(true, Ordering::Release);
                Poll::Ready(Some(Err(ProviderError::protocol(
                    "realtime event stream ended before Closed",
                ))))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for RealtimeSession {
    fn drop(&mut self) {
        if !self.terminal {
            self.abort.abort();
        }
        self.sender.closed.store(true, Ordering::Release);
        self.command_worker.abort();
    }
}

pub(crate) fn start_realtime_connect(
    target: UnaryInvocationTarget,
    adapter: Arc<dyn RealtimeAdapter>,
    request: RealtimeConnectRequest,
) -> Invocation<RealtimeSession> {
    spawn_invocation_with_pair(move |session_abort, abort_signal| async move {
        let total_deadline = invocation_total_deadline(&target);
        let context = prepare_adapter_context(
            &target,
            &request.headers,
            abort_signal.clone(),
            total_deadline,
        )
        .await?;
        let transport = guarded_adapter_call(
            adapter.connect(AdapterCall {
                model: target.descriptor.model_id.clone(),
                request,
                context,
            }),
            abort_signal.clone(),
            &target,
            ErrorPhase::RealtimeConnect,
            total_deadline,
        )
        .await?;
        Ok(build_session(
            transport,
            session_abort,
            abort_signal,
            target.timeout_policy.realtime_command_timeout(),
        ))
    })
}

fn build_session(
    transport: RealtimeAdapterTransport,
    abort: AbortHandle,
    abort_signal: AbortSignal,
    command_timeout: Option<std::time::Duration>,
) -> RealtimeSession {
    let (commands, receiver) = mpsc::channel(DEFAULT_REALTIME_COMMAND_CAPACITY);
    let closed = Arc::new(AtomicBool::new(false));
    let sender = RealtimeSender {
        commands,
        closed: closed.clone(),
        command_timeout,
    };
    let mut session_abort = abort_signal.clone();
    let abort_wait = Box::pin(async move {
        session_abort.wait_for_abort().await;
    });
    let command_worker = tokio::spawn(run_commands(
        receiver,
        transport.commands,
        abort_signal,
        closed,
    ));
    RealtimeSession {
        sender,
        events: transport.events,
        abort,
        abort_wait,
        command_worker,
        terminal: false,
        emitted_eof_error: false,
    }
}

async fn run_commands(
    mut receiver: mpsc::Receiver<CommandEnvelope>,
    sink: Arc<dyn RealtimeCommandSink>,
    mut abort: AbortSignal,
    closed: Arc<AtomicBool>,
) {
    loop {
        let envelope = tokio::select! {
            biased;
            _ = abort.wait_for_abort() => break,
            envelope = receiver.recv() => envelope,
        };
        let Some(envelope) = envelope else {
            break;
        };
        let result = match envelope.kind {
            CommandKind::Send(command) => sink.send(command).await,
            CommandKind::Close => {
                let result = sink.close().await;
                closed.store(true, Ordering::Release);
                let _ = envelope.result_tx.send(result);
                break;
            }
        };
        let _ = envelope.result_tx.send(result);
    }
    closed.store(true, Ordering::Release);
    while let Ok(envelope) = receiver.try_recv() {
        let _ = envelope.result_tx.send(Err(session_closed()));
    }
}

struct CommandEnvelope {
    kind: CommandKind,
    result_tx: oneshot::Sender<Result<(), ProviderError>>,
}

enum CommandKind {
    Send(RealtimeCommand),
    Close,
}

fn map_realtime_event(event: RealtimeAdapterEvent, close_requested: &AtomicBool) -> RealtimeEvent {
    match event {
        RealtimeAdapterEvent::InputTranscriptDelta { delta } => {
            RealtimeEvent::InputTranscriptDelta { delta }
        }
        RealtimeAdapterEvent::InputTranscriptDone { text } => {
            RealtimeEvent::InputTranscriptDone { text }
        }
        RealtimeAdapterEvent::OutputTextDelta { delta } => RealtimeEvent::OutputTextDelta { delta },
        RealtimeAdapterEvent::OutputTextDone { text } => RealtimeEvent::OutputTextDone { text },
        RealtimeAdapterEvent::OutputAudioDelta { audio } => {
            RealtimeEvent::OutputAudioDelta { audio }
        }
        RealtimeAdapterEvent::OutputAudioDone => RealtimeEvent::OutputAudioDone,
        RealtimeAdapterEvent::ResponseDone => RealtimeEvent::ResponseDone,
        RealtimeAdapterEvent::Warning { warning } => RealtimeEvent::Warning { warning },
        RealtimeAdapterEvent::Metadata { metadata } => RealtimeEvent::Metadata { metadata },
        RealtimeAdapterEvent::Closed { remote } => RealtimeEvent::Closed {
            reason: if close_requested.load(Ordering::Acquire) {
                RealtimeCloseReason::Requested
            } else if remote {
                RealtimeCloseReason::Remote
            } else {
                RealtimeCloseReason::Aborted
            },
        },
    }
}

async fn wait_with_command_deadline<F, T>(
    future: F,
    deadline: Option<std::time::Duration>,
) -> Result<T, ()>
where
    F: std::future::Future<Output = T>,
{
    match deadline {
        Some(deadline) => tokio::time::timeout(deadline, future).await.map_err(|_| ()),
        None => Ok(future.await),
    }
}

fn session_closed() -> ProviderError {
    ProviderError::new(
        ErrorKind::Aborted,
        ErrorPhase::RealtimeCommand,
        "realtime session is closed",
    )
}

fn command_timeout() -> ProviderError {
    ProviderError::new(
        ErrorKind::Timeout,
        ErrorPhase::RealtimeCommand,
        "realtime command deadline elapsed",
    )
}
