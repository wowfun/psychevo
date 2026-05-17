#[test]
fn cli_run_positional_prompt_outputs_final_answer_and_persists_metadata() {
    let server = MockSseServer::start(vec![sse_text("mock final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "hello",
            "world",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "mock final\n"
    );
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    let request = server.request_json(0);
    assert_eq!(request["model"], "mock-model");
    assert_eq!(user_contents(&request), vec!["hello world"]);

    let conn = Connection::open(db).expect("db");
    let (source, provider, model): (String, String, String) = conn
        .query_row("SELECT source, provider, model FROM sessions", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .expect("session");
    assert_eq!(source, "run");
    assert_eq!(provider, "mock");
    assert_eq!(model, "mock-model");
    let metadata_json: String = conn
        .query_row(
            "SELECT metadata_json FROM messages WHERE role = 'assistant'",
            [],
            |row| row.get(0),
        )
        .expect("metadata");
    let metadata: Value = serde_json::from_str(&metadata_json).expect("metadata json");
    assert!(metadata["elapsed_ms"].as_u64().is_some());
}

#[test]
fn cli_run_selected_main_agent_includes_description_and_body() {
    let server = MockSseServer::start(vec![sse_text("agent final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("git");
    std::fs::create_dir_all(workdir.join(".claude/agents")).expect("agents");
    std::fs::write(
        workdir.join(".claude/agents/translate.md"),
        "---\nname: translate\ndescription: Detect the source language automatically. Translate Chinese to English; translate all other languages to Chinese.\n---\nPreserve tone, meaning, punctuation, emoji, and inline formatting. Return only the translated text.",
    )
    .expect("agent");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--agent",
            "translate",
            "-f",
            "json",
            "shell",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let request = server.request_json(0);
    let system_messages = system_contents(&request);
    let selected = system_messages
        .iter()
        .find(|message| message.contains("Main session agent: translate"))
        .expect("selected agent system prompt");
    assert!(
        selected.contains("Purpose:\nDetect the source language automatically."),
        "{selected}"
    );
    assert!(
        selected.contains("Translate Chinese to English; translate all other languages to Chinese."),
        "{selected}"
    );
    assert!(
        selected.contains("Instructions:\nPreserve tone, meaning, punctuation"),
        "{selected}"
    );
    assert_eq!(user_contents(&request), vec!["shell"]);
}

#[test]
fn cli_run_child_agent_session_exports_prefix_and_last_request() {
    let server = MockSseServer::start(vec![
        sse_tool_agent_call("call_agent", "translate", "hello"),
        sse_text("child translated"),
        sse_text("parent final"),
    ]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let psychevo_home = init_tui_home(temp.path());
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("git");
    std::fs::create_dir_all(workdir.join(".claude/agents")).expect("agents");
    std::fs::write(
        workdir.join(".claude/agents/translate.md"),
        "---\nname: translate\ndescription: Translate English to Chinese.\n---\nReturn only the translated text.",
    )
    .expect("agent");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "-f",
            "json",
            "use translate",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let child_request = server.request_json(1);
    let child_system = system_contents(&child_request);
    assert!(child_system
        .iter()
        .any(|message| message.contains("Child agent: translate")));
    assert!(child_system
        .iter()
        .any(|message| message.contains("child agent. Return a concise final answer")));

    let conn = Connection::open(&db).expect("db");
    let child_session: String = conn
        .query_row("SELECT id FROM sessions WHERE source = 'agent'", [], |row| row.get(0))
        .expect("child session");
    let prefix_slots: Vec<String> = conn
        .prepare(
            "SELECT json_extract(value, '$.slot') FROM session_prompt_prefixes, json_each(slots_json) WHERE session_id = ?1 ORDER BY json_extract(value, '$.order')",
        )
        .expect("prepare")
        .query_map([child_session.as_str()], |row| row.get::<_, String>(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("slots");
    assert_eq!(
        prefix_slots,
        vec![
            "base/mode".to_string(),
            "selected_child_agent".to_string(),
            "child_agent_control".to_string(),
        ]
    );
    let evidence_slots: Vec<String> = conn
        .prepare(
            "SELECT json_extract(metadata_json, '$.slot') FROM context_evidence WHERE session_id = ?1 ORDER BY context_seq",
        )
        .expect("prepare")
        .query_map([child_session.as_str()], |row| row.get::<_, String>(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("evidence");
    assert_eq!(evidence_slots, prefix_slots);

    let export = isolated_tui_cmd(temp.path(), &psychevo_home, &config, &db)
        .args([
            "session",
            "export",
            &child_session,
            "-f",
            "json",
            "-i",
            "h,m,lpr",
        ])
        .output()
        .expect("child session export");
    assert!(
        export.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    let value: Value = serde_json::from_slice(&export.stdout).expect("json export");
    assert_eq!(
        value["header"]["prompt_prefix"]["slots"][1]["slot"],
        "selected_child_agent"
    );
    let exported_messages = value["last_provider_request"]["body"]["messages"]
        .as_array()
        .expect("provider messages");
    assert!(exported_messages
        .iter()
        .any(|message| message["content"]
            .as_str()
            .is_some_and(|content| content.contains("Child agent: translate"))));
    assert!(exported_messages
        .iter()
        .any(|message| message["content"]
            .as_str()
            .is_some_and(|content| content
                .contains("You are running as a child agent"))));
}

#[test]
fn cli_run_dir_controls_tool_workdir() {
    let server = MockSseServer::start(vec![sse_tool_read_then_done(), sse_text("read complete")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    std::fs::write(workdir.join("fixture.txt"), "fixture content\n").expect("fixture");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "read",
            "fixture.txt",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "read complete\n"
    );

    let conn = Connection::open(db).expect("db");
    let read_results: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE role = 'tool_result' AND tool_name = 'read' AND outcome = 'normal'",
            [],
            |row| row.get(0),
        )
        .expect("read results");
    assert_eq!(read_results, 1);
}

#[test]
fn cli_run_allows_more_than_thirty_two_tool_turns_before_final_answer() {
    let mut responses = (0..33)
        .map(|index| sse_tool_read_call(&format!("call_read_{index}")))
        .collect::<Vec<_>>();
    responses.push(sse_text("long workflow complete"));
    let server = MockSseServer::start(responses);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    std::fs::write(workdir.join("fixture.txt"), "fixture content\n").expect("fixture");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "read",
            "fixture.txt",
            "several",
            "times",
        ])
        .output()
        .expect("pevo run");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "long workflow complete\n"
    );

    let conn = Connection::open(db).expect("db");
    let read_results: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE role = 'tool_result' AND tool_name = 'read' AND outcome = 'normal'",
            [],
            |row| row.get(0),
        )
        .expect("read results");
    assert_eq!(read_results, 33);
    let end_reason: String = conn
        .query_row("SELECT end_reason FROM sessions", [], |row| row.get(0))
        .expect("end reason");
    assert_eq!(end_reason, "normal");
    assert_eq!(server.requests.lock().expect("requests").len(), 34);
}

#[test]
fn cli_run_budget_exhaustion_reports_model_turn_limit() {
    let responses = (0..128)
        .map(|index| sse_tool_read_call(&format!("call_read_{index}")))
        .collect::<Vec<_>>();
    let server = MockSseServer::start(responses);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    std::fs::write(workdir.join("fixture.txt"), "fixture content\n").expect("fixture");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "keep",
            "reading",
        ])
        .output()
        .expect("pevo run");

    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "\n");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("turn ended: failed - reached model-turn limit (128)"));

    let conn = Connection::open(db).expect("db");
    let end_reason: String = conn
        .query_row("SELECT end_reason FROM sessions", [], |row| row.get(0))
        .expect("end reason");
    assert_eq!(end_reason, "failed");
    assert_eq!(server.requests.lock().expect("requests").len(), 128);
}

#[test]
fn cli_run_json_budget_exhaustion_includes_terminal_reason() {
    let responses = (0..128)
        .map(|index| sse_tool_read_call(&format!("call_read_{index}")))
        .collect::<Vec<_>>();
    let server = MockSseServer::start(responses);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    std::fs::write(workdir.join("fixture.txt"), "fixture content\n").expect("fixture");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "keep",
            "reading",
        ])
        .output()
        .expect("pevo run");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("json line"))
        .collect::<Vec<_>>();
    assert!(events.iter().all(|event| event["type"] != "error"));
    let agent_end = events
        .iter()
        .find(|event| event["type"] == "agent_end")
        .expect("agent_end");
    assert_eq!(agent_end["outcome"], "failed");
    assert_eq!(agent_end["terminal_reason"]["type"], "max_turns_exceeded");
    assert_eq!(agent_end["terminal_reason"]["max_turns"], 128);
    assert!(
        agent_end["terminal_message"]
            .as_str()
            .expect("terminal message")
            .contains("model-turn limit (128)")
    );
    assert_eq!(server.requests.lock().expect("requests").len(), 128);
}

#[test]
fn cli_run_json_outputs_ndjson_events() {
    let server = MockSseServer::start(vec![sse_text("json final")]);
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
    assert_eq!(events.first().expect("first")["type"], "run_start");
    assert!(events.iter().any(|event| event["type"] == "agent_start"));
    assert!(events.iter().any(|event| event["type"] == "message_end"));
    let agent_end = events
        .iter()
        .position(|event| event["type"] == "agent_end")
        .expect("agent_end");
    let context_snapshots = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| (event["type"] == "context_snapshot").then_some(index))
        .collect::<Vec<_>>();
    assert!(context_snapshots.len() <= 1);
    if let Some(context_snapshot) = context_snapshots.first() {
        assert!(*context_snapshot > agent_end);
    }
    assert!(!stdout.contains("json final\njson final"));
}

