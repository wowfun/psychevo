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
        transcript_line_count(&ui.transcript, 80, ui.thinking_visible, &ui.cwd),
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
    assert!(transcript_line_count(&ui.transcript, 80, ui.thinking_visible, &ui.cwd) > 0);
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
        cwd: app.cwd.clone(),
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
    tx.send(RunStreamEvent::value(serde_json::json!({
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
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "tool_execution_start",
        "tool_call_id": "call_read_fixture",
        "tool_name": "read",
        "args": {"path": "fixture.txt"}
    })))
    .expect("send start");
    tx.send(RunStreamEvent::value(serde_json::json!({
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
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
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
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "run_start",
        "session_id": "streamed-session",
        "provider": "xiaomi-token-plan",
        "model": "mimo-v2.5-pro",
        "mode": "default"
    })))
    .expect("send run start");
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "tool_execution_end",
        "tool_call_id": "call_sqlite",
        "tool_name": "exec_command",
        "args": {"cmd": "sqlite3 feeds.db"},
        "result": {"output": "[]", "exit_code": 1},
        "outcome": "failed"
    })))
    .expect("send tool end");
    tx.send(RunStreamEvent::value(serde_json::json!({
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
            cwd: temp.path().to_path_buf(),
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
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
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

    tx.send(RunStreamEvent::value(serde_json::json!({
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
    tx.send(RunStreamEvent::value(serde_json::json!({
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
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "tool_execution_start",
        "tool_call_id": "call_write_report",
        "tool_name": "write",
        "args": {
            "path": "/tmp/hackernews-hot-05-39.md",
            "content": "report body"
        }
    })))
    .expect("send start");
    tx.send(RunStreamEvent::value(serde_json::json!({
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
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
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
    assert!(
        ui.transcript
            .iter()
            .all(|row| !(row.title == "write" && row.tool_call_id.is_none())),
        "{:?}",
        ui.transcript
    );
    let updating = ui
        .transcript
        .iter()
        .position(|row| row.title == "write /tmp/hackernews-hot-05-39.md")
        .expect("typed write row");
    assert!(thinking < updating);
    assert!(ui.transcript[updating].tool_started.is_some());
    assert_eq!(
        ui.transcript[updating].tool_call_id.as_deref(),
        Some("call_write_report")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
    assert!(ui.running.is_some());
    assert_eq!(ui.deferred_stream_events.len(), 2);

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
