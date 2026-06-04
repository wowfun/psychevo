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
    assert!(ui.transcript.iter().any(|row| row.title == "write"));
    assert_eq!(ui.deferred_stream_events.len(), 2);
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

#[tokio::test]
pub(crate) async fn typed_gateway_final_answer_restores_turn_meta_after_task_completion() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("typed-session".to_string());
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(GatewayEvent::TurnStarted {
        thread_id: Some("typed-session".to_string()),
        turn_id: "turn-1".to_string(),
        selected_skills: Vec::new(),
    })
    .expect("send turn start");
    tx.send(GatewayEvent::EntryCompleted {
        turn_id: "turn-1".to_string(),
        entry: TranscriptEntry {
            id: "live:turn-1:assistant".to_string(),
            thread_id: "typed-session".to_string(),
            turn_id: Some("turn-1".to_string()),
            message_seq: None,
            role: TranscriptEntryRole::Assistant,
            status: TranscriptBlockStatus::Completed,
            source: "runtime.stream".to_string(),
            blocks: vec![TranscriptBlock {
                id: "live:turn-1:assistant:text".to_string(),
                kind: TranscriptBlockKind::Text,
                status: TranscriptBlockStatus::Completed,
                order: 0,
                source: "runtime.stream".to_string(),
                title: None,
                body: Some("All done.".to_string()),
                preview: Some("All done.".to_string()),
                detail: Some("All done.".to_string()),
                artifact_ids: Vec::new(),
                metadata: Some(serde_json::json!({
                    "provider": "mock",
                    "model": "mock-model",
                    "finish_reason": "stop",
                    "outcome": "normal",
                    "metadata": {"elapsed_ms": 2_000},
                    "usage": {"input_tokens": 12},
                    "accounting": {"estimated_cost_nanodollars": 10}
                })),
                result: None,
                created_at_ms: 1,
                updated_at_ms: 2,
            }],
            metadata: None,
            usage: None,
            accounting: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        },
    })
    .expect("send answer");
    tx.send(GatewayEvent::TurnCompleted {
        thread_id: Some("typed-session".to_string()),
        turn_id: "turn-1".to_string(),
        outcome: Some("normal".to_string()),
        committed_entries: Vec::new(),
    })
    .expect("send turn complete");
    drop(tx);

    let task = tokio::spawn(async move {
        Ok(psychevo_runtime::RunResult {
            session_id: "typed-session".to_string(),
            outcome: Outcome::Normal,
            terminal_reason: None,
            final_answer: "All done.".to_string(),
            db_path: temp.path().join("state.db"),
            workdir: temp.path().to_path_buf(),
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
        events: RunningTurnEvents::Gateway(rx),
        task: RunningTask::Agent(task),
    });
    while !ui.running.as_ref().expect("running").task.is_finished() {
        tokio::task::yield_now().await;
    }

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain typed gateway turn");

    assert!(ui.running.is_none());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "All done.")
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Meta
            && row.text.contains("mock/mock-model")
            && row.text.contains("2s")
    }));
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
    assert_eq!(answers, vec!["Second visible answer after tools."]);
    let first_preamble = ui
        .transcript
        .iter()
        .position(|row| {
            row.kind == TranscriptKind::Thinking
                && row.title == "Thinking"
                && row.text == "First visible answer before tools."
        })
        .expect("first preamble");
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
    assert!(first_preamble < tool);
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
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
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

#[test]
pub(crate) fn typed_gateway_preamble_completion_updates_existing_answer_row_by_item_id() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:assistant:0",
            TranscriptBlockKind::Text,
            TranscriptBlockStatus::Running,
            None,
            "好的，开始执行 X 日报流程。先运行 fetch.py 抓取数据。",
        ),
    );
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:assistant:0",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Completed,
            Some("Preamble"),
            "好的，开始执行 X 日报流程。先运行 fetch.py 抓取数据。",
        ),
    );

    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Answer)
            .count(),
        0,
        "{:?}",
        ui.transcript
    );
    let thinking_rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Thinking)
        .collect::<Vec<_>>();
    assert_eq!(thinking_rows.len(), 1, "{:?}", ui.transcript);
    assert_eq!(thinking_rows[0].title, "Thinking");
    assert_eq!(
        thinking_rows[0].text,
        "好的，开始执行 X 日报流程。先运行 fetch.py 抓取数据。"
    );
}

