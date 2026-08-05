use tempfile::tempdir;
use tokio::sync::mpsc;

use crate::tui::tests::fixtures::{
    buffer_text, draw_fullscreen_for_test, finished_turn_result, test_app,
    test_shell_running_control,
};
use crate::tui::tests::{runtime_turn_event, start_thread_fixture};
use crate::tui::{
    CrosstermEvent, FullscreenUi, KeyCode, KeyEvent, KeyModifiers, PermissionMode, QueuedInput,
    RunMode, RunningTask, RunningTurn, RunningTurnEvents, SlashCommand, StartedTurn, StartingTurn,
    TranscriptKind, TranscriptRow, TurnEvent, TurnResult, textarea_text, textarea_with_text,
    transcript_line_count,
};

fn install_test_turn_admission(
    app: &crate::tui::TuiApp,
    ui: &mut FullscreenUi<'_>,
    display_prompt: &str,
    task: tokio::task::JoinHandle<psychevo::Result<StartedTurn>>,
) {
    let optimistic_start = ui.transcript.len();
    ui.push_user(display_prompt.to_string());
    ui.mark_optimistic_rows_from(optimistic_start);
    let cancellation = psychevo::TurnAdmissionCancellation::new();
    ui.starting_turn = Some(StartingTurn {
        session_id: app.current_session.clone(),
        queue_owner_id: format!("starting:{}", uuid::Uuid::now_v7()),
        display_prompt: display_prompt.to_string(),
        images: Vec::new(),
        cancellation,
        task,
    });
    ui.start_assistant();
}

#[tokio::test]
pub(crate) async fn fullscreen_thinking_toggle_hides_existing_blocks_without_status() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
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
    let mut app = test_app(&temp).await;
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
    let mut app = test_app(&temp).await;
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

