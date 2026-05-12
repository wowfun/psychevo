struct BashTool(WorkdirTool);

const BASH_IO_DRAIN_TIMEOUT_MS: u64 = 2_000;
const BASH_TERMINATE_WAIT_MS: u64 = 2_000;
const READ_CHUNK_BYTES: usize = 8 * 1024;

impl BashTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a bounded foreground bash command in the working directory."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["command"],"properties":{"command":{"type":"string"},"timeout":{"type":"number"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let workdir = self.0.workdir.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match bash_tool_impl(workdir, args, abort).await {
                Ok((value, is_error)) => ToolOutput {
                    json: value,
                    is_error,
                },
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

async fn bash_tool_impl(
    workdir: PathBuf,
    args: Value,
    abort: AbortSignal,
) -> Result<(Value, bool)> {
    let command = required_string(&args, "command")?.to_string();
    let timeout_secs = optional_u64(&args, "timeout")?
        .unwrap_or(BASH_DEFAULT_TIMEOUT_SECS)
        .min(BASH_MAX_TIMEOUT_SECS);
    run_bash_command(workdir, command, timeout_secs, abort).await
}

pub(crate) async fn run_bash_command(
    workdir: PathBuf,
    command: String,
    timeout_secs: u64,
    abort: AbortSignal,
) -> Result<(Value, bool)> {
    let mut child = spawn_bash_child(&workdir, &command)?;
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let stdout_reader = spawn_output_reader(stdout);
    let stderr_reader = spawn_output_reader(stderr);
    let timeout = time::sleep(Duration::from_secs(timeout_secs));
    tokio::pin!(timeout);
    let mut abort = abort;
    let (exit_code, mut error) = tokio::select! {
        biased;
        _ = abort.wait_for_abort() => {
            terminate_child_tree(&mut child);
            wait_after_termination(&mut child).await;
            (None, Some("aborted".to_string()))
        }
        status = child.wait() => {
            match status {
                Ok(status) => (status.code(), None),
                Err(err) => return Err(err.into()),
            }
        }
        _ = &mut timeout => {
            terminate_child_tree(&mut child);
            wait_after_termination(&mut child).await;
            (
                None,
                Some(format!("command timed out after {timeout_secs} seconds")),
            )
        }
    };
    let drain_timeout = Duration::from_millis(BASH_IO_DRAIN_TIMEOUT_MS);
    let (mut output, stderr_output) = tokio::join!(
        collect_output(stdout_reader, drain_timeout),
        collect_output(stderr_reader, drain_timeout)
    );
    output.extend(stderr_output);
    let output = String::from_utf8_lossy(&output).to_string();
    let truncated = truncate_tail(&output, READ_MAX_BYTES, READ_MAX_LINES);
    if exit_code.is_some_and(|code| code != 0) && error.is_none() {
        error = Some(format!(
            "command exited with code {}",
            exit_code.unwrap_or_default()
        ));
    }
    let meaning = exit_code.and_then(|code| exit_code_meaning(&command, code));
    let is_error = error.is_some() || exit_code.is_some_and(|code| code != 0);
    let output_text = if truncated.content.is_empty() {
        "(no output)".to_string()
    } else {
        truncated.content
    };
    Ok((
        json!({
            "output": output_text,
            "exit_code": exit_code,
            "error": error,
            "exit_code_meaning": meaning,
            "truncated": truncated.truncated
        }),
        is_error,
    ))
}

fn spawn_bash_child(workdir: &Path, command: &str) -> Result<tokio::process::Child> {
    let mut child = Command::new("bash");
    child
        .arg("-lc")
        .arg(command)
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_process_group(&mut child);
    Ok(child.spawn()?)
}

struct OutputReader {
    task: tokio::task::JoinHandle<()>,
    buffer: Arc<std::sync::Mutex<Vec<u8>>>,
}

fn spawn_output_reader<R>(mut reader: R) -> OutputReader
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let buffer = Arc::new(std::sync::Mutex::new(Vec::new()));
    let task_buffer = Arc::clone(&buffer);
    let task = tokio::spawn(async move {
        let mut chunk = vec![0u8; READ_CHUNK_BYTES];
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) => break,
                Ok(n) => {
                    let Ok(mut output) = task_buffer.lock() else {
                        break;
                    };
                    output.extend_from_slice(&chunk[..n]);
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });
    OutputReader { task, buffer }
}

async fn collect_output(mut reader: OutputReader, timeout: Duration) -> Vec<u8> {
    if time::timeout(timeout, &mut reader.task).await.is_err() {
        reader.task.abort();
    }
    reader
        .buffer
        .lock()
        .map(|output| output.clone())
        .unwrap_or_default()
}

async fn wait_after_termination(child: &mut tokio::process::Child) {
    let _ = time::timeout(Duration::from_millis(BASH_TERMINATE_WAIT_MS), child.wait()).await;
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    #[cfg(target_os = "linux")]
    let parent_pid = unsafe { libc::getpid() };
    unsafe {
        command.pre_exec(move || {
            detach_from_tty()?;
            #[cfg(target_os = "linux")]
            set_parent_death_signal(parent_pid)?;
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn detach_from_tty() -> std::io::Result<()> {
    let result = unsafe { libc::setsid() };
    if result == -1 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EPERM) {
            return set_process_group();
        }
        return Err(err);
    }
    Ok(())
}

#[cfg(unix)]
fn set_process_group() -> std::io::Result<()> {
    let result = unsafe { libc::setpgid(0, 0) };
    if result == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn set_parent_death_signal(parent_pid: libc::pid_t) -> std::io::Result<()> {
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::getppid() } != parent_pid {
        unsafe {
            libc::raise(libc::SIGTERM);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn terminate_child_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        let _ = kill_process_group_by_pid(pid);
    }
    let _ = child.start_kill();
}

#[cfg(not(unix))]
fn terminate_child_tree(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
}

#[cfg(unix)]
fn kill_process_group_by_pid(pid: u32) -> std::io::Result<()> {
    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    if pgid == -1 {
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err);
        }
        return Ok(());
    }
    let result = unsafe { libc::killpg(pgid, libc::SIGKILL) };
    if result == -1 {
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err);
        }
    }
    Ok(())
}

fn exit_code_meaning(command: &str, code: i32) -> Option<String> {
    if code != 1 {
        return None;
    }
    let first = command.split_whitespace().next().unwrap_or_default();
    match first {
        "grep" | "rg" | "ag" | "ack" => Some("no matches found".to_string()),
        "diff" => Some("files differ".to_string()),
        "test" | "[" => Some("condition evaluated false".to_string()),
        _ => None,
    }
}