#[test]
pub(crate) fn typed_gateway_reasoning_update_uses_middle_fold_preview() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    let long = numbered_lines(1, 12);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:reasoning:0",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Running,
            Some("Thinking"),
            &long,
        ),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    assert!(!row.expanded);
    assert!(!row.details_collapsed);
    assert_eq!(row.full_text.as_deref(), Some(long.as_str()));
    assert!(row.text.contains("line 1"), "{}", row.text);
    assert!(row.text.contains("line 2"), "{}", row.text);
    assert!(row.text.contains("... 6 more lines"), "{}", row.text);
    assert!(row.text.contains("line 9"), "{}", row.text);
    assert!(row.text.contains("line 12"), "{}", row.text);
    assert!(!row.text.contains("line 8"), "{}", row.text);
}

#[test]
pub(crate) fn streaming_thinking_preview_tail_updates_while_collapsed() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: numbered_lines(1, 8),
        },
        true,
        false,
    );
    let idx = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    assert!(ui.transcript[idx].text.contains("line 8"));

    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: format!("\n{}", numbered_lines(9, 12)),
        },
        true,
        false,
    );

    let row = &ui.transcript[idx];
    assert!(!row.expanded);
    assert!(!row.details_collapsed);
    assert!(row.text.contains("line 9"), "{}", row.text);
    assert!(row.text.contains("line 12"), "{}", row.text);
    assert!(!row.text.contains("line 8"), "{}", row.text);
}

#[test]
pub(crate) fn typed_gateway_assistant_preamble_defaults_to_middle_fold_preview() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    let long = numbered_lines(1, 12);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:assistant:0",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Completed,
            Some("Preamble"),
            &long,
        ),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    assert_eq!(row.title, "Thinking");
    assert!(!row.expanded);
    assert_eq!(row.full_text.as_deref(), Some(long.as_str()));
    assert!(row.text.contains("... 6 more lines"), "{}", row.text);
    assert!(row.text.contains("line 12"), "{}", row.text);
    assert!(!row.text.contains("line 8"), "{}", row.text);
}

#[test]
pub(crate) fn typed_gateway_reasoning_completion_is_idempotent_by_item_id() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    let running = gateway_test_entry(
        "live:turn-1:reasoning:0",
        TranscriptBlockKind::Reasoning,
        TranscriptBlockStatus::Running,
        Some("Thinking"),
        "The command is still running.",
    );
    let completed = gateway_test_entry(
        "live:turn-1:reasoning:0",
        TranscriptBlockKind::Reasoning,
        TranscriptBlockStatus::Completed,
        Some("Thinking"),
        "The command is still running.",
    );
    app.apply_gateway_transcript_entry(&mut ui, Some("session-1"), running);
    app.apply_gateway_transcript_entry(&mut ui, Some("session-1"), completed.clone());
    app.apply_gateway_transcript_entry(&mut ui, Some("session-1"), completed);

    let thinking_rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Thinking)
        .collect::<Vec<_>>();
    assert_eq!(thinking_rows.len(), 1, "{:?}", ui.transcript);
    assert_eq!(thinking_rows[0].text, "The command is still running.");
    assert!(thinking_rows[0].tool_started.is_none());
}

#[test]
pub(crate) fn typed_gateway_final_answer_does_not_enter_thinking_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:reasoning:0",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Completed,
            Some("Thinking"),
            "Now I can write the report.",
        ),
    );
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:assistant:0",
            TranscriptBlockKind::Text,
            TranscriptBlockStatus::Completed,
            None,
            "日报完成。",
        ),
    );

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "日报完成。"),
        "{:?}",
        ui.transcript
    );
    assert!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Thinking)
            .all(|row| !row.text.contains("日报完成")),
        "{:?}",
        ui.transcript
    );
}

#[test]
pub(crate) fn gateway_yielded_exec_entry_keeps_original_command_title() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_tool_entry(
            "live:turn-1:tool:call_exec",
            "runtime.stream",
            None,
            TranscriptBlockStatus::Running,
            "exec_command",
            Some(serde_json::json!({"cmd": "python fetch.py"})),
            Some(serde_json::json!({"session_id": 7, "exit_code": null, "output": "live\n"})),
        ),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.tool_name.as_deref() == Some("exec_command"))
        .expect("exec row");
    assert_eq!(row.title, "exec_command python fetch.py");
    assert_eq!(row.kind, TranscriptKind::Ran);
    assert_eq!(row.text, "live\n");
}

