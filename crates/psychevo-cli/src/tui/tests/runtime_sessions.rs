#[tokio::test]
async fn fullscreen_thinking_toggle_hides_existing_blocks_without_status() {
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
async fn tab_completes_slash_command_without_switching_mode() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/ren");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .expect("tab");

    assert_eq!(textarea_text(&ui.textarea), "/rename");
    assert_eq!(app.current_mode, RunMode::Build);
}

#[tokio::test]
async fn shift_tab_cycles_mode_without_status_row() {
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
    assert!(
        !ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Status && row.text.contains("mode:"))
    );
}

fn finished_run_result(app: &TuiApp) -> psychevo_runtime::RunResult {
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
async fn fullscreen_drain_keeps_queued_events_after_task_completion() {
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
        .find(|row| row.title == "Exploring fixture.txt")
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
        .find(|row| row.title == "Explored fixture.txt")
        .expect("tool evidence row");
    assert_eq!(tool_row.kind, TranscriptKind::Explored);
    assert_eq!(tool_row.text, "fixture content");
    let tool_index = ui
        .transcript
        .iter()
        .position(|row| row.title == "Explored fixture.txt")
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
async fn fast_reasoning_only_write_renders_updating_before_completion() {
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
        .position(|row| row.title == "Updating files")
        .expect("provisional updating row");
    assert!(thinking < updating);
    assert!(ui.transcript[updating].tool_started.is_some());
    assert!(ui.transcript[updating].tool_call_id.is_none());
    assert!(ui
        .transcript
        .iter()
        .all(|row| row.kind != TranscriptKind::Meta));
    assert!(ui.running.is_some());
    assert_eq!(ui.deferred_stream_events.len(), 3);

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("second drain");
    assert!(ui.running.is_some());
    assert_eq!(ui.deferred_stream_events.len(), 1);
    assert!(ui
        .transcript
        .iter()
        .any(|row| row.title == "Updating /tmp/hackernews-hot-05-39.md"));

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("third drain");
    assert!(ui.running.is_none());
    assert!(ui
        .transcript
        .iter()
        .any(|row| row.title == "Updated feeds/2026-05-10/hackernews-hot-05-39.md"));
}

#[tokio::test]
async fn pending_write_tool_input_defers_later_completion_events() {
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
    assert!(ui
        .transcript
        .iter()
        .any(|row| row.title == "Updating files"));
    assert_eq!(ui.deferred_stream_events.len(), 3);
    assert!(ui.running.is_some());

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("second drain");
    assert!(ui
        .transcript
        .iter()
        .any(|row| row.title == "Updating /tmp/hackernews-hot-05-39.md"));
    assert_eq!(ui.deferred_stream_events.len(), 1);

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("third drain");
    assert!(ui.running.is_none());
    assert!(ui
        .transcript
        .iter()
        .any(|row| row.title == "Updated feeds/2026-05-10/hackernews-hot-05-39.md"));
}

#[test]
fn multi_message_turn_preserves_answer_rows_across_tool_cycles() {
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
        .position(|row| row.title == "Explored fixture.txt")
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
async fn fullscreen_agent_end_releases_turn_before_auxiliary_task_finishes() {
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
async fn fullscreen_refreshes_title_after_detached_agent_task_finishes() {
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
            events: Vec::new(),
            warnings: Vec::new(),
        })
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
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
async fn interrupted_turn_restores_queued_inputs_to_composer_without_autostart() {
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
        events: Vec::new(),
        warnings: Vec::new(),
    };
    let task = tokio::spawn(async move { Ok(result) });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        control,
        rx,
        task: RunningTask::Agent(task),
    });
    ui.start_assistant();
    ui.turn_outcome = Some(Outcome::Aborted);
    ui.interrupt_requested = true;
    ui.queued_inputs.push_back(QueuedInput::Prompt {
        prompt: "queued prompt".to_string(),
        display_prompt: "[Image #1] queued prompt".to_string(),
        images: vec![PendingImageAttachment {
            placeholder: "[Image #1]".to_string(),
            image: ImageInput::ImageUrl("https://example.test/image.png".to_string()),
        }],
    });
    ui.queued_inputs
        .push_back(QueuedInput::Shell("printf queued-shell".to_string()));
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
fn normal_turn_with_tool_failure_does_not_add_contradictory_error_row() {
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
            "tool_name": "bash",
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
fn streamed_budget_exhaustion_renders_specific_error_row() {
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

    assert!(app.had_error);
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Error
                && row.text.contains("model-turn limit (128)"))
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "turn ended: failed")
    );
}

