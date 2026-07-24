const ACP_TERMINAL_DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;
const ACP_TERMINAL_MAX_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const ACP_TERMINAL_MAX_ARGS: usize = 1024;
const ACP_TERMINAL_MAX_ENV: usize = 128;
const ACP_TERMINAL_MAX_FIELD_CHARS: usize = 65_536;

#[derive(Clone, Default)]
struct AcpTerminalRegistry {
    records: Arc<Mutex<HashMap<String, AcpTerminalRecord>>>,
}

#[derive(Clone)]
struct AcpTerminalRecord {
    session_id: String,
    state: Arc<Mutex<AcpTerminalState>>,
    kill: watch::Sender<bool>,
    completed: Arc<tokio::sync::Notify>,
}

struct AcpTerminalState {
    output: String,
    output_byte_limit: usize,
    truncated: bool,
    exit_status: Option<TerminalExitStatus>,
}

impl AcpTerminalRegistry {
    /// Removes every terminal owned by one ACP session and cooperatively kills
    /// any child that is still running. Lifecycle cleanup must call this after
    /// close/delete acknowledgement and during generation teardown.
    fn terminate_session(&self, session_id: &str) -> psychevo_runtime::Result<()> {
        let removed = {
            let mut records = self.records.lock().map_err(|_| {
                Error::Message("ACP terminal registry lock poisoned".to_string())
            })?;
            let terminal_ids = records
                .iter()
                .filter(|(_, record)| record.session_id == session_id)
                .map(|(terminal_id, _)| terminal_id.clone())
                .collect::<Vec<_>>();
            terminal_ids
                .into_iter()
                .filter_map(|terminal_id| records.remove(&terminal_id))
                .collect::<Vec<_>>()
        };
        for record in removed {
            let _ = record.kill.send(true);
        }
        Ok(())
    }

    fn terminate_all(&self) -> psychevo_runtime::Result<()> {
        let removed = {
            let mut records = self.records.lock().map_err(|_| {
                Error::Message("ACP terminal registry lock poisoned".to_string())
            })?;
            std::mem::take(&mut *records).into_values().collect::<Vec<_>>()
        };
        for record in removed {
            let _ = record.kill.send(true);
        }
        Ok(())
    }
}

impl AcpTerminalState {
    fn new(output_byte_limit: usize) -> Self {
        Self {
            output: String::new(),
            output_byte_limit,
            truncated: false,
            exit_status: None,
        }
    }

    fn append(&mut self, chunk: &[u8]) {
        self.output.push_str(&String::from_utf8_lossy(chunk));
        if self.output.len() <= self.output_byte_limit {
            return;
        }
        self.truncated = true;
        if self.output_byte_limit == 0 {
            self.output.clear();
            return;
        }
        let mut start = self.output.len().saturating_sub(self.output_byte_limit);
        while start < self.output.len() && !self.output.is_char_boundary(start) {
            start += 1;
        }
        self.output.drain(..start);
    }
}

