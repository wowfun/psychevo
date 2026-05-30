#[allow(unused_imports)]
pub(crate) use super::*;

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn exec_command_yielded_session_emits_background_lifecycle_events() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let events = Arc::new(Mutex::new(Vec::<crate::types::RunStreamEvent>::new()));
    let sink_events = Arc::clone(&events);
    let stream: crate::types::RunStreamSink = Arc::new(move |event| {
        sink_events.lock().expect("events").push(event);
    });
    let tools = crate::tools::coding_core_tools_for_mode_with_context(
        &workdir,
        RunMode::Default,
        crate::tools::ToolRuntimeContext {
            task_id: "exec-lifecycle-test".to_string(),
            lsp: crate::config::LspConfig::default(),
            lsp_manager: crate::tools::write_support::default_lsp_manager(),
            allow_login_shell: false,
            stream_events: Some(stream),
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
        },
    );
    let exec = tools
        .iter()
        .find(|tool| tool.name() == "exec_command")
        .expect("exec_command");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let result = exec
        .execute(
            "call_exec_lifecycle".to_string(),
            json!({
                "cmd": "printf start; sleep 0.5; printf done",
                "yield_time_ms": 250
            }),
            receivers.abort_signal(),
        )
        .await;

    assert!(!result.is_error, "{:?}", result.json);
    let session_id = result.json["session_id"].as_u64().expect("session id");
    assert_eq!(result.json["exit_code"], Value::Null);
    assert!(
        result.json["output"]
            .as_str()
            .unwrap_or_default()
            .contains("start")
    );
    assert!(result.json.get("error").is_none(), "{:?}", result.json);
    assert_event_type(&events, "exec_session_yielded");
    let delta = wait_for_event_type(&events, "exec_session_output_delta").await;
    assert_eq!(delta["session_id"], session_id);
    assert!(
        delta["output"]
            .as_str()
            .unwrap_or_default()
            .contains("done"),
        "{delta}"
    );
    let finished = wait_for_event_type(&events, "exec_session_finished").await;
    assert_eq!(finished["session_id"], session_id);
    assert_eq!(finished["exit_code"], 0);
    assert_eq!(finished["interrupted"], false);

    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let _ = crate::tools::write_stdin_tool_impl(
        json!({"session_id": session_id, "yield_time_ms": 5000}),
        receivers.abort_signal(),
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn interrupt_exec_sessions_for_task_emits_interrupted_finish() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let events = Arc::new(Mutex::new(Vec::<crate::types::RunStreamEvent>::new()));
    let sink_events = Arc::clone(&events);
    let stream: crate::types::RunStreamSink = Arc::new(move |event| {
        sink_events.lock().expect("events").push(event);
    });
    let tools = crate::tools::coding_core_tools_for_mode_with_context(
        &workdir,
        RunMode::Default,
        crate::tools::ToolRuntimeContext {
            task_id: "exec-interrupt-test".to_string(),
            lsp: crate::config::LspConfig::default(),
            lsp_manager: crate::tools::write_support::default_lsp_manager(),
            allow_login_shell: false,
            stream_events: Some(stream),
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
        },
    );
    let exec = tools
        .iter()
        .find(|tool| tool.name() == "exec_command")
        .expect("exec_command");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let result = exec
        .execute(
            "call_exec_interrupt".to_string(),
            json!({"cmd": "sleep 30", "yield_time_ms": 250}),
            receivers.abort_signal(),
        )
        .await;

    assert!(!result.is_error, "{:?}", result.json);
    let session_id = result.json["session_id"].as_u64().expect("session id");
    crate::tools::interrupt_exec_sessions_for_task("exec-interrupt-test");
    let finished = wait_for_event_type(&events, "exec_session_finished").await;
    assert_eq!(finished["session_id"], session_id);
    assert_eq!(finished["interrupted"], true);
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn exec_command_rejects_shell_background_wrappers() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let err = crate::tools::exec_command_tool_impl(
        workdir,
        false,
        json!({"cmd": "sleep 30 &"}),
        receivers.abort_signal(),
    )
    .await
    .expect_err("background wrapper should fail");

    assert!(err.to_string().contains("background"));
}

#[tokio::test]
pub(crate) async fn exec_command_pipe_stdin_is_closed_for_prompt_style_commands() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let result = crate::tools::exec_command_tool_impl(
        workdir,
        false,
        json!({
            "cmd": "if read line; then printf 'read:%s\\n' \"$line\"; else printf 'stdin closed\\n'; fi"
        }),
        receivers.abort_signal(),
    )
    .await
    .expect("exec result");

    assert_eq!(result["output"], "stdin closed\n");
}