#[test]
pub(crate) fn committed_turn_entries_replace_live_overlay_and_optimistic_prompt() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    ui.loaded_session_message_count = 2;

    let optimistic_start = ui.transcript.len();
    ui.push_user_with_images("$hackernews-daily".to_string(), &[]);
    ui.mark_optimistic_rows_from(optimistic_start);
    ui.bind_unbound_optimistic_rows_to_turn("turn-1");
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:reasoning:0",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Running,
            Some("Thinking"),
            "live thinking",
        ),
    );
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_tool_entry(
            "live:turn-1:tool:call_exec",
            "runtime.stream",
            None,
            TranscriptBlockStatus::Running,
            "exec_command",
            Some(serde_json::json!({"cmd": "python fetch.py"})),
            Some(serde_json::json!({"session_id": 7, "exit_code": null, "output": "live\n"})),
        ),
    );
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:assistant:0",
            TranscriptBlockKind::Text,
            TranscriptBlockStatus::Running,
            None,
            "live answer",
        ),
    );

    app.apply_committed_turn_entries(
        &mut ui,
        Some("session-1"),
        "turn-1",
        vec![
            durable_text_entry(3, TranscriptEntryRole::User, "$hackernews-daily"),
            durable_assistant_entry(
                4,
                vec![
                    durable_block(
                        "message:4:block:0",
                        TranscriptBlockKind::Reasoning,
                        TranscriptBlockStatus::Completed,
                        Some("Thinking"),
                        Some("durable thinking"),
                        None,
                    ),
                    durable_tool_block(
                        "tool:call_exec",
                        TranscriptBlockStatus::Completed,
                        "exec_command",
                        serde_json::json!({"cmd": "python fetch.py"}),
                        serde_json::json!({"session_id": null, "exit_code": 0, "output": "done\n"}),
                    ),
                    durable_block(
                        "message:4:block:2",
                        TranscriptBlockKind::Text,
                        TranscriptBlockStatus::Completed,
                        None,
                        Some("durable answer"),
                        None,
                    ),
                ],
            ),
        ],
    );

    assert!(
        ui.transcript.iter().all(|row| !matches!(
            row.transcript_source.as_deref(),
            Some("runtime.stream" | "tui.optimistic")
        )),
        "{:?}",
        ui.transcript
    );
    assert_eq!(ui.loaded_session_message_count, 4);
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Prompt && row.text == "$hackernews-daily")
            .count(),
        1,
        "{:?}",
        ui.transcript
    );
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Thinking && row.text == "durable thinking")
            .count(),
        1,
        "{:?}",
        ui.transcript
    );
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Ran
                && row.tool_name.as_deref() == Some("exec_command"))
            .count(),
        1,
        "{:?}",
        ui.transcript
    );
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Answer && row.text == "durable answer")
            .count(),
        1,
        "{:?}",
        ui.transcript
    );
}

#[test]
pub(crate) fn committed_turn_entries_remove_live_meta_without_removing_committed_footer() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    ui.loaded_session_message_count = 2;

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        durable_assistant_entry(
            2,
            vec![durable_block(
                "message:2:block:0",
                TranscriptBlockKind::Text,
                TranscriptBlockStatus::Completed,
                None,
                Some("previous answer"),
                Some(serde_json::json!({
                    "provider": "mock",
                    "model": "mock-model",
                    "finish_reason": "stop",
                    "outcome": "normal",
                    "metadata": {"elapsed_ms": 2_000}
                })),
            )],
        ),
    );

    let optimistic_start = ui.transcript.len();
    ui.push_user_with_images("你有哪些技能".to_string(), &[]);
    ui.mark_optimistic_rows_from(optimistic_start);
    ui.bind_unbound_optimistic_rows_to_turn("turn-2");

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        live_text_entry_with_turn(
            "live:turn-2:assistant",
            "turn-2",
            TranscriptBlockStatus::Completed,
            "live answer",
        ),
    );
    assert!(
        ui.transcript.iter().any(|row| {
            row.kind == TranscriptKind::Meta
                && row.text.contains("mock/mock-model")
                && row.transcript_source.as_deref() == Some("runtime.stream")
        }),
        "{:?}",
        ui.transcript
    );

    let mut committed_user = durable_text_entry(3, TranscriptEntryRole::User, "你有哪些技能");
    committed_user.turn_id = Some("turn-2".to_string());
    let mut committed_assistant = durable_assistant_entry(
        4,
        vec![durable_block(
            "message:4:block:0",
            TranscriptBlockKind::Text,
            TranscriptBlockStatus::Completed,
            None,
            Some("committed answer"),
            Some(serde_json::json!({
                "provider": "mock",
                "model": "mock-model",
                "finish_reason": "stop",
                "outcome": "normal",
                "metadata": {"elapsed_ms": 3_000}
            })),
        )],
    );
    committed_assistant.turn_id = Some("turn-2".to_string());

    app.apply_committed_turn_entries(
        &mut ui,
        Some("session-1"),
        "turn-2",
        vec![committed_user, committed_assistant],
    );

    assert!(
        ui.transcript.iter().all(|row| !matches!(
            row.transcript_source.as_deref(),
            Some("runtime.stream" | "tui.optimistic")
        )),
        "{:?}",
        ui.transcript
    );
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Prompt && row.text == "你有哪些技能")
            .count(),
        1,
        "{:?}",
        ui.transcript
    );
    let committed_meta_entries = ui
        .transcript
        .iter()
        .filter(|row| {
            row.kind == TranscriptKind::Meta
                && row.transcript_source.as_deref() == Some("runtime.message")
        })
        .filter_map(|row| row.transcript_entry_id.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        committed_meta_entries,
        vec!["message:2", "message:4"],
        "{:?}",
        ui.transcript
    );
}

