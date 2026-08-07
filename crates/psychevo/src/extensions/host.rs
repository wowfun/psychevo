use std::collections::{BTreeMap, VecDeque};
#[cfg(windows)]
use std::ffi::OsString;
use std::process::Stdio;
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::Duration;

use psychevo_extension_protocol::{
    ChannelConnectionParams, ChannelPollResult, ChannelSendParams, ChannelStartParams,
    CommandEffect, CommandRunParams, ContributionDescriptors, HostCapabilities, InitializeParams,
    InitializeResult, PROTOCOL_VERSION, RpcRequest, RpcResponse,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
#[cfg(not(windows))]
use tokio::process::Command;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

use super::{ExtensionInstallRecord, ExtensionManifest};
use crate::error::{Error, Result};

#[cfg(not(test))]
const RPC_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const RPC_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(not(test))]
const CHANNEL_POLL_TIMEOUT: Duration = Duration::from_secs(45);
#[cfg(test)]
const CHANNEL_POLL_TIMEOUT: Duration = Duration::from_secs(4);
const FRAME_LIMIT: usize = 16 * 1024 * 1024;
const STDERR_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionHostMode {
    OneShot,
    Leased { idle_timeout: Duration },
}

pub struct ExtensionRuntime {
    record: ExtensionInstallRecord,
    manifest: ExtensionManifest,
    env: BTreeMap<String, String>,
    capabilities: HostCapabilities,
    mode: ExtensionHostMode,
    state: Mutex<RuntimeState>,
}

struct RuntimeState {
    session: Option<Arc<ExtensionSession>>,
    channel_starts: BTreeMap<String, ChannelStartParams>,
    activity_lock: Option<super::package::ExtensionActivityLock>,
    leases: usize,
    idle_generation: u64,
    idle_task: Option<JoinHandle<()>>,
    stopping: bool,
}

impl ExtensionRuntime {
    pub fn new(
        record: ExtensionInstallRecord,
        manifest: ExtensionManifest,
        env: BTreeMap<String, String>,
        mode: ExtensionHostMode,
    ) -> Result<Arc<Self>> {
        Self::with_capabilities(record, manifest, env, mode, HostCapabilities::default())
    }

    pub fn with_capabilities(
        record: ExtensionInstallRecord,
        manifest: ExtensionManifest,
        env: BTreeMap<String, String>,
        mode: ExtensionHostMode,
        capabilities: HostCapabilities,
    ) -> Result<Arc<Self>> {
        if record.id != manifest.id || record.version != manifest.version {
            return Err(Error::Config(format!(
                "Extension record `{}` does not match manifest `{}@{}`",
                record.id, manifest.id, manifest.version
            )));
        }
        if !record.enabled {
            return Err(Error::Config(format!(
                "Extension `{}` is disabled",
                record.id
            )));
        }
        if record.fingerprint != record.trusted_fingerprint {
            return Err(Error::Config(format!(
                "Extension `{}` fingerprint changed and is not trusted",
                record.id
            )));
        }
        Ok(Arc::new(Self {
            record,
            manifest,
            env,
            capabilities,
            mode,
            state: Mutex::new(RuntimeState {
                session: None,
                channel_starts: BTreeMap::new(),
                activity_lock: None,
                leases: 0,
                idle_generation: 0,
                idle_task: None,
                stopping: false,
            }),
        }))
    }

    pub async fn acquire(self: &Arc<Self>) -> Result<ExtensionLease> {
        let mut state = self.state.lock().await;
        if state.stopping {
            return Err(Error::Message(format!(
                "Extension `{}` is shutting down",
                self.record.id
            )));
        }
        if let Some(task) = state.idle_task.take() {
            task.abort();
        }
        if state.activity_lock.is_none() {
            state.activity_lock = Some(super::package::acquire_extension_activity_lock(
                &self.record,
            )?);
        }
        state.idle_generation = state.idle_generation.saturating_add(1);
        state.leases = state.leases.saturating_add(1);
        Ok(ExtensionLease {
            runtime: Arc::clone(self),
            active: true,
        })
    }

    pub async fn started(&self) -> bool {
        self.state
            .lock()
            .await
            .session
            .as_ref()
            .is_some_and(|session| !session.is_stopped())
    }

