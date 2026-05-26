#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn fullscreen_thinking_toggle_hides_existing_blocks_without_status() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "private reasoning",
    ));

    app.handle_fullscreen_command(&mut ui, SlashCommand::ThinkingSet(false))
        .await
        .expect("thinking off");

    assert!(!ui.thinking_visible);
    assert_eq!(
        transcript_line_count(&ui.transcript, 80, ui.thinking_visible, &ui.workdir),
        0
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Status)
    );

    app.handle_fullscreen_command(&mut ui, SlashCommand::ThinkingSet(true))
        .await
        .expect("thinking on");
    assert!(ui.thinking_visible);
    assert!(transcript_line_count(&ui.transcript, 80, ui.thinking_visible, &ui.workdir) > 0);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Status)
    );
}

#[tokio::test]
pub(crate) async fn tab_completes_slash_command_without_switching_mode() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/ren");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .expect("tab");

    assert_eq!(textarea_text(&ui.textarea), "/rename");
    assert_eq!(app.current_mode, RunMode::Default);
}

#[tokio::test]
pub(crate) async fn shift_tab_cycles_mode_without_status_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
    )
    .await
    .expect("shift tab");

    assert_eq!(app.current_mode, RunMode::Plan);
    assert_eq!(app.current_permission_mode, PermissionMode::Default);
    assert!(
        !ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Status && row.text.contains("mode:"))
    );
}

pub(crate) fn finished_run_result(app: &TuiApp) -> psychevo_runtime::RunResult {
    psychevo_runtime::RunResult {
        session_id: "finished-session".to_string(),
        outcome: Outcome::Normal,
        terminal_reason: None,
        final_answer: "done".to_string(),
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
        events: Vec::new(),
        warnings: Vec::new(),
    }
}

#[tokio::test]
pub(crate) async fn fullscreen_drain_keeps_queued_events_after_task_completion() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "message_end",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": "final answer"}],
            "timestamp_ms": 1,
            "finish_reason": "stop",
            "outcome": "normal"
        }
    })))
    .expect("send answer");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "tool_execution_start",
        "tool_call_id": "call_read_fixture",
        "tool_name": "read",
        "args": {"path": "fixture.txt"}
    })))
    .expect("send start");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "tool_execution_end",
        "tool_call_id": "call_read_fixture",
        "tool_name": "read",
        "args": {"path": "fixture.txt"},
        "result": {"path": "fixture.txt", "content": "fixture content"},
        "outcome": "normal"
    })))
    .expect("send end");
    drop(tx);

    let result = finished_run_result(&app);
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

    let active_tool_row = ui
        .transcript
        .iter()
        .find(|row| row.title == "read fixture.txt")
        .expect("active tool evidence row");
    assert!(active_tool_row.tool_started.is_some());
    assert!(ui.running.is_some());
    assert_eq!(ui.deferred_stream_events.len(), 1);

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("second drain");

    let tool_row = ui
        .transcript
        .iter()
        .find(|row| row.title == "read fixture.txt")
        .expect("tool evidence row");
    assert_eq!(tool_row.kind, TranscriptKind::Explored);
    assert_eq!(tool_row.text, "fixture content");
    let tool_index = ui
        .transcript
        .iter()
        .position(|row| row.title == "read fixture.txt")
        .expect("tool index");
    let answer_index = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Answer)
        .expect("answer index");
    assert!(answer_index < tool_index);
    assert!(ui.running.is_none());
}

#[tokio::test]
pub(crate) async fn final_message_defers_turn_meta_while_foreground_task_is_running() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "run_start",
        "session_id": "streamed-session",
        "provider": "xiaomi-token-plan",
        "model": "mimo-v2.5-pro",
        "mode": "default"
    })))
    .expect("send run start");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "tool_execution_end",
        "tool_call_id": "call_sqlite",
        "tool_name": "exec_command",
        "args": {"cmd": "sqlite3 feeds.db"},
        "result": {"output": "[]", "exit_code": 1},
        "outcome": "failed"
    })))
    .expect("send tool end");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "message_end",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": "I can continue with the remaining data."}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro"
        },
        "metadata": {"elapsed_ms": 2_000}
    })))
    .expect("send answer");

    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = done_rx.await;
        Ok(psychevo_runtime::RunResult {
            session_id: "streamed-session".to_string(),
            outcome: Outcome::Normal,
            terminal_reason: None,
            final_answer: "I can continue with the remaining data.".to_string(),
            db_path: temp.path().join("state.db"),
            workdir: temp.path().to_path_buf(),
            provider: "xiaomi-token-plan".to_string(),
            model: "mimo-v2.5-pro".to_string(),
            base_url: "http://127.0.0.1".to_string(),
            api_key_env: None,
            reasoning_effort: None,
            context_limit: None,
            tool_failures: 1,
            selected_agent: None,
            selected_skills: Vec::new(),
            context_snapshot: None,
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

    assert!(ui.running.is_some());
    assert!(
        ui.status_running_elapsed(app.current_session.as_deref())
            .is_some()
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Answer && row.text == "I can continue with the remaining data."
    }));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );

    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "agent_end",
        "outcome": "normal",
        "messages": []
    })))
    .expect("send agent end");
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain agent end");

    assert!(ui.running.is_none());
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Meta
            && row.text.contains("xiaomi-token-plan/mimo-v2.5-pro")
            && row.text.contains("1 failure")
    }));
    let _ = done_tx.send(());
}

