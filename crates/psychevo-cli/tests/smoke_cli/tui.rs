#[allow(unused_imports)]
pub(crate) use super::*;
#[test]
pub(crate) fn cli_tui_initial_prompt_shows_thinking_by_default() {
    let server = MockSseServer::start(vec![sse_reasoning_then_text(
        "private chain",
        "visible tui",
    )]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let output = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir"), "hello"])
        .output()
        .expect("pevo tui");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("Thinking:"));
    assert!(stdout.contains("private chain"));
    assert!(stdout.contains("visible tui"));
}

#[test]
pub(crate) fn cli_tui_bang_shell_rejects_missing_provider_config_before_execution() {
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    let marker = workdir.join("marker");
    let config = temp.path().join("missing-config.toml");

    let output = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args([
            "tui",
            "--dir",
            workdir.to_str().expect("workdir"),
            "!touch marker",
        ])
        .output()
        .expect("pevo tui shell");

    assert!(!output.status.success());
    assert!(!marker.exists());
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(!stdout.contains("exec_command touch marker"), "{stdout}");
    assert!(!stdout.contains("Prompt:"), "{stdout}");
    assert!(!stdout.contains("Answer:"), "{stdout}");
}

#[test]
pub(crate) fn cli_tui_bang_shell_persists_context_with_config() {
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    let config = write_run_config(&temp.path().join("config"), "http://127.0.0.1:9");

    let output = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args([
            "tui",
            "--dir",
            workdir.to_str().expect("workdir"),
            "!printf shell-cli-ok",
        ])
        .output()
        .expect("pevo tui shell");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("! printf shell-cli-ok"), "{stdout}");
    assert!(!stdout.contains("Prompt:"), "{stdout}");
    assert!(!stdout.contains("Answer:"), "{stdout}");

    let conn = Connection::open(&db).expect("db");
    let session_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("session count");
    assert_eq!(session_count, 1);
    let (content_text, message_json, metadata_json): (String, String, String) = conn
        .query_row(
            "SELECT content_text, message_json, metadata_json FROM messages",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("message");
    assert_eq!(content_text, "!printf shell-cli-ok");
    assert!(message_json.contains("<user_shell_command>"));
    assert!(message_json.contains("<command>printf shell-cli-ok</command>"));
    let metadata: Value = serde_json::from_str(&metadata_json).expect("metadata");
    assert_eq!(
        metadata["user_shell"]["command"].as_str(),
        Some("printf shell-cli-ok")
    );
}

#[test]
pub(crate) fn cli_tui_debug_shows_usage_metadata_summary() {
    let server = MockSseServer::start(vec![sse_metadata_usage_then_text("debug metrics")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let output = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args([
            "tui",
            "--debug",
            "--dir",
            workdir.to_str().expect("workdir"),
            "hello",
        ])
        .output()
        .expect("pevo tui");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("Answer:"));
    assert!(stdout.contains("debug metrics"));
    assert!(stdout.contains("Meta:"));
    assert!(stdout.contains("usage 3 input 4 output"));
    assert!(stdout.contains("response resp_1"));
    assert!(!stdout.contains("total_tokens="));
    assert!(!stdout.contains("provider_response_id="));
}

#[test]
pub(crate) fn cli_tui_thinking_toggle_hides_reasoning_and_persists() {
    let server = MockSseServer::start(vec![sse_reasoning_then_text("debug chain", "visible tui")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/show-thinking off\nhello\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("thinking: off"));
    assert!(!stdout.contains("Thinking: hidden"));
    assert!(!stdout.contains("debug chain"));
    assert!(stdout.contains("visible tui"));

    let state = std::fs::read_to_string(home.join("tui-state.json")).expect("state");
    assert!(state.contains(r#""thinking_visible": false"#));
}

#[test]
pub(crate) fn cli_tui_status_shows_configured_default_variant() {
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config_with_reasoning(
        &temp.path().join("config"),
        "http://127.0.0.1:9",
        Some("xhigh"),
    );

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/status\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(!stdout.contains("> /status"));
    assert!(!stdout.contains('└'));
    assert!(stdout.contains("model: mock/mock-model"));
    assert!(stdout.contains("variant: xhigh"));
    let expected_status = format!(
        "workdir: {}\nhome: {}\ndb: {}\nsession: (none)\nmodel: mock/mock-model\nvariant: xhigh\nmode: default\npermission_mode: default\nagent: (default)\nagents: on\ndebug: off",
        workdir.display(),
        home.display(),
        db.display()
    );
    assert!(
        stdout.contains(&expected_status),
        "stdout did not contain status block:\n{stdout}"
    );
}

#[test]
pub(crate) fn cli_tui_help_prints_commands_from_registry() {
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), "http://127.0.0.1:9");

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/help\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(!stdout.contains("> /help"));
    assert!(!stdout.contains('└'));
    assert!(stdout.contains("General\n"));
    assert!(stdout.contains("\nCommands\n"));
    assert!(stdout.contains("\nCustom commands\n"));
    assert!(stdout.contains("/usage - local usage and cost (aliases: /stats)"));
    assert!(stdout.contains("Reads persisted SQLite accounting and cost estimates"));
    assert!(stdout.contains("No custom commands available"));
    assert!(!stdout.contains("pevo run"));
}

#[test]
pub(crate) fn cli_tui_new_is_silent_until_next_prompt() {
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), "http://127.0.0.1:9");

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/new\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(!stdout.contains("new session will start on next prompt"));
}