    pub async fn shutdown(&self) -> Result<()> {
        let (session, activity_lock) = {
            let mut state = self.state.lock().await;
            state.stopping = true;
            if let Some(task) = state.idle_task.take() {
                task.abort();
            }
            (state.session.take(), state.activity_lock.take())
        };
        if let Some(session) = session {
            session.shutdown().await.map_err(Error::Message)?;
        }
        drop(activity_lock);
        Ok(())
    }

    async fn session(&self) -> std::result::Result<Arc<ExtensionSession>, String> {
        let mut state = self.state.lock().await;
        if state.stopping {
            return Err(format!("Extension `{}` is shutting down", self.record.id));
        }
        if let Some(session) = state.session.as_ref()
            && !session.is_stopped()
        {
            return Ok(Arc::clone(session));
        }
        if let Some(session) = state.session.take() {
            session.stop().await;
        }
        let session = ExtensionSession::start(
            &self.record,
            &self.manifest,
            &self.env,
            self.capabilities.clone(),
        )
        .await?;
        for params in state.channel_starts.values() {
            if let Err(error) = session
                .call(
                    "channel/start",
                    serde_json::to_value(params).map_err(|err| err.to_string())?,
                )
                .await
            {
                session.stop().await;
                return Err(format!(
                    "Extension `{}` failed to restore Channel connection `{}`: {error}",
                    self.record.id, params.connection_id
                ));
            }
        }
        state.session = Some(Arc::clone(&session));
        Ok(session)
    }

    async fn release_lease(self: &Arc<Self>) -> Result<()> {
        let mut immediate = None;
        let mut immediate_lock = None;
        let mut idle = None;
        {
            let mut state = self.state.lock().await;
            if state.leases == 0 {
                return Ok(());
            }
            state.leases -= 1;
            if state.leases != 0 || state.stopping {
                return Ok(());
            }
            state.idle_generation = state.idle_generation.saturating_add(1);
            let generation = state.idle_generation;
            match self.mode {
                ExtensionHostMode::OneShot => {
                    immediate = state.session.take();
                    immediate_lock = state.activity_lock.take();
                }
                ExtensionHostMode::Leased { idle_timeout } => {
                    if state.session.is_some() {
                        idle = Some((generation, idle_timeout));
                    } else {
                        immediate_lock = state.activity_lock.take();
                    }
                }
            }
        }
        if let Some(session) = immediate {
            session.shutdown().await.map_err(Error::Message)?;
        }
        drop(immediate_lock);
        if let Some((generation, idle_timeout)) = idle {
            let weak = Arc::downgrade(self);
            let task = tokio::spawn(async move {
                tokio::time::sleep(idle_timeout).await;
                if let Some(runtime) = weak.upgrade() {
                    runtime.shutdown_if_idle(generation).await;
                }
            });
            let mut state = self.state.lock().await;
            if state.leases == 0
                && state.idle_generation == generation
                && !state.stopping
                && state.session.is_some()
            {
                if let Some(previous) = state.idle_task.replace(task) {
                    previous.abort();
                }
            } else {
                task.abort();
            }
        }
        Ok(())
    }

    async fn shutdown_if_idle(&self, generation: u64) {
        let (session, activity_lock) = {
            let mut state = self.state.lock().await;
            if state.leases != 0 || state.idle_generation != generation || state.stopping {
                return;
            }
            state.idle_task.take();
            (state.session.take(), state.activity_lock.take())
        };
        if let Some(session) = session {
            let _ = session.shutdown().await;
        }
        drop(activity_lock);
    }
}

impl Drop for ExtensionRuntime {
    fn drop(&mut self) {
        let Ok(mut state) = self.state.try_lock() else {
            return;
        };
        if let Some(task) = state.idle_task.take() {
            task.abort();
        }
        state.session.take();
        state.activity_lock.take();
    }
}

pub struct ExtensionLease {
    runtime: Arc<ExtensionRuntime>,
    active: bool,
}

impl ExtensionLease {
    pub fn extension_id(&self) -> &str {
        &self.runtime.record.id
    }

    pub async fn contributions(&self) -> Result<ContributionDescriptors> {
        self.ensure_active()?;
        let value = self
            .runtime
            .session()
            .await
            .map_err(Error::Message)?
            .call("contributions/list", json!({}))
            .await
            .map_err(Error::Message)?;
        serde_json::from_value(value).map_err(|err| {
            Error::Message(format!(
                "Extension `{}` returned invalid contributions: {err}",
                self.runtime.record.id
            ))
        })
    }

