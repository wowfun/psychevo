#[test]
pub(crate) fn load_history_omits_bottom_context_usage_without_context_limit() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
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
        effective_total_tokens: Some(11),
        reported_total_tokens: 11,
        total_status: "reported".to_string(),
        accounted_provider_call_count: 1,
        unaccounted_provider_call_count: 0,
        estimated_cost_nanodollars: 10_000_000,
        cost_status: "estimated".to_string(),
        estimated_pricing_count: 1,
        free_pricing_count: 0,
        included_pricing_count: 0,
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
            &app.cwd,
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
    assert_eq!(row.write_preview_phase.as_deref(), Some("cancelled"));
    assert!(row.expandable_text().contains("report body"));
    assert!(row.expandable_text().contains("interrupted"));
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
            &app.cwd,
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
    assert_eq!(row.write_preview_phase.as_deref(), Some("writing"));
    assert!(row.expandable_text().contains("report body"));
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
pub(crate) fn load_history_keeps_unfinished_tool_call_active_with_foreign_gateway_activity() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.cwd,
            "web",
            "mimo-v2.5-pro",
            "xiaomi-token-plan",
            None,
        )
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_unfinished_write_call(&conn, &session_id);
    claim_foreign_gateway_activity(&store, &conn, &session_id, wall_now_ms() + 60_000, 91_000);

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    let row = ui
        .transcript
        .iter()
        .find(|row| row.title == "write feeds/2026-05-10/hackernews-hot-06-42.md")
        .expect("write row");
    assert_eq!(row.write_preview_phase.as_deref(), Some("writing"));
    assert!(row.expandable_text().contains("report body"));
    assert!(!row.interrupted);
    assert!(row.tool_started.is_some());
    assert!(ui.tool_rows.contains_key(&tool_id_key("call_write_report")));
    assert!(ui.status_has_running(Some(&session_id)));
    assert!(
        ui.status_running_elapsed(Some(&session_id))
            .is_some_and(|elapsed| elapsed >= Duration::from_secs(90))
    );
}

#[test]
pub(crate) fn load_history_keeps_stale_foreign_gateway_activity_interrupted() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.cwd,
            "web",
            "mimo-v2.5-pro",
            "xiaomi-token-plan",
            None,
        )
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_unfinished_write_call(&conn, &session_id);
    claim_foreign_gateway_activity(&store, &conn, &session_id, wall_now_ms() - 1, 91_000);

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    let row = ui
        .transcript
        .iter()
        .find(|row| row.title == "write feeds/2026-05-10/hackernews-hot-06-42.md")
        .expect("write row");
    assert_eq!(row.write_preview_phase.as_deref(), Some("cancelled"));
    assert!(row.expandable_text().contains("report body"));
    assert!(row.expandable_text().contains("interrupted"));
    assert!(row.interrupted);
    assert!(row.tool_started.is_none());
    assert!(!ui.status_has_running(Some(&session_id)));
}

#[test]
pub(crate) fn current_foreign_gateway_activity_interrupt_routes_control_command() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.cwd,
            "web",
            "mimo-v2.5-pro",
            "xiaomi-token-plan",
            None,
        )
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_unfinished_write_call(&conn, &session_id);
    claim_foreign_gateway_activity(&store, &conn, &session_id, wall_now_ms() + 60_000, 1_000);

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    assert!(app.request_current_session_interrupt(&mut ui));
    assert!(ui.interrupt_requested);
    let commands = store
        .pending_gateway_control_commands("gateway:web:test", 10)
        .expect("pending commands");
    assert!(
        commands
            .iter()
            .any(|command| command.command_kind == "interrupt"),
        "{commands:?}"
    );
}