#[test]
fn cli_context_reports_latest_session_json() {
    let server = MockSseServer::start(vec![sse_metadata_usage_then_text("context final")]);
    let temp = tempdir().expect("temp");
    let psychevo_home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let run = isolated_tui_cmd(temp.path(), &psychevo_home, &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "hello",
        ])
        .output()
        .expect("pevo run");
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let context = isolated_tui_cmd(temp.path(), &psychevo_home, &config, &db)
        .args([
            "context",
            "--session",
            "latest",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--json",
        ])
        .output()
        .expect("pevo context");
    assert!(
        context.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&context.stderr)
    );
    let snapshot: Value = serde_json::from_slice(&context.stdout).expect("context json");
    assert_eq!(snapshot["type"], "context_snapshot");
    assert_eq!(snapshot["scope"], "session_estimate");
    assert_eq!(snapshot["total"]["tokens"], 3);
    assert!(snapshot["categories"]["history"]["tokens"].as_u64().is_some());
    assert!(snapshot["categories"].get("input_messages").is_none());
    assert!(!String::from_utf8_lossy(&context.stdout).contains("unavailable"));

    let text_context = isolated_tui_cmd(temp.path(), &psychevo_home, &config, &db)
        .args([
            "context",
            "--session",
            "latest",
            "--dir",
            workdir.to_str().expect("workdir"),
        ])
        .output()
        .expect("pevo context text");
    assert!(
        text_context.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&text_context.stderr)
    );
    let text = String::from_utf8_lossy(&text_context.stdout);
    assert!(text.starts_with("Context Usage\n"));
    assert!(!text.contains("> /context"));
    assert!(!text.contains('└'));
    assert!(text.contains("\ntokens: 3 tokens\n"));
    assert!(text.contains("\ninput_history:"));
    assert!(!text.contains("\nmessages:"));
    assert!(text.contains("\nscope: session estimate\nmodel: mock/mock-model\n"));
    assert!(!text.contains("bar:"));
    assert!(!text.contains("provider"));
    assert!(!text.contains("unavailable"));
}

