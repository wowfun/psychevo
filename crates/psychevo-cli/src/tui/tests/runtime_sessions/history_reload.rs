#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn fullscreen_refreshes_title_after_detached_agent_task_finishes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let session_id = SqliteStore::open(&app.db_path)
        .expect("store")
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("session");
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "run_start",
        "session_id": session_id.clone(),
        "provider": "mock",
        "model": "mock-model",
        "mode": "default"
    })))
    .expect("send run start");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "agent_end",
        "outcome": "normal",
        "messages": []
    })))
    .expect("send agent end");

    let db_path = app.db_path.clone();
    let workdir = app.workdir.clone();
    let task_session_id = session_id.clone();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = done_rx.await;
        SqliteStore::open(&db_path)?.set_session_title(&task_session_id, "X Daily")?;
        Ok(psychevo_runtime::RunResult {
            session_id: task_session_id,
            outcome: Outcome::Normal,
            terminal_reason: None,
            final_answer: "done".to_string(),
            db_path,
            workdir,
            provider: "mock".to_string(),
            model: "mock-model".to_string(),
            base_url: "http://127.0.0.1".to_string(),
            api_key_env: None,
            reasoning_effort: None,
            context_limit: None,
            tool_failures: 0,
            selected_agent: None,
            selected_skills: Vec::new(),
            context_snapshot: None,
            capability_snapshot: None,
            events: Vec::new(),
            warnings: Vec::new(),
        })
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        rx,
        task: RunningTask::Agent(task),
    });

    app.drain_fullscreen_events(&mut ui).await.expect("drain");
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);
    assert_eq!(app.current_session_title, None);

    let _ = done_tx.send(());
    while !ui
        .auxiliary_agent_tasks
        .iter()
        .all(|agent| agent.task.is_finished())
    {
        tokio::task::yield_now().await;
    }
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain auxiliary");

    assert_eq!(app.current_session_title.as_deref(), Some("X Daily"));
    assert_eq!(ui.sidebar.title, "X Daily");
    assert!(ui.auxiliary_agent_tasks.is_empty());
}

#[tokio::test]
pub(crate) async fn interrupted_turn_restores_queued_inputs_to_composer_without_autostart() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: "aborted-session".to_string(),
        outcome: Outcome::Aborted,
        terminal_reason: None,
        final_answer: String::new(),
        db_path: app.db_path.clone(),
        workdir: app.workdir.clone(),
        provider: "mock".to_string(),
        model: "mock-model".to_string(),
        base_url: "http://127.0.0.1".to_string(),
        api_key_env: Some("TEST_PROVIDER_KEY".to_string()),
        reasoning_effort: None,
        context_limit: None,
        tool_failures: 0,
        selected_agent: None,
        selected_skills: Vec::new(),
        context_snapshot: None,
        capability_snapshot: None,
        events: Vec::new(),
        warnings: Vec::new(),
    };
    let task = tokio::spawn(async move { Ok(result) });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        rx,
        task: RunningTask::Agent(task),
    });
    ui.start_assistant();
    ui.turn_outcome = Some(Outcome::Aborted);
    ui.interrupt_requested = true;
    let prompt_sequence = ui.next_pending_input_sequence();
    ui.queued_inputs.push_back(QueuedInput::Prompt {
        session_id: app.current_session.clone(),
        prompt: "queued prompt".to_string(),
        display_prompt: "[Image #1] queued prompt".to_string(),
        images: vec![PendingImageAttachment {
            placeholder: "[Image #1]".to_string(),
            image: ImageInput::ImageUrl("https://example.test/image.png".to_string()),
        }],
        sequence: prompt_sequence,
    });
    let shell_sequence = ui.next_pending_input_sequence();
    ui.queued_inputs.push_back(QueuedInput::Shell {
        session_id: app.current_session.clone(),
        command: "printf queued-shell".to_string(),
        sequence: shell_sequence,
    });
    ui.textarea = textarea_with_text("draft");

    app.finish_streamed_agent_turn(&mut ui);

    assert!(ui.running.is_none());
    assert!(ui.queued_inputs.is_empty());
    assert_eq!(
        textarea_text(&ui.textarea),
        "[Image #1] queued prompt\n!printf queued-shell\ndraft"
    );
    assert_eq!(
        ui.pending_images,
        vec![PendingImageAttachment {
            placeholder: "[Image #1]".to_string(),
            image: ImageInput::ImageUrl("https://example.test/image.png".to_string()),
        }]
    );
    assert!(!app.had_error);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "turn ended: aborted")
    );
}