#[test]
pub(crate) fn load_history_replays_foreign_gateway_live_events_into_active_tool_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.cwd,
            "web",
            "mimo-v2.5-pro",
            "xiaomi-token-plan",
            None,
        )
        .expect("session");
    let unrelated_session = store
        .create_session_with_metadata(&app.cwd, "web", "mock-model", "mock", None)
        .expect("unrelated session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_unfinished_write_call(&conn, &session_id);
    claim_foreign_gateway_activity(&store, &conn, &session_id, wall_now_ms() + 60_000, 91_000);
    append_foreign_gateway_event(
        &store,
        &session_id,
        GatewayEvent::EntryUpdated {
            turn_id: "turn-web".to_string(),
            entry: gateway_tool_entry(
                &session_id,
                "turn-web",
                "call_write_report",
                TranscriptBlockStatus::Running,
                "writing report",
            ),
        },
    );
    append_foreign_gateway_event(
        &store,
        &unrelated_session,
        GatewayEvent::EntryUpdated {
            turn_id: "turn-other".to_string(),
            entry: gateway_tool_entry(
                &unrelated_session,
                "turn-other",
                "call_other",
                TranscriptBlockStatus::Running,
                "unrelated update",
            ),
        },
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    let rows = ui
        .transcript
        .iter()
        .filter(|row| row.tool_call_id.as_deref() == Some("call_write_report"))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1, "{:?}", ui.transcript);
    assert_eq!(rows[0].text, "running");
    assert!(!rows[0].interrupted);
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("unrelated update")),
        "{:?}",
        ui.transcript
    );

    append_foreign_gateway_event(
        &store,
        &session_id,
        GatewayEvent::TurnCompleted {
            thread_id: Some(session_id.clone()),
            turn_id: "turn-web".to_string(),
            turn: GatewayTurn {
                id: "turn-web".to_string(),
                thread_id: Some(session_id.clone()),
                status: GatewayTurnStatus::Completed,
                outcome: Some("normal".to_string()),
                error: None,
                started_at_ms: None,
                completed_at_ms: Some(1),
            },
            committed_entries: vec![gateway_answer_entry(&session_id, "turn-web", 2, "done")],
        },
    );
    assert!(
        app.drain_foreign_gateway_live_events(&mut ui)
            .expect("drain foreign events")
    );
    assert!(!ui.status_has_running(Some(&session_id)));
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "done"),
        "{:?}",
        ui.transcript
    );
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.tool_call_id.as_deref() == Some("call_write_report"))
            .count(),
        0,
        "{:?}",
        ui.transcript
    );
}

#[test]
pub(crate) fn load_history_drops_stale_foreign_gateway_pending_for_completed_tool() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.cwd,
            "web",
            "mimo-v2.5-pro",
            "xiaomi-token-plan",
            None,
        )
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_unfinished_write_call(&conn, &session_id);
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
    claim_foreign_gateway_activity(&store, &conn, &session_id, wall_now_ms() + 60_000, 91_000);
    append_foreign_gateway_event(
        &store,
        &session_id,
        GatewayEvent::EntryUpdated {
            turn_id: "turn-web".to_string(),
            entry: gateway_tool_entry(
                &session_id,
                "turn-web",
                "call_write_report",
                TranscriptBlockStatus::Pending,
                "",
            ),
        },
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    let rows = ui
        .transcript
        .iter()
        .filter(|row| row.tool_call_id.as_deref() == Some("call_write_report"))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1, "{:?}", ui.transcript);
    assert!(rows[0].tool_started.is_none(), "{:?}", rows[0]);
    assert!(!ui.tool_rows.contains_key(&tool_id_key("call_write_report")));
}

#[test]
pub(crate) fn load_history_does_not_rehydrate_aborted_tool_calls_as_running() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.cwd,
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
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
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
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
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