#[tokio::test]
async fn completed_normal_task_with_tool_failures_does_not_mark_tui_error() {
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
        events: Vec::new(),
        warnings: Vec::new(),
    };
    let task = tokio::spawn(async move { Ok(result) });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
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
async fn completed_budget_exhaustion_renders_specific_error_row() {
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
        events: Vec::new(),
        warnings: Vec::new(),
    };
    let task = tokio::spawn(async move { Ok(result) });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        control,
        rx,
        task: RunningTask::Agent(task),
    });
    while !ui
        .running
        .as_ref()
        .expect("running")
        .task
        .is_finished()
    {
        tokio::task::yield_now().await;
    }

    app.drain_fullscreen_events(&mut ui).await.expect("drain");

    assert!(app.had_error);
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Error
                && row.text.contains("model-turn limit (128)"))
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "turn ended: failed")
    );
}

#[test]
fn fullscreen_loads_current_session_history() {
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
fn load_history_omits_bottom_context_usage_without_context_limit() {
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
async fn fullscreen_new_command_clears_context_usage_state() {
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
fn load_history_rehydrates_pending_write_tool_call() {
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
    assert!(ui.transcript.iter().any(|row| {
        row.title == "Updating feeds/2026-05-10/hackernews-hot-06-42.md"
            && row.tool_started.is_some()
    }));
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
        "Updated feeds/2026-05-10/hackernews-hot-06-42.md"
    );
    assert!(rows[0].tool_started.is_none());
}

#[test]
fn load_history_does_not_rehydrate_aborted_tool_calls_as_running() {
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
                    "name": "bash",
                    "arguments": {
                        "command": "cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \"SELECT content FROM stories WHERE id = 48074265;\" 2>&1 | head -c 3000",
                        "timeout": 10
                    },
                    "arguments_json": "{\"command\":\"cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \\\"SELECT content FROM stories WHERE id = 48074265;\\\" 2>&1 | head -c 3000\",\"timeout\":10}",
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
        .expect("bash row");
    assert!(row.title.starts_with("Ran cd /home/kevin/Projects/feedgarden"));
    assert!(!row.title.starts_with("Running "));
    assert_eq!(row.text, "interrupted");
    assert!(row.interrupted);
    assert!(!row.failed);
    assert!(row.tool_started.is_none());
    assert!(ui.tool_rows.is_empty());
}

#[tokio::test]
async fn sessions_panel_switches_without_status_row() {
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

#[tokio::test]
async fn sessions_panel_selection_does_not_reorder_by_view_time() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let older = store
        .create_session_with_metadata(&app.workdir, "tui", "model-a", "mock", None)
        .expect("older");
    let newer = store
        .create_session_with_metadata(&app.workdir, "tui", "model-b", "mock", None)
        .expect("newer");
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    conn.execute(
        "UPDATE sessions SET started_at_ms = 1000, updated_at_ms = 1000 WHERE id = ?1",
        rusqlite::params![&older],
    )
    .expect("older times");
    conn.execute(
        "UPDATE sessions SET started_at_ms = 2000, updated_at_ms = 2000 WHERE id = ?1",
        rusqlite::params![&newer],
    )
    .expect("newer times");
    app.current_session = Some(newer.clone());
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(session_panel_ids(panel), vec![newer.clone(), older.clone()]);

    for ch in "model-a".chars() {
        app.handle_bottom_panel_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .expect("query");
    }
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select");
    assert_eq!(app.current_session.as_deref(), Some(older.as_str()));

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions again");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(session_panel_ids(panel), vec![newer, older.clone()]);
    let current_row = panel
        .rows
        .iter()
        .find(|row| matches!(&row.value, BottomSelectionValue::Session(id) if id == &older))
        .expect("older row");
    assert!(current_row.is_current);
}

fn session_panel_ids(panel: &BottomSelectionPanel) -> Vec<String> {
    panel
        .rows
        .iter()
        .filter_map(|row| match &row.value {
            BottomSelectionValue::Session(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn sessions_panel_up_down_wraps_between_first_and_last_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let first = store
        .create_session_with_metadata(&app.workdir, "tui", "model-a", "mock", None)
        .expect("first");
    store
        .create_session_with_metadata(&app.workdir, "tui", "model-b", "mock", None)
        .expect("second");
    app.current_session = Some(first);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .expect("wrap up");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(
        panel.selected,
        panel.filtered_indices().len().saturating_sub(1)
    );

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("wrap down");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.selected, 0);
}

#[tokio::test]
async fn sessions_panel_action_mode_archives_current_and_restores_from_archived_view() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "model-a", "mock", None)
        .expect("session");
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &session_id,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "restore me"}],
            "timestamp_ms": 1
        }),
    );
    app.current_session = Some(session_id.clone());
    app.current_session_title = Some("Restore Me".to_string());
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("old visible prompt".to_string());
    ui.replace_session_history_prompts(vec!["old visible prompt".to_string()]);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .expect("arm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .expect("archive");

    assert_eq!(app.current_session, None);
    assert!(app.force_new_once);
    assert!(ui.transcript.is_empty());
    assert!(ui.history.is_empty());
    assert!(
        store
            .list_sessions_for_workdir_with_sources(&app.workdir, TUI_SESSION_SOURCES)
            .expect("active")
            .is_empty()
    );
    assert_eq!(
        store
            .list_archived_sessions_for_workdir_with_sources(&app.workdir, TUI_SESSION_SOURCES)
            .expect("archived")
            .len(),
        1
    );

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .expect("archived view");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.session_view, Some(SessionListView::Archived));
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("restore select");

    assert_eq!(app.current_session.as_deref(), Some(session_id.as_str()));
    assert!(ui.bottom_panel.is_none());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Prompt && row.text == "restore me")
    );
    assert_eq!(ui.history.as_slice(), ["restore me"]);
}