#[test]
pub(crate) fn normal_turn_with_tool_failure_does_not_add_contradictory_error_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "mock",
            "model": "mock-model",
            "mode": "default"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_name": "exec_command",
            "tool_call_id": "call_1",
            "outcome": "failed",
            "result": { "error": "network failed" }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_end",
            "outcome": "normal",
            "messages": []
        }),
        false,
    );

    app.finish_streamed_agent_turn(&mut ui);

    assert!(app.compaction_task.is_none());
    assert!(!app.had_error);
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Meta && row.text.contains("1 failure"))
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| !(row.kind == TranscriptKind::Error
                && row.text.contains("turn ended: normal")))
    );
}

#[test]
pub(crate) fn streamed_budget_exhaustion_renders_specific_error_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_end",
            "outcome": "failed",
            "messages": [],
            "terminal_reason": {
                "type": "max_turns_exceeded",
                "max_turns": 128
            },
            "terminal_message": "reached model-turn limit (128) before final answer; resume this session to continue."
        }),
        false,
    );

    app.finish_streamed_agent_turn(&mut ui);

    assert!(app.compaction_task.is_none());
    assert!(app.had_error);
    assert!(ui.transcript.iter().any(
        |row| row.kind == TranscriptKind::Error && row.text.contains("model-turn limit (128)")
    ));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "turn ended: failed")
    );
}

#[tokio::test]
pub(crate) async fn completed_normal_task_with_tool_failures_does_not_mark_tui_error() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: "normal-with-tool-failure".to_string(),
        outcome: Outcome::Normal,
        terminal_reason: None,
        final_answer: "handled failure".to_string(),
        db_path: app.db_path.clone(),
        workdir: app.workdir.clone(),
        provider: "mock".to_string(),
        model: "mock-model".to_string(),
        base_url: "http://127.0.0.1".to_string(),
        api_key_env: Some("TEST_PROVIDER_KEY".to_string()),
        reasoning_effort: None,
        context_limit: None,
        tool_failures: 1,
        selected_agent: None,
        selected_skills: Vec::new(),
        context_snapshot: None,
        capability_snapshot: None,
        events: Vec::new(),
        warnings: Vec::new(),
    };
    let task = tokio::spawn(async move { Ok(result) });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        rx,
        task: RunningTask::Agent(task),
    });

    app.drain_fullscreen_events(&mut ui).await.expect("drain");

    assert!(!app.had_error);
    assert!(
        ui.transcript
            .iter()
            .all(|row| !(row.kind == TranscriptKind::Error
                && row.text.contains("turn ended: normal")))
    );
}

#[tokio::test]
pub(crate) async fn completed_budget_exhaustion_renders_specific_error_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: "budget-exhausted".to_string(),
        outcome: Outcome::Failed,
        terminal_reason: Some(psychevo_runtime::TerminalReason::MaxTurnsExceeded {
            max_turns: 128,
        }),
        final_answer: String::new(),
        db_path: app.db_path.clone(),
        workdir: app.workdir.clone(),
        provider: "mock".to_string(),
        model: "mock-model".to_string(),
        base_url: "http://127.0.0.1".to_string(),
        api_key_env: Some("TEST_PROVIDER_KEY".to_string()),
        reasoning_effort: None,
        context_limit: None,
        tool_failures: 0,
        selected_agent: None,
        selected_skills: Vec::new(),
        context_snapshot: None,
        capability_snapshot: None,
        events: Vec::new(),
        warnings: Vec::new(),
    };
    let task = tokio::spawn(async move { Ok(result) });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        rx,
        task: RunningTask::Agent(task),
    });
    while !ui.running.as_ref().expect("running").task.is_finished() {
        tokio::task::yield_now().await;
    }

    app.drain_fullscreen_events(&mut ui).await.expect("drain");

    assert!(app.had_error);
    assert!(ui.transcript.iter().any(
        |row| row.kind == TranscriptKind::Error && row.text.contains("model-turn limit (128)")
    ));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "turn ended: failed")
    );
}

