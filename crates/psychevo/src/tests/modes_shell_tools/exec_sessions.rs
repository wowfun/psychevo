use crate::state::StateRuntime;
use crate::tests::home_dir;
use crate::tests::modes_shell_tools::tool_modes::{assert_event_type, wait_for_event_type};
use crate::types::{RunMode, UserShellContextOptions};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tempfile::tempdir;

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn exec_command_yielded_session_emits_background_lifecycle_events() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let events = Arc::new(Mutex::new(Vec::<crate::types::RunStreamEvent>::new()));
    let sink_events = Arc::clone(&events);
    let stream: crate::types::RunStreamSink = Arc::new(move |event| {
        sink_events.lock().expect("events").push(event);
    });
    let tools = crate::tools::coding_core_tools_for_mode_with_context(
        &cwd,
        RunMode::Default,
        crate::tools::ToolRuntimeContext {
            task_id: "exec-lifecycle-test".to_string(),
            lsp: crate::config::LspConfig::default(),
            lsp_manager: crate::tools::write_support::patch_lsp::default_lsp_manager(),
            allow_login_shell: false,
            stream_events: Some(stream),
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            sandbox_policy: crate::sandbox::SandboxPolicy::disabled(),
            sandbox_grants: crate::sandbox::SandboxWriteGrants::default(),
            ..crate::tools::ToolRuntimeContext::default()
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
    let _ = crate::tools::exec_command::sessions::session_manager::write_stdin_tool_impl(
        json!({"session_id": session_id, "yield_time_ms": 5000}),
        receivers.abort_signal(),
    )
    .await;
}

#[test]
fn completed_tasks_without_exec_sessions_do_not_start_detach_workers() {
    for index in 0..1_000 {
        assert!(
            !crate::tools::exec_command::process::detach_exec_sessions_for_task(format!(
                "no-exec-session-{index}"
            )),
            "task without an exec session must take the allocation-free cleanup path"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn detached_exec_sessions_share_one_reaper_worker() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let mut session_ids = Vec::new();
    for task_id in ["exec-reaper-a", "exec-reaper-b"] {
        let tools = crate::tools::coding_core_tools_for_mode_with_context(
            &cwd,
            RunMode::Default,
            crate::tools::ToolRuntimeContext {
                task_id: task_id.to_string(),
                lsp: crate::config::LspConfig::default(),
                lsp_manager: crate::tools::write_support::patch_lsp::default_lsp_manager(),
                allow_login_shell: false,
                env: BTreeMap::new(),
                path_prefixes: Vec::new(),
                sandbox_policy: crate::sandbox::SandboxPolicy::disabled(),
                sandbox_grants: crate::sandbox::SandboxWriteGrants::default(),
                ..crate::tools::ToolRuntimeContext::default()
            },
        );
        let exec = tools
            .iter()
            .find(|tool| tool.name() == "exec_command")
            .expect("exec_command");
        let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
        let result = exec
            .execute(
                format!("call_{task_id}"),
                json!({"cmd": "sleep 30", "yield_time_ms": 250}),
                receivers.abort_signal(),
            )
            .await;
        assert!(!result.is_error, "{:?}", result.json);
        session_ids.push(result.json["session_id"].as_u64().expect("session id"));
        assert!(
            crate::tools::exec_command::process::detach_exec_sessions_for_task(task_id.to_string())
        );
    }

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if session_ids.iter().all(|session_id| {
                crate::tools::exec_command::process::get_exec_session(*session_id).is_none()
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("shared reaper cleanup");
    assert_eq!(
        crate::tools::exec_command::process::exec_session_reaper_start_count(),
        1,
        "all detached sessions must use the same process worker"
    );
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn interrupt_exec_sessions_for_task_emits_interrupted_finish() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let events = Arc::new(Mutex::new(Vec::<crate::types::RunStreamEvent>::new()));
    let sink_events = Arc::clone(&events);
    let stream: crate::types::RunStreamSink = Arc::new(move |event| {
        sink_events.lock().expect("events").push(event);
    });
    let tools = crate::tools::coding_core_tools_for_mode_with_context(
        &cwd,
        RunMode::Default,
        crate::tools::ToolRuntimeContext {
            task_id: "exec-interrupt-test".to_string(),
            lsp: crate::config::LspConfig::default(),
            lsp_manager: crate::tools::write_support::patch_lsp::default_lsp_manager(),
            allow_login_shell: false,
            stream_events: Some(stream),
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            sandbox_policy: crate::sandbox::SandboxPolicy::disabled(),
            sandbox_grants: crate::sandbox::SandboxWriteGrants::default(),
            ..crate::tools::ToolRuntimeContext::default()
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
    crate::tools::exec_command::process::interrupt_exec_sessions_for_task("exec-interrupt-test");
    let finished = wait_for_event_type(&events, "exec_session_finished").await;
    assert_eq!(finished["session_id"], session_id);
    assert_eq!(finished["interrupted"], true);
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn exec_command_rejects_shell_background_wrappers() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let err = crate::tools::exec_command::sessions::session_manager::exec_command_tool_impl(
        cwd,
        false,
        json!({"cmd": "sleep 30 &"}),
        receivers.abort_signal(),
    )
    .await
    .expect_err("background wrapper should fail");

    assert!(err.to_string().contains("background"));
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn exec_command_allows_foreground_heredoc_with_ampersand_content() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let result = crate::tools::exec_command::sessions::session_manager::exec_command_tool_impl(
        cwd.clone(),
        false,
        json!({
            "cmd": "cat > fixnull.c <<'EOF'\nint flags = value & mask;\nEOF\ncat fixnull.c",
            "yield_time_ms": 30000
        }),
        receivers.abort_signal(),
    )
    .await
    .expect("foreground heredoc should run");

    assert_eq!(result["exit_code"].as_i64(), Some(0), "{result}");
    assert_eq!(result["output"], "int flags = value & mask;\n");
    assert_eq!(
        fs::read_to_string(cwd.join("fixnull.c")).expect("heredoc output"),
        "int flags = value & mask;\n"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
pub(crate) async fn plan_exec_command_landlock_blocks_create_and_truncate() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let existing = cwd.join("existing.txt");
    fs::write(&existing, "preserved").expect("fixture");
    let policy = crate::sandbox::SandboxPolicy::from_config(
        &crate::config::SandboxConfig {
            enabled: false,
            mode: crate::sandbox::SandboxMode::WorkspaceWrite,
            writable_roots: Vec::new(),
            include_tmp: false,
            include_common_caches: false,
        },
        &cwd,
        RunMode::Plan,
        &BTreeMap::new(),
    )
    .expect("Plan sandbox");
    let tools = crate::tools::coding_core_tools_for_mode_with_context(
        &cwd,
        RunMode::Plan,
        crate::tools::ToolRuntimeContext {
            task_id: "plan-landlock-write-test".to_string(),
            lsp: crate::config::LspConfig::default(),
            lsp_manager: crate::tools::write_support::patch_lsp::default_lsp_manager(),
            allow_login_shell: false,
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            sandbox_policy: policy,
            sandbox_grants: crate::sandbox::SandboxWriteGrants::default(),
            ..crate::tools::ToolRuntimeContext::default()
        },
    );
    let exec = tools
        .iter()
        .find(|tool| tool.name() == "exec_command")
        .expect("exec_command");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let create = exec
        .execute(
            "plan-create".to_string(),
            json!({"cmd": "printf created > created.txt"}),
            receivers.abort_signal(),
        )
        .await;
    assert!(!create.is_error, "{:?}", create.json);
    assert_ne!(create.json["exit_code"], 0, "{:?}", create.json);
    assert!(!cwd.join("created.txt").exists());

    let truncate = exec
        .execute(
            "plan-truncate".to_string(),
            json!({"cmd": "printf changed > existing.txt"}),
            receivers.abort_signal(),
        )
        .await;
    assert!(!truncate.is_error, "{:?}", truncate.json);
    assert_ne!(truncate.json["exit_code"], 0, "{:?}", truncate.json);
    assert_eq!(fs::read_to_string(existing).expect("existing"), "preserved");
}

#[tokio::test]
pub(crate) async fn exec_command_pipe_stdin_is_closed_for_prompt_style_commands() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let result = crate::tools::exec_command::sessions::session_manager::exec_command_tool_impl(
        cwd,
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
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let result = crate::tools::exec_command::sessions::session_manager::exec_command_tool_impl(
        cwd,
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
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let result = crate::tools::exec_command::sessions::session_manager::exec_command_tool_impl(
        cwd,
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

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn write_stdin_polls_and_writes_to_tty_or_fallback_session() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let start = crate::tools::exec_command::sessions::session_manager::exec_command_tool_impl(
        cwd,
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
    let session_id = start["session_id"]
        .as_u64()
        .unwrap_or_else(|| panic!("interactive exec did not yield a session: {start}"));

    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let result = crate::tools::exec_command::sessions::session_manager::write_stdin_tool_impl(
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
            .contains("got:hello"),
        "{result}"
    );
}

#[tokio::test]
pub(crate) async fn write_stdin_rejects_non_tty_pipe_session_input() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let start = crate::tools::exec_command::sessions::session_manager::exec_command_tool_impl(
        cwd,
        false,
        json!({"cmd": "sleep 1", "yield_time_ms": 250}),
        receivers.abort_signal(),
    )
    .await
    .expect("exec result");
    let session_id = start["session_id"].as_u64().expect("session_id");

    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let err = crate::tools::exec_command::sessions::session_manager::write_stdin_tool_impl(
        json!({"session_id": session_id, "chars": "hello\n"}),
        receivers.abort_signal(),
    )
    .await
    .expect_err("pipe stdin should fail");
    assert!(err.to_string().contains("stdin is closed"));

    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let _ = crate::tools::exec_command::sessions::session_manager::write_stdin_tool_impl(
        json!({"session_id": session_id, "chars": "", "yield_time_ms": 5000}),
        receivers.abort_signal(),
    )
    .await;
}

#[tokio::test]
pub(crate) async fn write_stdin_unknown_session_fails() {
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let err = crate::tools::exec_command::sessions::session_manager::write_stdin_tool_impl(
        json!({"session_id": 999_999_u64}),
        receivers.abort_signal(),
    )
    .await
    .expect_err("unknown session");
    assert!(err.to_string().contains("unknown exec_command session_id"));
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn write_stdin_rejects_session_owned_by_another_task() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let owner_tools = crate::tools::coding_core_tools_for_mode_with_context(
        &cwd,
        RunMode::Default,
        crate::tools::ToolRuntimeContext {
            task_id: "exec-owner-a".to_string(),
            ..crate::tools::ToolRuntimeContext::default()
        },
    );
    let other_tools = crate::tools::coding_core_tools_for_mode_with_context(
        &cwd,
        RunMode::Default,
        crate::tools::ToolRuntimeContext {
            task_id: "exec-owner-b".to_string(),
            ..crate::tools::ToolRuntimeContext::default()
        },
    );
    let exec = owner_tools
        .iter()
        .find(|tool| tool.name() == "exec_command")
        .expect("exec_command");
    let write_stdin = other_tools
        .iter()
        .find(|tool| tool.name() == "write_stdin")
        .expect("write_stdin");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let start = exec
        .execute(
            "exec-owner-start".to_string(),
            json!({"cmd": "sleep 30", "yield_time_ms": 250}),
            receivers.abort_signal(),
        )
        .await;
    assert!(!start.is_error, "{:?}", start.json);
    let session_id = start.json["session_id"].as_u64().expect("session id");

    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let result = write_stdin
        .execute(
            "exec-owner-cross-task".to_string(),
            json!({"session_id": session_id, "chars": "x"}),
            receivers.abort_signal(),
        )
        .await;
    crate::tools::exec_command::process::interrupt_exec_sessions_for_task("exec-owner-a");

    assert!(result.is_error, "{:?}", result.json);
    assert_eq!(
        result.json["error"],
        format!("unknown exec_command session_id: {session_id}")
    );
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn plan_write_stdin_cannot_mutate_a_default_turn_session() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let target = cwd.join("plan-stdin-must-not-write");
    let task_id = "shared-default-plan-task";
    let default_tools = crate::tools::coding_core_tools_for_mode_with_context(
        &cwd,
        RunMode::Default,
        crate::tools::ToolRuntimeContext {
            task_id: task_id.to_string(),
            ..crate::tools::ToolRuntimeContext::default()
        },
    );
    let plan_policy = crate::sandbox::SandboxPolicy::from_config(
        &crate::sandbox::SandboxConfig::default(),
        &cwd,
        RunMode::Plan,
        &BTreeMap::new(),
    )
    .expect("Plan sandbox policy");
    let plan_tools = crate::tools::coding_core_tools_for_mode_with_context(
        &cwd,
        RunMode::Plan,
        crate::tools::ToolRuntimeContext {
            task_id: task_id.to_string(),
            sandbox_policy: plan_policy,
            ..crate::tools::ToolRuntimeContext::default()
        },
    );
    let exec = default_tools
        .iter()
        .find(|tool| tool.name() == "exec_command")
        .expect("exec_command");
    let write_stdin = plan_tools
        .iter()
        .find(|tool| tool.name() == "write_stdin")
        .expect("write_stdin");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let start = exec
        .execute(
            "default-start".to_string(),
            json!({
                "cmd": format!("read line; test \"$line\" = mutate && touch {}", target.display()),
                "tty": true,
                "yield_time_ms": 250
            }),
            receivers.abort_signal(),
        )
        .await;
    assert!(!start.is_error, "{:?}", start.json);
    let session_id = start.json["session_id"].as_u64().expect("session id");

    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let denied = write_stdin
        .execute(
            "plan-write".to_string(),
            json!({"session_id": session_id, "chars": "mutate\n"}),
            receivers.abort_signal(),
        )
        .await;
    assert!(denied.is_error, "{:?}", denied.json);
    assert!(
        denied.json["error"]
            .as_str()
            .is_some_and(|error| error.contains("denied by sandbox policy")),
        "{:?}",
        denied.json
    );

    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let poll = write_stdin
        .execute(
            "plan-poll".to_string(),
            json!({"session_id": session_id, "chars": "", "yield_time_ms": 5000}),
            receivers.abort_signal(),
        )
        .await;
    crate::tools::exec_command::process::interrupt_exec_sessions_for_task(task_id);

    assert!(!poll.is_error, "{:?}", poll.json);
    assert!(
        !target.exists(),
        "Plan stdin must not reach the Default shell"
    );
}

pub(crate) async fn configured_user_shell_context(
    temp: &tempfile::TempDir,
    _cwd: &std::path::Path,
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
        state: StateRuntime::open(temp.path().join("state.db"))
            .await
            .expect("state runtime"),
        session: None,
        continue_latest: true,
        source: "tui".to_string(),
        continue_sources: vec!["run".to_string(), "tui".to_string()],
        config_path: None,
        model: None,
        reasoning_effort: None,
        mode: RunMode::Default,
    }
}

pub(crate) fn configured_user_shell_environment(
    temp: &tempfile::TempDir,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_HOME".to_string(),
            home_dir(temp).to_string_lossy().to_string(),
        ),
    ])
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
