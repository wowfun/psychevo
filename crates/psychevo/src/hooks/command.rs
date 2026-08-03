use std::collections::BTreeMap;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use super::OUTPUT_LIMIT;
use crate::process_tree::ProcessTreeGuard;

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
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let deadline = tokio::time::Instant::now() + timeout;
    let inherited_env = std::env::vars().collect::<BTreeMap<_, _>>();
    let shell = match crate::tools::default_shell_for_env(&inherited_env) {
        Ok(shell) => shell,
        Err(err) => return failed_execution(started, err.to_string()),
    };
    let mut process = tokio::process::Command::new(shell);
    process
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    ProcessTreeGuard::configure_tokio(&mut process);
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
    let mut process_tree = match ProcessTreeGuard::attach_tokio(&child) {
        Ok(process_tree) => process_tree,
        Err(err) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return failed_execution(
                started,
                format!("failed to contain hook process tree: {err}"),
            );
        }
    };

    let mut stdout = child.stdout.take().map(OutputDrain::spawn);
    let mut stderr = child.stderr.take().map(OutputDrain::spawn);
    let stdin_timed_out = if let Some(mut stdin) = child.stdin.take() {
        let payload = payload.to_string();
        tokio::time::timeout_at(deadline, async move {
            let _ = stdin.write_all(payload.as_bytes()).await;
            let _ = stdin.shutdown().await;
        })
        .await
        .is_err()
    } else {
        false
    };

    let (mut status, mut timed_out, error) = if stdin_timed_out {
        (None, true, None)
    } else {
        match tokio::time::timeout_at(deadline, child.wait()).await {
            Ok(Ok(status)) => (Some(status), false, None),
            Ok(Err(err)) => (None, false, Some(err.to_string())),
            Err(_) => (None, true, None),
        }
    };
    if status.is_none() {
        process_tree.terminate();
        let _ = child.kill().await;
        status = tokio::time::timeout(Duration::from_secs(1), child.wait())
            .await
            .ok()
            .and_then(Result::ok);
    }

    if !timed_out
        && error.is_none()
        && !drains_finish_before(&mut stdout, &mut stderr, deadline).await
    {
        timed_out = true;
        process_tree.terminate();
    }
    if timed_out || error.is_some() {
        process_tree.terminate();
        let cleanup_deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        let _ = drains_finish_before(&mut stdout, &mut stderr, cleanup_deadline).await;
    } else {
        process_tree.terminate();
    }
    abort_drains(&mut stdout, &mut stderr);
    let (stdout, stdout_truncated) = output_snapshot(stdout.as_ref());
    let (stderr, stderr_truncated) = output_snapshot(stderr.as_ref());
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

#[derive(Default)]
struct BoundedCapture {
    bytes: Vec<u8>,
    truncated: bool,
}

struct OutputDrain {
    capture: Arc<Mutex<BoundedCapture>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl OutputDrain {
    fn spawn(mut reader: impl AsyncRead + Unpin + Send + 'static) -> Self {
        let capture = Arc::new(Mutex::new(BoundedCapture {
            bytes: Vec::with_capacity(OUTPUT_LIMIT),
            truncated: false,
        }));
        let capture_for_task = Arc::clone(&capture);
        let task = tokio::spawn(async move {
            let mut buffer = [0_u8; 8192];
            loop {
                let read = match reader.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                let mut capture = capture_for_task
                    .lock()
                    .expect("hook output capture lock poisoned");
                let remaining = OUTPUT_LIMIT.saturating_sub(capture.bytes.len());
                let keep = remaining.min(read);
                capture.bytes.extend_from_slice(&buffer[..keep]);
                capture.truncated |= keep < read;
            }
        });
        Self {
            capture,
            task: Some(task),
        }
    }

    async fn wait(&mut self) {
        if let Some(task) = self.task.as_mut() {
            let _ = task.await;
        }
        self.task.take();
    }

    fn abort(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn drains_finish_before(
    stdout: &mut Option<OutputDrain>,
    stderr: &mut Option<OutputDrain>,
    deadline: tokio::time::Instant,
) -> bool {
    tokio::time::timeout_at(deadline, async {
        if let Some(stdout) = stdout.as_mut() {
            stdout.wait().await;
        }
        if let Some(stderr) = stderr.as_mut() {
            stderr.wait().await;
        }
    })
    .await
    .is_ok()
}

fn abort_drains(stdout: &mut Option<OutputDrain>, stderr: &mut Option<OutputDrain>) {
    if let Some(stdout) = stdout.as_mut() {
        stdout.abort();
    }
    if let Some(stderr) = stderr.as_mut() {
        stderr.abort();
    }
}

fn output_snapshot(output: Option<&OutputDrain>) -> (String, bool) {
    let Some(output) = output else {
        return (String::new(), false);
    };
    let capture = output
        .capture
        .lock()
        .expect("hook output capture lock poisoned");
    let truncated = capture.truncated;
    let mut text = crate::process_env::decode_process_output(&capture.bytes)
        .trim()
        .to_string();
    if truncated {
        text.push_str("...[truncated]");
    }
    (text, truncated)
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