async fn create_terminal(
    registry: AcpTerminalRegistry,
    context: Arc<AcpClientContext>,
    request: CreateTerminalRequest,
) -> Result<CreateTerminalResponse, agent_client_protocol::Error> {
    if !context.terminal {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("terminal callbacks are not allowed for this ACP Agent"));
    }
    validate_acp_terminal_request(&request)?;
    let cwd = guarded_terminal_cwd(&context.cwd, request.cwd.as_deref())?;
    let mut env = context.terminal_env.clone();
    for variable in &request.env {
        if variable.name.is_empty()
            || variable.name.contains(['\0', '='])
            || variable.value.contains('\0')
        {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("ACP terminal environment contains an invalid entry"));
        }
        env.insert(variable.name.clone(), variable.value.clone());
    }
    approve_acp_terminal_create(&context, &request).await?;
    let program = resolve_executable_path(
        &request.command,
        &cwd,
        &ExecutableResolveOptions {
            platform: HostPlatform::current(),
            env: &env,
        },
    )
    .ok_or_else(|| {
        agent_client_protocol::Error::invalid_request().data(format!(
            "ACP terminal command `{}` could not be resolved",
            request.command
        ))
    })?;
    let args = request.args.iter().map(OsString::from).collect::<Vec<_>>();
    let mut command = psychevo_runtime::process_env::tokio_host_process_command(
        &program,
        &args,
        HostPlatform::current(),
        &env,
    )
    .map_err(acp_internal_error)?;
    command
        .current_dir(&cwd)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    psychevo_runtime::process_env::apply_tokio_process_env(
        &mut command,
        &env,
        psychevo_runtime::process_env::ProcessEnvOptions::new(&[]),
    )
    .map_err(acp_internal_error)?;
    let mut child = command.spawn().map_err(acp_internal_error)?;
    let stdout = child.stdout.take().ok_or_else(|| {
        agent_client_protocol::Error::internal_error().data("ACP terminal stdout is unavailable")
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        agent_client_protocol::Error::internal_error().data("ACP terminal stderr is unavailable")
    })?;
    let output_byte_limit = request
        .output_byte_limit
        .and_then(|limit| usize::try_from(limit).ok())
        .unwrap_or(ACP_TERMINAL_DEFAULT_OUTPUT_LIMIT)
        .min(ACP_TERMINAL_MAX_OUTPUT_LIMIT);
    let terminal_id = uuid::Uuid::now_v7().to_string();
    let state = Arc::new(Mutex::new(AcpTerminalState::new(output_byte_limit)));
    let completed = Arc::new(tokio::sync::Notify::new());
    let (kill, mut kill_rx) = watch::channel(false);
    let record = AcpTerminalRecord {
        session_id: request.session_id.to_string(),
        state: Arc::clone(&state),
        kill,
        completed: Arc::clone(&completed),
    };
    registry
        .records
        .lock()
        .map_err(|_| {
            agent_client_protocol::Error::internal_error()
                .data("ACP terminal registry lock poisoned")
        })?
        .insert(terminal_id.clone(), record);
    let stdout_task = tokio::spawn(read_acp_terminal_output(stdout, Arc::clone(&state)));
    let stderr_task = tokio::spawn(read_acp_terminal_output(stderr, Arc::clone(&state)));
    tokio::spawn(async move {
        let exit_status = tokio::select! {
            status = child.wait() => match status {
                Ok(status) => TerminalExitStatus::new()
                    .exit_code(status.code().and_then(|code| u32::try_from(code).ok()))
                    .signal(status.code().is_none().then(|| "terminated".to_string())),
                Err(error) => TerminalExitStatus::new().signal(error.to_string()),
            },
            _ = kill_rx.changed() => {
                psychevo_runtime::process_env::terminate_tokio_child_tree(&mut child).await;
                TerminalExitStatus::new().signal("killed".to_string())
            }
        };
        finish_acp_terminal_reader(stdout_task).await;
        finish_acp_terminal_reader(stderr_task).await;
        if let Ok(mut state) = state.lock() {
            state.exit_status = Some(exit_status);
        }
        completed.notify_waiters();
    });
    Ok(CreateTerminalResponse::new(terminal_id))
}

async fn finish_acp_terminal_reader(mut task: tokio::task::JoinHandle<()>) {
    if tokio::time::timeout(std::time::Duration::from_secs(2), &mut task)
        .await
        .is_err()
    {
        task.abort();
    }
}

async fn read_acp_terminal_output(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    state: Arc<Mutex<AcpTerminalState>>,
) {
    let mut chunk = [0u8; 8192];
    loop {
        let count = match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        if let Ok(mut state) = state.lock() {
            state.append(&chunk[..count]);
        } else {
            break;
        }
    }
}

async fn terminal_output(
    registry: AcpTerminalRegistry,
    request: TerminalOutputRequest,
) -> Result<TerminalOutputResponse, agent_client_protocol::Error> {
    let record = acp_terminal_record(
        &registry,
        &request.session_id.to_string(),
        &request.terminal_id.to_string(),
    )?;
    let state = record.state.lock().map_err(|_| {
        agent_client_protocol::Error::internal_error().data("ACP terminal state lock poisoned")
    })?;
    Ok(TerminalOutputResponse::new(
        state.output.clone(),
        state.truncated,
    )
    .exit_status(state.exit_status.clone()))
}

async fn wait_for_terminal_exit(
    registry: AcpTerminalRegistry,
    request: WaitForTerminalExitRequest,
) -> Result<WaitForTerminalExitResponse, agent_client_protocol::Error> {
    let record = acp_terminal_record(
        &registry,
        &request.session_id.to_string(),
        &request.terminal_id.to_string(),
    )?;
    loop {
        let completed = record.completed.notified();
        if let Some(exit_status) = record
            .state
            .lock()
            .map_err(|_| {
                agent_client_protocol::Error::internal_error()
                    .data("ACP terminal state lock poisoned")
            })?
            .exit_status
            .clone()
        {
            return Ok(WaitForTerminalExitResponse::new(exit_status));
        }
        completed.await;
    }
}

async fn kill_terminal(
    registry: AcpTerminalRegistry,
    request: KillTerminalRequest,
) -> Result<KillTerminalResponse, agent_client_protocol::Error> {
    let record = acp_terminal_record(
        &registry,
        &request.session_id.to_string(),
        &request.terminal_id.to_string(),
    )?;
    if record
        .state
        .lock()
        .map_err(|_| {
            agent_client_protocol::Error::internal_error().data("ACP terminal state lock poisoned")
        })?
        .exit_status
        .is_none()
    {
        let _ = record.kill.send(true);
    }
    Ok(KillTerminalResponse::new())
}