#[test]
fn cli_run_skill_marker_injects_context_and_preserves_prompt() {
    let server = MockSseServer::start(vec![sse_text("skill final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("workdir");
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

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("json line"))
        .collect::<Vec<_>>();
    assert_eq!(
        events.first().expect("run start")["selected_skills"][0]["name"],
        "reviewer"
    );

    let request = server.request_json(0);
    let contents = user_contents(&request);
    assert_eq!(contents.len(), 2);
    assert!(contents[0].contains("<skill>"));
    assert!(contents[0].contains("Follow the reviewer workflow."));
    assert_eq!(contents[1], "$reviewer do it");

    let conn = Connection::open(db).expect("db");
    let user_messages = conn
        .prepare("SELECT content_text FROM messages WHERE role = 'user' ORDER BY session_seq")
        .expect("prepare")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("rows");
    assert_eq!(user_messages, vec!["$reviewer do it"]);
}

#[test]
fn cli_run_skill_flag_injects_without_stdout_pollution() {
    let server = MockSseServer::start(vec![sse_text("flag final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("workdir");
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
            "--skill",
            "reviewer",
            "do it",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "flag final\n"
    );

    let contents = user_contents(&server.request_json(0));
    assert_eq!(contents.len(), 2);
    assert!(contents[0].contains("Follow the reviewer workflow."));
    assert_eq!(contents[1], "do it");
}

#[test]
fn cli_run_unknown_skill_marker_remains_plain_text() {
    let server = MockSseServer::start(vec![sse_text("plain final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("workdir");
    write_home_skill(temp.path(), "reviewer", "review code", "review body");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "$missing do it",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let first = serde_json::from_str::<Value>(stdout.lines().next().expect("run start"))
        .expect("run start json");
    assert_eq!(first["selected_skills"], serde_json::json!([]));
    assert_eq!(user_contents(&server.request_json(0)), vec!["$missing do it"]);
}

#[test]
fn cli_run_injects_agents_project_instructions_without_persisting_as_messages() {
    let server = MockSseServer::start(vec![sse_text("agents final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("git");
    std::fs::create_dir_all(workdir.join(".psychevo")).expect("psychevo");
    std::fs::write(workdir.join("AGENTS.md"), "Use root workflow.").expect("agents");
    std::fs::write(workdir.join(".psychevo/AGENTS.md"), "Use pevo workflow.")
        .expect("psychevo agents");
    std::fs::write(workdir.join("AGENTS.local.md"), "Use local workflow.").expect("local");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "do it",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let contents = user_contents(&server.request_json(0));
    assert_eq!(contents.len(), 2);
    assert!(contents[0].contains("# AGENTS.md instructions for"));
    assert!(contents[0].contains("Use root workflow."));
    assert!(contents[0].contains("Use pevo workflow."));
    assert!(contents[0].contains("Use local workflow."));
    assert_eq!(contents[1], "do it");

    let conn = Connection::open(db).expect("db");
    let user_messages = conn
        .prepare("SELECT content_text FROM messages WHERE role = 'user' ORDER BY session_seq")
        .expect("prepare")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("rows");
    assert_eq!(user_messages, vec!["do it"]);
    let evidence_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM context_evidence WHERE source_kind = 'project_instruction'",
            [],
            |row| row.get(0),
        )
        .expect("context evidence");
    assert_eq!(evidence_count, 3);
    let grouped_blocks: Vec<i64> = conn
        .prepare(
            "SELECT provider_block_index FROM context_evidence WHERE source_kind = 'project_instruction' AND provider_group = 'project_instructions' ORDER BY context_seq",
        )
        .expect("prepare")
        .query_map([], |row| row.get::<_, i64>(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("rows");
    assert_eq!(grouped_blocks, vec![0, 1, 2]);
}

#[test]
fn cli_run_keeps_agents_skill_and_prompt_as_separate_provider_messages() {
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

    let contents = user_contents(&server.request_json(0));
    assert_eq!(contents.len(), 3);
    assert!(contents[0].contains("# AGENTS.md instructions for"));
    assert!(contents[0].contains("Use project workflow."));
    assert!(contents[1].contains("<skill>"));
    assert!(contents[1].contains("Follow the reviewer workflow."));
    assert_eq!(contents[2], "$reviewer do it");

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
        vec![
            (
                "project_instruction".to_string(),
                "project_instructions".to_string()
            ),
            (
                "selected_skill".to_string(),
                "selected_skill:0:reviewer".to_string()
            ),
        ]
    );
}

#[test]
fn cli_run_warns_for_claude_memory_without_loading_it() {
    let server = MockSseServer::start(vec![sse_text("claude final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("git");
    std::fs::write(workdir.join("CLAUDE.md"), "Do not load this Claude memory.")
        .expect("claude");
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
        .find(|event| event["type"] == "warning")
        .expect("warning event");
    assert_eq!(warning["kind"], "project_instruction");
    assert!(
        warning["message"]
            .as_str()
            .expect("message")
            .contains("Psychevo only loads AGENTS-named")
    );
    assert_eq!(warning["suggestion"], "ln -s CLAUDE.md AGENTS.md");
    assert!(!user_contents(&server.request_json(0))[0].contains("Do not load this Claude memory."));
}

#[test]
fn cli_run_default_writes_claude_warning_to_stderr_only() {
    let server = MockSseServer::start(vec![sse_text("default final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("git");
    std::fs::write(workdir.join("CLAUDE.local.md"), "local claude").expect("claude local");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "hello",
        ])
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
fn cli_run_json_hides_reasoning_by_default_and_debug_flag_emits_it() {
    let server = MockSseServer::start(vec![
        sse_reasoning_then_text("private chain", "visible final"),
        sse_reasoning_then_text("debug chain", "debug final"),
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
    assert!(
        events
            .iter()
            .any(|event| { event["type"] == "reasoning_delta" && event["text"] == "debug chain" })
    );
    assert!(
        events
            .iter()
            .any(|event| { event["type"] == "reasoning_end" && event["text"] == "debug chain" })
    );
    assert!(shown_stdout.contains("debug final"));
}

#[test]
fn cli_run_include_reasoning_requires_json_format() {
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
fn cli_run_json_omits_metadata_only_message_updates() {
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
fn cli_run_reads_stdin_and_appends_to_positional_prompt() {
    let server = MockSseServer::start(vec![sse_text("stdin final"), sse_text("append final")]);
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
        user_contents(&server.request_json(1)),
        vec!["fix this\ndetails\n"]
    );
}

#[test]
fn cli_run_empty_prompt_rejects_before_session_creation() {
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
fn cli_run_errors_use_selected_output_format() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config_dir = temp.path().join("config");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    let config = config_dir.join("config.jsonc");
    std::fs::write(
        &config,
        r#"{
          "model": "custom/local",
          "provider": {
            "custom": {
              "options": {
                "base_url": "https://example.invalid/v1",
                "api_key_env": "PSYCHEVO_TEST_MISSING_KEY_SHOULD_NOT_EXIST"
              },
              "models": { "local": {} }
            }
          }
        }"#,
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
fn cli_run_requires_initialized_home_without_config_db_bypass() {
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
fn cli_run_rejects_removed_flags() {
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
        &["run", "--config", "config.jsonc", "hello"],
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
fn cli_run_model_override_requires_provider_qualified_model() {
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
fn cli_run_variant_overrides_reasoning_effort_and_none_suppresses_it() {
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
fn cli_run_continue_reuses_latest_matching_run_session() {
    let server = MockSseServer::start(vec![sse_text("first"), sse_text("second")]);
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
fn cli_run_continue_ignores_smoke_sessions() {
    let server = MockSseServer::start(vec![sse_text("run")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let smoke = pevo_cmd(temp.path())
        .args([
            "smoke",
            "--db",
            db.to_str().expect("db"),
            "--workdir",
            workdir.to_str().expect("workdir"),
        ])
        .output()
        .expect("smoke");
    assert!(smoke.status.success());

    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let run = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--continue",
            "hello",
        ])
        .output()
        .expect("run");
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let conn = Connection::open(&db).expect("db");
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("sessions");
    assert_eq!(sessions, 2);
}

#[test]
fn cli_stats_reports_current_workdir_and_json() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let init = pevo_cmd(temp.path())
        .arg("init")
        .output()
        .expect("init");
    assert!(
        init.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    let smoke = pevo_cmd(temp.path())
        .args([
            "smoke",
            "--db",
            db.to_str().expect("db"),
            "--workdir",
            workdir.to_str().expect("workdir"),
        ])
        .output()
        .expect("smoke");
    assert!(smoke.status.success());

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
