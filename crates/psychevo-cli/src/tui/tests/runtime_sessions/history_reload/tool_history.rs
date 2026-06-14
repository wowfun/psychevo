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
    ui.session_usage_summary = Some(SessionUsageSummary {
        session_id: "session".to_string(),
        provider: "mock".to_string(),
        model: "mock-model".to_string(),
        message_count: 1,
        assistant_message_count: 1,
        context_input_tokens: 9,
        billable_input_tokens: 7,
        billable_output_tokens: 2,
        reasoning_tokens: 0,
        cache_read_tokens: 2,
        cache_write_tokens: 0,
        reported_total_tokens: 11,
        estimated_cost_nanodollars: 10_000_000,
        unknown_pricing_count: 0,
        cache_read_percent: Some(22.0),
    });

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");

    assert_eq!(ui.sidebar_tokens, None);
    assert_eq!(ui.sidebar_context_limit, None);
    assert_eq!(ui.last_context_snapshot, None);
    assert_eq!(ui.session_usage_summary, None);
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
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
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

#[test]
pub(crate) fn message_history_orders_reasoning_before_assistant_text() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
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
                {"type": "reasoning", "text": "think first"},
                {"type": "text", "text": "answer second"}
            ],
            "timestamp_ms": 1,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    let thinking = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    let answer = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Answer)
        .expect("answer row");
    assert!(thinking < answer);
}

#[test]
pub(crate) fn message_history_orders_assistant_preamble_before_tool_after_reload() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
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
                {"type": "reasoning", "text": "数据查询完毕。现在撰写日报。"},
                {
                    "type": "tool_call",
                    "id": "call_write",
                    "name": "write",
                    "arguments": {"path": "feeds/2026-06-03/x-hot-09-41.md", "content": "body"},
                    "arguments_json": "{\"path\":\"feeds/2026-06-03/x-hot-09-41.md\",\"content\":\"body\"}",
                    "content_index": 1,
                    "call_index": 0
                }
            ],
            "timestamp_ms": 1,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    insert_tui_message(
        &conn,
        &session_id,
        2,
        "assistant",
        2,
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "reasoning", "text": "The file was written successfully."}],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    insert_tui_message(
        &conn,
        &session_id,
        3,
        "assistant",
        3,
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "X 日报生成完毕"}],
            "timestamp_ms": 3,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    let preamble = ui
        .transcript
        .iter()
        .position(|row| {
            row.kind == TranscriptKind::Thinking
                && row.title == "Thinking"
                && row.text == "数据查询完毕。现在撰写日报。"
        })
        .expect("preamble row");
    let tool = ui
        .transcript
        .iter()
        .position(|row| row.tool_call_id.as_deref() == Some("call_write"))
        .expect("write row");
    let later_reasoning = ui
        .transcript
        .iter()
        .position(|row| row.text == "The file was written successfully.")
        .expect("later reasoning");
    let final_answer = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Answer && row.text == "X 日报生成完毕")
        .expect("final answer");
    assert!(preamble < tool, "{:?}", ui.transcript);
    assert!(tool < later_reasoning, "{:?}", ui.transcript);
    assert!(later_reasoning < final_answer, "{:?}", ui.transcript);
    assert!(
        ui.transcript.iter().all(|row| row.title != "Preamble"),
        "{:?}",
        ui.transcript
    );
}
