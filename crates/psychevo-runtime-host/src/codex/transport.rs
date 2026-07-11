use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, Ordering},
};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::time::{Instant, timeout};

use crate::{RetryClass, RuntimeError, RuntimeErrorStage, RuntimeProfile};

use super::wire::{self, IncomingMessage, RequestId};

const OUTBOUND_CAPACITY: usize = 256;
const STDERR_LINE_LIMIT: usize = 128;
const STDERR_BYTES_PER_LINE: usize = 4 * 1024;
const STDOUT_MESSAGE_LIMIT: usize = 8 * 1024 * 1024;
const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub(super) enum TransportEvent {
    Request {
        id: RequestId,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Exited(RuntimeError),
}

pub(super) type TransportEventSink = Arc<dyn Fn(TransportEvent) + Send + Sync>;

struct PendingRpc {
    method: String,
    responder: oneshot::Sender<Result<Value, RuntimeError>>,
}

struct OutboundMessage {
    value: Value,
    written: oneshot::Sender<Result<(), RuntimeError>>,
}

enum SupervisorCommand {
    Shutdown {
        force: bool,
        completed: oneshot::Sender<()>,
    },
}

struct TransportShared {
    epoch: u64,
    disposed: AtomicBool,
    next_request_id: AtomicI64,
    pending: Mutex<HashMap<String, PendingRpc>>,
    diagnostics: Mutex<VecDeque<String>>,
    exit_notify: Notify,
    event_sink: TransportEventSink,
}

impl TransportShared {
    fn fail(&self, error: RuntimeError) {
        if self.disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        let pending =
            std::mem::take(&mut *self.pending.lock().expect("Codex pending RPC map poisoned"));
        for (_, request) in pending {
            let _ = request.responder.send(Err(error.clone()));
        }
        (self.event_sink)(TransportEvent::Exited(error));
        self.exit_notify.notify_waiters();
    }

    fn process_response(&self, id: &RequestId, result: Result<Value, wire::RpcError>) {
        let Some(request) = self
            .pending
            .lock()
            .expect("Codex pending RPC map poisoned")
            .remove(&id.key())
        else {
            return;
        };
        let result = result.map_err(|error| wire::rpc_error(error, &request.method));
        let _ = request.responder.send(result);
    }

    fn diagnostic_ref(&self) -> String {
        format!("codex-process-{}", self.epoch)
    }

    fn process_exit_error(&self, detail: impl Into<String>) -> RuntimeError {
        RuntimeError::new(
            "codex_process_exit",
            RuntimeErrorStage::Transport,
            RetryClass::Reconnect,
            detail,
        )
        .with_diagnostic_ref(self.diagnostic_ref())
    }
}

pub(super) struct CodexTransport {
    shared: Arc<TransportShared>,
    outbound_tx: mpsc::Sender<OutboundMessage>,
    supervisor_tx: mpsc::Sender<SupervisorCommand>,
}

impl std::fmt::Debug for CodexTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CodexTransport")
            .field("epoch", &self.shared.epoch)
            .field("disposed", &self.shared.disposed.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl CodexTransport {
    pub(super) async fn spawn(
        profile: &RuntimeProfile,
        cwd: &Path,
        epoch: u64,
        event_sink: TransportEventSink,
    ) -> Result<Arc<Self>, RuntimeError> {
        let command = profile
            .command
            .as_deref()
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .ok_or_else(|| {
                RuntimeError::new(
                    "codex_missing_command",
                    RuntimeErrorStage::Configuration,
                    RetryClass::UserAction,
                    "Codex runtime command is not configured",
                )
            })?;
        let args = effective_args(&profile.args)?;
        let mut process = Command::new(command);
        process
            .args(&args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        apply_profile_env(&mut process, &profile.env);
        let mut child = process.spawn().map_err(|error| {
            RuntimeError::new(
                "codex_launch_failed",
                RuntimeErrorStage::Launch,
                RetryClass::UserAction,
                format!("Failed to start Codex app-server: {error}"),
            )
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            RuntimeError::new(
                "codex_missing_stdin",
                RuntimeErrorStage::Launch,
                RetryClass::Never,
                "Codex app-server did not provide stdin",
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            RuntimeError::new(
                "codex_missing_stdout",
                RuntimeErrorStage::Launch,
                RetryClass::Never,
                "Codex app-server did not provide stdout",
            )
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            RuntimeError::new(
                "codex_missing_stderr",
                RuntimeErrorStage::Launch,
                RetryClass::Never,
                "Codex app-server did not provide stderr",
            )
        })?;

        let shared = Arc::new(TransportShared {
            epoch,
            disposed: AtomicBool::new(false),
            next_request_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            diagnostics: Mutex::new(VecDeque::with_capacity(STDERR_LINE_LIMIT)),
            exit_notify: Notify::new(),
            event_sink,
        });
        let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CAPACITY);
        let (supervisor_tx, supervisor_rx) = mpsc::channel(2);
        tokio::spawn(writer_loop(Arc::clone(&shared), stdin, outbound_rx));
        tokio::spawn(stdout_loop(Arc::clone(&shared), stdout));
        tokio::spawn(stderr_loop(Arc::clone(&shared), stderr));
        tokio::spawn(supervisor_loop(Arc::clone(&shared), child, supervisor_rx));
        Ok(Arc::new(Self {
            shared,
            outbound_tx,
            supervisor_tx,
        }))
    }

    pub(super) fn is_disposed(&self) -> bool {
        self.shared.disposed.load(Ordering::SeqCst)
    }

    pub(super) async fn request(&self, method: &str, params: Value) -> Result<Value, RuntimeError> {
        if self.is_disposed() {
            return Err(self
                .shared
                .process_exit_error("Codex app-server is no longer running"));
        }
        let id = RequestId::Integer(self.shared.next_request_id.fetch_add(1, Ordering::SeqCst));
        let (response_tx, response_rx) = oneshot::channel();
        self.shared
            .pending
            .lock()
            .expect("Codex pending RPC map poisoned")
            .insert(
                id.key(),
                PendingRpc {
                    method: method.to_string(),
                    responder: response_tx,
                },
            );
        let write_result = self
            .send_value(wire::request(id.clone(), method, params))
            .await;
        if let Err(error) = write_result {
            self.shared
                .pending
                .lock()
                .expect("Codex pending RPC map poisoned")
                .remove(&id.key());
            return Err(error);
        }
        match timeout(RPC_TIMEOUT, response_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(self
                .shared
                .process_exit_error(format!("Codex app-server dropped the response to {method}"))),
            Err(_) => {
                self.shared
                    .pending
                    .lock()
                    .expect("Codex pending RPC map poisoned")
                    .remove(&id.key());
                Err(RuntimeError::new(
                    "codex_rpc_timeout",
                    RuntimeErrorStage::Transport,
                    RetryClass::UnknownDelivery,
                    format!("Timed out waiting for Codex app-server response to {method}"),
                )
                .with_diagnostic_ref(self.shared.diagnostic_ref()))
            }
        }
    }

    pub(super) async fn notify(&self, method: &str, params: Value) -> Result<(), RuntimeError> {
        self.send_value(wire::notification(method, params)).await
    }

    pub(super) async fn respond(&self, id: RequestId, result: Value) -> Result<(), RuntimeError> {
        self.send_value(wire::response(id, result)).await
    }

    async fn send_value(&self, value: Value) -> Result<(), RuntimeError> {
        if self.is_disposed() {
            return Err(self
                .shared
                .process_exit_error("Codex app-server is no longer running"));
        }
        let (written_tx, written_rx) = oneshot::channel();
        self.outbound_tx
            .send(OutboundMessage {
                value,
                written: written_tx,
            })
            .await
            .map_err(|_| {
                self.shared
                    .process_exit_error("Codex app-server stdin writer stopped")
            })?;
        written_rx.await.map_err(|_| {
            self.shared
                .process_exit_error("Codex app-server stdin write result was lost")
        })?
    }

    pub(super) async fn shutdown(&self, force: bool) -> Result<(), RuntimeError> {
        if self.is_disposed() {
            return Ok(());
        }
        let (completed_tx, completed_rx) = oneshot::channel();
        if self
            .supervisor_tx
            .send(SupervisorCommand::Shutdown {
                force,
                completed: completed_tx,
            })
            .await
            .is_err()
        {
            return Ok(());
        }
        timeout(SHUTDOWN_TIMEOUT, completed_rx)
            .await
            .map_err(|_| {
                RuntimeError::new(
                    "codex_shutdown_timeout",
                    RuntimeErrorStage::Shutdown,
                    RetryClass::Never,
                    "Timed out stopping Codex app-server",
                )
            })?
            .map_err(|_| {
                RuntimeError::new(
                    "codex_shutdown_failed",
                    RuntimeErrorStage::Shutdown,
                    RetryClass::Never,
                    "Codex app-server shutdown task stopped unexpectedly",
                )
            })?;
        Ok(())
    }
}

fn effective_args(configured: &[String]) -> Result<Vec<String>, RuntimeError> {
    let args = if configured.is_empty() {
        vec!["app-server".to_string(), "--stdio".to_string()]
    } else {
        configured.to_vec()
    };
    if args.first().map(String::as_str) != Some("app-server") {
        return Err(RuntimeError::new(
            "codex_invalid_command",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "Codex direct runtime args must start with `app-server`",
        ));
    }
    for (index, arg) in args.iter().enumerate() {
        if arg == "--listen" {
            if args.get(index + 1).map(String::as_str) != Some("stdio://") {
                return Err(RuntimeError::new(
                    "codex_non_stdio_transport",
                    RuntimeErrorStage::Configuration,
                    RetryClass::UserAction,
                    "Codex direct runtime requires stdio transport",
                ));
            }
        } else if let Some(value) = arg.strip_prefix("--listen=")
            && value != "stdio://"
        {
            return Err(RuntimeError::new(
                "codex_non_stdio_transport",
                RuntimeErrorStage::Configuration,
                RetryClass::UserAction,
                "Codex direct runtime requires stdio transport",
            ));
        }
    }
    Ok(args)
}

fn apply_profile_env(command: &mut Command, env: &BTreeMap<String, String>) {
    for (key, value) in env {
        command.env(key, value);
    }
}

async fn writer_loop(
    shared: Arc<TransportShared>,
    mut stdin: tokio::process::ChildStdin,
    mut outbound_rx: mpsc::Receiver<OutboundMessage>,
) {
    while let Some(message) = outbound_rx.recv().await {
        let result = async {
            let mut bytes = serde_json::to_vec(&message.value).map_err(|error| {
                RuntimeError::new(
                    "codex_protocol_encode_failed",
                    RuntimeErrorStage::Transport,
                    RetryClass::Never,
                    format!("Failed to encode Codex request: {error}"),
                )
            })?;
            bytes.push(b'\n');
            stdin.write_all(&bytes).await.map_err(|error| {
                shared.process_exit_error(format!("Failed writing to Codex app-server: {error}"))
            })?;
            stdin.flush().await.map_err(|error| {
                shared
                    .process_exit_error(format!("Failed flushing Codex app-server stdin: {error}"))
            })?;
            Ok(())
        }
        .await;
        let failed = result.is_err();
        let failure = result.as_ref().err().cloned();
        let _ = message.written.send(result);
        if failed {
            shared.fail(failure.expect("writer failure"));
            break;
        }
    }
}

async fn stdout_loop(shared: Arc<TransportShared>, stdout: tokio::process::ChildStdout) {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) => {
                shared.fail(shared.process_exit_error("Codex app-server closed stdout"));
                return;
            }
            Ok(_) if line.len() > STDOUT_MESSAGE_LIMIT => {
                shared.fail(RuntimeError::new(
                    "codex_protocol_message_too_large",
                    RuntimeErrorStage::Transport,
                    RetryClass::Never,
                    "Codex app-server emitted an oversized protocol message",
                ));
                return;
            }
            Ok(_) => {}
            Err(error) => {
                shared.fail(shared.process_exit_error(format!(
                    "Failed reading Codex app-server stdout: {error}"
                )));
                return;
            }
        }
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        if line.is_empty() {
            continue;
        }
        let text = match std::str::from_utf8(&line) {
            Ok(text) => text,
            Err(error) => {
                shared.fail(RuntimeError::new(
                    "codex_protocol_invalid_utf8",
                    RuntimeErrorStage::Transport,
                    RetryClass::Never,
                    format!("Codex app-server emitted invalid UTF-8: {error}"),
                ));
                return;
            }
        };
        match wire::parse_incoming(text) {
            Ok(IncomingMessage::Response { id, result }) => {
                shared.process_response(&id, Ok(result));
            }
            Ok(IncomingMessage::Error { id, error }) => {
                shared.process_response(&id, Err(error));
            }
            Ok(IncomingMessage::Request { id, method, params }) => {
                (shared.event_sink)(TransportEvent::Request { id, method, params });
            }
            Ok(IncomingMessage::Notification { method, params }) => {
                (shared.event_sink)(TransportEvent::Notification { method, params });
            }
            Err(error) => {
                shared.fail(error.with_diagnostic_ref(shared.diagnostic_ref()));
                return;
            }
        }
    }
}