#[tokio::test]
pub(crate) async fn exec_command_nonzero_exit_is_successful_result() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let result = crate::tools::exec_command_tool_impl(
        workdir,
        false,
        json!({"cmd": "exit 7", "yield_time_ms": 250}),
        receivers.abort_signal(),
    )
    .await
    .expect("exec result");

    assert_eq!(result["exit_code"], 7);
    assert!(result.get("error").is_none(), "{result}");
    assert!(result["session_id"].is_null(), "{result}");
}

#[tokio::test]
pub(crate) async fn exec_command_token_truncates_output() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let result = crate::tools::exec_command_tool_impl(
        workdir,
        false,
        json!({
            "cmd": "printf 'one two three four five six seven eight nine ten eleven twelve'",
            "max_output_tokens": 5
        }),
        receivers.abort_signal(),
    )
    .await
    .expect("exec result");

    assert!(result["original_token_count"].as_u64().unwrap_or_default() > 5);
    assert!(result["output"].as_str().unwrap_or_default().len() < 64);
}

#[tokio::test]
pub(crate) async fn write_stdin_polls_and_writes_to_tty_or_fallback_session() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let start = crate::tools::exec_command_tool_impl(
        workdir,
        false,
        json!({
            "cmd": "read line; printf 'got:%s\\n' \"$line\"",
            "tty": true,
            "yield_time_ms": 250
        }),
        receivers.abort_signal(),
    )
    .await
    .expect("exec result");
    let session_id = start["session_id"].as_u64().expect("session_id");

    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let result = crate::tools::write_stdin_tool_impl(
        json!({
            "session_id": session_id,
            "chars": "hello\n",
            "yield_time_ms": 250
        }),
        receivers.abort_signal(),
    )
    .await
    .expect("stdin result");

    assert!(
        result["output"]
            .as_str()
            .unwrap_or_default()
            .contains("got:hello")
    );
}

#[tokio::test]
pub(crate) async fn write_stdin_rejects_non_tty_pipe_session_input() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let start = crate::tools::exec_command_tool_impl(
        workdir,
        false,
        json!({"cmd": "sleep 1", "yield_time_ms": 250}),
        receivers.abort_signal(),
    )
    .await
    .expect("exec result");
    let session_id = start["session_id"].as_u64().expect("session_id");

    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let err = crate::tools::write_stdin_tool_impl(
        json!({"session_id": session_id, "chars": "hello\n"}),
        receivers.abort_signal(),
    )
    .await
    .expect_err("pipe stdin should fail");
    assert!(err.to_string().contains("stdin is closed"));

    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let _ = crate::tools::write_stdin_tool_impl(
        json!({"session_id": session_id, "chars": "", "yield_time_ms": 5000}),
        receivers.abort_signal(),
    )
    .await;
}

#[tokio::test]
pub(crate) async fn write_stdin_unknown_session_fails() {
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let err = crate::tools::write_stdin_tool_impl(
        json!({"session_id": 999_999_u64}),
        receivers.abort_signal(),
    )
    .await
    .expect_err("unknown session");
    assert!(err.to_string().contains("unknown exec_command session_id"));
}

pub(crate) fn configured_user_shell_context(
    temp: &tempfile::TempDir,
    _workdir: &std::path::Path,
) -> UserShellContextOptions {
    let home = home_dir(temp);
    fs::create_dir_all(&home).expect("home");
    fs::write(
        home.join("config.toml"),
        r#"
model = "lmstudio/test-model"

[provider.lmstudio.models.test-model]
        "#,
    )
    .expect("config");
    UserShellContextOptions {
        state: StateRuntime::open(temp.path().join("state.db")).expect("state runtime"),
        session: None,
        continue_latest: true,
        source: "tui".to_string(),
        continue_sources: vec!["run".to_string(), "tui".to_string()],
        config_path: None,
        model: None,
        reasoning_effort: None,
        mode: RunMode::Default,
        inherited_env: Some(BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                home.to_string_lossy().to_string(),
            ),
        ])),
    }
}

#[cfg(unix)]
pub(crate) fn shell_quote_path(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"))
}

#[cfg(unix)]
pub(crate) async fn wait_for_pid_file(path: &std::path::Path) -> i32 {
    let started = Instant::now();
    loop {
        if path.exists() {
            return read_pid_file(path);
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timed out waiting for pid file {}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(unix)]
pub(crate) fn read_pid_file(path: &std::path::Path) -> i32 {
    fs::read_to_string(path)
        .expect("pid file")
        .trim()
        .parse()
        .expect("pid")
}

#[cfg(unix)]
pub(crate) fn process_exists(pid: i32) -> bool {
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(unix)]
pub(crate) async fn wait_for_process_exit(pid: i32, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !process_exists(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}
