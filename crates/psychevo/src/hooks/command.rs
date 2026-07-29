use std::collections::BTreeMap;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use super::OUTPUT_LIMIT;

#[derive(Debug)]
pub(crate) struct HookCommandExecution {
    pub(crate) status: Option<ExitStatus>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_truncated: bool,
    pub(crate) elapsed_ms: u128,
    pub(crate) timed_out: bool,
    pub(crate) error: Option<String>,
}

pub(crate) async fn run_hook_command(
    command: &str,
    cwd: &Path,
    payload: &Value,
    timeout_secs: u64,
) -> HookCommandExecution {
    let started = Instant::now();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let mut process = tokio::process::Command::new(shell);
    process
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut process);
    let inherited_env = std::env::vars().collect::<BTreeMap<_, _>>();
    if let Err(err) = crate::process_env::apply_tokio_process_env(
        &mut process,
        &inherited_env,
        crate::process_env::ProcessEnvOptions::new(&[]),
    ) {
        return failed_execution(started, err.to_string());
    }
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(err) => return failed_execution(started, err.to_string()),
    };

    if let Some(mut stdin) = child.stdin.take() {
        let payload = payload.to_string();
        let _ = stdin.write_all(payload.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }
    let stdout_task = child
        .stdout
        .take()
        .map(|stdout| tokio::spawn(drain_bounded(stdout)));
    let stderr_task = child
        .stderr
        .take()
        .map(|stderr| tokio::spawn(drain_bounded(stderr)));

    let timeout = Duration::from_secs(timeout_secs.max(1));
    let (status, timed_out, error) = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => (Some(status), false, None),
        Ok(Err(err)) => {
            crate::process_env::terminate_tokio_child_process_group(&mut child).await;
            (None, false, Some(err.to_string()))
        }
        Err(_) => {
            crate::process_env::terminate_tokio_child_process_group(&mut child).await;
            (child.wait().await.ok(), true, None)
        }
    };
    let (stdout, stdout_truncated) = collect_output(stdout_task).await;
    let (stderr, stderr_truncated) = collect_output(stderr_task).await;
    HookCommandExecution {
        status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        elapsed_ms: started.elapsed().as_millis(),
        timed_out,
        error,
    }
}

fn failed_execution(started: Instant, error: String) -> HookCommandExecution {
    HookCommandExecution {
        status: None,
        stdout: String::new(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        elapsed_ms: started.elapsed().as_millis(),
        timed_out: false,
        error: Some(error),
    }
}

async fn drain_bounded(mut reader: impl AsyncRead + Unpin) -> (Vec<u8>, bool) {
    let mut retained = Vec::with_capacity(OUTPUT_LIMIT);
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    while let Ok(read) = reader.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        let remaining = OUTPUT_LIMIT.saturating_sub(retained.len());
        let keep = remaining.min(read);
        retained.extend_from_slice(&buffer[..keep]);
        truncated |= keep < read;
    }
    (retained, truncated)
}

async fn collect_output(
    task: Option<tokio::task::JoinHandle<(Vec<u8>, bool)>>,
) -> (String, bool) {
    let (bytes, truncated) = match task {
        Some(task) => task.await.unwrap_or_default(),
        None => (Vec::new(), false),
    };
    let mut text = crate::process_env::decode_process_output(&bytes)
        .trim()
        .to_string();
    if truncated {
        text.push_str("...[truncated]");
    }
    (text, truncated)
}

#[cfg(unix)]
fn configure_process_group(command: &mut tokio::process::Command) {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut tokio::process::Command) {}
