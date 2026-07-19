
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
    async fn exec_command_applies_runtime_env_to_child_process() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        fs::create_dir_all(&cwd).expect("cwd");
        let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

        let value = exec_command_tool_impl_with_context(
            cwd,
            ToolRuntimeContext {
                env: BTreeMap::from([(
                    "PSYCHEVO_EXEC_ENV_TEST".to_string(),
                    "from-runtime-env".to_string(),
                )]),
                ..ToolRuntimeContext::default()
            },
            "exec_command".to_string(),
            json!({"cmd": "printf '%s' \"$PSYCHEVO_EXEC_ENV_TEST\""}),
            receivers.abort_signal(),
        )
        .await
        .expect("exec command should receive runtime env");

        assert_eq!(value["exit_code"].as_i64(), Some(0), "{value}");
        assert_eq!(value["output"], "from-runtime-env");
    }

    #[tokio::test]
    async fn exec_command_emits_one_opaque_workspace_invalidation_after_spawn() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        fs::create_dir_all(&cwd).expect("cwd");
        let mutations = Arc::new(Mutex::new(Vec::new()));
        let observed = mutations.clone();
        let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

        exec_command_tool_impl_with_context(
            cwd,
            ToolRuntimeContext {
                workspace_mutations: Some(WorkspaceMutationSink::new(move |mutation| {
                    observed.lock().expect("mutations poisoned").push(mutation);
                })),
                ..ToolRuntimeContext::default()
            },
            "exec-command-mutation".to_string(),
            json!({"cmd": "printf done"}),
            receivers.abort_signal(),
        )
        .await
        .expect("exec command");

        assert_eq!(
            *mutations.lock().expect("mutations poisoned"),
            vec![WorkspaceMutation::Opaque {
                source: "exec_command".to_string(),
            }]
        );
    }

    #[test]
    fn subprocess_path_prepends_prefixes_to_runtime_env_path() {
        let temp = tempfile::tempdir().expect("temp");
        let tools = temp.path().join("tools");
        let inherited = temp.path().join("inherited");
        fs::create_dir_all(&tools).expect("tools");
        fs::create_dir_all(&inherited).expect("inherited");
        let env = BTreeMap::from([(
            "PATH".to_string(),
            inherited.to_string_lossy().to_string(),
        )]);

        let path = crate::process_env::prefixed_path_overlay(std::slice::from_ref(&tools), &env)
            .expect("path")
            .expect("prefixed path")
            .1;
        let entries = std::env::split_paths(&path).collect::<Vec<_>>();

        assert_eq!(entries.first(), Some(&tools));
        assert_eq!(entries.get(1), Some(&inherited));
    }

    #[test]
    fn windows_utf8_defaults_do_not_override_explicit_python_encoding() {
        let defaults = crate::process_env::windows_utf8_default_env(&BTreeMap::from([(
            "PYTHONIOENCODING".to_string(),
            "utf-16".to_string(),
        )]));

        assert!(
            defaults
                .iter()
                .all(|(key, _)| !key.eq_ignore_ascii_case("PYTHONIOENCODING")),
            "{defaults:?}"
        );
        assert!(defaults.iter().any(|(key, _)| *key == "PYTHONUTF8"));
    }

    #[test]
    fn exec_output_decodes_utf8_and_windows_gbk_bytes() {
        assert_eq!(
            crate::process_env::decode_process_output_for_platform("中文".as_bytes(), true),
            "中文"
        );
        assert_eq!(
            crate::process_env::decode_process_output_for_platform(
                &[0xD6, 0xD0, 0xCE, 0xC4],
                true
            ),
            "中文"
        );
    }

    #[test]
    fn exec_output_invalid_bytes_fall_back_to_lossy_text() {
        let output =
            crate::process_env::decode_process_output_for_platform(&[0xFF, 0xFF], true);

        assert!(!output.is_empty());
        assert!(output.contains('\u{FFFD}'), "{output:?}");
    }

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
            temp.path().canonicalize().expect("cwd"),
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
    async fn exec_command_sandbox_allows_standard_device_sinks() {
        if !std::path::Path::new("/dev/null").exists()
            || !std::path::Path::new("/dev/zero").exists()
        {
            eprintln!("skipping device sink sandbox test: standard devices are unavailable");
            return;
        }

        let work = tempfile::tempdir().expect("work");
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

        let value = exec_command_tool_impl_with_context(
            work.path().canonicalize().expect("cwd"),
            ToolRuntimeContext {
                sandbox_policy: policy,
                ..ToolRuntimeContext::default()
            },
            "exec_command".to_string(),
            json!({
                "cmd": "exec 3<>/dev/null && exec 4<>/dev/zero && echo ok",
                "yield_time_ms": 30000
            }),
            receivers.abort_signal(),
        )
        .await
        .expect("sandboxed command should open standard device sinks");

        assert_eq!(value["exit_code"].as_i64(), Some(0), "{value}");
        assert!(
            value["output"].as_str().unwrap_or_default().contains("ok"),
            "{value}"
        );
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
            work.path().canonicalize().expect("cwd"),
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