    pub async fn command_run(&self, params: CommandRunParams) -> Result<CommandEffect> {
        self.ensure_active()?;
        if !self
            .runtime
            .manifest
            .contributions
            .commands
            .iter()
            .any(|command| command.name == params.command)
        {
            return Err(Error::Config(format!(
                "Extension `{}` did not statically declare command `{}`",
                self.runtime.record.id, params.command
            )));
        }
        let value = self
            .runtime
            .session()
            .await
            .map_err(Error::Message)?
            .call(
                "command/run",
                serde_json::to_value(params).map_err(Error::Json)?,
            )
            .await
            .map_err(Error::Message)?;
        serde_json::from_value(value).map_err(|err| {
            Error::Message(format!(
                "Extension `{}` returned an invalid command effect: {err}",
                self.runtime.record.id
            ))
        })
    }

    pub async fn channel_start(&self, params: ChannelStartParams) -> Result<()> {
        self.ensure_active()?;
        if !self
            .runtime
            .manifest
            .contributions
            .channels
            .iter()
            .any(|channel| channel.channel == params.channel)
        {
            return Err(Error::Config(format!(
                "Extension `{}` did not statically declare Channel `{}`",
                self.runtime.record.id, params.channel
            )));
        }
        let connection_id = params.connection_id.clone();
        self.call_unit("channel/start", &params).await?;
        self.runtime
            .state
            .lock()
            .await
            .channel_starts
            .insert(connection_id, params);
        Ok(())
    }

    pub async fn channel_poll(&self, params: ChannelConnectionParams) -> Result<ChannelPollResult> {
        self.ensure_active()?;
        let value = self.call_value("channel/poll", params).await?;
        serde_json::from_value(value).map_err(|err| {
            Error::Message(format!(
                "Extension `{}` returned invalid Channel ingress: {err}",
                self.runtime.record.id
            ))
        })
    }

    pub async fn channel_send(&self, params: ChannelSendParams) -> Result<()> {
        self.ensure_active()?;
        self.call_unit("channel/send", params).await
    }

    pub async fn channel_stop(&self, params: ChannelConnectionParams) -> Result<()> {
        self.ensure_active()?;
        let result = self.call_unit("channel/stop", &params).await;
        self.runtime
            .state
            .lock()
            .await
            .channel_starts
            .remove(&params.connection_id);
        result
    }

    pub async fn channel_control(&self, method: &str, params: Value) -> Result<Value> {
        self.ensure_active()?;
        if !method.starts_with("channel/") {
            return Err(Error::Config(format!(
                "Extension Channel control method must start with `channel/`, got `{method}`"
            )));
        }
        self.call_value(method, params).await
    }

    async fn call_unit(&self, method: &str, params: impl serde::Serialize) -> Result<()> {
        self.call_value(method, params).await.map(|_| ())
    }

    async fn call_value(&self, method: &str, params: impl serde::Serialize) -> Result<Value> {
        self.runtime
            .session()
            .await
            .map_err(Error::Message)?
            .call(method, serde_json::to_value(params).map_err(Error::Json)?)
            .await
            .map_err(Error::Message)
    }

    pub async fn release(mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        self.runtime.release_lease().await
    }

    fn ensure_active(&self) -> Result<()> {
        if self.active {
            Ok(())
        } else {
            Err(Error::Message("Extension lease is closed".to_string()))
        }
    }
}

impl Drop for ExtensionLease {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let runtime = Arc::clone(&self.runtime);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = runtime.release_lease().await;
            });
        }
    }
}

struct ExtensionSession {
    io: Arc<ExtensionSessionIo>,
    process: Mutex<ExtensionProcess>,
    reader_task: SyncMutex<Option<JoinHandle<()>>>,
}

struct ExtensionSessionIo {
    stdin: Mutex<Option<ChildStdin>>,
    state: SyncMutex<ExtensionSessionIoState>,
}

struct ExtensionSessionIoState {
    next_id: u64,
    stopped: bool,
    pending: BTreeMap<u64, oneshot::Sender<ExtensionCallResult>>,
}

struct ExtensionProcess {
    child: Child,
    stderr_tail: Arc<SyncMutex<VecDeque<u8>>>,
    stderr_task: Option<JoinHandle<()>>,
    tree: Option<crate::process_tree::ProcessTreeGuard>,
    stopped: bool,
}

