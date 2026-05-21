struct ExecCommandTool(WorkdirTool);

struct WriteStdinTool;

static EXEC_SESSIONS: LazyLock<Mutex<ExecSessionRegistry>> =
    LazyLock::new(|| Mutex::new(ExecSessionRegistry::default()));

const EXEC_OUTPUT_MAX_BYTES: usize = 1024 * 1024;
const PTY_FALLBACK_NOTICE: &str =
    "[exec_command] tty=true requested but PTY was unavailable; running with pipes instead.\n";

impl ExecCommandTool {
    fn new(workdir: PathBuf, context: ToolRuntimeContext) -> Self {
        Self(WorkdirTool::with_context(workdir, context))
    }
}

impl WriteStdinTool {
    fn new() -> Self {
        Self
    }
}

impl ToolBinding for ExecCommandTool {
    fn name(&self) -> &str {
        "exec_command"
    }

    fn description(&self) -> &str {
        "Run a bounded shell command in the working directory. Prefer read/write/edit for file I/O instead of shell cat/head/tail/sed or redirection. Commands that keep running return a session_id after yield_time_ms; use write_stdin with empty chars to poll or non-empty chars to send stdin."
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
            allow_login_shell,
            stream_events: None,
        },
        "exec_command".to_string(),
        args,
        abort,
    )
    .await
}