#[tokio::test]
pub(crate) async fn fast_reasoning_only_write_renders_updating_before_completion() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(RunStreamEvent::ReasoningDelta {
        text: "Let me compose the full report now. I have all the data. Let me write it out."
            .to_string(),
    })
    .expect("send reasoning");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "message_end",
        "message": {
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_write_report",
                "name": "write",
                "arguments": {
                    "path": "/tmp/hackernews-hot-05-39.md",
                    "content": "report body"
                },
                "arguments_json": "{\"path\":\"/tmp/hackernews-hot-05-39.md\",\"content\":\"report body\"}",
                "arguments_error": null,
                "content_index": 0,
                "call_index": 0
            }],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mimo-v2.5-pro",
            "provider": "xiaomi-token-plan"
        },
        "metadata": {
            "elapsed_ms": 190_546,
            "reasoning_effort": "low"
        }
    })))
    .expect("send message end");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "tool_execution_start",
        "tool_call_id": "call_write_report",
        "tool_name": "write",
        "args": {
            "path": "/tmp/hackernews-hot-05-39.md",
            "content": "report body"
        }
    })))
    .expect("send start");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "tool_execution_end",
        "tool_call_id": "call_write_report",
        "tool_name": "write",
        "result": {
            "path": "feeds/2026-05-10/hackernews-hot-05-39.md",
            "bytes_written": 24968,
            "error": null
        },
        "outcome": "normal",
        "elapsed_ms": 0
    })))
    .expect("send end");
    drop(tx);

    let result = finished_run_result(&app);
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

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("first drain");
    let thinking = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    let updating = ui
        .transcript
        .iter()
        .position(|row| row.title == "write")
        .expect("provisional updating row");
    assert!(thinking < updating);
    assert!(ui.transcript[updating].tool_started.is_some());
    assert!(ui.transcript[updating].tool_call_id.is_none());
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
    assert!(ui.running.is_some());
    assert_eq!(ui.deferred_stream_events.len(), 3);

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("second drain");
    assert!(ui.running.is_some());
    assert_eq!(ui.deferred_stream_events.len(), 1);
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.title == "write /tmp/hackernews-hot-05-39.md")
    );

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("third drain");
    assert!(ui.running.is_none());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.title == "write feeds/2026-05-10/hackernews-hot-05-39.md")
    );
}

#[tokio::test]
pub(crate) async fn pending_write_tool_input_defers_later_completion_events() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "message_update",
        "message": {
            "role": "assistant",
            "content": [{
                "type": "text",
                "text": "Now I have all the data needed. Let me write the complete report."
            }],
            "timestamp_ms": 2,
            "outcome": "normal"
        }
    })))
    .expect("send text");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "tool_call_pending",
        "tool_call_id": "call_write_report",
        "tool_name": "write",
        "arguments_json": "",
        "content_index": 1,
        "call_index": 0
    })))
    .expect("send pending");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "tool_execution_start",
        "tool_call_id": "call_write_report",
        "tool_name": "write",
        "args": {
            "path": "/tmp/hackernews-hot-05-39.md",
            "content": "report body"
        }
    })))
    .expect("send start");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "tool_execution_end",
        "tool_call_id": "call_write_report",
        "tool_name": "write",
        "result": {
            "path": "feeds/2026-05-10/hackernews-hot-05-39.md",
            "bytes_written": 24968,
            "error": null
        },
        "outcome": "normal",
        "elapsed_ms": 0
    })))
    .expect("send end");
    drop(tx);

    let result = finished_run_result(&app);
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

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("first drain");
    assert!(ui.transcript.iter().any(|row| row.title == "write"));
    assert_eq!(ui.deferred_stream_events.len(), 3);
    assert!(ui.running.is_some());

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("second drain");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.title == "write /tmp/hackernews-hot-05-39.md")
    );
    assert_eq!(ui.deferred_stream_events.len(), 1);

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("third drain");
    assert!(ui.running.is_none());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.title == "write feeds/2026-05-10/hackernews-hot-05-39.md")
    );
}

