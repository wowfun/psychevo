
pub(crate) fn spawn_pty_waiter_thread(
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

pub(crate) async fn await_session_result(
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

pub(crate) fn session_completed(session: &ExecSession) -> bool {
    session
        .state
        .lock()
        .is_ok_and(|state| state.exited && state.readers_active == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exec_command_rejects_tty_when_sandbox_enabled() {
        let temp = tempfile::tempdir().expect("temp");
        let env = BTreeMap::new();
        let policy = crate::sandbox::SandboxPolicy::from_config(
            &crate::sandbox::SandboxConfig {
                enabled: true,
                mode: crate::sandbox::SandboxMode::WorkspaceWrite,
                writable_roots: Vec::new(),
                include_tmp: false,
                include_common_caches: false,
            },
            temp.path(),
            RunMode::Default,
            &env,
        )
        .expect("sandbox policy");
        let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

        let err = exec_command_tool_impl_with_context(
            temp.path().canonicalize().expect("workdir"),
            ToolRuntimeContext {
                sandbox_policy: policy,
                ..ToolRuntimeContext::default()
            },
            "exec_command".to_string(),
            json!({"cmd": "printf hi", "tty": true}),
            receivers.abort_signal(),
        )
        .await
        .expect_err("sandbox tty denial");

        assert!(err.to_string().contains("tty=true is not supported"));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn exec_command_sandbox_blocks_writes_outside_workspace() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        let outside_file = outside.path().join("blocked.txt");
        let env = BTreeMap::new();
        let policy = crate::sandbox::SandboxPolicy::from_config(
            &crate::sandbox::SandboxConfig {
                enabled: true,
                mode: crate::sandbox::SandboxMode::WorkspaceWrite,
                writable_roots: Vec::new(),
                include_tmp: false,
                include_common_caches: false,
            },
            work.path(),
            RunMode::Default,
            &env,
        )
        .expect("sandbox policy");
        let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

        let result = exec_command_tool_impl_with_context(
            work.path().canonicalize().expect("workdir"),
            ToolRuntimeContext {
                sandbox_policy: policy,
                ..ToolRuntimeContext::default()
            },
            "exec_command".to_string(),
            json!({
                "cmd": format!("printf no > {}", outside_file.display()),
                "yield_time_ms": 30000
            }),
            receivers.abort_signal(),
        )
        .await;

        match result {
            Ok(value) => assert_ne!(value["exit_code"].as_i64(), Some(0), "{value}"),
            Err(err) => assert!(
                err.to_string().contains("denied by sandbox policy"),
                "{err}"
            ),
        }
        assert!(!outside_file.exists());
    }
}