async fn exec_command_tool_impl_with_context(
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

async fn write_stdin_tool_impl_with_call(
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
    abort: AbortSignal,
) -> Result<(Value, bool)> {
    let invocation = ExecInvocation {
        cmd: command,
        workdir,
        shell: default_shell(),
        login: false,
        tty: false,
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
        return Err(Error::Message("command timed out after 120 seconds".to_string()));
    }
    final_value["output"] = Value::String(output);
    Ok((final_value, false))
}

#[derive(Clone)]
struct ExecInvocation {
    cmd: String,
    workdir: PathBuf,
    shell: String,
    login: bool,
    tty: bool,
}

#[derive(Default)]
struct ExecSessionRegistry {
    next_id: u64,
    sessions: HashMap<u64, Arc<ExecSession>>,
}

#[derive(Clone)]
struct ExecSessionContext {
    task_id: String,
    root_tool_call_id: String,
    stream_events: Option<RunStreamSink>,
}

struct ExecSession {
    id: u64,
    task_id: String,
    root_tool_call_id: String,
    cmd: String,
    workdir: PathBuf,
    stream_events: Option<RunStreamSink>,
    started: Instant,
    started_at_ms: i64,
    process: ExecProcess,
    state: Mutex<ExecSessionState>,
    stdin: Mutex<Option<Box<dyn Write + Send>>>,
    stdin_allowed: bool,
}

struct ExecSessionState {
    output: Vec<u8>,
    read_offset: usize,
    readers_active: usize,
    exited: bool,
    exit_code: Option<i32>,
    chunk_id: u64,
    output_seq: u64,
    yielded: bool,
    finish_emitted: bool,
    interrupted: bool,
}

enum ExecProcess {
    Pipe(Arc<Mutex<std::process::Child>>),
    Pty(Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>),
}

struct ExecSessionIo {
    process: ExecProcess,
    readers_active: usize,
    stdin: Option<Box<dyn Write + Send>>,
    stdin_allowed: bool,
    initial_output: Vec<u8>,
}

impl ExecSession {
    fn new(
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

    fn append_output(&self, bytes: &[u8]) {
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

    fn reader_done(&self) {
        let event = if let Ok(mut state) = self.state.lock() {
            state.readers_active = state.readers_active.saturating_sub(1);
            self.finish_event_locked(&mut state)
        } else {
            None
        };
        self.emit_event(event);
    }

    fn mark_exit(&self, exit_code: Option<i32>) {
        let event = if let Ok(mut state) = self.state.lock() {
            state.exited = true;
            state.exit_code = exit_code;
            self.finish_event_locked(&mut state)
        } else {
            None
        };
        self.emit_event(event);
    }

    fn interrupt(&self) {
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

    fn finish_event_locked(&self, state: &mut ExecSessionState) -> Option<Value> {
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

    fn emit_stdin(&self, write_tool_call_id: &str, chars: &str) {
        self.emit_event(Some(json!({
            "type": "exec_session_stdin",
            "session_id": self.id,
            "tool_call_id": self.root_tool_call_id.clone(),
            "write_tool_call_id": write_tool_call_id,
            "chars": bounded_stdin_chars(chars),
        })));
    }

    fn emit_event(&self, event: Option<Value>) {
        let Some(event) = event else {
            return;
        };
        if let Some(stream) = &self.stream_events {
            stream(RunStreamEvent::Event(event));
        }
    }

    fn kill(&self) {
        self.process.kill();
    }

    fn write_stdin(&self, bytes: &[u8]) -> Result<()> {
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
            return Err(Error::Message("stdin is closed for this session".to_string()));
        };
        stdin.write_all(bytes)?;
        stdin.flush()?;
        Ok(())
    }
}

impl ExecProcess {
    fn kill(&self) {
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

fn spawn_exec_session(
    invocation: ExecInvocation,
    context: ExecSessionContext,
) -> Result<Arc<ExecSession>> {
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

fn spawn_pipe_session(
    id: u64,
    context: ExecSessionContext,
    invocation: &ExecInvocation,
    stdin_allowed: bool,
    initial_output: &[u8],
) -> Result<Arc<ExecSession>> {
    let mut command = std::process::Command::new(&invocation.shell);
    command
        .args(shell_args(invocation.login, &invocation.cmd))
        .current_dir(&invocation.workdir)
        .stdin(if stdin_allowed {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut command);
    let mut child = command.spawn()?;
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

fn spawn_pty_session(
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

fn spawn_reader_thread<R>(session: Arc<ExecSession>, mut reader: R)
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

fn spawn_pipe_waiter_thread(
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

fn spawn_pty_waiter_thread(
    session: Arc<ExecSession>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
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
                    session.mark_exit(Some(status.exit_code() as i32));
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

async fn await_session_result(
    session: Arc<ExecSession>,
    yield_duration: Duration,
    max_output_tokens: usize,
    mut abort: AbortSignal,
    mark_yielded_on_timeout: bool,
) -> Result<Value> {
    let started = Instant::now();
    loop {
        if abort.aborted() {
            session.interrupt();
            remove_exec_session(session.id);
            return Err(Error::Message("aborted".to_string()));
        }
        if session_completed(&session) || started.elapsed() >= yield_duration {
            break;
        }
        tokio::select! {
            biased;
            _ = abort.wait_for_abort() => {
                session.interrupt();
                remove_exec_session(session.id);
                return Err(Error::Message("aborted".to_string()));
            }
            _ = time::sleep(Duration::from_millis(10)) => {}
        }
    }
    let (value, completed) = session_result_json(
        &session,
        started.elapsed(),
        max_output_tokens,
        mark_yielded_on_timeout,
    )?;
    if completed {
        remove_exec_session(session.id);
    }
    Ok(value)
}

fn session_completed(session: &ExecSession) -> bool {
    session.state.lock().is_ok_and(|state| {
        state.exited && state.readers_active == 0
    })
}

fn session_result_json(
    session: &ExecSession,
    elapsed: Duration,
    max_output_tokens: usize,
    mark_yielded_on_timeout: bool,
) -> Result<(Value, bool)> {
    let (chunk_id, chunk, exit_code, completed, emit_yielded) = {
        let mut state = session
            .state
            .lock()
            .map_err(|_| Error::Message("session lock poisoned".to_string()))?;
        let completed = state.exited && state.readers_active == 0;
        let emit_yielded = !completed && mark_yielded_on_timeout && !state.yielded;
        if emit_yielded {
            state.yielded = true;
        }
        let chunk = state.output[state.read_offset..].to_vec();
        state.read_offset = state.output.len();
        let chunk_id = state.chunk_id;
        state.chunk_id = state.chunk_id.saturating_add(1);
        (
            chunk_id,
            chunk,
            if completed { state.exit_code } else { None },
            completed,
            emit_yielded,
        )
    };
    if emit_yielded {
        session.emit_event(Some(json!({
            "type": "exec_session_yielded",
            "session_id": session.id,
            "tool_call_id": session.root_tool_call_id.clone(),
            "cmd": session.cmd.clone(),
            "workdir": session.workdir.display().to_string(),
            "started_at_ms": session.started_at_ms,
        })));
    }
    let output = String::from_utf8_lossy(&chunk).to_string();
    let (output, original_token_count) = truncate_output_tokens(&output, max_output_tokens);
    Ok((
        json!({
            "chunk_id": chunk_id,
            "wall_time_seconds": elapsed.as_secs_f64(),
            "exit_code": exit_code,
            "session_id": if completed { Value::Null } else { json!(session.id) },
            "original_token_count": original_token_count,
            "output": output,
        }),
        completed,
    ))
}

fn truncate_output_tokens(output: &str, max_tokens: usize) -> (String, usize) {
    let Some(enc) = tiktoken::get_encoding("cl100k_base") else {
        return truncate_output_chars(output, max_tokens);
    };
    let tokens = enc.encode(output);
    let original = tokens.len();
    if original <= max_tokens {
        return (output.to_string(), original);
    }
    let start = original.saturating_sub(max_tokens);
    let truncated = enc
        .decode_to_string(&tokens[start..])
        .unwrap_or_else(|_| output.chars().rev().take(max_tokens * 4).collect());
    (truncated, original)
}

fn truncate_output_chars(output: &str, max_tokens: usize) -> (String, usize) {
    let original = output.chars().count().div_ceil(4);
    let max_chars = max_tokens.saturating_mul(4);
    if output.chars().count() <= max_chars {
        return (output.to_string(), original);
    }
    let chars = output.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(max_chars);
    (chars[start..].iter().collect(), original)
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn duration_ms_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn bounded_stdin_chars(chars: &str) -> String {
    if chars.chars().count() <= EXEC_STDIN_EVENT_MAX_CHARS {
        return chars.to_string();
    }
    chars
        .chars()
        .take(EXEC_STDIN_EVENT_MAX_CHARS)
        .collect::<String>()
}

fn reserve_exec_session_id() -> Result<u64> {
    let mut registry = EXEC_SESSIONS
        .lock()
        .map_err(|_| Error::Message("exec session registry lock poisoned".to_string()))?;
    if active_exec_session_count(&registry) >= MAX_EXEC_SESSIONS {
        return Err(Error::Message(format!(
            "too many active exec_command sessions; maximum is {MAX_EXEC_SESSIONS}"
        )));
    }
    let id = registry.next_id;
    registry.next_id = registry.next_id.saturating_add(1).max(1);
    Ok(id)
}

fn insert_exec_session(session: Arc<ExecSession>) -> Result<()> {
    let mut registry = EXEC_SESSIONS
        .lock()
        .map_err(|_| Error::Message("exec session registry lock poisoned".to_string()))?;
    if active_exec_session_count(&registry) >= MAX_EXEC_SESSIONS {
        session.kill();
        return Err(Error::Message(format!(
            "too many active exec_command sessions; maximum is {MAX_EXEC_SESSIONS}"
        )));
    }
    registry.sessions.insert(session.id, session);
    Ok(())
}

fn active_exec_session_count(registry: &ExecSessionRegistry) -> usize {
    registry
        .sessions
        .values()
        .filter(|session| !session_completed(session))
        .count()
}

fn get_exec_session(session_id: u64) -> Option<Arc<ExecSession>> {
    EXEC_SESSIONS
        .lock()
        .ok()
        .and_then(|registry| registry.sessions.get(&session_id).cloned())
}

fn remove_exec_session(session_id: u64) {
    if let Ok(mut registry) = EXEC_SESSIONS.lock() {
        registry.sessions.remove(&session_id);
    }
}

pub(crate) fn interrupt_exec_sessions_for_task(task_id: &str) {
    let sessions = sessions_for_task(task_id);
    for session in sessions {
        session.interrupt();
        remove_exec_session(session.id);
    }
}

pub(crate) fn detach_exec_sessions_for_task(task_id: String) {
    thread::spawn(move || {
        thread::sleep(EXEC_DETACHED_SESSION_TTL);
        let sessions = sessions_for_task(&task_id);
        for session in sessions {
            if session_completed(&session) {
                remove_exec_session(session.id);
            } else {
                session.interrupt();
                remove_exec_session(session.id);
            }
        }
    });
}

fn sessions_for_task(task_id: &str) -> Vec<Arc<ExecSession>> {
    EXEC_SESSIONS
        .lock()
        .map(|registry| {
            registry
                .sessions
                .values()
                .filter(|session| session.task_id == task_id)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn resolve_exec_workdir(accepted_workdir: &Path, raw: Option<&str>) -> Result<PathBuf> {
    let path = match raw {
        Some(raw) if Path::new(raw).is_absolute() => PathBuf::from(raw),
        Some(raw) => accepted_workdir.join(raw),
        None => accepted_workdir.to_path_buf(),
    };
    let path = path.canonicalize()?;
    if !path.is_dir() {
        return Err(Error::Message(format!(
            "workdir is not a directory: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn default_shell() -> String {
    #[cfg(windows)]
    {
        env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    }
    #[cfg(not(windows))]
    {
        env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    }
}

fn shell_args(login: bool, command: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        let _ = login;
        vec!["/C".to_string(), command.to_string()]
    }
    #[cfg(not(windows))]
    {
        vec![
            if login { "-lc" } else { "-c" }.to_string(),
            command.to_string(),
        ]
    }
}

fn clamp_yield_ms(value: Option<i64>, default: u64, min: u64, max: u64) -> u64 {
    let value = value.unwrap_or(default as i64);
    value.clamp(min as i64, max as i64) as u64
}

fn output_token_limit(value: Option<i64>) -> Result<usize> {
    let value = value.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS as i64);
    if value < 1 {
        return Err(Error::Message("max_output_tokens must be >= 1".to_string()));
    }
    Ok(value as usize)
}

fn required_u64(args: &Value, key: &str) -> Result<u64> {
    args.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Message(format!("{key} must be an integer")))
}

fn reject_untracked_background_command(command: &str) -> Result<()> {
    let normalized = normalize_exec_command(command);
    if normalized.ends_with(" &")
        || normalized.contains(" & ")
        || normalized.starts_with("nohup ")
        || normalized.contains(" nohup ")
        || normalized.starts_with("disown")
        || normalized.contains("; disown")
        || normalized.contains("&& disown")
        || normalized.starts_with("setsid ")
        || normalized.contains(" setsid ")
    {
        return Err(Error::Message(
            "shell-level background wrappers are not supported; run the foreground command and let exec_command return a session_id"
                .to_string(),
        ));
    }
    Ok(())
}

fn normalize_exec_command(command: &str) -> String {
    command
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[cfg(unix)]
fn configure_process_group(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;

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
fn configure_process_group(_command: &mut std::process::Command) {}

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
fn terminate_std_child_tree(child: &mut std::process::Child) {
    let _ = kill_process_group_by_pid(child.id());
    let _ = child.kill();
}

#[cfg(not(unix))]
fn terminate_std_child_tree(child: &mut std::process::Child) {
    let _ = child.kill();
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
