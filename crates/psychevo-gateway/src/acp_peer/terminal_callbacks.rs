use std::collections::HashMap;
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(any(unix, test))]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    CreateTerminalRequest, CreateTerminalResponse, KillTerminalRequest, KillTerminalResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, TerminalExitStatus, TerminalOutputRequest,
    TerminalOutputResponse, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
};
use psychevo::Error;
#[cfg(unix)]
use psychevo::{
    application::{PermissionApprovalOutcome, PermissionApprovalRequest},
    host_paths::{ExecutableResolveOptions, HostPlatform, resolve_executable_path},
};
#[cfg(unix)]
use tokio::io::AsyncReadExt as _;
use tokio::sync::watch;

#[cfg(any(unix, test))]
use super::metadata_permissions::acp_internal_error;
use super::turn::AcpClientContext;

#[cfg(unix)]
const ACP_TERMINAL_DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;
#[cfg(unix)]
const ACP_TERMINAL_MAX_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
#[cfg(unix)]
const ACP_TERMINAL_MAX_ARGS: usize = 1024;
#[cfg(unix)]
const ACP_TERMINAL_MAX_ENV: usize = 128;
#[cfg(unix)]
const ACP_TERMINAL_MAX_FIELD_CHARS: usize = 65_536;

#[derive(Clone, Default)]
pub(super) struct AcpTerminalRegistry {
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
    #[cfg(unix)]
    output: String,
    #[cfg(unix)]
    output_byte_limit: usize,
    #[cfg(unix)]
    truncated: bool,
    exit_status: Option<TerminalExitStatus>,
}

