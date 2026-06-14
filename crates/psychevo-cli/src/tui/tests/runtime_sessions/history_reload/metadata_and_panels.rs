#[test]
pub(crate) fn message_history_completed_assistant_restores_turn_meta() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("session");
    app.current_session = Some(session_id.clone());
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        1,
        "assistant",
        "done",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "done"}],
            "timestamp_ms": 1,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        Some(serde_json::json!({
            "elapsed_ms": 2_000
        })),
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    let answer = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Answer && row.text == "done")
        .expect("answer row");
    let meta = ui
        .transcript
        .iter()
        .position(|row| {
            row.kind == TranscriptKind::Meta
                && row.text.contains("mock/mock-model")
                && row.text.contains("2s")
        })
        .expect("meta row");
    assert!(answer < meta);
}

#[test]
pub(crate) fn message_history_merges_write_stdin_into_exec_command_row() {
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
            "content": [{
                "type": "tool_call",
                "id": "call_exec",
                "name": "exec_command",
                "arguments": {"cmd": "printf first"},
                "arguments_json": "{\"cmd\":\"printf first\"}",
                "arguments_error": null,
                "content_index": 0,
                "call_index": 0
            }],
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
        "tool_result",
        2,
        serde_json::json!({
            "role": "tool_result",
            "tool_call_id": "call_exec",
            "tool_name": "exec_command",
            "content": "{\"session_id\":99,\"exit_code\":null,\"output\":\"first\\n\"}",
            "is_error": false,
            "timestamp_ms": 2
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
            "content": [{
                "type": "tool_call",
                "id": "call_poll",
                "name": "write_stdin",
                "arguments": {"session_id": 99, "yield_time_ms": 60000},
                "arguments_json": "{\"session_id\":99,\"yield_time_ms\":60000}",
                "arguments_error": null,
                "content_index": 0,
                "call_index": 0
            }],
            "timestamp_ms": 3,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    insert_tui_message(
        &conn,
        &session_id,
        4,
        "tool_result",
        4,
        serde_json::json!({
            "role": "tool_result",
            "tool_call_id": "call_poll",
            "tool_name": "write_stdin",
            "content": "{\"session_id\":null,\"exit_code\":0,\"output\":\"second\\n\"}",
            "is_error": false,
            "timestamp_ms": 4
        }),
    );
    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    let rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Ran)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tool_name.as_deref(), Some("exec_command"));
    assert!(rows[0].text.contains("first"));
    assert!(rows[0].text.contains("second"));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.tool_name.as_deref() != Some("write_stdin")
                && !row.title.contains("write_stdin"))
    );
    assert!(rows[0].tool_started.is_none());
}

#[test]
pub(crate) fn typed_empty_reasoning_completion_does_not_create_blank_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:reasoning",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Completed,
            None,
            None,
            None,
        ),
    );

    assert!(ui.transcript.is_empty());
}

#[test]
pub(crate) fn typed_write_stdin_completion_uses_cached_args_and_hides_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:tool:call_exec",
            TranscriptBlockKind::Shell,
            TranscriptBlockStatus::Completed,
            Some("exec_command"),
            None,
            Some(serde_json::json!({
                "projection": "tool",
                "tool_name": "exec_command",
                "tool_call_id": "call_exec",
                "args": {"cmd": "printf first"},
                "result": {"session_id": 7, "exit_code": null, "output": "first\n"},
                "outcome": "normal"
            })),
        ),
    );
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:tool:call_poll",
            TranscriptBlockKind::Shell,
            TranscriptBlockStatus::Pending,
            Some("write_stdin"),
            None,
            Some(serde_json::json!({
                "projection": "tool",
                "tool_name": "write_stdin",
                "tool_call_id": "call_poll",
                "args": {"session_id": 7, "yield_time_ms": 60000},
                "outcome": "normal"
            })),
        ),
    );
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:tool:call_poll",
            TranscriptBlockKind::Shell,
            TranscriptBlockStatus::Completed,
            Some("write_stdin"),
            None,
            Some(serde_json::json!({
                "projection": "tool",
                "tool_name": "write_stdin",
                "tool_call_id": "call_poll",
                "result": {"session_id": null, "exit_code": 0, "output": "second\n"},
                "outcome": "normal"
            })),
        ),
    );

    let rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Ran)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tool_name.as_deref(), Some("exec_command"));
    assert!(rows[0].text.contains("first"));
    assert!(rows[0].text.contains("second"));
    assert!(rows[0].tool_started.is_none());
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.tool_name.as_deref() != Some("write_stdin")
                && !row.title.contains("write_stdin"))
    );
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
