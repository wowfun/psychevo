#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

#[allow(unused_imports)]
use serde_json::json;

pub(crate) struct ExecCommandTool(WorkdirTool);

pub(crate) struct WriteStdinTool;

pub(crate) static EXEC_SESSIONS: LazyLock<Mutex<ExecSessionRegistry>> =
    LazyLock::new(|| Mutex::new(ExecSessionRegistry::default()));

pub(crate) const EXEC_OUTPUT_MAX_BYTES: usize = 1024 * 1024;
pub(crate) const PTY_FALLBACK_NOTICE: &str =
    "[exec_command] tty=true requested but PTY was unavailable; running with pipes instead.\n";

impl ExecCommandTool {
    pub(crate) fn new(workdir: PathBuf, context: ToolRuntimeContext) -> Self {
        Self(WorkdirTool::with_context(workdir, context))
    }
}

impl WriteStdinTool {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl ToolBinding for ExecCommandTool {
    fn name(&self) -> &str {
        "exec_command"
    }

    fn description(&self) -> &str {
        "Run a bounded shell command in the working directory. Prefer read/write/edit for workspace file I/O, and use shell redirection only for shell-local temporary artifacts. Prefer rg for text search and rg --files for project file listing. Commands that keep running return a session_id after yield_time_ms; use write_stdin with empty chars to poll or non-empty chars to send stdin."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["cmd"],
            "properties": {
                "cmd": {
                    "type": "string",
                    "description": "Shell command to run."
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for the command. Relative paths resolve against the accepted workdir; absolute paths must pass permission and resource gates."
                },
                "shell": {
                    "type": "string",
                    "description": "Shell executable to use. Defaults to the user's shell."
                },
                "tty": {
                    "type": "boolean",
                    "default": false,
                    "description": "Run the command in a PTY and keep stdin writable. If the PTY backend is unavailable, runtime falls back to writable pipes and prefixes the first output chunk with a notice."
                },
                "yield_time_ms": {
                    "type": "integer",
                    "default": EXEC_DEFAULT_YIELD_TIME_MS,
                    "minimum": EXEC_MIN_YIELD_TIME_MS,
                    "maximum": EXEC_MAX_YIELD_TIME_MS,
                    "description": "Milliseconds to wait for completion before returning a session_id for a still-running command. Values outside the range are clamped."
                },
                "max_output_tokens": {
                    "type": "integer",
                    "default": DEFAULT_MAX_OUTPUT_TOKENS,
                    "minimum": 1,
                    "description": "Maximum model-visible output tokens for this result."
                },
                "login": {
                    "type": "boolean",
                    "default": false,
                    "description": "Run the shell as a login shell. Disabled unless permissions.allow_login_shell is true."
                }
            }
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let accepted_workdir = self.0.workdir.clone();
        let context = self.0.context.clone();
        Box::pin(async move {
            match exec_command_tool_impl_with_context(
                accepted_workdir,
                context,
                tool_call_id,
                args,
                abort,
            )
            .await
            {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

impl ToolBinding for WriteStdinTool {
    fn name(&self) -> &str {
        "write_stdin"
    }

    fn description(&self) -> &str {
        "Poll a yielded exec_command session or write text to its stdin. Empty chars means poll; non-empty chars requires a stdin-capable session started with tty=true or PTY fallback."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["session_id"],
            "properties": {
                "session_id": {
                    "type": "integer",
                    "description": "Session id returned by exec_command."
                },
                "chars": {
                    "type": "string",
                    "default": "",
                    "description": "Text to write to stdin. Empty string polls without writing."
                },
                "yield_time_ms": {
                    "type": "integer",
                    "default": WRITE_STDIN_DEFAULT_YIELD_TIME_MS,
                    "description": "Milliseconds to wait before returning output. Empty polls clamp to 5000..300000; non-empty writes clamp to 250..30000."
                },
                "max_output_tokens": {
                    "type": "integer",
                    "default": DEFAULT_MAX_OUTPUT_TOKENS,
                    "minimum": 1,
                    "description": "Maximum model-visible output tokens for this result."
                }
            }
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        Box::pin(async move {
            match write_stdin_tool_impl_with_call(tool_call_id, args, abort).await {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

#[cfg(test)]
pub(crate) async fn exec_command_tool_impl(
    accepted_workdir: PathBuf,
    allow_login_shell: bool,
    args: Value,
    abort: AbortSignal,
) -> Result<Value> {
    exec_command_tool_impl_with_context(
        accepted_workdir,
        ToolRuntimeContext {
            task_id: "default".to_string(),
            lsp: LspConfig::default(),
            lsp_manager: default_lsp_manager(),
            allow_login_shell,
            stream_events: None,
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            sandbox_policy: SandboxPolicy::disabled(),
            sandbox_grants: crate::sandbox::SandboxWriteGrants::default(),
        },
        "exec_command".to_string(),
        args,
        abort,
    )
    .await
}

pub(crate) async fn exec_command_tool_impl_with_context(
    accepted_workdir: PathBuf,
    context: ToolRuntimeContext,
    tool_call_id: String,
    args: Value,
    abort: AbortSignal,
) -> Result<Value> {
    let cmd = required_string(&args, "cmd")?.to_string();
    if cmd.trim().is_empty() {
        return Err(Error::Message("cmd must not be empty".to_string()));
    }
    reject_untracked_background_command(&cmd)?;
    let workdir = resolve_exec_workdir(&accepted_workdir, optional_string(&args, "workdir")?)?;
    let shell = optional_string(&args, "shell")?
        .map(ToOwned::to_owned)
        .unwrap_or_else(default_shell);
    let tty = optional_bool(&args, "tty")?.unwrap_or(false);
    if tty && context.sandbox_policy.enabled {
        return Err(crate::sandbox::sandbox_denied(
            "tty=true is not supported when sandbox is enabled",
        ));
    }
    let login = optional_bool(&args, "login")?.unwrap_or(false);
    if login && !context.allow_login_shell {
        return Err(Error::Message(
            "login shell disabled; set permissions.allow_login_shell=true to allow login=true"
                .to_string(),
        ));
    }
    let yield_ms = clamp_yield_ms(
        optional_i64(&args, "yield_time_ms")?,
        EXEC_DEFAULT_YIELD_TIME_MS,
        EXEC_MIN_YIELD_TIME_MS,
        EXEC_MAX_YIELD_TIME_MS,
    );
    let max_output_tokens = output_token_limit(optional_i64(&args, "max_output_tokens")?)?;
    let invocation = ExecInvocation {
        cmd,
        workdir,
        shell,
        login,
        tty,
        path_prefixes: context.path_prefixes.clone(),
        sandbox_policy: context.sandbox_policy.clone(),
    };
    let session = spawn_exec_session(
        invocation,
        ExecSessionContext {
            task_id: context.task_id,
            root_tool_call_id: tool_call_id,
            stream_events: context.stream_events,
        },
    )?;
    await_session_result(
        session,
        Duration::from_millis(yield_ms),
        max_output_tokens,
        abort,
        true,
    )
    .await
}

#[cfg(test)]
pub(crate) async fn write_stdin_tool_impl(args: Value, abort: AbortSignal) -> Result<Value> {
    write_stdin_tool_impl_with_call("write_stdin".to_string(), args, abort).await
}

pub(crate) async fn write_stdin_tool_impl_with_call(
    tool_call_id: String,
    args: Value,
    abort: AbortSignal,
) -> Result<Value> {
    let session_id = required_u64(&args, "session_id")?;
    let chars = optional_string(&args, "chars")?.unwrap_or("").to_string();
    let yield_ms = if chars.is_empty() {
        clamp_yield_ms(
            optional_i64(&args, "yield_time_ms")?,
            WRITE_STDIN_DEFAULT_YIELD_TIME_MS,
            EMPTY_POLL_MIN_YIELD_TIME_MS,
            EMPTY_POLL_MAX_YIELD_TIME_MS,
        )
    } else {
        clamp_yield_ms(
            optional_i64(&args, "yield_time_ms")?,
            WRITE_STDIN_DEFAULT_YIELD_TIME_MS,
            EXEC_MIN_YIELD_TIME_MS,
            EXEC_MAX_YIELD_TIME_MS,
        )
    };
    let max_output_tokens = output_token_limit(optional_i64(&args, "max_output_tokens")?)?;
    let session = get_exec_session(session_id)
        .ok_or_else(|| Error::Message(format!("unknown exec_command session_id: {session_id}")))?;
    if !chars.is_empty() {
        session.write_stdin(chars.as_bytes())?;
        session.emit_stdin(&tool_call_id, &chars);
    }
    await_session_result(
        session,
        Duration::from_millis(yield_ms),
        max_output_tokens,
        abort,
        false,
    )
    .await
}

pub(crate) async fn run_exec_command_for_user_shell(
    workdir: PathBuf,
    command: String,
    sandbox_policy: SandboxPolicy,
    abort: AbortSignal,
) -> Result<(Value, bool)> {
    let invocation = ExecInvocation {
        cmd: command,
        workdir,
        shell: default_shell(),
        login: false,
        tty: false,
        path_prefixes: Vec::new(),
        sandbox_policy,
    };
    let session = spawn_exec_session(
        invocation,
        ExecSessionContext {
            task_id: "user_shell".to_string(),
            root_tool_call_id: "user_shell".to_string(),
            stream_events: None,
        },
    )?;
    let started = Instant::now();
    let mut output = String::new();
    let mut final_value = await_session_result(
        Arc::clone(&session),
        Duration::from_millis(EXEC_MAX_YIELD_TIME_MS),
        DEFAULT_MAX_OUTPUT_TOKENS,
        abort.clone(),
        false,
    )
    .await?;
    output.push_str(final_value["output"].as_str().unwrap_or_default());
    while final_value["session_id"].is_u64() && started.elapsed() < Duration::from_secs(120) {
        let Some(session_id) = final_value["session_id"].as_u64() else {
            break;
        };
        let Some(session) = get_exec_session(session_id) else {
            break;
        };
        final_value = await_session_result(
            session,
            Duration::from_millis(EXEC_MAX_YIELD_TIME_MS),
            DEFAULT_MAX_OUTPUT_TOKENS,
            abort.clone(),
            false,
        )
        .await?;
        output.push_str(final_value["output"].as_str().unwrap_or_default());
    }
    if final_value["session_id"].is_u64() {
        session.kill();
        remove_exec_session(session.id);
        return Err(Error::Message(
            "command timed out after 120 seconds".to_string(),
        ));
    }
    final_value["output"] = Value::String(output);
    Ok((final_value, false))
}

#[derive(Clone)]
pub(crate) struct ExecInvocation {
    pub(crate) cmd: String,
    pub(crate) workdir: PathBuf,
    pub(crate) shell: String,
    pub(crate) login: bool,
    pub(crate) tty: bool,
    pub(crate) path_prefixes: Vec<PathBuf>,
    pub(crate) sandbox_policy: SandboxPolicy,
}

#[derive(Default)]
pub(crate) struct ExecSessionRegistry {
    pub(crate) next_id: u64,
    pub(crate) sessions: HashMap<u64, Arc<ExecSession>>,
}

#[derive(Clone)]
pub(crate) struct ExecSessionContext {
    pub(crate) task_id: String,
    pub(crate) root_tool_call_id: String,
    pub(crate) stream_events: Option<RunStreamSink>,
}

pub(crate) struct ExecSession {
    pub(crate) id: u64,
    pub(crate) task_id: String,
    pub(crate) root_tool_call_id: String,
    pub(crate) cmd: String,
    pub(crate) workdir: PathBuf,
    pub(crate) stream_events: Option<RunStreamSink>,
    pub(crate) started: Instant,
    pub(crate) started_at_ms: i64,
    pub(crate) process: ExecProcess,
    pub(crate) state: Mutex<ExecSessionState>,
    pub(crate) stdin: Mutex<Option<Box<dyn Write + Send>>>,
    pub(crate) stdin_allowed: bool,
}

pub(crate) struct ExecSessionState {
    pub(crate) output: Vec<u8>,
    pub(crate) read_offset: usize,
    pub(crate) readers_active: usize,
    pub(crate) exited: bool,
    pub(crate) exit_code: Option<i32>,
    pub(crate) chunk_id: u64,
    pub(crate) output_seq: u64,
    pub(crate) yielded: bool,
    pub(crate) finish_emitted: bool,
    pub(crate) interrupted: bool,
}

pub(crate) enum ExecProcess {
    Pipe(Arc<Mutex<std::process::Child>>),
    Pty(Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>),
}

pub(crate) struct ExecSessionIo {
    pub(crate) process: ExecProcess,
    pub(crate) readers_active: usize,
    pub(crate) stdin: Option<Box<dyn Write + Send>>,
    pub(crate) stdin_allowed: bool,
    pub(crate) initial_output: Vec<u8>,
}

impl ExecSession {
    pub(crate) fn new(
        id: u64,
        context: ExecSessionContext,
        invocation: &ExecInvocation,
        io: ExecSessionIo,
    ) -> Self {
        Self {
            id,
            task_id: context.task_id,
            root_tool_call_id: context.root_tool_call_id,
            cmd: invocation.cmd.clone(),
            workdir: invocation.workdir.clone(),
            stream_events: context.stream_events,
            started: Instant::now(),
            started_at_ms: now_unix_ms(),
            process: io.process,
            state: Mutex::new(ExecSessionState {
                output: io.initial_output,
                read_offset: 0,
                readers_active: io.readers_active,
                exited: false,
                exit_code: None,
                chunk_id: 0,
                output_seq: 0,
                yielded: false,
                finish_emitted: false,
                interrupted: false,
            }),
            stdin: Mutex::new(io.stdin),
            stdin_allowed: io.stdin_allowed,
        }
    }

    pub(crate) fn append_output(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let event = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            state.output.extend_from_slice(bytes);
            if state.output.len() > EXEC_OUTPUT_MAX_BYTES {
                let excess = state.output.len() - EXEC_OUTPUT_MAX_BYTES;
                state.output.drain(..excess);
                state.read_offset = state.read_offset.saturating_sub(excess);
            }
            if !state.yielded || state.finish_emitted {
                None
            } else {
                let seq = state.output_seq;
                state.output_seq = state.output_seq.saturating_add(1);
                Some(json!({
                    "type": "exec_session_output_delta",
                    "session_id": self.id,
                    "tool_call_id": self.root_tool_call_id.clone(),
                    "seq": seq,
                    "output": String::from_utf8_lossy(bytes).to_string(),
                }))
            }
        };
        self.emit_event(event);
    }

    pub(crate) fn reader_done(&self) {
        let event = if let Ok(mut state) = self.state.lock() {
            state.readers_active = state.readers_active.saturating_sub(1);
            self.finish_event_locked(&mut state)
        } else {
            None
        };
        self.emit_event(event);
    }

    pub(crate) fn mark_exit(&self, exit_code: Option<i32>) {
        let event = if let Ok(mut state) = self.state.lock() {
            state.exited = true;
            state.exit_code = exit_code;
            self.finish_event_locked(&mut state)
        } else {
            None
        };
        self.emit_event(event);
    }

    pub(crate) fn interrupt(&self) {
        self.process.kill();
        let event = if let Ok(mut state) = self.state.lock() {
            state.interrupted = true;
            state.exited = true;
            self.finish_event_locked(&mut state)
        } else {
            None
        };
        self.emit_event(event);
    }

    pub(crate) fn finish_event_locked(&self, state: &mut ExecSessionState) -> Option<Value> {
        if state.finish_emitted
            || !state.yielded
            || !state.exited
            || (state.readers_active != 0 && !state.interrupted)
        {
            return None;
        }
        state.finish_emitted = true;
        Some(json!({
            "type": "exec_session_finished",
            "session_id": self.id,
            "tool_call_id": self.root_tool_call_id.clone(),
            "exit_code": state.exit_code,
            "elapsed_ms": duration_ms_u64(self.started.elapsed()),
            "interrupted": state.interrupted,
        }))
    }

    pub(crate) fn emit_stdin(&self, write_tool_call_id: &str, chars: &str) {
        self.emit_event(Some(json!({
            "type": "exec_session_stdin",
            "session_id": self.id,
            "tool_call_id": self.root_tool_call_id.clone(),
            "write_tool_call_id": write_tool_call_id,
            "chars": bounded_stdin_chars(chars),
        })));
    }

    pub(crate) fn emit_event(&self, event: Option<Value>) {
        let Some(event) = event else {
            return;
        };
        if let Some(stream) = &self.stream_events {
            stream(RunStreamEvent::Event(event));
        }
    }

    pub(crate) fn kill(&self) {
        self.process.kill();
    }

    pub(crate) fn write_stdin(&self, bytes: &[u8]) -> Result<()> {
        if !self.stdin_allowed {
            return Err(Error::Message(
                "stdin is closed for this session; rerun exec_command with tty=true to keep stdin open"
                    .to_string(),
            ));
        }
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| Error::Message("stdin lock poisoned".to_string()))?;
        let Some(stdin) = stdin.as_mut() else {
            return Err(Error::Message(
                "stdin is closed for this session".to_string(),
            ));
        };
        stdin.write_all(bytes)?;
        stdin.flush()?;
        Ok(())
    }
}

impl ExecProcess {
    pub(crate) fn kill(&self) {
        match self {
            Self::Pipe(child) => {
                if let Ok(mut child) = child.lock() {
                    terminate_std_child_tree(&mut child);
                }
            }
            Self::Pty(child) => {
                if let Ok(mut child) = child.lock() {
                    let _ = child.kill();
                }
            }
        }
    }
}

pub(crate) fn spawn_exec_session(
    invocation: ExecInvocation,
    context: ExecSessionContext,
) -> Result<Arc<ExecSession>> {
    if invocation.tty && invocation.sandbox_policy.enabled {
        return Err(crate::sandbox::sandbox_denied(
            "tty=true is not supported when sandbox is enabled",
        ));
    }
    let id = reserve_exec_session_id()?;
    let session = if invocation.tty {
        match spawn_pty_session(id, context.clone(), &invocation) {
            Ok(session) => session,
            Err(_) => spawn_pipe_session(
                id,
                context,
                &invocation,
                true,
                PTY_FALLBACK_NOTICE.as_bytes(),
            )?,
        }
    } else {
        spawn_pipe_session(id, context, &invocation, false, &[])?
    };
    insert_exec_session(Arc::clone(&session))?;
    Ok(session)
}

pub(crate) fn spawn_pipe_session(
    id: u64,
    context: ExecSessionContext,
    invocation: &ExecInvocation,
    stdin_allowed: bool,
    initial_output: &[u8],
) -> Result<Arc<ExecSession>> {
    invocation.sandbox_policy.ensure_shell_supported()?;
    let mut command = pipe_command(invocation)?;
    command
        .current_dir(&invocation.workdir)
        .stdin(if stdin_allowed {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = subprocess_path(&invocation.path_prefixes)? {
        command.env("PATH", path);
    }
    for (key, value) in invocation.sandbox_policy.env_markers() {
        command.env(key, value);
    }
    configure_process_group(&mut command);
    configure_sandbox_pre_exec(&mut command, &invocation.sandbox_policy);
    let mut child = if invocation.sandbox_policy.enabled {
        command.spawn().map_err(|err| {
            Error::Message(format!(
                "denied by sandbox policy: failed to spawn sandboxed command: {err}"
            ))
        })?
    } else {
        command.spawn()?
    };
    let stdin = if stdin_allowed {
        child
            .stdin
            .take()
            .map(|stdin| Box::new(stdin) as Box<dyn Write + Send>)
    } else {
        None
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Message("stdout pipe unavailable".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::Message("stderr pipe unavailable".to_string()))?;
    let child = Arc::new(Mutex::new(child));
    let session = Arc::new(ExecSession::new(
        id,
        context,
        invocation,
        ExecSessionIo {
            process: ExecProcess::Pipe(Arc::clone(&child)),
            readers_active: 2,
            stdin,
            stdin_allowed,
            initial_output: initial_output.to_vec(),
        },
    ));
    spawn_reader_thread(Arc::clone(&session), stdout);
    spawn_reader_thread(Arc::clone(&session), stderr);
    spawn_pipe_waiter_thread(Arc::clone(&session), child);
    Ok(session)
}

fn pipe_command(invocation: &ExecInvocation) -> Result<std::process::Command> {
    #[cfg(target_os = "macos")]
    if invocation.sandbox_policy.enabled
        && matches!(
            invocation.sandbox_policy.backend,
            crate::sandbox::SandboxBackend::Seatbelt
        )
    {
        let mut command = std::process::Command::new("/usr/bin/sandbox-exec");
        command
            .arg("-p")
            .arg(crate::sandbox::seatbelt_profile(&invocation.sandbox_policy))
            .arg("--")
            .arg(&invocation.shell)
            .args(shell_args(invocation.login, &invocation.cmd));
        return Ok(command);
    }

    let mut command = std::process::Command::new(&invocation.shell);
    command.args(shell_args(invocation.login, &invocation.cmd));
    Ok(command)
}

#[cfg(target_os = "linux")]
fn configure_sandbox_pre_exec(command: &mut std::process::Command, policy: &SandboxPolicy) {
    use std::os::unix::process::CommandExt;

    if !policy.enabled {
        return;
    }
    let policy = policy.clone();
    unsafe {
        command.pre_exec(move || crate::sandbox::apply_landlock(&policy));
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_sandbox_pre_exec(_command: &mut std::process::Command, _policy: &SandboxPolicy) {}

pub(crate) fn spawn_pty_session(
    id: u64,
    context: ExecSessionContext,
    invocation: &ExecInvocation,
) -> Result<Arc<ExecSession>> {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| Error::Message(err.to_string()))?;
    let mut command = portable_pty::CommandBuilder::new(&invocation.shell);
    command.args(shell_args(invocation.login, &invocation.cmd));
    command.cwd(invocation.workdir.as_os_str());
    if let Some(path) = subprocess_path(&invocation.path_prefixes)? {
        command.env("PATH", path);
    }
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|err| Error::Message(err.to_string()))?;
    drop(pair.slave);
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| Error::Message(err.to_string()))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|err| Error::Message(err.to_string()))?;
    let child = Arc::new(Mutex::new(child));
    let session = Arc::new(ExecSession::new(
        id,
        context,
        invocation,
        ExecSessionIo {
            process: ExecProcess::Pty(Arc::clone(&child)),
            readers_active: 1,
            stdin: Some(writer),
            stdin_allowed: true,
            initial_output: Vec::new(),
        },
    ));
    spawn_reader_thread(Arc::clone(&session), reader);
    spawn_pty_waiter_thread(Arc::clone(&session), child);
    Ok(session)
}

pub(crate) fn spawn_reader_thread<R>(session: Arc<ExecSession>, mut reader: R)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => session.append_output(&chunk[..n]),
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        session.reader_done();
    });
}

pub(crate) fn spawn_pipe_waiter_thread(
    session: Arc<ExecSession>,
    child: Arc<Mutex<std::process::Child>>,
) {
    thread::spawn(move || {
        loop {
            let status = {
                let Ok(mut child) = child.lock() else {
                    return;
                };
                child.try_wait()
            };
            match status {
                Ok(Some(status)) => {
                    session.mark_exit(status.code());
                    return;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(_) => {
                    session.mark_exit(None);
                    return;
                }
            }
        }
    });
}