impl AcpTerminalRegistry {
    #[cfg(test)]
    fn terminate_session(&self, session_id: &str) -> psychevo::Result<()> {
        let removed = {
            let mut records = self
                .records
                .lock()
                .map_err(|_| Error::Message("ACP terminal registry lock poisoned".to_string()))?;
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

    pub(super) async fn terminate_session_and_wait(
        &self,
        session_id: &str,
    ) -> psychevo::Result<()> {
        let removed = {
            let mut records = self
                .records
                .lock()
                .map_err(|_| Error::Message("ACP terminal registry lock poisoned".to_string()))?;
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
        for record in &removed {
            let _ = record.kill.send(true);
        }
        for record in removed {
            loop {
                let notified = record.completed.notified();
                if record
                    .state
                    .lock()
                    .map_err(|_| Error::Message("ACP terminal state lock poisoned".to_string()))?
                    .exit_status
                    .is_some()
                {
                    break;
                }
                notified.await;
            }
        }
        Ok(())
    }

    pub(super) async fn terminate_all_and_wait(&self) -> psychevo::Result<()> {
        let removed = {
            let mut records = self
                .records
                .lock()
                .map_err(|_| Error::Message("ACP terminal registry lock poisoned".to_string()))?;
            std::mem::take(&mut *records)
                .into_values()
                .collect::<Vec<_>>()
        };
        for record in &removed {
            let _ = record.kill.send(true);
        }
        for record in removed {
            loop {
                let notified = record.completed.notified();
                if record
                    .state
                    .lock()
                    .map_err(|_| Error::Message("ACP terminal state lock poisoned".to_string()))?
                    .exit_status
                    .is_some()
                {
                    break;
                }
                notified.await;
            }
        }
        Ok(())
    }

    #[cfg(all(test, unix))]
    pub(super) fn insert_test_terminal(
        &self,
        terminal_id: &str,
        session_id: &str,
    ) -> watch::Receiver<bool> {
        let (kill, receiver) = watch::channel(false);
        let state = Arc::new(Mutex::new(AcpTerminalState::new(128)));
        let completed = Arc::new(tokio::sync::Notify::new());
        self.records.lock().expect("terminal records").insert(
            terminal_id.to_string(),
            AcpTerminalRecord {
                session_id: session_id.to_string(),
                state: Arc::clone(&state),
                kill,
                completed: Arc::clone(&completed),
            },
        );
        let mut exit = receiver.clone();
        tokio::spawn(async move {
            let _ = exit.changed().await;
            state.lock().expect("terminal state").exit_status =
                Some(TerminalExitStatus::new().signal(Some("killed".to_string())));
            completed.notify_waiters();
        });
        receiver
    }
}

impl AcpTerminalState {
    #[cfg(any(unix, test))]
    fn new(output_byte_limit: usize) -> Self {
        #[cfg(not(unix))]
        let _ = output_byte_limit;
        Self {
            #[cfg(unix)]
            output: String::new(),
            #[cfg(unix)]
            output_byte_limit,
            #[cfg(unix)]
            truncated: false,
            exit_status: None,
        }
    }

    #[cfg(unix)]
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

#[cfg(unix)]
pub(super) async fn create_terminal(
    registry: AcpTerminalRegistry,
    context: Arc<AcpClientContext>,
    request: CreateTerminalRequest,
) -> Result<CreateTerminalResponse, agent_client_protocol::Error> {
    context.attachment.ensure_active()?;
    if !context.terminal {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("terminal callbacks are not allowed for this ACP Agent"));
    }
    validate_acp_terminal_request(&request)?;
    let cwd = guarded_terminal_cwd(
        &context.cwd,
        &context.workspace_roots,
        request.cwd.as_deref(),
    )?;
    let cwd_handle = context
        .filesystem_authorizer
        .as_ref()
        .ok_or_else(|| {
            agent_client_protocol::Error::invalid_request()
                .data("terminal Workspace authorization is unavailable")
        })?
        .open_workspace_directory(&cwd)
        .map_err(|error| agent_client_protocol::Error::invalid_request().data(error))?;
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
    context.attachment.ensure_active()?;
    let resolved_program = resolve_executable_path(
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
    let program = executable_from_captured_cwd(&cwd_handle, &cwd, &resolved_program)
        .map_err(acp_internal_error)?;
    let args = request.args.iter().map(OsString::from).collect::<Vec<_>>();
    let mut command = psychevo::process_env::tokio_host_process_command(
        &program,
        &args,
        HostPlatform::current(),
        &env,
    )
    .map_err(acp_internal_error)?;
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    psychevo::process_env::apply_tokio_process_env(
        &mut command,
        &env,
        psychevo::process_env::ProcessEnvOptions::new(&[]),
    )
    .map_err(acp_internal_error)?;
    {
        use std::os::fd::AsRawFd as _;
        use std::os::unix::process::CommandExt as _;

        let cwd_fd = cwd_handle.as_raw_fd();
        command.as_std_mut().process_group(0);
        unsafe {
            command.as_std_mut().pre_exec(move || {
                if libc::fchdir(cwd_fd) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }
    context.attachment.ensure_active()?;
    let mut child = command.spawn().map_err(acp_internal_error)?;
    if let Err(error) = context.attachment.ensure_active() {
        psychevo::process_env::terminate_tokio_child_process_group(&mut child).await;
        let _ = child.wait().await;
        return Err(error);
    }
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
                psychevo::process_env::terminate_tokio_child_process_group(&mut child).await;
                let _ = child.wait().await;
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
    if let Err(error) = context.attachment.ensure_active() {
        registry
            .terminate_session_and_wait(&request.session_id.to_string())
            .await
            .map_err(acp_internal_error)?;
        return Err(error);
    }
    Ok(CreateTerminalResponse::new(terminal_id))
}

#[cfg(not(unix))]
pub(super) async fn create_terminal(
    _registry: AcpTerminalRegistry,
    context: Arc<AcpClientContext>,
    _request: CreateTerminalRequest,
) -> Result<CreateTerminalResponse, agent_client_protocol::Error> {
    context.attachment.ensure_active()?;
    Err(agent_client_protocol::Error::invalid_request()
        .data("identity-bound ACP terminal cwd is unsupported on this platform"))
}

#[cfg(unix)]
async fn finish_acp_terminal_reader(mut task: tokio::task::JoinHandle<()>) {
    if tokio::time::timeout(std::time::Duration::from_secs(2), &mut task)
        .await
        .is_err()
    {
        task.abort();
    }
}

#[cfg(unix)]
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

#[cfg(unix)]
pub(super) async fn terminal_output(
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
    Ok(
        TerminalOutputResponse::new(state.output.clone(), state.truncated)
            .exit_status(state.exit_status.clone()),
    )
}

#[cfg(not(unix))]
pub(super) async fn terminal_output(
    _registry: AcpTerminalRegistry,
    _request: TerminalOutputRequest,
) -> Result<TerminalOutputResponse, agent_client_protocol::Error> {
    Err(unsupported_terminal_callback())
}

#[cfg(unix)]
pub(super) async fn wait_for_terminal_exit(
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

#[cfg(not(unix))]
pub(super) async fn wait_for_terminal_exit(
    _registry: AcpTerminalRegistry,
    _request: WaitForTerminalExitRequest,
) -> Result<WaitForTerminalExitResponse, agent_client_protocol::Error> {
    Err(unsupported_terminal_callback())
}

#[cfg(unix)]
pub(super) async fn kill_terminal(
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

#[cfg(not(unix))]
pub(super) async fn kill_terminal(
    _registry: AcpTerminalRegistry,
    _request: KillTerminalRequest,
) -> Result<KillTerminalResponse, agent_client_protocol::Error> {
    Err(unsupported_terminal_callback())
}

#[cfg(unix)]
pub(super) async fn release_terminal(
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

#[cfg(not(unix))]
pub(super) async fn release_terminal(
    _registry: AcpTerminalRegistry,
    _request: ReleaseTerminalRequest,
) -> Result<ReleaseTerminalResponse, agent_client_protocol::Error> {
    Err(unsupported_terminal_callback())
}

#[cfg(not(unix))]
fn unsupported_terminal_callback() -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_request()
        .data("identity-bound ACP terminal callbacks are unsupported on this platform")
}

#[cfg(unix)]
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

#[cfg(unix)]
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
        .chain(
            request
                .env
                .iter()
                .flat_map(|entry| [entry.name.as_str(), entry.value.as_str()]),
        )
        .any(|value| value.contains('\0') || value.chars().count() > ACP_TERMINAL_MAX_FIELD_CHARS)
    {
        return Err(agent_client_protocol::Error::invalid_params()
            .data("ACP terminal request contains an invalid or oversized field"));
    }
    Ok(())
}

#[cfg(any(unix, test))]
fn guarded_terminal_cwd(
    default_cwd: &Path,
    workspace_roots: &[PathBuf],
    requested: Option<&Path>,
) -> Result<PathBuf, agent_client_protocol::Error> {
    let default_cwd = default_cwd.canonicalize().map_err(acp_internal_error)?;
    let requested = requested.unwrap_or(&default_cwd);
    if !requested.is_absolute() {
        return Err(agent_client_protocol::Error::invalid_params()
            .data("ACP terminal cwd must be absolute"));
    }
    let requested = requested
        .canonicalize()
        .map_err(|error| agent_client_protocol::Error::invalid_request().data(error.to_string()))?;
    let mut allowed = false;
    for root in workspace_roots {
        let root = root.canonicalize().map_err(acp_internal_error)?;
        if requested.starts_with(root) {
            allowed = true;
            break;
        }
    }
    if !allowed {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("ACP terminal cwd is outside the captured workspace"));
    }
    Ok(requested)
}

#[cfg(unix)]
fn executable_from_captured_cwd(
    cwd_handle: &std::fs::File,
    cwd: &Path,
    resolved_program: &Path,
) -> std::io::Result<PathBuf> {
    use std::os::fd::AsRawFd as _;

    let Ok(relative) = resolved_program.strip_prefix(cwd) else {
        return Ok(resolved_program.to_path_buf());
    };
    if relative.as_os_str().is_empty() {
        return Ok(resolved_program.to_path_buf());
    }
    let fd = cwd_handle.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let mut anchored = PathBuf::from(format!("/proc/self/fd/{fd}"));
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let mut anchored = PathBuf::from(format!("/dev/fd/{fd}"));
    anchored.push(relative);
    Ok(anchored)
}

#[cfg(unix)]
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
            mcp_startup: None,
            timeout_secs: handler.timeout_secs(),
        })
        .await;
    if matches!(decision.outcome, PermissionApprovalOutcome::Deny) {
        Err(agent_client_protocol::Error::invalid_request().data("permission denied"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::sync::watch;

    #[cfg(unix)]
    use super::executable_from_captured_cwd;
    use super::{AcpTerminalRecord, AcpTerminalRegistry, AcpTerminalState, guarded_terminal_cwd};

    #[test]
    fn terminal_callbacks_accept_secondary_workspace_roots() {
        let temp = tempfile::tempdir().expect("temp");
        let primary = temp.path().join("primary");
        let secondary = temp.path().join("secondary");
        std::fs::create_dir_all(&primary).expect("primary");
        std::fs::create_dir_all(&secondary).expect("secondary");

        assert_eq!(
            guarded_terminal_cwd(
                &primary,
                &[primary.clone(), secondary.clone()],
                Some(&secondary),
            )
            .expect("secondary terminal cwd"),
            secondary.canonicalize().expect("canonical secondary")
        );
    }

    #[cfg(unix)]
    #[test]
    fn relative_executable_is_resolved_from_the_captured_cwd_object() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("workspace");
        let old_root = temp.path().join("old-workspace");
        std::fs::create_dir(&root).expect("root");
        let original = root.join("script");
        std::fs::write(&original, "#!/bin/sh\necho original\n").expect("original script");
        std::fs::set_permissions(&original, std::fs::Permissions::from_mode(0o755))
            .expect("original mode");
        let cwd_handle = std::fs::File::open(&root).expect("captured cwd");
        std::fs::rename(&root, &old_root).expect("retain old cwd object");
        std::fs::create_dir(&root).expect("replacement root");
        let replacement = root.join("script");
        std::fs::write(&replacement, "#!/bin/sh\necho replacement\n").expect("replacement script");
        std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o755))
            .expect("replacement mode");

        let program = executable_from_captured_cwd(&cwd_handle, &root, &replacement)
            .expect("anchored executable");
        let output = std::process::Command::new(program)
            .output()
            .expect("run anchored executable");

        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "original");
    }

    #[test]
    fn terminal_cleanup_is_session_scoped() {
        let registry = AcpTerminalRegistry::default();
        let state = Arc::new(Mutex::new(AcpTerminalState::new(128)));
        let completed = Arc::new(tokio::sync::Notify::new());
        let (first_kill, first_kill_rx) = watch::channel(false);
        let (second_kill, second_kill_rx) = watch::channel(false);
        registry.records.lock().expect("terminal records").extend([
            (
                "first".to_string(),
                AcpTerminalRecord {
                    session_id: "native-first".to_string(),
                    state: Arc::clone(&state),
                    kill: first_kill,
                    completed: Arc::clone(&completed),
                },
            ),
            (
                "second".to_string(),
                AcpTerminalRecord {
                    session_id: "native-second".to_string(),
                    state,
                    kill: second_kill,
                    completed,
                },
            ),
        ]);

        registry
            .terminate_session("native-first")
            .expect("terminate first session");

        assert!(*first_kill_rx.borrow());
        assert!(!*second_kill_rx.borrow());
        assert_eq!(registry.records.lock().expect("terminal records").len(), 1);
    }

    #[tokio::test]
    async fn terminal_cleanup_waits_for_process_exit() {
        let registry = AcpTerminalRegistry::default();
        let state = Arc::new(Mutex::new(AcpTerminalState::new(128)));
        let completed = Arc::new(tokio::sync::Notify::new());
        let (kill, kill_rx) = watch::channel(false);
        registry.records.lock().expect("terminal records").insert(
            "terminal".to_string(),
            AcpTerminalRecord {
                session_id: "session".to_string(),
                state: Arc::clone(&state),
                kill,
                completed: Arc::clone(&completed),
            },
        );
        let terminating = {
            let registry = registry.clone();
            tokio::spawn(async move { registry.terminate_session_and_wait("session").await })
        };
        tokio::task::yield_now().await;

        assert!(*kill_rx.borrow());
        assert!(!terminating.is_finished());
        state.lock().expect("terminal state").exit_status = Some(
            agent_client_protocol::schema::v1::TerminalExitStatus::new()
                .signal(Some("killed".to_string())),
        );
        completed.notify_waiters();

        terminating
            .await
            .expect("termination task")
            .expect("termination");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn terminal_process_group_cleanup_kills_background_descendants() {
        use std::os::unix::process::CommandExt as _;

        let temp = tempfile::tempdir().expect("temp");
        let pid_file = temp.path().join("background.pid");
        let mut command = tokio::process::Command::new("sh");
        command
            .arg("-c")
            .arg(format!("sleep 60 & echo $! > {}; wait", pid_file.display()));
        command.as_std_mut().process_group(0);
        let mut child = command.spawn().expect("terminal process group");
        for _ in 0..100 {
            if pid_file.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let background_pid: libc::pid_t = std::fs::read_to_string(&pid_file)
            .expect("background pid")
            .trim()
            .parse()
            .expect("numeric pid");

        psychevo::process_env::terminate_tokio_child_process_group(&mut child).await;
        let _ = child.wait().await;
        for _ in 0..100 {
            let alive = unsafe { libc::kill(background_pid, 0) } == 0;
            if !alive {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("background terminal descendant {background_pid} survived process-group cleanup");
    }
}
