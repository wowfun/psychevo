use std::collections::{BTreeMap, VecDeque};
#[cfg(windows)]
use std::ffi::OsString;
use std::process::Stdio;
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::Duration;

use futures::future::BoxFuture;
use psychevo_agent_core::{
    ToolBinding, ToolDisplayBodyPolicy, ToolDisplayCategory, ToolDisplaySpec, ToolExecutionMode,
    ToolOutput,
};
use psychevo_ai::AbortSignal;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
#[cfg(not(windows))]
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[cfg(test)]
use super::manifest::load_plugin_manifest;
use super::types::{LoadedPluginManifest, PluginInstallRecord, PluginWorkerSpec};

#[cfg(not(test))]
const WORKER_RPC_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const WORKER_RPC_TIMEOUT: Duration = Duration::from_secs(2);
const WORKER_FRAME_LIMIT: usize = 16 * 1024 * 1024;
const WORKER_STDERR_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct WorkerToolDescriptor {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) parameters: Value,
}

#[derive(Clone)]
pub(crate) struct PluginWorkerTool {
    pub(crate) plugin_name: String,
    pub(crate) runtime: Arc<PluginWorkerRuntime>,
    pub(crate) descriptor: WorkerToolDescriptor,
}

impl ToolBinding for PluginWorkerTool {
    fn name(&self) -> &str {
        &self.descriptor.name
    }

    fn description(&self) -> &str {
        &self.descriptor.description
    }

    fn parameters(&self) -> Value {
        self.descriptor.parameters.clone()
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn display_spec(&self) -> ToolDisplaySpec {
        ToolDisplaySpec {
            category: ToolDisplayCategory::Run,
            title_arg_keys: vec!["action".to_string(), "path".to_string()],
            title_result_keys: vec!["status".to_string()],
            summary_keys: vec![
                "plugin".to_string(),
                "tool".to_string(),
                "status".to_string(),
            ],
            body_keys: vec!["content".to_string(), "result".to_string()],
            body_policy: ToolDisplayBodyPolicy::Body,
        }
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let plugin_name = self.plugin_name.clone();
        let runtime = Arc::clone(&self.runtime);
        let descriptor = self.descriptor.clone();
        Box::pin(async move {
            let error_tool = descriptor.name.clone();
            match runtime
                .call_tool(&descriptor.name, &tool_call_id, args, Some(abort))
                .await
            {
                Ok(output) => output,
                Err(err) => ToolOutput::error(format!(
                    "plugin `{}` tool `{}` failed: {err}",
                    plugin_name, error_tool
                )),
            }
        })
    }
}

pub(crate) struct PluginWorkerRuntime {
    record: PluginInstallRecord,
    manifest: LoadedPluginManifest,
    spec: PluginWorkerSpec,
    env: BTreeMap<String, String>,
    session: Mutex<Option<Arc<PluginWorkerSession>>>,
}

impl std::fmt::Debug for PluginWorkerRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginWorkerRuntime")
            .field("plugin", &self.record.name)
            .finish_non_exhaustive()
    }
}

impl PluginWorkerRuntime {
    pub(crate) fn new(
        record: PluginInstallRecord,
        manifest: LoadedPluginManifest,
        spec: PluginWorkerSpec,
        env: BTreeMap<String, String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            record,
            manifest,
            spec,
            env,
            session: Mutex::new(None),
        })
    }

    pub(crate) fn plugin_name(&self) -> &str {
        &self.record.name
    }

    pub(crate) fn source_id(&self) -> String {
        format!("plugin:{}@{}", self.record.name, self.record.source_slug)
    }

    async fn session(&self) -> std::result::Result<Arc<PluginWorkerSession>, String> {
        let mut session = self.session.lock().await;
        if let Some(session) = session.as_ref() {
            return Ok(Arc::clone(session));
        }
        let started =
            PluginWorkerSession::start(&self.record, &self.manifest, &self.spec, &self.env).await?;
        *session = Some(Arc::clone(&started));
        Ok(started)
    }

    pub(crate) async fn tools(&self) -> std::result::Result<Vec<WorkerToolDescriptor>, String> {
        let session = self.session().await?;
        worker_tools_in_session(&session).await
    }

    pub(crate) async fn call(
        &self,
        method: &str,
        params: Value,
    ) -> std::result::Result<Value, String> {
        self.session().await?.call(method, params).await
    }

    async fn call_tool(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        args: Value,
        abort: Option<AbortSignal>,
    ) -> std::result::Result<ToolOutput, String> {
        let session = self.session().await?;
        call_worker_tool_in_session(&session, tool_name, tool_call_id, args, abort).await
    }

    pub(crate) async fn shutdown(&self) -> std::result::Result<(), String> {
        let session = self.session.lock().await.take();
        match session {
            Some(session) => session.shutdown().await,
            None => Ok(()),
        }
    }

    #[cfg(test)]
    pub(crate) async fn started(&self) -> bool {
        self.session.lock().await.is_some()
    }
}

