use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::output::bounded_output;

#[derive(Debug)]
pub(crate) struct HookCommandExecution {
    pub(crate) status: Option<ExitStatus>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) elapsed_ms: u128,
    pub(crate) timed_out: bool,
    pub(crate) error: Option<String>,
}

pub(crate) fn run_hook_command_blocking(
    command: &str,
    cwd: &Path,
    payload: &Value,
    timeout_secs: u64,
) -> HookCommandExecution {
    let started = Instant::now();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let mut process = Command::new(shell);
    process
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let inherited_env = std::env::vars().collect::<BTreeMap<_, _>>();
    if let Err(err) = crate::process_env::apply_process_env(
        &mut process,
        &inherited_env,
        crate::process_env::ProcessEnvOptions::new(&[]),
    ) {
        return HookCommandExecution {
            status: None,
            stdout: String::new(),
            stderr: String::new(),
            elapsed_ms: started.elapsed().as_millis(),
            timed_out: false,
            error: Some(err.to_string()),
        };
    }
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(err) => {
            return HookCommandExecution {
                status: None,
                stdout: String::new(),
                stderr: String::new(),
                elapsed_ms: started.elapsed().as_millis(),
                timed_out: false,
                error: Some(err.to_string()),
            };
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.to_string().as_bytes());
    }
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = stdout.map(|mut stdout| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.read_to_end(&mut bytes);
            bytes
        })
    });
    let stderr_reader = stderr.map(|mut stderr| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.read_to_end(&mut bytes);
            bytes
        })
    });

    let timeout = Duration::from_secs(timeout_secs.max(1));
    let status;
    let timed_out;
    loop {
        match child.try_wait() {
            Ok(Some(done)) => {
                status = Some(done);
                timed_out = false;
                break;
            }
            Ok(None) if started.elapsed() >= timeout => {
                crate::process_env::terminate_std_child_tree(&mut child);
                status = child.wait().ok();
                timed_out = true;
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(err) => {
                return HookCommandExecution {
                    status: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    elapsed_ms: started.elapsed().as_millis(),
                    timed_out: false,
                    error: Some(err.to_string()),
                };
            }
        }
    }
    let stdout = stdout_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let stderr = stderr_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    HookCommandExecution {
        status,
        stdout: bounded_output(&stdout),
        stderr: bounded_output(&stderr),
        elapsed_ms: started.elapsed().as_millis(),
        timed_out,
        error: None,
    }
}