#[tokio::test]
async fn sessions_panel_delete_requires_repeat_action_and_can_cancel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = None;
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "model-a", "mock", None)
        .expect("session");
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &session_id,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "delete me"}],
            "timestamp_ms": 1
        }),
    );
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .expect("arm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .expect("first delete");
    assert!(
        store
            .session_summary(&session_id)
            .expect("summary")
            .is_some()
    );
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.delete_confirm.as_deref(), Some(session_id.as_str()));

    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .expect("cancel");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.delete_confirm, None);

    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .expect("arm again");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .expect("first delete again");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .expect("arm confirm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .expect("confirm delete");

    assert!(
        store
            .session_summary(&session_id)
            .expect("summary")
            .is_none()
    );
    assert!(
        store
            .load_messages(&session_id)
            .expect("messages")
            .is_empty()
    );
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.notice.as_deref(), Some("session deleted"));
}

#[tokio::test]
async fn sessions_panel_action_mode_does_not_pollute_search_and_rejects_running_current() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "model-a", "mock", None)
        .expect("session");
    app.current_session = Some(session_id.clone());
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        control,
        rx,
        task: RunningTask::Agent(task),
    });

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .expect("arm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
    )
    .expect("unknown action");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.query, "");
    assert_eq!(panel.notice.as_deref(), Some("action: A archive  D delete"));

    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .expect("arm archive");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .expect("archive");
    assert!(
        store
            .session_summary(&session_id)
            .expect("summary")
            .is_some()
    );
    assert_eq!(app.current_session.as_deref(), Some(session_id.as_str()));
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(
        panel.notice.as_deref(),
        Some("cannot archive the current session while a turn is running")
    );

    if let Some(running) = ui.running.take() {
        running.task.abort();
    }
}

#[test]
fn session_display_messages_count_visible_prompts_and_answers() {
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
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "visible prompt"}],
            "timestamp_ms": 1
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
            "content": [{"type": "text", "text": "visible answer"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
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
            "content": [{
                "type": "reasoning",
                "text": "folded only",
                "provider_evidence": null
            }],
            "timestamp_ms": 3,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    insert_tui_message(
        &conn,
        &session_id,
        4,
        "assistant",
        4,
        serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_read",
                "name": "read",
                "arguments": {"path": "Cargo.toml"},
                "arguments_json": "{\"path\":\"Cargo.toml\"}",
                "arguments_error": null,
                "content_index": 0,
                "call_index": 0
            }],
            "timestamp_ms": 4,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    insert_tui_message(
        &conn,
        &session_id,
        5,
        "tool_result",
        5,
        serde_json::json!({
            "role": "tool_result",
            "tool_call_id": "call_read",
            "tool_name": "read",
            "content": "{\"path\":\"Cargo.toml\",\"content\":\"ok\"}",
            "is_error": false,
            "timestamp_ms": 5
        }),
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    assert_eq!(visible_transcript_message_count(&ui.transcript), 2);
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| matches!(row.kind, TranscriptKind::Explored))
            .count(),
        1
    );
    assert_eq!(
        app.session_list_lines().expect("session list"),
        [format!(
            "{} tui mock/mock-model messages=2",
            short_session(&session_id)
        )]
    );
    let panel = app
        .session_selection_panel(SessionListView::Active)
        .expect("session panel");
    let row = panel
        .rows
        .iter()
        .find(|row| matches!(&row.value, BottomSelectionValue::Session(id) if id == &session_id))
        .expect("session row");
    assert_eq!(
        row.description.as_deref(),
        Some("mock/mock-model  messages=2")
    );
}