fn insert_unfinished_write_call(conn: &rusqlite::Connection, session_id: &str) {
    insert_tui_message(
        conn,
        session_id,
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
}

fn claim_foreign_gateway_activity(
    store: &SqliteStore,
    conn: &rusqlite::Connection,
    session_id: &str,
    lease_expires_at_ms: i64,
    started_elapsed_ms: i64,
) {
    store
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: "activity-web",
            thread_id: Some(session_id),
            source_key: Some("source:web:test"),
            turn_id: Some("turn-web"),
            kind: "turn",
            owner_id: "gateway:web:test",
            owner_surface: Some("web"),
            lease_expires_at_ms,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .expect("claim foreign activity");
    conn.execute(
        "UPDATE gateway_activities SET started_at_ms = ?2 WHERE activity_id = ?1",
        rusqlite::params![
            "activity-web",
            wall_now_ms().saturating_sub(started_elapsed_ms)
        ],
    )
    .expect("set activity start");
}

fn append_foreign_gateway_event(store: &SqliteStore, session_id: &str, event: GatewayEvent) {
    match &event {
        GatewayEvent::EntryStarted { turn_id, entry }
        | GatewayEvent::EntryUpdated { turn_id, entry }
        | GatewayEvent::EntryCompleted { turn_id, entry } => {
            let event_kind = match event {
                GatewayEvent::EntryStarted { .. } => "entryStarted",
                GatewayEvent::EntryUpdated { .. } => "entryUpdated",
                GatewayEvent::EntryCompleted { .. } => "entryCompleted",
                _ => unreachable!(),
            };
            store
                .upsert_gateway_live_snapshot(psychevo_runtime::GatewayLiveSnapshotInput {
                    snapshot_key: &format!("activity-web:{turn_id}:{}", entry.id),
                    activity_id: Some("activity-web"),
                    owner_id: Some("gateway:web:test"),
                    thread_id: Some(session_id),
                    turn_id: Some(turn_id),
                    event_kind,
                    event: serde_json::to_value(&event).expect("gateway event value"),
                })
                .expect("upsert gateway live snapshot");
        }
        _ => {
            let event = serde_json::to_value(event).expect("gateway event value");
            store
                .append_gateway_live_event(
                    Some("activity-web"),
                    Some("gateway:web:test"),
                    Some(session_id),
                    Some("turn-web"),
                    &event,
                )
                .expect("append gateway live event");
        }
    }
}

fn gateway_tool_entry(
    session_id: &str,
    turn_id: &str,
    tool_call_id: &str,
    status: TranscriptBlockStatus,
    text: &str,
) -> TranscriptEntry {
    TranscriptEntry {
        id: format!("entry-{tool_call_id}"),
        thread_id: session_id.to_string(),
        turn_id: Some(turn_id.to_string()),
        message_seq: None,
        role: TranscriptEntryRole::Assistant,
        status,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("live:{turn_id}:tool:{tool_call_id}"),
            kind: TranscriptBlockKind::File,
            status,
            order: 0,
            phase_ordinal: None,
            source: "runtime.stream".to_string(),
            title: Some("write feeds/2026-05-10/hackernews-hot-06-42.md".to_string()),
            body: Some(text.to_string()),
            preview: Some(text.to_string()),
            detail: Some(text.to_string()),
            artifact_ids: Vec::new(),
            metadata: Some(serde_json::json!({
                "projection": "tool",
                "tool_call_id": tool_call_id,
                "args": {
                    "path": "feeds/2026-05-10/hackernews-hot-06-42.md",
                    "content": "report body"
                }
            })),
            result: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn gateway_answer_entry(
    session_id: &str,
    turn_id: &str,
    message_seq: i64,
    text: &str,
) -> TranscriptEntry {
    TranscriptEntry {
        id: format!("answer-{message_seq}"),
        thread_id: session_id.to_string(),
        turn_id: Some(turn_id.to_string()),
        message_seq: Some(message_seq),
        role: TranscriptEntryRole::Assistant,
        status: TranscriptBlockStatus::Completed,
        source: "runtime.message".to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("answer-{message_seq}:text"),
            kind: TranscriptBlockKind::Text,
            status: TranscriptBlockStatus::Completed,
            order: 0,
            phase_ordinal: None,
            source: "runtime.message".to_string(),
            title: None,
            body: Some(text.to_string()),
            preview: Some(text.to_string()),
            detail: Some(text.to_string()),
            artifact_ids: Vec::new(),
            metadata: None,
            result: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}
