#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn cli_run_keeps_agents_skill_and_prompt_as_separate_provider_messages() {
    let server = MockSseServer::start(vec![sse_text("combined final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("git");
    std::fs::write(workdir.join("AGENTS.md"), "Use project workflow.").expect("agents");
    write_home_skill(
        temp.path(),
        "reviewer",
        "review code",
        "Follow the reviewer workflow.",
    );
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "$reviewer do it",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let request = server.request_json(0);
    let contents = user_contents(&request);
    assert_eq!(contents.len(), 2);
    assert!(contents[0].contains("<skill>"));
    assert!(contents[0].contains("Follow the reviewer workflow."));
    assert_eq!(contents[1], "$reviewer do it");
    assert!(
        system_contents(&request)
            .iter()
            .any(|message| message.contains("# AGENTS.md instructions for")
                && message.contains("Use project workflow."))
    );

    let conn = Connection::open(db).expect("db");
    let user_evidence: Vec<(String, String)> = conn
        .prepare(
            "SELECT source_kind, provider_group FROM context_evidence WHERE role = 'user' ORDER BY context_seq",
        )
        .expect("prepare")
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("rows");
    assert_eq!(
        user_evidence,
        vec![(
            "selected_skill".to_string(),
            "selected_skill:0:reviewer".to_string()
        )]
    );
    let project_evidence: Vec<(String, String)> = conn
        .prepare(
            "SELECT role, provider_group FROM context_evidence WHERE source_kind = 'project_instruction' ORDER BY context_seq",
        )
        .expect("prepare")
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("rows");
    assert_eq!(
        project_evidence,
        vec![(
            "system".to_string(),
            "prefix_prompt_instructions".to_string()
        )]
    );
}

#[test]
pub(crate) fn cli_run_warns_for_claude_memory_without_loading_it() {
    let server = MockSseServer::start(vec![sse_text("claude final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("git");
    std::fs::write(workdir.join("CLAUDE.md"), "Do not load this Claude memory.").expect("claude");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "hello",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("json line"))
        .collect::<Vec<_>>();
    let warning = events
        .iter()
        .find(|event| {
            event["type"] == "entry.completed"
                && event["entry"]["source"] == "runtime.warning"
                && event["entry"]["metadata"]["kind"] == "project_instruction"
        })
        .expect("warning entry");
    assert!(
        warning["entry"]["blocks"][0]["body"]
            .as_str()
            .expect("message")
            .contains("Psychevo only loads AGENTS-named")
    );
    assert_eq!(
        warning["entry"]["metadata"]["suggestion"],
        "ln -s CLAUDE.md AGENTS.md"
    );
    assert!(!user_contents(&server.request_json(0))[0].contains("Do not load this Claude memory."));
}

#[test]
pub(crate) fn cli_run_default_writes_claude_warning_to_stderr_only() {
    let server = MockSseServer::start(vec![sse_text("default final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("git");
    std::fs::write(workdir.join("CLAUDE.local.md"), "local claude").expect("claude local");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir"), "hello"])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "default final\n"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("warning: Detected"));
    assert!(stderr.contains("suggestion: ln -s CLAUDE.local.md AGENTS.local.md"));
}

#[test]
pub(crate) fn cli_run_json_hides_reasoning_by_default_and_debug_flag_emits_it() {
    let server = MockSseServer::start(vec![
        sse_reasoning_then_text("private chain", "visible final"),
        sse_text("Hidden run title"),
        sse_reasoning_then_text("debug chain", "debug final"),
        sse_text("Shown run title"),
    ]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let hidden = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "hello",
        ])
        .output()
        .expect("pevo run hidden");
    assert!(
        hidden.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&hidden.stderr)
    );
    let hidden_stdout = String::from_utf8(hidden.stdout).expect("stdout");
    assert!(hidden_stdout.contains("visible final"));
    assert!(!hidden_stdout.contains("private chain"));
    assert!(!hidden_stdout.contains("reasoning_content"));

    let shown = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "--include-reasoning",
            "hello",
        ])
        .output()
        .expect("pevo run shown");
    assert!(
        shown.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&shown.stderr)
    );
    let shown_stdout = String::from_utf8(shown.stdout).expect("stdout");
    let events = shown_stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("json line"))
        .collect::<Vec<_>>();
    assert!(events.iter().any(|event| {
        event["type"] == "entry.completed"
            && event["entry"]["blocks"].as_array().is_some_and(|blocks| {
                blocks
                    .iter()
                    .any(|block| block["kind"] == "reasoning" && block["body"] == "debug chain")
            })
    }));
    assert!(shown_stdout.contains("debug final"));
}

#[test]
pub(crate) fn cli_run_include_reasoning_requires_json_format() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), "http://127.0.0.1:9");
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--include-reasoning",
            "hello",
        ])
        .output()
        .expect("pevo run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--format json"));
}