pub(crate) struct PluginWorkerSession {
    process: Mutex<PluginWorkerProcess>,
}

struct PluginWorkerProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr_tail: Arc<SyncMutex<VecDeque<u8>>>,
    stderr_task: Option<JoinHandle<()>>,
    next_id: u64,
    stopped: bool,
}

enum WorkerMessage {
    Response {
        id: u64,
        result: std::result::Result<Value, String>,
    },
    Notification,
    ProtocolError(String),
}

impl std::fmt::Debug for PluginWorkerSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PluginWorkerSession(..)")
    }
}

impl PluginWorkerSession {
    pub(crate) async fn start(
        record: &PluginInstallRecord,
        manifest: &LoadedPluginManifest,
        spec: &PluginWorkerSpec,
        env: &BTreeMap<String, String>,
    ) -> std::result::Result<Arc<Self>, String> {
        #[cfg(windows)]
        let mut command = {
            let args = spec.args.iter().map(OsString::from).collect::<Vec<_>>();
            crate::process_env::tokio_host_process_command(
                &spec.command,
                &args,
                crate::host_paths::HostPlatform::current(),
                env,
            )
            .map_err(|err| err.to_string())?
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new(&spec.command);
            command.args(&spec.args);
            command
        };
        command
            .current_dir(&record.package_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::process_env::apply_tokio_process_env(
            &mut command,
            env,
            crate::process_env::ProcessEnvOptions::new(&[]),
        )
        .map_err(|err| err.to_string())?;
        command
            .env("PSYCHEVO_PLUGIN_NAME", &record.name)
            .env("PSYCHEVO_PLUGIN_ROOT", &record.package_root)
            .env("PSYCHEVO_PLUGIN_DATA", &record.data_root);
        let mut child = command
            .spawn()
            .map_err(|err| format!("failed to start worker {}: {err}", spec.command.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "worker stdin unavailable".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "worker stdout unavailable".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "worker stderr unavailable".to_string())?;
        let stderr_tail = Arc::new(SyncMutex::new(VecDeque::with_capacity(
            WORKER_STDERR_LIMIT,
        )));
        let stderr_task = tokio::spawn(drain_bounded_stderr(stderr, Arc::clone(&stderr_tail)));
        let session = Arc::new(Self {
            process: Mutex::new(PluginWorkerProcess {
                child,
                stdin: Some(stdin),
                stdout: BufReader::new(stdout),
                stderr_tail,
                stderr_task: Some(stderr_task),
                next_id: 1,
                stopped: false,
            }),
        });
        session
            .call(
                "initialize",
                json!({
                    "plugin": {
                        "name": record.name,
                        "version": record.version,
                        "source": record.source_slug,
                        "root": record.package_root,
                        "data_root": record.data_root,
                    },
                    "manifest": {
                        "path": manifest.manifest_path,
                        "resources": manifest.manifest_resources.iter().cloned().collect::<Vec<_>>(),
                        "psychevo_extensions": manifest.psychevo_extensions.iter().cloned().collect::<Vec<_>>(),
                    }
                }),
            )
            .await?;
        Ok(session)
    }

    pub(crate) async fn call(
        &self,
        method: &str,
        params: Value,
    ) -> std::result::Result<Value, String> {
        self.call_with_abort(method, params, None).await
    }

    async fn call_with_abort(
        &self,
        method: &str,
        params: Value,
        mut abort: Option<AbortSignal>,
    ) -> std::result::Result<Value, String> {
        if abort.as_ref().is_some_and(AbortSignal::aborted) {
            return Err(format!("worker {method} aborted before dispatch"));
        }
        let mut process = self.process.lock().await;
        if process.stopped {
            return Err("plugin worker session is closed".to_string());
        }
        let id = process.next_id;
        process.next_id = process.next_id.saturating_add(1);
        if let Err(error) = send_json_rpc(&mut process, id, method, params).await {
            stop_worker(&mut process).await;
            return Err(error);
        }

        let response = wait_for_response(&mut process, id);
        let outcome = if let Some(abort) = abort.as_mut() {
            tokio::select! {
                result = tokio::time::timeout(WORKER_RPC_TIMEOUT, response) => {
                    result.map_err(|_| format!(
                        "worker timed out waiting for {method} response after {}",
                        worker_timeout_label()
                    ))
                }
                _ = abort.wait_for_abort() => {
                    Err(format!("worker {method} aborted"))
                }
            }
        } else {
            tokio::time::timeout(WORKER_RPC_TIMEOUT, response)
                .await
                .map_err(|_| {
                    format!(
                        "worker timed out waiting for {method} response after {}",
                        worker_timeout_label()
                    )
                })
        };
        match outcome {
            Ok(Ok(result)) => result,
            Ok(Err(error)) | Err(error) => {
                stop_worker(&mut process).await;
                Err(error)
            }
        }
    }

    pub(crate) async fn shutdown(&self) -> std::result::Result<(), String> {
        let mut process = self.process.lock().await;
        if process.stopped {
            return Ok(());
        }
        let id = process.next_id;
        let _ = send_json_rpc(&mut process, id, "shutdown", json!({})).await;
        process.stdin.take();
        let status = match tokio::time::timeout(WORKER_RPC_TIMEOUT, process.child.wait()).await {
            Ok(result) => result.map_err(|err| err.to_string())?,
            Err(_) => {
                crate::process_env::terminate_tokio_child_tree(&mut process.child).await;
                let _ = process.child.wait().await;
                process.stopped = true;
                stop_stderr_task(&mut process).await;
                return Err(format!(
                    "worker timed out waiting for exit after {}",
                    worker_timeout_label()
                ));
            }
        };
        process.stopped = true;
        stop_stderr_task(&mut process).await;
        if status.success() {
            return Ok(());
        }
        let stderr = stderr_text(&process).trim().to_string();
        if stderr.is_empty() {
            Err(format!("worker exited with status {status}"))
        } else {
            Err(stderr)
        }
    }
}

impl Drop for PluginWorkerSession {
    fn drop(&mut self) {
        let Ok(mut process) = self.process.try_lock() else {
            return;
        };
        process.stdin.take();
        let _ = process.child.start_kill();
        if let Some(task) = process.stderr_task.take() {
            task.abort();
        }
        process.stopped = true;
    }
}

pub(crate) async fn worker_tools(
    record: &PluginInstallRecord,
    manifest: &LoadedPluginManifest,
    spec: &PluginWorkerSpec,
    env: &BTreeMap<String, String>,
) -> std::result::Result<Vec<WorkerToolDescriptor>, String> {
    let session = PluginWorkerSession::start(record, manifest, spec, env).await?;
    let result = worker_tools_in_session(&session).await;
    let shutdown = session.shutdown().await;
    let result = result?;
    shutdown?;
    Ok(result)
}

pub(crate) async fn worker_tools_in_session(
    session: &PluginWorkerSession,
) -> std::result::Result<Vec<WorkerToolDescriptor>, String> {
    let result = session.call("contributions/list", json!({})).await?;
    let tools = result
        .get("tools")
        .or_else(|| result.pointer("/capabilities/tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for tool in tools {
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        let name = sanitize_tool_name(name);
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Plugin worker tool")
            .to_string();
        let parameters = tool
            .get("parameters")
            .or_else(|| tool.get("input_schema"))
            .or_else(|| tool.get("inputSchema"))
            .cloned()
            .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
        out.push(WorkerToolDescriptor {
            name,
            description,
            parameters,
        });
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) async fn call_worker_tool(
    record: &PluginInstallRecord,
    spec: &PluginWorkerSpec,
    env: &BTreeMap<String, String>,
    tool_name: &str,
    tool_call_id: &str,
    args: Value,
) -> std::result::Result<ToolOutput, String> {
    let manifest =
        load_plugin_manifest(&record.package_root, true).map_err(|err| err.to_string())?;
    let session = PluginWorkerSession::start(record, &manifest, spec, env).await?;
    let result =
        call_worker_tool_in_session(&session, tool_name, tool_call_id, args, None).await;
    let shutdown = session.shutdown().await;
    let result = result?;
    shutdown?;
    Ok(result)
}

pub(crate) async fn call_worker_tool_in_session(
    session: &PluginWorkerSession,
    tool_name: &str,
    tool_call_id: &str,
    args: Value,
    abort: Option<AbortSignal>,
) -> std::result::Result<ToolOutput, String> {
    let result = session
        .call_with_abort(
            "tools/call",
            json!({
                "name": tool_name,
                "tool_call_id": tool_call_id,
                "arguments": args,
            }),
            abort,
        )
        .await?;
    let is_error = result
        .get("is_error")
        .or_else(|| result.get("isError"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let model_content = result
        .get("model_content")
        .or_else(|| result.get("modelContent"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            result
                .get("content")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let json_value = result.get("json").cloned().unwrap_or(result);
    Ok(ToolOutput {
        json: json_value,
        model_content,
        attachments: Vec::new(),
        is_error,
    })
}

async fn send_json_rpc(
    process: &mut PluginWorkerProcess,
    id: u64,
    method: &str,
    params: Value,
) -> std::result::Result<(), String> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    let bytes = serde_json::to_vec(&request).map_err(|err| err.to_string())?;
    if bytes.len() > WORKER_FRAME_LIMIT {
        return Err(format!(
            "worker request exceeds {} byte limit",
            WORKER_FRAME_LIMIT
        ));
    }
    let Some(stdin) = process.stdin.as_mut() else {
        return Err("worker stdin unavailable".to_string());
    };
    stdin
        .write_all(&bytes)
        .await
        .map_err(|err| err.to_string())?;
    stdin.write_all(b"\n").await.map_err(|err| err.to_string())?;
    stdin.flush().await.map_err(|err| err.to_string())
}

fn parse_worker_message(line: &str) -> WorkerMessage {
    let response: Value = match serde_json::from_str(line) {
        Ok(response) => response,
        Err(error) => return WorkerMessage::ProtocolError(error.to_string()),
    };
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return WorkerMessage::ProtocolError(
            "worker message must declare jsonrpc 2.0".to_string(),
        );
    }
    if response.get("method").is_some() {
        return if response.get("id").is_none() {
            WorkerMessage::Notification
        } else {
            WorkerMessage::ProtocolError(
                "worker-initiated requests are not supported".to_string(),
            )
        };
    }
    let Some(id) = response.get("id").and_then(Value::as_u64) else {
        return WorkerMessage::ProtocolError(
            "worker response must carry a numeric id".to_string(),
        );
    };
    let result = if let Some(error) = response.get("error") {
        Err(error.to_string())
    } else if response.get("result").is_some() {
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    } else {
        Err("worker response must carry result or error".to_string())
    };
    WorkerMessage::Response { id, result }
}

async fn wait_for_response(
    process: &mut PluginWorkerProcess,
    id: u64,
) -> std::result::Result<std::result::Result<Value, String>, String> {
    loop {
        let line = read_bounded_json_line(&mut process.stdout).await?;
        match parse_worker_message(line.trim()) {
            WorkerMessage::Notification => continue,
            WorkerMessage::Response {
                id: response_id,
                result,
            } if response_id == id => return Ok(result),
            WorkerMessage::Response {
                id: response_id, ..
            } => {
                return Err(format!(
                    "worker response id {response_id} does not match request id {id}"
                ));
            }
            WorkerMessage::ProtocolError(error) => {
                return Err(format!("worker protocol error: {error}"));
            }
        }
    }
}

async fn read_bounded_json_line(
    reader: &mut (impl AsyncBufRead + Unpin),
) -> std::result::Result<String, String> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().await.map_err(|err| err.to_string())?;
        if available.is_empty() {
            return Err("worker closed stdout before response".to_string());
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if bytes.len().saturating_add(take) > WORKER_FRAME_LIMIT {
            return Err(format!(
                "worker response exceeds {} byte limit",
                WORKER_FRAME_LIMIT
            ));
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if bytes.last() == Some(&b'\n') {
            break;
        }
    }
    String::from_utf8(bytes).map_err(|err| format!("worker response is not UTF-8: {err}"))
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
            while tail.len() > WORKER_STDERR_LIMIT {
                tail.pop_front();
            }
        }
    }
}

async fn stop_stderr_task(process: &mut PluginWorkerProcess) {
    if let Some(mut task) = process.stderr_task.take()
        && tokio::time::timeout(Duration::from_millis(100), &mut task)
            .await
            .is_err()
    {
        task.abort();
    }
}

fn stderr_text(process: &PluginWorkerProcess) -> String {
    let bytes = process
        .stderr_tail
        .lock()
        .map(|tail| tail.iter().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    crate::process_env::decode_process_output(&bytes)
}

async fn stop_worker(process: &mut PluginWorkerProcess) {
    if process.stopped {
        return;
    }
    process.stdin.take();
    crate::process_env::terminate_tokio_child_tree(&mut process.child).await;
    let _ = process.child.wait().await;
    stop_stderr_task(process).await;
    process.stopped = true;
}

fn worker_timeout_label() -> String {
    if WORKER_RPC_TIMEOUT.as_secs() > 0 {
        format!("{}s", WORKER_RPC_TIMEOUT.as_secs())
    } else {
        format!("{}ms", WORKER_RPC_TIMEOUT.as_millis())
    }
}

fn sanitize_tool_name(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "plugin_tool".to_string()
    } else if out.as_bytes()[0].is_ascii_digit() {
        format!("plugin_{out}")
    } else {
        out
    }
}
