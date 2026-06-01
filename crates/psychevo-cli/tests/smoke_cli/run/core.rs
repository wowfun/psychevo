#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn cli_run_positional_prompt_outputs_final_answer_and_persists_metadata() {
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
pub(crate) fn cli_run_selected_main_agent_includes_description_and_body() {
    let server = MockSseServer::start(vec![sse_text("agent final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".git")).expect("git");
    std::fs::create_dir_all(workdir.join(".claude/agents")).expect("agents");
    std::fs::write(
        workdir.join("AGENTS.md"),
        "Do not translate this project instruction.",
    )
    .expect("agents");
    std::fs::write(
        workdir.join(".claude/agents/translate.md"),
        "---\nname: translate\ndescription: Detect the source language automatically. Translate Chinese to English; translate all other languages to Chinese.\ntools: []\nprojectInstructions: false\n---\nPreserve tone, meaning, punctuation, emoji, and inline formatting. Return only the translated text.",
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
        selected
            .contains("Translate Chinese to English; translate all other languages to Chinese."),
        "{selected}"
    );
    assert!(
        selected.contains("Instructions:\nPreserve tone, meaning, punctuation"),
        "{selected}"
    );
    assert!(!request.as_object().expect("request").contains_key("tools"));
    assert!(
        !system_messages
            .iter()
            .any(|message| message.contains("Do not translate this project instruction"))
    );
    assert!(
        system_messages
            .iter()
            .any(|message| message.contains("No callable tools are available"))
    );
    assert_eq!(user_contents(&request), vec!["shell"]);
}

#[test]
pub(crate) fn cli_run_child_agent_session_exports_prefix_and_last_request() {
    let server = MockSseServer::start(vec![
        sse_tool_agent_call(
            "call_agent",
            "translate",
            "Translate greeting\nDo not echo raw prompt detail",
        ),
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
    assert!(
        child_system
            .iter()
            .any(|message| message.contains("Child agent: translate"))
    );
    assert!(
        child_system
            .iter()
            .any(|message| message.contains("child agent. Return a concise final answer"))
    );

    let parent_continuation = server.request_json(2);
    let parent_tool_result = parent_continuation["messages"]
        .as_array()
        .expect("parent messages")
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == "call_agent")
        .expect("Agent tool result");
    let parent_tool_content = parent_tool_result["content"].as_str().expect("content");
    let parent_tool_summary: Value =
        serde_json::from_str(parent_tool_content).expect("summary json");
    assert_eq!(parent_tool_summary["agent_name"], "translate");
    assert_eq!(parent_tool_summary["task"], "Translate greeting");
    assert_eq!(parent_tool_summary["status"], "completed");
    assert_eq!(parent_tool_summary["summary"], "child translated");
    assert!(!parent_tool_content.contains("agent_id"));
    assert!(!parent_tool_content.contains("child_session_id"));
    assert!(!parent_tool_content.contains("latest_usage"));
    assert!(!parent_tool_content.contains("effective_max_spawn_depth"));
    assert!(!parent_tool_content.contains("Do not echo raw prompt detail"));

    let conn = Connection::open(&db).expect("db");
    let child_session: String = conn
        .query_row(
            "SELECT id FROM sessions WHERE source = 'agent'",
            [],
            |row| row.get(0),
        )
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
            "runtime_environment".to_string(),
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
        value["header"]["prompt_prefix"]["slots"][2]["slot"],
        "selected_child_agent"
    );
    let exported_messages = value["last_provider_request"]["body"]["messages"]
        .as_array()
        .expect("provider messages");
    assert!(exported_messages.iter().any(|message| {
        message["content"]
            .as_str()
            .is_some_and(|content| content.contains("Child agent: translate"))
    }));
    assert!(exported_messages.iter().any(|message| {
        message["content"]
            .as_str()
            .is_some_and(|content| content.contains("Current working directory:"))
    }));
    assert!(exported_messages.iter().any(|message| {
        message["content"]
            .as_str()
            .is_some_and(|content| content.contains("You are running as a child agent"))
    }));
}

#[test]
pub(crate) fn cli_run_dir_controls_tool_workdir() {
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
pub(crate) fn cli_run_allows_more_than_thirty_two_tool_turns_before_final_answer() {
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
pub(crate) fn cli_run_budget_exhaustion_reports_model_turn_limit() {
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
pub(crate) fn cli_run_json_budget_exhaustion_includes_terminal_reason() {
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
pub(crate) fn cli_run_json_outputs_ndjson_events() {
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
    assert_eq!(events.first().expect("first")["type"], "thread.started");
    assert_eq!(events.get(1).expect("second")["type"], "turn.started");
    assert!(events.iter().any(|event| event["type"] == "item.completed"));
    assert!(events.iter().any(|event| event["type"] == "turn.completed"));
    let turn_end = events
        .iter()
        .position(|event| event["type"] == "turn.completed")
        .expect("turn.completed");
    let context_snapshots = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| (event["type"] == "context_snapshot").then_some(index))
        .collect::<Vec<_>>();
    assert!(context_snapshots.len() <= 1);
    if let Some(context_snapshot) = context_snapshots.first() {
        assert!(*context_snapshot > turn_end);
    }
    assert!(!stdout.contains("json final\njson final"));
}

#[test]
pub(crate) fn cli_context_reports_latest_session_json() {
    let server = MockSseServer::start(vec![sse_metadata_usage_then_text("context final")]);
    let temp = tempdir().expect("temp");
    let psychevo_home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let run = isolated_tui_cmd(temp.path(), &psychevo_home, &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir"), "hello"])
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
    assert!(
        snapshot["categories"]["history"]["tokens"]
            .as_u64()
            .is_some()
    );
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
pub(crate) fn cli_run_skill_marker_injects_context_and_preserves_prompt() {
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
pub(crate) fn cli_run_skill_flag_injects_without_stdout_pollution() {
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
pub(crate) fn cli_run_unknown_skill_marker_remains_plain_text() {
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
    assert_eq!(
        user_contents(&server.request_json(0)),
        vec!["$missing do it"]
    );
}

#[test]
pub(crate) fn cli_run_injects_agents_project_instructions_without_persisting_as_messages() {
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

    let request = server.request_json(0);
    let contents = user_contents(&request);
    assert_eq!(contents, vec!["do it"]);
    let system_messages = system_contents(&request);
    assert!(
        system_messages
            .iter()
            .any(|message| message.contains("# AGENTS.md instructions for")
                && message.contains("Use root workflow."))
    );
    assert!(
        system_messages
            .iter()
            .any(|message| message.contains("Use pevo workflow."))
    );
    assert!(
        system_messages
            .iter()
            .any(|message| message.contains("Use local workflow."))
    );

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
    let grouped_blocks: Vec<(String, String, i64)> = conn
        .prepare(
            "SELECT role, provider_group, provider_block_index FROM context_evidence WHERE source_kind = 'project_instruction' ORDER BY context_seq",
        )
        .expect("prepare")
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("rows");
    assert_eq!(
        grouped_blocks,
        vec![
            (
                "system".to_string(),
                "prefix_prompt_instructions".to_string(),
                3
            ),
            (
                "system".to_string(),
                "prefix_prompt_instructions".to_string(),
                4
            ),
            (
                "system".to_string(),
                "prefix_prompt_instructions".to_string(),
                5
            ),
        ]
    );
}

#[test]
pub(crate) fn cli_run_project_context_cwd_ignores_repo_root_agents() {
    let server = MockSseServer::start(vec![sse_text("cwd final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let repo = temp.path().join("repo");
    let workdir = repo.join("task");
    std::fs::create_dir_all(workdir.join(".psychevo")).expect("dirs");
    std::fs::create_dir(repo.join(".git")).expect("git");
    std::fs::write(repo.join("AGENTS.md"), "Use repo-root workflow.").expect("root agents");
    std::fs::write(workdir.join("AGENTS.md"), "Use task workflow.").expect("task agents");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--project-context",
            "cwd",
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

    let system_messages = system_contents(&server.request_json(0));
    assert!(
        system_messages
            .iter()
            .any(|message| message.contains("Current working directory:")
                && message.contains(workdir.to_str().expect("workdir utf8")))
    );
    assert!(
        system_messages
            .iter()
            .any(|message| message.contains("Use task workflow."))
    );
    assert!(
        !system_messages
            .iter()
            .any(|message| message.contains("Use repo-root workflow."))
    );
}