#[test]
pub(crate) fn fullscreen_loads_current_session_history() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.workdir,
            "tui",
            "mock-model",
            "mock",
            Some(serde_json::json!({"context_limit": 64_000})),
        )
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    conn.execute(
        r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json, content_text
            ) VALUES (?1, 1, 'user', 1000, ?2, 'hello')
            "#,
        rusqlite::params![
            &session_id,
            serde_json::json!({
                "role": "user",
                "content": [{"text": "hello"}],
                "timestamp_ms": 1000
            })
            .to_string()
        ],
    )
    .expect("insert user");
    conn.execute(
        r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json, content_text,
                usage_json, metadata_json
            ) VALUES (?1, 2, 'assistant', 2500, ?2, 'hi', ?3, ?4)
            "#,
        rusqlite::params![
            &session_id,
            serde_json::json!({
                "role": "assistant",
                "content": [
                    {
                        "type": "reasoning",
                        "text": "folded thought",
                        "provider_evidence": {
                            "reasoning_details": [{ "type": "thinking", "text": "opaque" }]
                        }
                    },
                    {"type": "text", "text": "hi"}
                ],
                "timestamp_ms": 2500,
                "finish_reason": "stop",
                "outcome": "normal",
                "model": "mock-model",
                "provider": "mock"
            })
            .to_string(),
            serde_json::json!({
                "input_tokens": 9,
                "output_tokens": 3,
                "total_tokens": 12
            })
            .to_string(),
            serde_json::json!({"provider_response_id": "resp_1"}).to_string()
        ],
    )
    .expect("insert assistant");
    insert_tui_message(
        &conn,
        &session_id,
        3,
        "user",
        3000,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "follow-up"}],
            "timestamp_ms": 3000
        }),
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    assert_eq!(ui.transcript[0].kind, TranscriptKind::Prompt);
    assert_eq!(ui.transcript[0].text, "hello");
    assert_eq!(ui.transcript[1].kind, TranscriptKind::Thinking);
    assert_eq!(ui.transcript[1].text, "folded thought");
    assert_eq!(ui.transcript[2].kind, TranscriptKind::Answer);
    assert_eq!(ui.transcript[2].text, "hi");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Meta
                && row.text.contains("1s")
                && !row.text.contains("response resp_1"))
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("tokens="))
    );
    assert_eq!(ui.sidebar_tokens, Some(9));
    assert_eq!(ui.sidebar_context_limit, Some(64_000));
    let status = bottom_status_context_for_width(&app, &ui, 80).expect("status context");
    assert_eq!(status, "9/64.0k (0.0%) · ~/work");
    assert_eq!(ui.history, ["hello", "follow-up"]);
    ui.textarea = textarea_with_text("draft");
    ui.recall_history(-1);
    assert_eq!(textarea_text(&ui.textarea), "follow-up");
    ui.recall_history(1);
    assert_eq!(textarea_text(&ui.textarea), "draft");
}

#[test]
pub(crate) fn load_history_omits_bottom_context_usage_without_context_limit() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    conn.execute(
        r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json,
                content_text, usage_json
            ) VALUES (?1, 1, 'assistant', 1000, ?2, 'hi', ?3)
            "#,
        rusqlite::params![
            &session_id,
            serde_json::json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "hi"}],
                "timestamp_ms": 1000,
                "finish_reason": "stop",
                "outcome": "normal",
                "model": "mock-model",
                "provider": "mock"
            })
            .to_string(),
            serde_json::json!({"input_tokens": 9}).to_string()
        ],
    )
    .expect("insert assistant");

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    assert_eq!(ui.sidebar_tokens, Some(9));
    assert_eq!(ui.sidebar_context_limit, None);
    let status = bottom_status_context_for_width(&app, &ui, 80).expect("status context");
    assert_eq!(status, "~/work");
}