#[tokio::test]
pub(crate) async fn fullscreen_drain_keeps_queued_events_after_task_completion() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(runtime_turn_event(serde_json::json!({
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
    tx.send(runtime_turn_event(serde_json::json!({
        "type": "tool_execution_start",
        "tool_call_id": "call_read_fixture",
        "tool_name": "read",
        "args": {"path": "fixture.txt"}
    })))
    .expect("send start");
    tx.send(runtime_turn_event(serde_json::json!({
        "type": "tool_execution_end",
        "tool_call_id": "call_read_fixture",
        "tool_name": "read",
        "args": {"path": "fixture.txt"},
        "result": {"path": "fixture.txt", "content": "fixture content"},
        "outcome": "normal"
    })))
    .expect("send end");
    drop(tx);

    let result = finished_turn_result("finished-session");
    let task = tokio::spawn(async move { Ok(result) });
    let control = test_shell_running_control(&app);
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::TurnTest(rx),
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
pub(crate) async fn resync_reloads_history_before_turn_completion_reconciles_tools() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let session_id = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    app.current_session = Some(session_id.clone());
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(runtime_turn_event(serde_json::json!({
        "type": "run_start",
        "session_id": session_id.clone(),
        "provider": "mock",
        "model": "model-a",
        "mode": "default"
    })))
    .expect("run start");
    tx.send(runtime_turn_event(serde_json::json!({
        "type": "tool_execution_start",
        "tool_call_id": "call-missed-end",
        "tool_name": "read",
        "args": {"path": "fixture.txt"}
    })))
    .expect("tool start");
    tx.send(TurnEvent::ResyncRequired { missed: 1 })
        .expect("resync");
    drop(tx);
    let task_session = app.current_session.clone().expect("session");
    let task = tokio::spawn(async move { Ok(finished_turn_result(&task_session)) });
    ui.running = Some(RunningTurn {
        session_id: app.current_session.clone(),
        control: test_shell_running_control(&app),
        selector: None,
        turn_id: Some("turn-resync".to_string()),
        events: RunningTurnEvents::TurnTest(rx),
        task: RunningTask::Agent(task),
    });
    while !ui.running.as_ref().expect("running").task.is_finished() {
        tokio::task::yield_now().await;
    }

    for _ in 0..3 {
        app.drain_fullscreen_events(&mut ui).await.expect("drain");
        if ui.running.is_none() {
            break;
        }
    }
    assert!(ui.running.is_none());
    assert!(
        ui.transcript.iter().all(|row| {
            row.tool_call_id.as_deref() != Some("call-missed-end") && !row.interrupted
        })
    );
}

#[tokio::test]
pub(crate) async fn final_message_defers_turn_meta_while_foreground_task_is_running() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(runtime_turn_event(serde_json::json!({
        "type": "run_start",
        "session_id": "streamed-session",
        "provider": "xiaomi-token-plan",
        "model": "mimo-v2.5-pro",
        "mode": "default"
    })))
    .expect("send run start");
    tx.send(runtime_turn_event(serde_json::json!({
        "type": "tool_execution_end",
        "tool_call_id": "call_sqlite",
        "tool_name": "exec_command",
        "args": {"cmd": "sqlite3 feeds.db"},
        "result": {"output": "[]", "exit_code": 1},
        "outcome": "failed"
    })))
    .expect("send tool end");
    tx.send(runtime_turn_event(serde_json::json!({
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
        Ok(TurnResult {
            final_answer: "I can continue with the remaining data.".to_string(),
            provider: "xiaomi-token-plan".to_string(),
            model: "mimo-v2.5-pro".to_string(),
            tool_failures: 1,
            ..finished_turn_result("streamed-session")
        })
    });
    let control = test_shell_running_control(&app);
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::TurnTest(rx),
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

    tx.send(runtime_turn_event(serde_json::json!({
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
    let mut app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(TurnEvent::ReasoningDelta {
        text: "Let me compose the full report now. I have all the data. Let me write it out."
            .to_string(),
    })
    .expect("send reasoning");
    tx.send(runtime_turn_event(serde_json::json!({
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
    tx.send(runtime_turn_event(serde_json::json!({
        "type": "tool_execution_start",
        "tool_call_id": "call_write_report",
        "tool_name": "write",
        "args": {
            "path": "/tmp/hackernews-hot-05-39.md",
            "content": "report body"
        }
    })))
    .expect("send start");
    tx.send(runtime_turn_event(serde_json::json!({
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

    let result = finished_turn_result("finished-session");
    let task = tokio::spawn(async move { Ok(result) });
    let control = test_shell_running_control(&app);
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::TurnTest(rx),
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
pub(crate) async fn turn_admission_failure_stays_fullscreen_and_restores_the_draft() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);
    let task: tokio::task::JoinHandle<psychevo::Result<StartedTurn>> = tokio::spawn(async move {
        Err(psychevo::Error::Message(
            "deterministic admission rejection".to_string(),
        ))
    });
    install_test_turn_admission(&app, &mut ui, "retry this prompt", task);
    let queue_owner_id = ui
        .starting_turn
        .as_ref()
        .expect("starting Turn")
        .queue_owner_id
        .clone();
    ui.queued_inputs.push_back(QueuedInput::Prompt {
        session_id: Some("other-session".to_string()),
        prompt: "other session input".to_string(),
        display_prompt: "other session input".to_string(),
        images: Vec::new(),
        mission: None,
        sequence: 1,
    });
    ui.queued_inputs.push_back(QueuedInput::Shell {
        session_id: Some(queue_owner_id),
        command: "owned-shell".to_string(),
        sequence: 2,
    });
    ui.set_composer_text("newer draft");
    while !ui
        .starting_turn
        .as_ref()
        .expect("starting Turn")
        .task
        .is_finished()
    {
        tokio::task::yield_now().await;
    }

    assert!(app.drain_fullscreen_events(&mut ui).await.expect("drain"));

    assert!(ui.starting_turn.is_none());
    assert!(ui.running.is_none());
    assert!(app.current_session.is_none());
    assert!(!ui.quit_requested);
    assert_eq!(
        textarea_text(&ui.textarea),
        "retry this prompt\n!owned-shell\nnewer draft"
    );
    assert_eq!(ui.queued_inputs.len(), 1);
    assert!(matches!(
        ui.queued_inputs.front(),
        Some(QueuedInput::Prompt { session_id, .. })
            if session_id.as_deref() == Some("other-session")
    ));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.transcript_source.as_deref() != Some("tui.optimistic"))
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Error && row.text.contains("deterministic admission rejection")
    }));

    app.handle_fullscreen_event(
        &mut ui,
        CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
    )
    .await
    .expect("fullscreen remains interactive");
    assert!(textarea_text(&ui.textarea).ends_with('x'));
}

#[tokio::test]
pub(crate) async fn pending_turn_admission_does_not_block_input_or_redraw() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let task: tokio::task::JoinHandle<psychevo::Result<StartedTurn>> = tokio::spawn(async move {
        let _ = release_rx.await;
        Err(psychevo::Error::Message(
            "released delayed admission".to_string(),
        ))
    });
    install_test_turn_admission(&app, &mut ui, "pending admission", task);

    tokio::time::timeout(
        std::time::Duration::from_millis(100),
        app.handle_fullscreen_event(
            &mut ui,
            CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)),
        ),
    )
    .await
    .expect("input handling is independent from admission")
    .expect("input event");
    let rendered = buffer_text(&draw_fullscreen_for_test(&app, &mut ui, 100, 18));

    assert_eq!(textarea_text(&ui.textarea), "d");
    assert!(rendered.contains("pending admission"));
    assert!(rendered.contains('d'));
    assert!(ui.starting_turn.is_some());
    assert!(
        ui.status_running_elapsed(app.current_session.as_deref())
            .is_some()
    );

    release_tx.send(()).expect("release admission");
    while !ui
        .starting_turn
        .as_ref()
        .expect("starting Turn")
        .task
        .is_finished()
    {
        tokio::task::yield_now().await;
    }
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain released admission");
}