#[test]
pub(crate) fn cli_run_json_omits_metadata_only_message_updates() {
    let server = MockSseServer::start(vec![sse_metadata_usage_then_text("metadata final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "hello",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("json line"))
        .collect::<Vec<_>>();
    let empty_updates = events
        .iter()
        .filter(|event| {
            event["type"] == "message_update"
                && event["message"]["role"] == "assistant"
                && event["message"]["content"]
                    .as_array()
                    .is_some_and(|content| content.is_empty())
        })
        .count();
    assert_eq!(empty_updates, 0);
}

#[test]
pub(crate) fn cli_run_reads_stdin_and_appends_to_positional_prompt() {
    let server = MockSseServer::start(vec![
        sse_text("stdin final"),
        sse_text("Stdin title"),
        sse_text("append final"),
        sse_text("Append title"),
    ]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let mut stdin_only = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    stdin_only
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"from stdin\n")
        .expect("write stdin");
    let output = stdin_only.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut appended = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "fix",
            "this",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    appended
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"details\n")
        .expect("write stdin");
    let output = appended.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(user_contents(&server.request_json(0)), vec!["from stdin\n"]);
    assert_eq!(
        user_contents(&server.request_json(2)),
        vec!["fix this\ndetails\n"]
    );
}

#[test]
pub(crate) fn cli_run_empty_prompt_rejects_before_session_creation() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), "http://127.0.0.1:9");
    let mut child = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"   \n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("You must provide a message"));
    assert!(!db.exists());
}

#[test]
pub(crate) fn cli_run_errors_use_selected_output_format() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config_dir = temp.path().join("config");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    let config = config_dir.join("config.toml");
    std::fs::write(
        &config,
        r#"
model = "custom/local"

[provider.custom.options]
base_url = "https://example.invalid/v1"
api_key_env = "PSYCHEVO_TEST_MISSING_KEY_SHOULD_NOT_EXIST"

[provider.custom.models.local]
"#,
    )
    .expect("config");

    let default_output = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir"), "hello"])
        .output()
        .expect("pevo run");
    assert!(!default_output.status.success());
    assert!(String::from_utf8_lossy(&default_output.stderr).contains("requires credentials"));

    let json_output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "hello",
        ])
        .output()
        .expect("pevo run json");
    assert!(!json_output.status.success());
    assert!(String::from_utf8_lossy(&json_output.stderr).is_empty());
    let stdout = String::from_utf8(json_output.stdout).expect("stdout");
    let error = serde_json::from_str::<Value>(stdout.trim()).expect("error json");
    assert_eq!(error["type"], "error");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("requires credentials")
    );
}