#[tokio::test]
pub(crate) async fn fullscreen_new_command_clears_context_usage_state() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_tokens = Some(9);
    ui.sidebar_context_limit = Some(64_000);
    ui.last_context_snapshot = Some(test_context_snapshot());

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");

    assert_eq!(ui.sidebar_tokens, None);
    assert_eq!(ui.sidebar_context_limit, None);
    assert_eq!(ui.last_context_snapshot, None);
    let status = bottom_status_context_for_width(&app, &ui, 80).expect("status context");
    assert_eq!(status, "~/work");
}

#[test]
pub(crate) fn load_history_marks_orphan_tool_call_interrupted_but_merges_result() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.workdir,
            "tui",
            "mimo-v2.5-pro",
            "xiaomi-token-plan",
            None,
        )
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");

    insert_tui_message(
        &conn,
        &session_id,
        1,
        "assistant",
        1,
        serde_json::json!({
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": "NYT is behind a paywall. Based on comments, I can still summarize the Meta article. Let me now write the report."
                },
                {
                    "type": "tool_call",
                    "id": "call_write_report",
                    "name": "write",
                    "arguments": {
                        "path": "feeds/2026-05-10/hackernews-hot-06-42.md",
                        "content": "report body"
                    },
                    "arguments_json": "{\"path\":\"feeds/2026-05-10/hackernews-hot-06-42.md\",\"content\":\"report body\"}",
                    "arguments_error": null,
                    "content_index": 1,
                    "call_index": 0
                }
            ],
            "timestamp_ms": 1,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mimo-v2.5-pro",
            "provider": "xiaomi-token-plan"
        }),
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");
    let row = ui
        .transcript
        .iter()
        .find(|row| row.title == "write feeds/2026-05-10/hackernews-hot-06-42.md")
        .expect("write row");
    assert_eq!(row.text, "interrupted");
    assert!(row.interrupted);
    assert!(row.tool_started.is_none());
    assert!(ui.tool_rows.is_empty());
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );

    insert_tui_message(
        &conn,
        &session_id,
        2,
        "tool_result",
        2,
        serde_json::json!({
            "role": "tool_result",
            "tool_call_id": "call_write_report",
            "tool_name": "write",
            "content": "{\"bytes_written\":26779,\"dirs_created\":false,\"error\":null,\"path\":\"feeds/2026-05-10/hackernews-hot-06-42.md\"}",
            "is_error": false,
            "timestamp_ms": 2
        }),
    );
    ui.clear_transcript();
    app.load_current_session_history(&mut ui).expect("history");
    let rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Updated)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].title,
        "write feeds/2026-05-10/hackernews-hot-06-42.md"
    );
    assert!(rows[0].tool_started.is_none());
}

#[tokio::test]
pub(crate) async fn load_history_keeps_unfinished_tool_call_active_with_live_owner() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.workdir,
            "tui",
            "mimo-v2.5-pro",
            "xiaomi-token-plan",
            None,
        )
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &session_id,
        1,
        "assistant",
        1,
        serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_write_report",
                "name": "write",
                "arguments": {
                    "path": "feeds/2026-05-10/hackernews-hot-06-42.md",
                    "content": "report body"
                },
                "arguments_json": "{\"path\":\"feeds/2026-05-10/hackernews-hot-06-42.md\",\"content\":\"report body\"}",
                "arguments_error": null,
                "content_index": 0,
                "call_index": 0
            }],
            "timestamp_ms": 1,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mimo-v2.5-pro",
            "provider": "xiaomi-token-plan"
        }),
    );

    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: Some(session_id.clone()),
        control,
        rx,
        task: RunningTask::Agent(task),
    });

    app.load_current_session_history(&mut ui).expect("history");
    let row = ui
        .transcript
        .iter()
        .find(|row| row.title == "write feeds/2026-05-10/hackernews-hot-06-42.md")
        .expect("write row");
    assert_eq!(row.text, "preparing");
    assert!(row.tool_started.is_some());
    assert!(ui.tool_rows.contains_key(&tool_id_key("call_write_report")));

    if let Some(running) = ui.running.take()
        && let RunningTask::Agent(task) = running.task
    {
        task.abort();
        let _ = task.await;
    }
}

