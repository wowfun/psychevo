use std::io::{Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::types::{HookMetadata, HookWorkerAdapter};

pub(crate) fn call_worker_hook(
    worker: &HookWorkerAdapter,
    metadata: &HookMetadata,
    payload: Value,
) -> std::result::Result<Value, String> {
    call_worker_json(
        worker,
        "hooks/call",
        json!({
            "hook": {
                "key": metadata.key,
                "event": metadata.event,
                "matcher": metadata.matcher,
                "handler_type": metadata.handler_type.as_str(),
            },
            "payload": payload,
        }),
    )
}

fn call_worker_json(
    worker: &HookWorkerAdapter,
    method: &str,
    params: Value,
) -> std::result::Result<Value, String> {
    let mut command = Command::new(&worker.command);
    command
        .args(&worker.args)
        .current_dir(&worker.plugin_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process_env::apply_process_env(
        &mut command,
        &worker.env,
        crate::process_env::ProcessEnvOptions::new(&[]),
    )
    .map_err(|err| err.to_string())?;
    command
        .env("PSYCHEVO_PLUGIN_NAME", &worker.plugin_name)
        .env("PSYCHEVO_PLUGIN_ROOT", &worker.plugin_root)
        .env("PSYCHEVO_PLUGIN_DATA", &worker.plugin_data);
    let mut child = command
        .spawn()
        .map_err(|err| format!("failed to start worker {}: {err}", worker.command.display()))?;
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
        let mut text = String::new();
        let mut reader = std::io::BufReader::new(stdout);
        loop {
            text.clear();
            match std::io::BufRead::read_line(&mut reader, &mut text) {
                Ok(0) => {
                    let _ =
                        response_tx.send(Err("worker closed stdout before response".to_string()));
                    break;
                }
                Ok(_) => {
                    let _ = response_tx.send(parse_json_rpc_result(text.trim()));
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
        let _ = std::io::BufReader::new(stderr).read_to_end(&mut bytes);
        let text = crate::process_env::decode_process_output(&bytes);
        let _ = stderr_tx.send(text);
    });
    send_json_rpc(
        &mut stdin,
        1,
        "initialize",
        json!({
            "plugin": {
                "name": worker.plugin_name,
                "version": worker.plugin_version,
                "source": worker.plugin_source,
                "root": worker.plugin_root,
                "data_root": worker.plugin_data,
            },
            "manifest": {
                "path": worker.manifest_path,
                "resources": worker.manifest_resources,
                "psychevo_extensions": worker.psychevo_extensions,
            }
        }),
    )?;
    read_json_rpc_result_timeout(&response_rx, &mut child, "initialize")?;
    send_json_rpc(&mut stdin, 2, method, params)?;
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

fn parse_json_rpc_result(line: &str) -> std::result::Result<Value, String> {
    let value: Value = serde_json::from_str(line).map_err(|err| err.to_string())?;
    if let Some(error) = value.get("error") {
        return Err(error.to_string());
    }
    Ok(value.get("result").cloned().unwrap_or(value))
}

fn read_json_rpc_result_timeout(
    rx: &mpsc::Receiver<std::result::Result<Value, String>>,
    child: &mut std::process::Child,
    method: &str,
) -> std::result::Result<Value, String> {
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            terminate_worker(child);
            Err(format!("timed out waiting for {method} response"))
        }
        Err(err) => Err(err.to_string()),
    }
}

fn wait_worker_exit(child: &mut std::process::Child) -> std::result::Result<ExitStatus, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() > Duration::from_secs(10) => {
                terminate_worker(child);
                return Err("timed out waiting for worker exit".to_string());
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(err) => return Err(err.to_string()),
        }
    }
}

fn terminate_worker(child: &mut std::process::Child) {
    crate::process_env::terminate_std_child_tree(child);
    let _ = child.wait();
}