#[test]
pub(crate) fn cli_run_requires_initialized_home_without_config_db_bypass() {
    let temp = tempdir().expect("temp");
    let output = pevo_cmd(temp.path())
        .args(["run", "hello"])
        .output()
        .expect("pevo run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("pevo init"));
    assert!(!temp.path().join(".psychevo").exists());
}

#[test]
pub(crate) fn cli_run_rejects_removed_flags() {
    let temp = tempdir().expect("temp");
    let cases: &[&[&str]] = &[
        &["run", "--prompt", "hello"],
        &["run", "--json", "hello"],
        &["run", "--provider", "deepseek", "hello"],
        &["run", "--base-url", "http://127.0.0.1:9", "hello"],
        &["run", "--api-key-env", "KEY", "hello"],
        &["run", "--db", "state.db", "hello"],
        &["run", "--workdir", ".", "hello"],
        &["run", "--max-context-messages", "1", "hello"],
        &["run", "--verbose", "hello"],
        &["run", "--config", "config.toml", "hello"],
    ];
    for args in cases {
        let output = pevo_cmd(temp.path())
            .args(*args)
            .output()
            .expect("pevo run");
        assert!(!output.status.success(), "expected failure for {args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(args[1]),
            "stderr did not mention rejected flag {args:?}: {stderr}"
        );
    }
}

#[test]
pub(crate) fn cli_run_model_override_requires_provider_qualified_model() {
    let server = MockSseServer::start(vec![sse_text("model final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "-m",
            "mock/mock-model",
            "hello",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(server.request_json(0)["model"], "mock-model");

    let invalid = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "-m",
            "mock-model",
            "hello",
        ])
        .output()
        .expect("pevo run invalid");
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("provider/model"));
}

#[test]
pub(crate) fn cli_run_variant_overrides_reasoning_effort_and_none_suppresses_it() {
    let high_server = MockSseServer::start(vec![sse_text("high")]);
    let temp = tempdir().expect("temp");
    let high_db = temp.path().join("high.db");
    let workdir = temp.path().join("work");
    let high_config = write_run_config(&temp.path().join("high-config"), &high_server.base_url);
    let high = isolated_run_cmd(temp.path(), &high_config, &high_db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--variant",
            "high",
            "hello",
        ])
        .output()
        .expect("pevo run high");
    assert!(high.status.success());
    assert_eq!(high_server.request_json(0)["reasoning_effort"], "high");

    let none_server = MockSseServer::start(vec![sse_text("none")]);
    let none_db = temp.path().join("none.db");
    let none_config = write_run_config_with_reasoning(
        &temp.path().join("none-config"),
        &none_server.base_url,
        Some("high"),
    );
    let none = isolated_run_cmd(temp.path(), &none_config, &none_db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--variant",
            "none",
            "hello",
        ])
        .output()
        .expect("pevo run none");
    assert!(none.status.success());
    assert!(
        none_server
            .request_json(0)
            .get("reasoning_effort")
            .is_none()
    );
}

#[test]
pub(crate) fn cli_run_continue_reuses_latest_matching_run_session() {
    let server = MockSseServer::start(vec![
        sse_text("first"),
        sse_text("First run title"),
        sse_text("second"),
    ]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let first = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir"), "first"])
        .output()
        .expect("first run");
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--continue",
            "second",
        ])
        .output()
        .expect("second run");
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let conn = Connection::open(&db).expect("db");
    let sessions: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE source = 'run'",
            [],
            |row| row.get(0),
        )
        .expect("sessions");
    assert_eq!(sessions, 1);
    let messages: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("messages");
    assert_eq!(messages, 4);

    let conflict = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--continue",
            "--session",
            "session-id",
            "hello",
        ])
        .output()
        .expect("conflict");
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("--continue"));
}

#[test]
pub(crate) fn cli_stats_reports_current_workdir_and_json() {
    let server = MockSseServer::start(vec![sse_text("stats")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let init = pevo_cmd(temp.path()).arg("init").output().expect("init");
    assert!(
        init.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let run = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir"), "hello"])
        .output()
        .expect("run");
    assert!(run.status.success());

    let stats = pevo_cmd(temp.path())
        .env("PSYCHEVO_DB", &db)
        .args([
            "stats",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--days",
            "30",
            "--limit",
            "3",
            "--json",
        ])
        .output()
        .expect("stats");
    assert!(
        stats.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&stats.stderr)
    );
    let report: Value = serde_json::from_slice(&stats.stdout).expect("stats json");
    assert_eq!(report["scope"]["all"], false);
    assert_eq!(report["totals"]["sessions"], 1);
    assert_eq!(report["totals"]["messages"], 2);
}