#[test]
pub(crate) fn cli_tui_scripted_undo_and_redo_print_deterministic_status() {
    let server = MockSseServer::start(vec![sse_text("visible tui")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&workdir)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"hello\n/undo\n/redo\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("visible tui"));
    assert!(stdout.contains("undone 2 messages; prompt restored"));
    assert!(stdout.contains("redone 2 messages; complete"));
}

#[test]
pub(crate) fn cli_tui_mode_set_plan_persists_and_uses_read_only_tools() {
    let server = MockSseServer::start(vec![sse_text("planned")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/mode plan\nhello\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("mode: plan"));
    assert!(stdout.contains("planned"));

    let request = server.request_json(0);
    let tool_names = request["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["function"]["name"].as_str().expect("tool").to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        tool_names,
        vec![
            "read",
            "exec_command",
            "write_stdin",
            "clarify",
            "list_skills",
            "view_skill",
            "skill_hub",
            "skill_config",
            "spawn_agent",
            "list_agents",
            "wait_agent",
            "send_message",
            "close_agent",
            "resume_agent",
        ]
    );
    assert_eq!(request["messages"][0]["role"], "system");
    assert!(
        request["messages"][0]["content"]
            .as_str()
            .expect("system")
            .contains("read-only")
    );

    let state = std::fs::read_to_string(home.join("tui-state.json")).expect("state");
    assert!(state.contains(r#""mode": "plan""#));

    let conn = Connection::open(&db).expect("db");
    let system_messages: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE role = 'system'",
            [],
            |row| row.get(0),
        )
        .expect("system messages");
    assert_eq!(system_messages, 0);
}

#[test]
pub(crate) fn cli_tui_model_lists_configured_entries_without_prompt() {
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_multi_model_config(&temp.path().join("config"), "http://127.0.0.1:9");

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/model\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("mock/mock-model"));
    assert!(stdout.contains("mock/other-model variant=high"));
}

#[test]
pub(crate) fn cli_tui_continues_latest_run_or_tui_session_and_new_creates_tui_session() {
    let server = MockSseServer::start(vec![
        sse_text("first"),
        sse_text("second"),
        sse_text("third"),
    ]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let run = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir"), "first"])
        .output()
        .expect("pevo run");
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let continued = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir"), "second"])
        .output()
        .expect("pevo tui continue");
    assert!(
        continued.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&continued.stderr)
    );

    let conn = Connection::open(&db).expect("db");
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("sessions");
    assert_eq!(sessions, 1);
    let messages: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("messages");
    assert_eq!(messages, 4);

    let new_session = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args([
            "tui",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--new",
            "third",
        ])
        .output()
        .expect("pevo tui new");
    assert!(
        new_session.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&new_session.stderr)
    );

    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("sessions");
    assert_eq!(sessions, 2);
    let tui_sessions: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE source = 'tui'",
            [],
            |row| row.get(0),
        )
        .expect("tui sessions");
    assert_eq!(tui_sessions, 1);
}

#[test]
pub(crate) fn cli_tui_sessions_lists_sessions_and_unknown_slash_falls_back_to_prompt() {
    let initial_server = MockSseServer::start(vec![
        sse_reasoning_then_text("hidden chain", "visible"),
        sse_text("initial title"),
    ]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config_dir = temp.path().join("config");
    let config = write_run_config(&config_dir, &initial_server.base_url);

    let first = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir"), "hello"])
        .output()
        .expect("pevo tui");
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let fallback_server = MockSseServer::start(vec![
        sse_reasoning_then_text("fallback chain", "fallback visible"),
        sse_text("fallback title"),
    ]);
    let config = write_run_config(&config_dir, &fallback_server.base_url);
    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/sessions\n/session show latest\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(!stderr.contains("unknown slash command: /session"));
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("mock/mock-model"));
    assert!(stdout.contains("fallback visible"));
    assert!(stdout.contains("fallback chain"));
}

#[test]
pub(crate) fn cli_tui_sessions_scripted_fallback_lists_sessions() {
    let server = MockSseServer::start(vec![sse_reasoning_then_text("hidden chain", "visible")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let first = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir"), "hello"])
        .output()
        .expect("pevo tui");
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/sessions\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("mock/mock-model"));
    assert!(!stdout.contains("hidden chain"));
}

#[test]
pub(crate) fn cli_tui_sessions_scripted_fallback_hides_archived_sessions() {
    let server = MockSseServer::start(vec![sse_reasoning_then_text("hidden chain", "visible")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let first = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir"), "hello"])
        .output()
        .expect("pevo tui");
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let conn = Connection::open(&db).expect("db");
    conn.execute("UPDATE sessions SET archived_at_ms = updated_at_ms + 1", [])
        .expect("archive");

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/sessions\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("no sessions"));
    assert!(!stdout.contains("mock/mock-model"));
    assert!(!stdout.contains("hidden chain"));
}
