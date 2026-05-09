struct BashTool(WorkdirTool);

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
    let mut child = Command::new("bash")
        .arg("-lc")
        .arg(&command)
        .current_dir(&workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf).await;
        buf
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        buf
    });
    let timeout = time::sleep(Duration::from_secs(timeout_secs));
    tokio::pin!(timeout);
    let mut abort = abort;
    let (exit_code, mut error) = tokio::select! {
        biased;
        _ = abort.wait_for_abort() => {
            let _ = child.kill().await;
            (None, Some("aborted".to_string()))
        }
        status = child.wait() => {
            match status {
                Ok(status) => (status.code(), None),
                Err(err) => return Err(err.into()),
            }
        }
        _ = &mut timeout => {
            let _ = child.kill().await;
            (
                None,
                Some(format!("command timed out after {timeout_secs} seconds")),
            )
        }
    };
    let mut output = stdout_task.await.unwrap_or_default();
    output.extend(stderr_task.await.unwrap_or_default());
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
