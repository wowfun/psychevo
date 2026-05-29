#[allow(unused_imports)]
use super::*;

pub(crate) fn run_process(
    spec: &CommandManifest,
    task_dir: &Path,
    workspace: &Path,
) -> Result<ProcessOutcome> {
    if spec.command.is_empty() {
        bail!("command declaration is empty");
    }
    let program = resolve_command_part(&spec.command[0], task_dir);
    let mut command = Command::new(program);
    for arg in &spec.command[1..] {
        command.arg(resolve_command_part(arg, task_dir));
    }
    command
        .current_dir(workspace)
        .env("PEVAL_WORKSPACE", workspace)
        .env("PEVAL_TASK_DIR", task_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    wait_for_command(
        command,
        spec.timeout_seconds.map(Duration::from_secs),
        workspace,
    )
}

pub(crate) fn wait_for_command(
    mut command: Command,
    timeout: Option<Duration>,
    workspace: &Path,
) -> Result<ProcessOutcome> {
    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to spawn evaluator command in {}",
            workspace.display()
        )
    })?;
    if let Some(timeout) = timeout {
        let started = Instant::now();
        loop {
            if child.try_wait()?.is_some() {
                let output = child.wait_with_output()?;
                return Ok(ProcessOutcome {
                    success: output.status.success(),
                    code: output.status.code(),
                    timed_out: false,
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                let output = child.wait_with_output()?;
                return Ok(ProcessOutcome {
                    success: false,
                    code: output.status.code(),
                    timed_out: true,
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    let output = child.wait_with_output()?;
    Ok(ProcessOutcome {
        success: output.status.success(),
        code: output.status.code(),
        timed_out: false,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}
