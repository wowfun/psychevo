use tempfile::tempdir;
use tokio::sync::mpsc;

use crate::tui::tests::fixtures::{finished_turn_result, test_app, test_shell_running_control};
use crate::tui::tests::{insert_tui_message, runtime_turn_event, start_thread_fixture};
use crate::tui::{
    FullscreenUi, ImageInput, Outcome, PendingImageAttachment, QueuedInput, RunningTask,
    RunningTurn, RunningTurnEvents, StartThreadRequest, TranscriptKind, TurnOutcome, TurnResult,
    bottom_status_context_for_width, textarea_text, textarea_with_text,
};

#[tokio::test]
pub(crate) async fn fullscreen_refreshes_title_after_detached_agent_task_finishes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    let mut request = StartThreadRequest::new(&app.cwd);
    request.source = "tui".to_string();
    let thread = app
        .runtime
        .client()
        .start_thread(request)
        .await
        .expect("Thread");
    let session_id = thread.id().to_string();
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(runtime_turn_event(serde_json::json!({
        "type": "run_start",
        "session_id": session_id.clone(),
        "provider": "mock",
        "model": "mock-model",
        "mode": "default"
    })))
    .expect("send run start");
    tx.send(runtime_turn_event(serde_json::json!({
        "type": "agent_end",
        "outcome": "normal",
        "messages": []
    })))
    .expect("send agent end");

    let task_session_id = session_id.clone();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = done_rx.await;
        thread.set_title("X Daily").await?;
        Ok(finished_turn_result(task_session_id))
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
    let mut app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let result = TurnResult {
        outcome: TurnOutcome::Interrupted,
        final_answer: String::new(),
        ..finished_turn_result("aborted-session")
    };
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
        mission: None,
        sequence: prompt_sequence,
    });
    let shell_sequence = ui.next_pending_input_sequence();
    ui.queued_inputs.push_back(QueuedInput::Shell {
        session_id: app.current_session.clone(),
        command: "printf queued-shell".to_string(),
        sequence: shell_sequence,
    });
    ui.textarea = textarea_with_text("draft");

    app.finish_streamed_agent_turn(&mut ui).await;

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

#[tokio::test]
pub(crate) async fn normal_turn_with_tool_failure_does_not_add_contradictory_error_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
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

    app.finish_streamed_agent_turn(&mut ui).await;

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

#[tokio::test]
pub(crate) async fn streamed_budget_exhaustion_renders_specific_error_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
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

    app.finish_streamed_agent_turn(&mut ui).await;

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
    let mut app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let result = TurnResult {
        final_answer: "handled failure".to_string(),
        tool_failures: 1,
        ..finished_turn_result("normal-with-tool-failure")
    };
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
    let mut app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let result = TurnResult {
        outcome: TurnOutcome::Failed,
        terminal_reason: Some(psychevo::application::TerminalReason::MaxTurnsExceeded {
            max_turns: 128,
        }),
        final_answer: String::new(),
        ..finished_turn_result("budget-exhausted")
    };
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
pub(crate) async fn fullscreen_loads_current_session_history() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let session_id = start_thread_fixture(
        &app,
        &app.cwd,
        "tui",
        "mock-model",
        "mock",
        Some(serde_json::json!({"context_limit": 64_000})),
    )
    .await;
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
                usage_json, metadata_json, context_input_tokens, billable_input_tokens,
                billable_output_tokens, reasoning_tokens, cache_read_tokens,
                cache_write_tokens, reported_total_tokens, estimated_cost_nanodollars,
                pricing_source, cost_status
            ) VALUES (
                ?1, 2, 'assistant', 2500, ?2, 'hi', ?3, ?4,
                9, 7, 3, 1, 2, 1, 12, 10000000, 'test', 'estimated'
            )
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
    app.load_current_session_history(&mut ui)
        .await
        .expect("history");

    assert_eq!(ui.transcript[0].kind, TranscriptKind::Prompt);
    assert_eq!(ui.transcript[0].text, "hello");
    assert_eq!(ui.transcript[1].kind, TranscriptKind::Thinking);
    assert_eq!(ui.transcript[1].text, "folded thought");
    assert!(ui.transcript[1].details_collapsed);
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
    assert_eq!(ui.sidebar_tokens, Some(12));
    assert_eq!(ui.sidebar_context_limit, Some(64_000));
    let summary = ui
        .session_usage_summary
        .as_ref()
        .expect("session usage summary");
    assert_eq!(summary.reported_total_tokens, 12);
    assert_eq!(summary.cache_read_tokens, 2);
    assert_eq!(summary.estimated_cost_nanodollars, 10_000_000);
    let status = bottom_status_context_for_width(&app, &ui, 80).expect("status context");
    assert_eq!(
        status,
        "12/64.0k (0.0%) · cache 22% · tok 12 · cost $0.010000 · ~/work"
    );
    assert_eq!(ui.history, ["hello", "follow-up"]);
    ui.textarea = textarea_with_text("draft");
    ui.recall_history(-1);
    assert_eq!(textarea_text(&ui.textarea), "follow-up");
    ui.recall_history(1);
    assert_eq!(textarea_text(&ui.textarea), "draft");
}