#[test]
pub(crate) fn multi_message_turn_preserves_answer_rows_across_tool_cycles() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
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
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "First visible answer before tools."}],
                "timestamp_ms": 1,
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_read_fixture",
            "tool_name": "read",
            "args": {"path": "fixture.txt"}
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_read_fixture",
            "tool_name": "read",
            "args": {"path": "fixture.txt"},
            "result": {"path": "fixture.txt", "content": "fixture content"},
            "outcome": "normal"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Second visible answer after tools."}],
                "timestamp_ms": 2,
                "finish_reason": "stop",
                "outcome": "normal"
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Second visible answer after tools."}],
                "timestamp_ms": 2,
                "finish_reason": "stop",
                "outcome": "normal"
            }
        }),
        false,
    );

    let answers = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Answer)
        .map(|row| row.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        answers,
        vec![
            "First visible answer before tools.",
            "Second visible answer after tools."
        ]
    );
    let first_answer = ui
        .transcript
        .iter()
        .position(|row| row.text == "First visible answer before tools.")
        .expect("first answer");
    let tool = ui
        .transcript
        .iter()
        .position(|row| row.title == "read fixture.txt")
        .expect("tool row");
    let second_answer = ui
        .transcript
        .iter()
        .position(|row| row.text == "Second visible answer after tools.")
        .expect("second answer");
    assert!(first_answer < tool);
    assert!(tool < second_answer);
}

#[tokio::test]
pub(crate) async fn fullscreen_agent_end_releases_turn_before_auxiliary_task_finishes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "run_start",
        "session_id": "streamed-session",
        "provider": "mock",
        "model": "mock-model",
        "mode": "default"
    })))
    .expect("send run start");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "message_end",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": "hi"}],
            "timestamp_ms": 1,
            "finish_reason": "stop",
            "outcome": "normal",
            "provider": "mock",
            "model": "mock-model"
        }
    })))
    .expect("send answer");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "agent_end",
        "outcome": "normal",
        "messages": []
    })))
    .expect("send agent end");

    let result = psychevo_runtime::RunResult {
        session_id: "streamed-session".to_string(),
        outcome: Outcome::Normal,
        terminal_reason: None,
        final_answer: "hi".to_string(),
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
        events: Vec::new(),
        warnings: Vec::new(),
    };
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = done_rx.await;
        Ok(result)
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
    assert_eq!(app.current_session.as_deref(), Some("streamed-session"));
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "hi")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "a turn is already running")
    );
    ui.running_elapsed_override = Some(Duration::from_secs(15));
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 12);
    let text = buffer_text(&buffer);
    assert!(!text.contains("15s · Esc"), "{text}");
    assert!(!text.contains("Esc"), "{text}");
    let _ = done_tx.send(());
}

#[tokio::test]
pub(crate) async fn visible_live_auxiliary_turn_defers_terminal_message_meta() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let session_id = "visible-live-session".to_string();
    app.current_session = Some(session_id.clone());
    let mut ui = FullscreenUi::new(&app);
    attach_background_agent_running(&mut ui, &session_id);
    ui.running_elapsed_override = Some(Duration::from_secs(11));

    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "session_id": session_id,
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2-omni",
            "mode": "default"
        }),
        false,
    );
    assert!(
        ui.status_running_elapsed(app.current_session.as_deref())
            .is_some()
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "final answer has arrived"}],
                "timestamp_ms": 2,
                "finish_reason": "stop",
                "outcome": "normal",
                "model": "mimo-v2-omni",
                "provider": "xiaomi-token-plan"
            },
            "metadata": {
                "elapsed_ms": 171_000,
                "reasoning_effort": "high"
            }
        }),
        false,
    );

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "final answer has arrived")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );

    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn live_session_history_reload_defers_latest_terminal_meta() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.workdir,
            "tui",
            "mimo-v2-omni",
            "xiaomi-token-plan",
            None,
        )
        .expect("session");
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        1,
        "user",
        "prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "prompt"}],
            "timestamp_ms": 1
        }),
        None,
    );
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        2,
        "assistant",
        "final answer has arrived",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "final answer has arrived"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mimo-v2-omni",
            "provider": "xiaomi-token-plan"
        }),
        Some(serde_json::json!({
            "elapsed_ms": 171_000,
            "reasoning_effort": "high"
        })),
    );
    app.current_session = Some(session_id.clone());
    let mut ui = FullscreenUi::new(&app);
    attach_background_agent_running(&mut ui, &session_id);

    app.load_current_session_history(&mut ui)
        .expect("load history");

    assert!(
        ui.status_running_elapsed(app.current_session.as_deref())
            .is_some()
    );
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "final answer has arrived")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );

    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}