#[test]
pub(crate) fn committed_turn_entries_skip_already_loaded_message_sequences() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    ui.loaded_session_message_count = 2;

    app.apply_committed_turn_entries(
        &mut ui,
        Some("session-1"),
        "turn-1",
        vec![
            durable_text_entry(1, TranscriptEntryRole::User, "old prompt"),
            durable_text_entry(3, TranscriptEntryRole::User, "new prompt"),
        ],
    );

    assert!(
        ui.transcript.iter().all(|row| row.text != "old prompt"),
        "{:?}",
        ui.transcript
    );
    assert!(
        ui.transcript.iter().any(|row| row.text == "new prompt"),
        "{:?}",
        ui.transcript
    );
    assert_eq!(ui.loaded_session_message_count, 3);
}

#[test]
pub(crate) fn committed_reasoning_entry_uses_middle_fold_preview() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    let long = numbered_lines(1, 12);

    app.apply_committed_turn_entries(
        &mut ui,
        Some("session-1"),
        "turn-1",
        vec![durable_assistant_entry(
            1,
            vec![durable_block(
                "message:1:block:0",
                TranscriptBlockKind::Reasoning,
                TranscriptBlockStatus::Completed,
                Some("Thinking"),
                Some(&long),
                None,
            )],
        )],
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    assert!(!row.expanded);
    assert_eq!(row.full_text.as_deref(), Some(long.as_str()));
    assert!(row.text.contains("line 1"), "{}", row.text);
    assert!(row.text.contains("line 2"), "{}", row.text);
    assert!(row.text.contains("... 6 more lines"), "{}", row.text);
    assert!(row.text.contains("line 9"), "{}", row.text);
    assert!(row.text.contains("line 12"), "{}", row.text);
    assert!(!row.text.contains("line 8"), "{}", row.text);
}