enum ExtensionCallResult {
    Response(std::result::Result<Value, String>),
    Closed(String),
}

enum ExtensionMessage {
    Response {
        id: u64,
        result: std::result::Result<Value, String>,
    },
    Notification,
    ProtocolError(String),
}

impl ExtensionSession {
    async fn start(
        record: &ExtensionInstallRecord,
        manifest: &ExtensionManifest,
        env: &BTreeMap<String, String>,
        capabilities: HostCapabilities,
    ) -> std::result::Result<Arc<Self>, String> {
        #[cfg(windows)]
        let mut command = {
            let args = manifest
                .runtime
                .args
                .iter()
                .map(OsString::from)
                .collect::<Vec<_>>();
            crate::process_env::tokio_host_process_command(
                &manifest.runtime.executable,
                &args,
                crate::host_paths::HostPlatform::current(),
                env,
            )
            .map_err(|err| err.to_string())?
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new(&manifest.runtime.executable);
            command.args(&manifest.runtime.args);
            command
        };
        command
            .current_dir(&record.package_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::process_tree::ProcessTreeGuard::configure_tokio(&mut command);
        crate::process_env::apply_tokio_process_env(
            &mut command,
            env,
            crate::process_env::ProcessEnvOptions::new(&[]),
        )
        .map_err(|err| err.to_string())?;
        command
            .env("PSYCHEVO_EXTENSION_ID", &record.id)
            .env("PSYCHEVO_EXTENSION_ROOT", &record.package_root)
            .env("PSYCHEVO_EXTENSION_DATA", &record.data_root);
        let mut child = command.spawn().map_err(|err| {
            format!(
                "failed to start Extension `{}` executable {}: {err}",
                record.id,
                manifest.runtime.executable.display()
            )
        })?;
        let tree = crate::process_tree::ProcessTreeGuard::attach_tokio(&child).map_err(|err| {
            format!(
                "failed to own Extension `{}` process tree: {err}",
                record.id
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Extension stdin unavailable".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Extension stdout unavailable".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Extension stderr unavailable".to_string())?;
        let stderr_tail = Arc::new(SyncMutex::new(VecDeque::with_capacity(STDERR_LIMIT)));
        let stderr_task = tokio::spawn(drain_bounded_stderr(stderr, Arc::clone(&stderr_tail)));
        let io = Arc::new(ExtensionSessionIo {
            stdin: Mutex::new(Some(stdin)),
            state: SyncMutex::new(ExtensionSessionIoState {
                next_id: 1,
                stopped: false,
                pending: BTreeMap::new(),
            }),
        });
        let reader_io = Arc::clone(&io);
        let reader_task = tokio::spawn(async move {
            read_extension_responses(BufReader::new(stdout), reader_io).await;
        });
        let session = Arc::new(Self {
            io,
            process: Mutex::new(ExtensionProcess {
                child,
                stderr_tail,
                stderr_task: Some(stderr_task),
                tree: Some(tree),
                stopped: false,
            }),
            reader_task: SyncMutex::new(Some(reader_task)),
        });
        let value = session
            .call(
                "initialize",
                serde_json::to_value(InitializeParams {
                    protocol: PROTOCOL_VERSION.to_string(),
                    extension_id: record.id.clone(),
                    extension_version: record.version.clone(),
                    scope: record.scope.as_str().to_string(),
                    package_root: record.package_root.clone(),
                    data_root: record.data_root.clone(),
                    capabilities,
                })
                .map_err(|err| err.to_string())?,
            )
            .await?;
        let initialized: InitializeResult = serde_json::from_value(value)
            .map_err(|err| format!("invalid initialize result: {err}"))?;
        if initialized.protocol != PROTOCOL_VERSION || initialized.extension_id != record.id {
            session.stop().await;
            return Err(format!(
                "Extension `{}` initialize identity or protocol mismatch",
                record.id
            ));
        }
        Ok(session)
    }

    async fn call(&self, method: &str, params: Value) -> std::result::Result<Value, String> {
        let (id, receiver) = {
            let mut state = self
                .io
                .state
                .lock()
                .map_err(|_| "Extension session state is poisoned".to_string())?;
            if state.stopped {
                return Err("Extension session is closed".to_string());
            }
            let id = state.next_id;
            state.next_id = state.next_id.saturating_add(1);
            let (sender, receiver) = oneshot::channel();
            state.pending.insert(id, sender);
            (id, receiver)
        };
        let mut pending = PendingExtensionCall::new(id, Arc::clone(&self.io));
        if let Err(error) = send_request(&self.io, id, method, params).await {
            self.stop().await;
            return Err(error);
        }
        let timeout = rpc_timeout(method);
        let result = tokio::time::timeout(timeout, receiver).await;
        pending.complete();
        match result {
            Ok(Ok(ExtensionCallResult::Response(result))) => result,
            Ok(Ok(ExtensionCallResult::Closed(error))) => {
                self.stop().await;
                Err(error)
            }
            Ok(Err(_)) => {
                self.stop().await;
                Err("Extension session closed before response".to_string())
            }
            Err(_) => {
                let error = format!(
                    "Extension timed out waiting for `{method}` after {} seconds",
                    timeout.as_secs()
                );
                self.stop().await;
                Err(error)
            }
        }
    }

    async fn shutdown(&self) -> std::result::Result<(), String> {
        {
            if self.is_stopped() {
                return Ok(());
            }
        }
        let call_result = self.call("shutdown", json!({})).await;
        let mut process = self.process.lock().await;
        if process.stopped {
            return call_result.map(|_| ());
        }
        self.io.stdin.lock().await.take();
        let status = match tokio::time::timeout(RPC_TIMEOUT, process.child.wait()).await {
            Ok(result) => result.map_err(|err| err.to_string())?,
            Err(_) => {
                stop_process(&mut process).await;
                return Err(format!(
                    "Extension timed out waiting for exit after {} seconds",
                    RPC_TIMEOUT.as_secs()
                ));
            }
        };
        process.stopped = true;
        close_extension_io(&self.io, "Extension session shut down");
        stop_stderr_task(&mut process).await;
        process.tree.take();
        self.abort_reader_task();
        call_result?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("Extension exited with status {status}"))
        }
    }

    async fn stop(&self) {
        close_extension_io(&self.io, "Extension session is closed");
        self.io.stdin.lock().await.take();
        let mut process = self.process.lock().await;
        stop_process(&mut process).await;
        self.abort_reader_task();
    }

    fn is_stopped(&self) -> bool {
        self.io.state.lock().map_or(true, |state| state.stopped)
    }

    fn abort_reader_task(&self) {
        if let Ok(mut task) = self.reader_task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

impl Drop for ExtensionSession {
    fn drop(&mut self) {
        close_extension_io(&self.io, "Extension session was dropped");
        if let Ok(mut stdin) = self.io.stdin.try_lock() {
            stdin.take();
        }
        if let Ok(task) = self.reader_task.get_mut()
            && let Some(task) = task.take()
        {
            task.abort();
        }
        let Ok(mut process) = self.process.try_lock() else {
            return;
        };
        if let Some(mut tree) = process.tree.take() {
            tree.terminate();
        }
        let _ = process.child.start_kill();
        if let Some(task) = process.stderr_task.take() {
            task.abort();
        }
        process.stopped = true;
    }
}

async fn send_request(
    io: &ExtensionSessionIo,
    id: u64,
    method: &str,
    params: Value,
) -> std::result::Result<(), String> {
    let request = RpcRequest {
        jsonrpc: "2.0".to_string(),
        id,
        method: method.to_string(),
        params,
    };
    let bytes = serde_json::to_vec(&request).map_err(|err| err.to_string())?;
    if bytes.len() > FRAME_LIMIT {
        return Err(format!(
            "Extension request exceeds {FRAME_LIMIT} byte limit"
        ));
    }
    let mut stdin = io.stdin.lock().await;
    let Some(stdin) = stdin.as_mut() else {
        return Err("Extension stdin unavailable".to_string());
    };
    stdin
        .write_all(&bytes)
        .await
        .map_err(|err| err.to_string())?;
    stdin
        .write_all(b"\n")
        .await
        .map_err(|err| err.to_string())?;
    stdin.flush().await.map_err(|err| err.to_string())
}

async fn read_extension_responses(mut stdout: BufReader<ChildStdout>, io: Arc<ExtensionSessionIo>) {
    loop {
        let line = match read_bounded_json_line(&mut stdout).await {
            Ok(line) => line,
            Err(error) => {
                close_extension_io(&io, &error);
                return;
            }
        };
        match parse_message(line.trim()) {
            ExtensionMessage::Notification => continue,
            ExtensionMessage::Response { id, result } => {
                let sender = io
                    .state
                    .lock()
                    .ok()
                    .and_then(|mut state| state.pending.remove(&id));
                if let Some(sender) = sender {
                    let _ = sender.send(ExtensionCallResult::Response(result));
                }
            }
            ExtensionMessage::ProtocolError(error) => {
                close_extension_io(&io, &format!("Extension protocol error: {error}"));
                return;
            }
        }
    }
}

fn close_extension_io(io: &ExtensionSessionIo, error: &str) {
    let pending = {
        let Ok(mut state) = io.state.lock() else {
            return;
        };
        state.stopped = true;
        std::mem::take(&mut state.pending)
    };
    for sender in pending.into_values() {
        let _ = sender.send(ExtensionCallResult::Closed(error.to_string()));
    }
}

fn rpc_timeout(method: &str) -> Duration {
    if method == "channel/poll" {
        CHANNEL_POLL_TIMEOUT
    } else {
        RPC_TIMEOUT
    }
}

struct PendingExtensionCall {
    id: u64,
    io: Arc<ExtensionSessionIo>,
    active: bool,
}

impl PendingExtensionCall {
    fn new(id: u64, io: Arc<ExtensionSessionIo>) -> Self {
        Self {
            id,
            io,
            active: true,
        }
    }

    fn complete(&mut self) {
        self.active = false;
    }
}

impl Drop for PendingExtensionCall {
    fn drop(&mut self) {
        if self.active
            && let Ok(mut state) = self.io.state.lock()
        {
            state.pending.remove(&self.id);
        }
    }
}

fn parse_message(line: &str) -> ExtensionMessage {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => return ExtensionMessage::ProtocolError(error.to_string()),
    };
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return ExtensionMessage::ProtocolError("message must declare jsonrpc 2.0".to_string());
    }
    if value.get("method").is_some() {
        return if value.get("id").is_none() {
            ExtensionMessage::Notification
        } else {
            ExtensionMessage::ProtocolError(
                "Extension-initiated requests are not supported".to_string(),
            )
        };
    }
    let response: RpcResponse = match serde_json::from_value(value) {
        Ok(response) => response,
        Err(error) => return ExtensionMessage::ProtocolError(error.to_string()),
    };
    let result = match (response.result, response.error) {
        (Some(result), None) => Ok(result),
        (None, Some(error)) => Err(format!("{} ({})", error.message, error.code)),
        _ => Err("response must carry exactly one of result or error".to_string()),
    };
    ExtensionMessage::Response {
        id: response.id,
        result,
    }
}

async fn read_bounded_json_line(
    reader: &mut (impl AsyncBufRead + Unpin),
) -> std::result::Result<String, String> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().await.map_err(|err| err.to_string())?;
        if available.is_empty() {
            return Err("Extension closed stdout before response".to_string());
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if bytes.len().saturating_add(take) > FRAME_LIMIT {
            return Err(format!(
                "Extension response exceeds {FRAME_LIMIT} byte limit"
            ));
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if bytes.last() == Some(&b'\n') {
            break;
        }
    }
    String::from_utf8(bytes).map_err(|err| format!("Extension response is not UTF-8: {err}"))
}

async fn drain_bounded_stderr(
    mut stderr: tokio::process::ChildStderr,
    tail: Arc<SyncMutex<VecDeque<u8>>>,
) {
    let mut chunk = [0_u8; 4096];
    while let Ok(read) = stderr.read(&mut chunk).await {
        if read == 0 {
            break;
        }
        if let Ok(mut tail) = tail.lock() {
            tail.extend(&chunk[..read]);
            while tail.len() > STDERR_LIMIT {
                tail.pop_front();
            }
        }
    }
}

async fn stop_stderr_task(process: &mut ExtensionProcess) {
    if let Some(mut task) = process.stderr_task.take()
        && tokio::time::timeout(Duration::from_millis(100), &mut task)
            .await
            .is_err()
    {
        task.abort();
    }
    if let Ok(mut tail) = process.stderr_tail.lock() {
        tail.clear();
    }
}

async fn stop_process(process: &mut ExtensionProcess) {
    if process.stopped {
        return;
    }
    if let Some(mut tree) = process.tree.take() {
        tree.terminate();
    }
    crate::process_env::terminate_tokio_child_process_group(&mut process.child).await;
    let _ = process.child.wait().await;
    stop_stderr_task(process).await;
    process.stopped = true;
}