async fn release_terminal(
    registry: AcpTerminalRegistry,
    request: ReleaseTerminalRequest,
) -> Result<ReleaseTerminalResponse, agent_client_protocol::Error> {
    let terminal_id = request.terminal_id.to_string();
    let record = {
        let mut records = registry.records.lock().map_err(|_| {
            agent_client_protocol::Error::internal_error()
                .data("ACP terminal registry lock poisoned")
        })?;
        let record = records.get(&terminal_id).cloned().ok_or_else(|| {
            agent_client_protocol::Error::invalid_request()
                .data(format!("unknown ACP terminal: {terminal_id}"))
        })?;
        if record.session_id != request.session_id.to_string() {
            return Err(agent_client_protocol::Error::invalid_request()
                .data("ACP terminal belongs to another session"));
        }
        records.remove(&terminal_id).expect("terminal existed")
    };
    let _ = record.kill.send(true);
    Ok(ReleaseTerminalResponse::new())
}

fn acp_terminal_record(
    registry: &AcpTerminalRegistry,
    session_id: &str,
    terminal_id: &str,
) -> Result<AcpTerminalRecord, agent_client_protocol::Error> {
    let record = registry
        .records
        .lock()
        .map_err(|_| {
            agent_client_protocol::Error::internal_error()
                .data("ACP terminal registry lock poisoned")
        })?
        .get(terminal_id)
        .cloned()
        .ok_or_else(|| {
            agent_client_protocol::Error::invalid_request()
                .data(format!("unknown ACP terminal: {terminal_id}"))
        })?;
    if record.session_id != session_id {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("ACP terminal belongs to another session"));
    }
    Ok(record)
}

fn validate_acp_terminal_request(
    request: &CreateTerminalRequest,
) -> Result<(), agent_client_protocol::Error> {
    if request.command.trim().is_empty() || request.command.contains('\0') {
        return Err(agent_client_protocol::Error::invalid_params()
            .data("ACP terminal command must be non-empty"));
    }
    if request.args.len() > ACP_TERMINAL_MAX_ARGS || request.env.len() > ACP_TERMINAL_MAX_ENV {
        return Err(agent_client_protocol::Error::invalid_params()
            .data("ACP terminal request exceeds argument or environment limits"));
    }
    if std::iter::once(request.command.as_str())
        .chain(request.args.iter().map(String::as_str))
        .chain(request.env.iter().flat_map(|entry| [entry.name.as_str(), entry.value.as_str()]))
        .any(|value| value.contains('\0') || value.chars().count() > ACP_TERMINAL_MAX_FIELD_CHARS)
    {
        return Err(agent_client_protocol::Error::invalid_params()
            .data("ACP terminal request contains an invalid or oversized field"));
    }
    Ok(())
}

fn guarded_terminal_cwd(
    root: &Path,
    requested: Option<&Path>,
) -> Result<PathBuf, agent_client_protocol::Error> {
    let root = root.canonicalize().map_err(acp_internal_error)?;
    let requested = requested.unwrap_or(&root);
    if !requested.is_absolute() {
        return Err(agent_client_protocol::Error::invalid_params()
            .data("ACP terminal cwd must be absolute"));
    }
    let requested = requested.canonicalize().map_err(|error| {
        agent_client_protocol::Error::invalid_request().data(error.to_string())
    })?;
    if !requested.starts_with(&root) {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("ACP terminal cwd is outside the captured workspace"));
    }
    Ok(requested)
}

async fn approve_acp_terminal_create(
    context: &AcpClientContext,
    request: &CreateTerminalRequest,
) -> Result<(), agent_client_protocol::Error> {
    let Some(handler) = &context.approval_handler else {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("ACP terminal permission handler is unavailable"));
    };
    let summary = std::iter::once(request.command.as_str())
        .chain(request.args.iter().map(String::as_str))
        .take(16)
        .collect::<Vec<_>>()
        .join(" ");
    let decision = handler
        .request_permission(PermissionApprovalRequest {
            tool_call_id: format!("acp-terminal-{}", uuid::Uuid::now_v7()),
            tool_name: "terminal/create".to_string(),
            summary,
            reason: "ACP Agent requested command execution".to_string(),
            matched_rule: None,
            suggested_rule: None,
            allow_always: false,
            filesystem: None,
            timeout_secs: handler.timeout_secs(),
        })
        .await;
    if matches!(decision.outcome, PermissionApprovalOutcome::Deny) {
        Err(agent_client_protocol::Error::invalid_request().data("permission denied"))
    } else {
        Ok(())
    }
}