#[test]
pub(crate) fn load_history_does_not_rehydrate_aborted_tool_calls_as_running() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.workdir,
            "tui",
            "mimo-v2.5-pro",
            "xiaomi-token-plan",
            None,
        )
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");

    insert_tui_message(
        &conn,
        &session_id,
        1,
        "assistant",
        1,
        serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "reasoning", "text": "Let me continue fetching the remaining stories."},
                {
                    "type": "tool_call",
                    "id": "call_story",
                    "name": "exec_command",
                    "arguments": {
                        "cmd": "cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \"SELECT content FROM stories WHERE id = 48074265;\" 2>&1 | head -c 3000",
                        "timeout": 10
                    },
                    "arguments_json": "{\"cmd\":\"cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \\\"SELECT content FROM stories WHERE id = 48074265;\\\" 2>&1 | head -c 3000\",\"timeout\":10}",
                    "arguments_error": null,
                    "content_index": 1,
                    "call_index": 0
                }
            ],
            "timestamp_ms": 1,
            "finish_reason": "aborted",
            "outcome": "aborted",
            "model": "mimo-v2.5-pro",
            "provider": "xiaomi-token-plan"
        }),
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");
    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("exec_command row");
    assert!(
        row.title
            .starts_with("exec_command cd /home/kevin/Projects/feedgarden")
    );
    assert!(!row.title.starts_with("Running "));
    assert_eq!(row.text, "interrupted");
    assert!(row.interrupted);
    assert!(!row.failed);
    assert!(row.tool_started.is_none());
    assert!(ui.tool_rows.is_empty());
}

#[tokio::test]
pub(crate) async fn sessions_panel_switches_without_status_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let first = store
        .create_session_with_metadata(&app.workdir, "tui", "model-a", "mock", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&app.workdir, "tui", "model-b", "mock", None)
        .expect("second");
    app.current_session = Some(first.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &first,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "first prompt"}],
            "timestamp_ms": 1
        }),
    );
    conn.execute(
        r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json, content_text
            ) VALUES (?1, 1, 'user', 1, ?2, 'second prompt')
            "#,
        rusqlite::params![
            &second,
            serde_json::json!({
                "role": "user",
                "content": [{"text": "second prompt"}],
                "timestamp_ms": 1
            })
            .to_string()
        ],
    )
    .expect("insert second prompt");

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui)
        .expect("first history");
    assert_eq!(ui.history.as_slice(), ["first prompt"]);
    ui.push_submitted_history("/sessions".to_string());
    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    for ch in second.chars().take(8) {
        app.handle_bottom_panel_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .expect("query");
    }
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select");

    assert_eq!(app.current_session.as_deref(), Some(second.as_str()));
    assert!(ui.bottom_panel.is_none());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Prompt && row.text == "second prompt")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Status)
    );
    assert_eq!(ui.history.as_slice(), ["second prompt", "/sessions"]);
    ui.textarea = textarea_with_text("draft");
    ui.recall_history(-1);
    assert_eq!(textarea_text(&ui.textarea), "/sessions");
    ui.recall_history(-1);
    assert_eq!(textarea_text(&ui.textarea), "second prompt");
    ui.recall_history(1);
    assert_eq!(textarea_text(&ui.textarea), "/sessions");
    ui.recall_history(1);
    assert_eq!(textarea_text(&ui.textarea), "draft");
}
