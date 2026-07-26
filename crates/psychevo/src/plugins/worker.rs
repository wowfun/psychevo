use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use psychevo_agent_core::{
    ToolBinding, ToolDisplayBodyPolicy, ToolDisplayCategory, ToolDisplaySpec, ToolExecutionMode,
    ToolOutput,
};
use psychevo_ai::AbortSignal;
use serde::Serialize;
use serde_json::{Value, json};

use super::manifest::load_plugin_manifest;
use super::types::{LoadedPluginManifest, PluginInstallRecord, PluginWorkerSpec};

#[cfg(not(test))]
const WORKER_RPC_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const WORKER_RPC_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct WorkerToolDescriptor {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) parameters: Value,
}

#[derive(Clone)]
pub(crate) struct PluginWorkerTool {
    pub(crate) record: PluginInstallRecord,
    pub(crate) spec: PluginWorkerSpec,
    pub(crate) descriptor: WorkerToolDescriptor,
    pub(crate) env: BTreeMap<String, String>,
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
        let record = self.record.clone();
        let spec = self.spec.clone();
        let descriptor = self.descriptor.clone();
        let env = self.env.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("plugin worker tool aborted before dispatch");
            }
            let error_plugin = record.name.clone();
            let error_tool = descriptor.name.clone();
            let worker_record = record.clone();
            let worker_descriptor = descriptor.clone();
            match tokio::task::spawn_blocking(move || {
                call_worker_tool(
                    &worker_record,
                    &spec,
                    &env,
                    &worker_descriptor.name,
                    &tool_call_id,
                    args,
                )
            })
            .await
            {
                Ok(Ok(output)) => output,
                Ok(Err(err)) => ToolOutput::error(format!(
                    "plugin `{}` tool `{}` failed: {err}",
                    error_plugin, error_tool
                )),
                Err(err) => ToolOutput::error(format!(
                    "plugin `{}` tool `{}` worker task failed: {err}",
                    error_plugin, error_tool
                )),
            }
        })
    }
}

pub(crate) fn worker_tools(
    record: &PluginInstallRecord,
    manifest: &LoadedPluginManifest,
    spec: &PluginWorkerSpec,
    env: &BTreeMap<String, String>,
) -> std::result::Result<Vec<WorkerToolDescriptor>, String> {
    let result = call_worker_json(record, manifest, spec, env, "contributions/list", json!({}))?;
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

pub(crate) fn call_worker_tool(
    record: &PluginInstallRecord,
    spec: &PluginWorkerSpec,
    env: &BTreeMap<String, String>,
    tool_name: &str,
    tool_call_id: &str,
    args: Value,
) -> std::result::Result<ToolOutput, String> {
    let result = call_worker_json(
        record,
        &load_plugin_manifest(&record.package_root, true).map_err(|err| err.to_string())?,
        spec,
        env,
        "tools/call",
        json!({
            "name": tool_name,
            "tool_call_id": tool_call_id,
            "arguments": args,
        }),
    )?;
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
    if is_error {
        Ok(ToolOutput {
            json: json_value,
            model_content,
            attachments: Vec::new(),
            is_error: true,
        })
    } else {
        Ok(ToolOutput {
            json: json_value,
            model_content,
            attachments: Vec::new(),
            is_error: false,
        })
    }
}

fn call_worker_json(
    record: &PluginInstallRecord,
    manifest: &LoadedPluginManifest,
    spec: &PluginWorkerSpec,
    env: &BTreeMap<String, String>,
    method: &str,
    params: Value,
) -> std::result::Result<Value, String> {
    let mut command = Command::new(&spec.command);
    command
        .args(&spec.args)
        .current_dir(&record.package_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process_env::apply_process_env(
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
    let mut stdin = child
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
    let (response_tx, response_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ =
                        response_tx.send(Err("worker closed stdout before response".to_string()));
                    break;
                }
                Ok(_) => {
                    let _ = response_tx.send(parse_json_rpc_result(line.trim()));
                }
                Err(err) => {
                    let _ = response_tx.send(Err(err.to_string()));
                    break;
                }
            }
        }
    });
    let (stderr_tx, stderr_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = BufReader::new(stderr).read_to_end(&mut bytes);
        let text = crate::process_env::decode_process_output(&bytes);
        let _ = stderr_tx.send(text);
    });

    if let Err(err) = send_json_rpc(
        &mut stdin,
        1,
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
    ) {
        terminate_worker(&mut child);
        return Err(err);
    }
    read_json_rpc_result_timeout(&response_rx, &mut child, "initialize")?;
    if let Err(err) = send_json_rpc(&mut stdin, 2, method, params) {
        terminate_worker(&mut child);
        return Err(err);
    }
    let result = read_json_rpc_result_timeout(&response_rx, &mut child, method)?;
    let _ = send_json_rpc(&mut stdin, 3, "shutdown", json!({}));
    drop(stdin);
    let status = wait_worker_exit(&mut child)?;
    if !status.success() {
        let stderr = stderr_rx
            .recv_timeout(Duration::from_millis(100))
            .unwrap_or_default()
            .trim()
            .to_string();
        if stderr.is_empty() {
            return Err(format!("worker exited with status {status}"));
        }
        return Err(stderr);
    }
    Ok(result)
}

fn send_json_rpc(
    stdin: &mut impl Write,
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
    writeln!(stdin, "{request}").map_err(|err| err.to_string())?;
    stdin.flush().map_err(|err| err.to_string())
}

fn read_json_rpc_result_timeout(
    response_rx: &mpsc::Receiver<std::result::Result<Value, String>>,
    child: &mut Child,
    phase: &str,
) -> std::result::Result<Value, String> {
    match response_rx.recv_timeout(WORKER_RPC_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            terminate_worker(child);
            Err(format!(
                "worker timed out waiting for {phase} response after {}",
                worker_timeout_label()
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!(
            "worker stdout reader stopped before {phase} response"
        )),
    }
}

fn parse_json_rpc_result(line: &str) -> std::result::Result<Value, String> {
    let response: Value = serde_json::from_str(line).map_err(|err| err.to_string())?;
    if let Some(error) = response.get("error") {
        return Err(error.to_string());
    }
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

fn wait_worker_exit(child: &mut Child) -> std::result::Result<ExitStatus, String> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            return Ok(status);
        }
        if started.elapsed() >= WORKER_RPC_TIMEOUT {
            terminate_worker(child);
            return Err(format!(
                "worker timed out waiting for exit after {}",
                worker_timeout_label()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn terminate_worker(child: &mut Child) {
    crate::process_env::terminate_std_child_tree(child);
    let _ = child.wait();
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