async fn stderr_loop(shared: Arc<TransportShared>, stderr: tokio::process::ChildStderr) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(mut line)) = lines.next_line().await {
        if line.len() > STDERR_BYTES_PER_LINE {
            line.truncate(STDERR_BYTES_PER_LINE);
            line.push_str("...");
        }
        let mut diagnostics = shared
            .diagnostics
            .lock()
            .expect("Codex stderr diagnostics poisoned");
        if diagnostics.len() == STDERR_LINE_LIMIT {
            diagnostics.pop_front();
        }
        diagnostics.push_back(line);
    }
}

async fn supervisor_loop(
    shared: Arc<TransportShared>,
    mut child: tokio::process::Child,
    mut commands: mpsc::Receiver<SupervisorCommand>,
) {
    let mut poll = tokio::time::interval(Duration::from_millis(25));
    loop {
        tokio::select! {
            _ = poll.tick() => {
                if shared.disposed.load(Ordering::SeqCst) {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return;
                }
                match child.try_wait() {
                    Ok(Some(status)) => {
                        shared.fail(shared.process_exit_error(format!(
                            "Codex app-server exited with {status}"
                        )));
                        return;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        shared.fail(shared.process_exit_error(format!(
                            "Failed polling Codex app-server: {error}"
                        )));
                        return;
                    }
                }
            }
            command = commands.recv() => {
                let Some(SupervisorCommand::Shutdown { force, completed }) = command else {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return;
                };
                if !force {
                    // Closing stdin is not available while the writer owns it. Give the process a
                    // short bounded window to finish naturally before forcing termination.
                    let deadline = Instant::now() + Duration::from_millis(150);
                    while Instant::now() < deadline {
                        if child.try_wait().ok().flatten().is_some() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
                if child.try_wait().ok().flatten().is_none() {
                    let _ = child.kill().await;
                }
                let _ = child.wait().await;
                shared.fail(shared.process_exit_error("Codex app-server was shut down"));
                let _ = completed.send(());
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_stdio_listen_args() {
        let error = effective_args(&[
            "app-server".to_string(),
            "--listen".to_string(),
            "ws://127.0.0.1:0".to_string(),
        ])
        .expect_err("non-stdio transport");
        assert_eq!(error.code, "codex_non_stdio_transport");
    }
}
