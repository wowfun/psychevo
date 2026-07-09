#[allow(unused_imports)]
pub(crate) use super::*;

#[allow(unused_imports)]
use serde_json::json;

pub(crate) fn session_result_json(
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
            "cwd": session.cwd.display().to_string(),
            "started_at_ms": session.started_at_ms,
        })));
    }
    let output = decode_exec_output(&chunk);
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

pub(crate) fn truncate_output_tokens(output: &str, max_tokens: usize) -> (String, usize) {
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

pub(crate) fn decode_exec_output(bytes: &[u8]) -> String {
    crate::process_env::decode_process_output(bytes)
}

pub(crate) fn truncate_output_chars(output: &str, max_tokens: usize) -> (String, usize) {
    let original = output.chars().count().div_ceil(4);
    let max_chars = max_tokens.saturating_mul(4);
    if output.chars().count() <= max_chars {
        return (output.to_string(), original);
    }
    let chars = output.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(max_chars);
    (chars[start..].iter().collect(), original)
}

pub(crate) fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(crate) fn duration_ms_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

pub(crate) fn bounded_stdin_chars(chars: &str) -> String {
    if chars.chars().count() <= EXEC_STDIN_EVENT_MAX_CHARS {
        return chars.to_string();
    }
    chars
        .chars()
        .take(EXEC_STDIN_EVENT_MAX_CHARS)
        .collect::<String>()
}

pub(crate) fn reserve_exec_session_id() -> Result<u64> {
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

pub(crate) fn insert_exec_session(session: Arc<ExecSession>) -> Result<()> {
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

pub(crate) fn active_exec_session_count(registry: &ExecSessionRegistry) -> usize {
    registry
        .sessions
        .values()
        .filter(|session| !session_completed(session))
        .count()
}

pub(crate) fn get_exec_session(session_id: u64) -> Option<Arc<ExecSession>> {
    EXEC_SESSIONS
        .lock()
        .ok()
        .and_then(|registry| registry.sessions.get(&session_id).cloned())
}

pub(crate) fn remove_exec_session(session_id: u64) {
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

pub(crate) fn sessions_for_task(task_id: &str) -> Vec<Arc<ExecSession>> {
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

pub(crate) fn resolve_exec_cwd(accepted_cwd: &Path, raw: Option<&str>) -> Result<PathBuf> {
    let path = match raw {
        Some(raw) => crate::host_paths::resolve_input_path(raw, accepted_cwd)?,
        None => accepted_cwd.to_path_buf(),
    };
    let path = crate::paths::normalize_canonical_cwd(path.canonicalize()?);
    if !path.is_dir() {
        return Err(Error::Message(format!(
            "cwd is not a directory: {}",
            path.display()
        )));
    }
    Ok(path)
}

pub(crate) fn default_shell_for_env(env_map: &BTreeMap<String, String>) -> Result<String> {
    #[cfg(windows)]
    {
        Ok(crate::host_paths::GitBashRuntime::discover(env_map)?
            .bash
            .display()
            .to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = env_map;
        Ok(std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()))
    }
}

pub(crate) fn shell_args(shell: &str, login: bool, command: &str) -> Result<Vec<String>> {
    #[cfg(windows)]
    {
        if !crate::host_paths::shell_is_git_bash(shell) {
            return Err(Error::Message(
                "native Windows shell execution supports Git Bash only; install Git for Windows or set PSYCHEVO_GIT_BASH_PATH"
                    .to_string(),
            ));
        }
        Ok(vec![
            if login { "-lc" } else { "-c" }.to_string(),
            command.to_string(),
        ])
    }
    #[cfg(not(windows))]
    {
        let _ = shell;
        Ok(vec![
            if login { "-lc" } else { "-c" }.to_string(),
            command.to_string(),
        ])
    }
}

pub(crate) fn apply_subprocess_env(
    command: &mut std::process::Command,
    invocation: &ExecInvocation,
) -> Result<()> {
    crate::process_env::apply_process_env(
        command,
        &invocation.env,
        crate::process_env::ProcessEnvOptions::new(&invocation.path_prefixes),
    )
}

pub(crate) fn apply_pty_subprocess_env(
    command: &mut portable_pty::CommandBuilder,
    invocation: &ExecInvocation,
) -> Result<()> {
    crate::process_env::apply_pty_process_env(
        command,
        &invocation.env,
        crate::process_env::ProcessEnvOptions::new(&invocation.path_prefixes),
    )
}

pub(crate) fn clamp_yield_ms(value: Option<i64>, default: u64, min: u64, max: u64) -> u64 {
    let value = value.unwrap_or(default as i64);
    value.clamp(min as i64, max as i64) as u64
}

pub(crate) fn output_token_limit(value: Option<i64>) -> Result<usize> {
    let value = value.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS as i64);
    if value < 1 {
        return Err(Error::Message("max_output_tokens must be >= 1".to_string()));
    }
    Ok(value as usize)
}

pub(crate) fn required_u64(args: &Value, key: &str) -> Result<u64> {
    args.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Message(format!("{key} must be an integer")))
}

pub(crate) fn reject_untracked_background_command(command: &str) -> Result<()> {
    if crate::permissions::shell_has_untracked_background(command) {
        return Err(Error::Message(
            "shell-level background wrappers are not supported; run the foreground command and let exec_command return a session_id"
                .to_string(),
        ));
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn configure_process_group(command: &mut std::process::Command) {
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
pub(crate) fn configure_process_group(_command: &mut std::process::Command) {}

#[cfg(unix)]
pub(crate) fn detach_from_tty() -> std::io::Result<()> {
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
pub(crate) fn set_process_group() -> std::io::Result<()> {
    let result = unsafe { libc::setpgid(0, 0) };
    if result == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn set_parent_death_signal(parent_pid: libc::pid_t) -> std::io::Result<()> {
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
pub(crate) fn terminate_std_child_tree(child: &mut std::process::Child) {
    crate::process_env::terminate_std_child_process_group(child);
}

#[cfg(not(unix))]
pub(crate) fn terminate_std_child_tree(child: &mut std::process::Child) {
    crate::process_env::terminate_std_child_process_group(child);
}

#[cfg(test)]
mod tests {
    #[test]
    fn exec_command_cwd_normalizer_removes_windows_verbatim_prefix() {
        let cwd = crate::paths::normalize_canonical_cwd(std::path::PathBuf::from(r"\\?\C:\repo"));

        assert_eq!(cwd, std::path::PathBuf::from(r"C:\repo"));
    }
}