fn gateway_test_entry(
    id: &str,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    title: Option<&str>,
    text: &str,
) -> TranscriptEntry {
    TranscriptEntry {
        id: id.to_string(),
        thread_id: "session-1".to_string(),
        turn_id: Some("turn-1".to_string()),
        message_seq: None,
        role: TranscriptEntryRole::Assistant,
        status,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("{id}:block"),
            kind,
            status,
            order: 0,
            source: "runtime.stream".to_string(),
            title: title.map(str::to_string),
            preview: Some(text.to_string()),
            detail: Some(text.to_string()),
            body: Some(text.to_string()),
            artifact_ids: Vec::new(),
            metadata: if title == Some("Preamble") {
                Some(serde_json::json!({"projection": "assistant_preamble"}))
            } else {
                None
            },
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

fn numbered_lines(start: usize, end: usize) -> String {
    (start..=end)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn gateway_tool_entry(
    id: &str,
    source: &str,
    message_seq: Option<i64>,
    status: TranscriptBlockStatus,
    tool_name: &str,
    args: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
) -> TranscriptEntry {
    let mut metadata = serde_json::Map::new();
    metadata.insert("projection".to_string(), serde_json::json!("tool"));
    metadata.insert("tool_name".to_string(), serde_json::json!(tool_name));
    metadata.insert("tool_call_id".to_string(), serde_json::json!("call_exec"));
    if let Some(args) = args {
        metadata.insert("args".to_string(), args);
    }
    if let Some(result) = result {
        metadata.insert("result".to_string(), result);
    }
    metadata.insert("outcome".to_string(), serde_json::json!("normal"));
    TranscriptEntry {
        id: id.to_string(),
        thread_id: "session-1".to_string(),
        turn_id: Some("turn-1".to_string()),
        message_seq,
        role: TranscriptEntryRole::Assistant,
        status,
        source: source.to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("{id}:block"),
            kind: TranscriptBlockKind::Shell,
            status,
            order: 0,
            source: source.to_string(),
            title: Some(tool_name.to_string()),
            preview: None,
            detail: None,
            body: None,
            artifact_ids: Vec::new(),
            metadata: Some(serde_json::Value::Object(metadata)),
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

fn live_text_entry_with_turn(
    id: &str,
    turn_id: &str,
    status: TranscriptBlockStatus,
    text: &str,
) -> TranscriptEntry {
    TranscriptEntry {
        id: id.to_string(),
        thread_id: "session-1".to_string(),
        turn_id: Some(turn_id.to_string()),
        message_seq: None,
        role: TranscriptEntryRole::Assistant,
        status,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("{id}:block"),
            kind: TranscriptBlockKind::Text,
            status,
            order: 0,
            source: "runtime.stream".to_string(),
            title: None,
            preview: Some(text.to_string()),
            detail: Some(text.to_string()),
            body: Some(text.to_string()),
            artifact_ids: Vec::new(),
            metadata: Some(serde_json::json!({
                "provider": "mock",
                "model": "mock-model",
                "finish_reason": "stop",
                "outcome": "normal",
                "metadata": {"elapsed_ms": 1_000}
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

fn durable_text_entry(seq: i64, role: TranscriptEntryRole, text: &str) -> TranscriptEntry {
    TranscriptEntry {
        id: format!("message:{seq}"),
        thread_id: "session-1".to_string(),
        turn_id: Some("turn-1".to_string()),
        message_seq: Some(seq),
        role,
        status: TranscriptBlockStatus::Completed,
        source: "runtime.message".to_string(),
        blocks: vec![durable_block(
            &format!("message:{seq}:block:0"),
            TranscriptBlockKind::Text,
            TranscriptBlockStatus::Completed,
            None,
            Some(text),
            None,
        )],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn durable_assistant_entry(seq: i64, blocks: Vec<TranscriptBlock>) -> TranscriptEntry {
    TranscriptEntry {
        id: format!("message:{seq}"),
        thread_id: "session-1".to_string(),
        turn_id: Some("turn-1".to_string()),
        message_seq: Some(seq),
        role: TranscriptEntryRole::Assistant,
        status: TranscriptBlockStatus::Completed,
        source: "runtime.message".to_string(),
        blocks,
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn durable_tool_block(
    id: &str,
    status: TranscriptBlockStatus,
    tool_name: &str,
    args: serde_json::Value,
    result: serde_json::Value,
) -> TranscriptBlock {
    let mut metadata = serde_json::Map::new();
    metadata.insert("projection".to_string(), serde_json::json!("tool"));
    metadata.insert("tool_name".to_string(), serde_json::json!(tool_name));
    metadata.insert("tool_call_id".to_string(), serde_json::json!("call_exec"));
    metadata.insert("args".to_string(), args);
    metadata.insert("result".to_string(), result);
    metadata.insert("outcome".to_string(), serde_json::json!("normal"));
    durable_block(
        id,
        TranscriptBlockKind::Shell,
        status,
        Some(tool_name),
        None,
        Some(serde_json::Value::Object(metadata)),
    )
}

fn durable_block(
    id: &str,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    title: Option<&str>,
    body: Option<&str>,
    metadata: Option<serde_json::Value>,
) -> TranscriptBlock {
    TranscriptBlock {
        id: id.to_string(),
        kind,
        status,
        order: 0,
        source: "runtime.message".to_string(),
        title: title.map(str::to_string),
        preview: body.map(str::to_string),
        detail: body.map(str::to_string),
        body: body.map(str::to_string),
        artifact_ids: Vec::new(),
        metadata,
        result: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}
