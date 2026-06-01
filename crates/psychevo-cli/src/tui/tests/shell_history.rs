#[allow(unused_imports)]
pub(crate) use super::*;
#[test]
pub(crate) fn shell_escape_parser_accepts_leading_space_and_preserves_command() {
    let parsed = parse_shell_escape_input("  !echo hi").expect("shell escape");

    assert_eq!(parsed.command, "echo hi");
    assert_eq!(parsed.history_text, "!echo hi");
    assert_eq!(
        parse_shell_escape_input("!").expect("bare shell").command,
        ""
    );
    assert!(parse_shell_escape_input("echo hi").is_none());
}

#[test]
pub(crate) fn shell_escape_history_survives_session_history_replacement() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_submitted_history("first prompt".to_string());
    ui.push_submitted_history("!echo hi".to_string());
    ui.replace_session_history_prompts(vec!["second prompt".to_string()]);

    assert_eq!(ui.history.as_slice(), ["second prompt", "!echo hi"]);
    assert_eq!(
        ui.history_kinds.as_slice(),
        [
            ComposerHistoryKind::SessionPrompt,
            ComposerHistoryKind::ProcessCommand
        ]
    );
}

#[tokio::test]
pub(crate) async fn esc_clears_empty_shell_mode_composer() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.enter_shell_mode();

    let should_quit = app
        .handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc");

    assert!(!should_quit);
    assert_eq!(textarea_text(&ui.textarea), "");
    assert!(!ui.shell_mode);
    assert!(ui.running.is_none());
}

#[test]
pub(crate) fn status_line_marks_shell_mode_for_bang_input() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.enter_shell_mode();
    ui.textarea = textarea_with_text("printf shell");

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);

    assert!(text.contains("mock/model  high  shell"), "{text}");
}

#[tokio::test]
pub(crate) async fn running_status_line_shows_spinner_elapsed_and_esc_hint() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
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
    ui.start_assistant();
    ui.running_elapsed_override = Some(Duration::from_secs(12));

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);

    assert!(text.contains("mock/model  high"), "{text}");
    assert!(text.contains("⠼ 12s · Esc"), "{text}");
    assert!(!text.contains("Working"), "{text}");

    ui.running_elapsed_override = Some(Duration::from_secs(140));
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);

    assert!(text.contains("⠦ 2m20s · Esc"), "{text}");
    if let Some(running) = ui.running.take() {
        running.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn status_line_elapsed_survives_run_and_tool_phase_changes() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
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
    ui.start_assistant();
    ui.visible_turn_started = Some(
        Instant::now()
            .checked_sub(Duration::from_millis(12_500))
            .expect("instant"),
    );

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
            "type": "tool_execution_start",
            "tool_call_id": "call_read",
            "tool_name": "read",
            "args": {"path": "README.md"}
        }),
        false,
    );

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("12s · Esc"), "{text}");
    assert!(!text.contains("0s · Esc"), "{text}");

    if let Some(running) = ui.running.take() {
        running.task.abort();
    }
}

#[test]
pub(crate) fn historical_unfinished_prompt_without_live_work_does_not_show_running_elapsed() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("session");
    let prompt_ms = wall_now_ms() - 12_500;
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &session,
        1,
        "user",
        prompt_ms,
        serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "old prompt"}],
            "timestamp_ms": prompt_ms
        }),
    );
    app.current_session = Some(session);
    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);
    assert!(!text.contains("12s · Esc"), "{text}");
    assert!(!text.contains("Esc"), "{text}");
}

#[tokio::test]
pub(crate) async fn esc_interrupts_running_turn_without_transcript_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
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
    ui.start_assistant();
    ui.running_elapsed_override = Some(Duration::from_secs(12));

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc");

    assert!(ui.interrupt_requested);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "interrupt requested")
    );
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("mock/model  high"), "{text}");
    assert!(text.contains("interrupting 12s"), "{text}");

    if let Some(running) = ui.running.take() {
        running.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn esc_dismisses_slash_menu_before_interrupting_running_turn() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
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
    ui.start_assistant();
    ui.textarea = textarea_with_text("/mo");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc dismiss");
    assert!(!ui.interrupt_requested);
    assert!(ui.slash_menu_dismissed("/mo"));

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc interrupt");
    assert!(ui.interrupt_requested);

    if let Some(running) = ui.running.take() {
        running.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn shifted_one_key_enters_shell_mode() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::SHIFT),
    )
    .await
    .expect("bang key");

    assert!(ui.shell_mode);
    assert_eq!(textarea_text(&ui.textarea), "");
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("mock/model  high  shell"), "{text}");
}

#[tokio::test]
pub(crate) async fn empty_shell_mode_uses_bang_prompt_and_backspace_exits() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.enter_shell_mode();

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 48, 10);
    let composer_y = 8;
    assert_eq!(
        buffer
            .cell((0, composer_y))
            .expect("shell composer marker")
            .symbol(),
        "!"
    );
    assert_eq!(
        buffer
            .cell((1, composer_y))
            .expect("shell composer spacer")
            .symbol(),
        " "
    );
    assert!(!buffer_text(&buffer).contains("Ask pevo"));

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .await
    .expect("backspace");

    assert!(!ui.shell_mode);
    assert_eq!(textarea_text(&ui.textarea), "");
}

#[tokio::test]
pub(crate) async fn pasted_bang_input_imports_shell_mode_without_literal_bang() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_event(
        &mut ui,
        CrosstermEvent::Paste("  !printf pasted".to_string()),
    )
    .await
    .expect("paste");

    assert!(ui.shell_mode);
    assert_eq!(textarea_text(&ui.textarea), "printf pasted");
}

#[tokio::test]
pub(crate) async fn shell_mode_submit_records_bang_history_and_executes_command_text() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);
    ui.enter_shell_mode();
    ui.textarea = textarea_with_text("printf shell-mode");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert!(!ui.shell_mode);
    assert_eq!(
        ui.history.last().map(String::as_str),
        Some("!printf shell-mode")
    );

    drain_fullscreen_until_idle(&mut app, &mut ui).await;

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Ran
                && row.title == "! printf shell-mode"
                && row.text == "shell-mode")
    );
}

#[test]
pub(crate) fn user_shell_transcript_row_uses_prompt_surface_command_line() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran ! ls", "feeds\ntmp1.txt");
    row.user_shell = true;
    ui.transcript.push(row);

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 60, 12);
    let text = buffer_text(&buffer);
    let lines = text.lines().collect::<Vec<_>>();
    let command_y = lines
        .iter()
        .position(|line| line.contains("! ls"))
        .expect("user shell command row") as u16;

    assert!(lines[command_y as usize].contains("! ls"), "{text}");
    assert!(!lines[command_y as usize].contains("Ran"), "{text}");
    assert!(!lines[command_y as usize].contains("•"), "{text}");
    assert_eq!(
        buffer
            .cell((0, command_y))
            .expect("user shell marker")
            .symbol(),
        "!"
    );
    assert_eq!(
        buffer.cell((0, command_y)).expect("user shell marker").fg,
        TUI_CYAN
    );
    assert_eq!(
        buffer.cell((0, command_y)).expect("user shell marker").bg,
        TUI_SURFACE_BG
    );
    assert_eq!(
        buffer
            .cell((30, command_y))
            .expect("user shell row padding")
            .bg,
        TUI_SURFACE_BG
    );
    assert!(text.contains("└ feeds"), "{text}");
}

#[tokio::test]
pub(crate) async fn fullscreen_user_shell_runs_locally_and_drains_queued_shell_escape() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);

    app.start_fullscreen_shell(&mut ui, "printf shell-one".to_string())
        .expect("start shell");
    app.submit_fullscreen_text(&mut ui, "!printf shell-two".to_string(), true)
        .await
        .expect("queue shell");

    assert_eq!(
        ui.history.last().map(String::as_str),
        Some("!printf shell-two")
    );
    assert_eq!(ui.queued_inputs.len(), 1);

    drain_fullscreen_until_idle(&mut app, &mut ui).await;

    let ran_rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Ran)
        .collect::<Vec<_>>();
    assert_eq!(ran_rows.len(), 2, "{ran_rows:#?}");
    assert!(
        ran_rows
            .iter()
            .any(|row| { row.title == "! printf shell-one" && row.text == "shell-one" })
    );
    assert!(
        ran_rows
            .iter()
            .any(|row| { row.title == "! printf shell-two" && row.text == "shell-two" })
    );
    assert!(!app.had_error);
}

#[tokio::test]
pub(crate) async fn fullscreen_user_shell_during_agent_turn_waits_for_run_start_then_starts_auxiliary_task()
 {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.current_session = None;
    let session_id = SqliteStore::open(&app.db_path)
        .expect("store")
        .create_session_with_metadata(&app.workdir, "tui", "mock/model", "mock", None)
        .expect("session");
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
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
    ui.start_assistant();

    app.submit_fullscreen_text(&mut ui, "!printf aux-shell".to_string(), true)
        .await
        .expect("submit shell");

    assert!(ui.queued_inputs.is_empty());
    assert_eq!(ui.pending_auxiliary_shell_commands.len(), 1);
    assert_eq!(ui.auxiliary_shell_tasks.len(), 0);
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("shell 1"), "{text}");

    app.apply_fullscreen_stream_event(
        &mut ui,
        RunStreamEvent::Event(serde_json::json!({
            "type": "run_start",
            "session_id": session_id,
            "provider": "mock",
            "model": "mock/model",
            "mode": "default"
        })),
    );
    assert!(ui.pending_auxiliary_shell_commands.is_empty());
    assert_eq!(ui.auxiliary_shell_tasks.len(), 1);
    for _ in 0..200 {
        app.drain_fullscreen_events(&mut ui)
            .await
            .expect("drain events");
        if ui.auxiliary_shell_tasks.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(ui.auxiliary_shell_tasks.is_empty());
    assert!(ui.running.is_some());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Ran
                && row.title == "! printf aux-shell"
                && row.text == "aux-shell")
    );

    if let Some(running) = ui.running.take() {
        running.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn auxiliary_user_shell_missing_config_does_not_execute_marker_command() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let session_id = SqliteStore::open(&app.db_path)
        .expect("store")
        .create_session_with_metadata(&app.workdir, "tui", "mock/model", "mock", None)
        .expect("session");
    app.current_session = Some(session_id.clone());
    let marker = app.workdir.join("should-not-exist");
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
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
    ui.start_assistant();
    app.apply_fullscreen_stream_event(
        &mut ui,
        RunStreamEvent::Event(serde_json::json!({
            "type": "run_start",
            "session_id": session_id,
            "provider": "mock",
            "model": "mock/model",
            "mode": "default"
        })),
    );

    app.submit_fullscreen_text(
        &mut ui,
        format!("!printf nope > {}", marker.display()),
        true,
    )
    .await
    .expect("submit shell");

    assert_eq!(ui.auxiliary_shell_tasks.len(), 1);
    for _ in 0..200 {
        app.drain_fullscreen_events(&mut ui)
            .await
            .expect("drain events");
        if ui.auxiliary_shell_tasks.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert!(ui.auxiliary_shell_tasks.is_empty());
    assert!(!marker.exists());
    assert!(app.had_error);

    if let Some(running) = ui.running.take() {
        running.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn persisted_user_shell_history_reloads_as_ran_evidence() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);

    app.start_fullscreen_shell(&mut ui, "printf reload-shell".to_string())
        .expect("start shell");
    drain_fullscreen_until_idle(&mut app, &mut ui).await;
    assert!(app.current_session.is_some());

    let mut reloaded = FullscreenUi::new(&app);
    app.load_current_session_history(&mut reloaded)
        .expect("load history");
    assert_eq!(reloaded.history.as_slice(), ["!printf reload-shell"]);
    assert!(
        reloaded
            .transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Ran
                && row.title == "! printf reload-shell"
                && row.text == "reload-shell")
    );
    assert!(
        reloaded
            .transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Prompt)
    );
}

#[test]
pub(crate) fn composer_history_recall_preserves_draft() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.history = vec!["first".to_string(), "second".to_string()];
    ui.textarea = textarea_with_text("draft");

    ui.recall_history(-1);
    assert_eq!(textarea_text(&ui.textarea), "second");
    assert!(
        !ui.textarea
            .cursor_line_style()
            .has_modifier(Modifier::UNDERLINED)
    );
    ui.recall_history(-1);
    assert_eq!(textarea_text(&ui.textarea), "first");
    ui.recall_history(1);
    assert_eq!(textarea_text(&ui.textarea), "second");
    ui.recall_history(1);
    assert_eq!(textarea_text(&ui.textarea), "draft");
    assert_eq!(ui.history_index, None);
}

#[test]
pub(crate) fn shell_history_recall_restores_shell_mode_and_strips_bang() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_submitted_history("!echo hi".to_string());
    ui.textarea = textarea_with_text("draft");

    ui.recall_history(-1);
    assert!(ui.shell_mode);
    assert_eq!(textarea_text(&ui.textarea), "echo hi");

    ui.recall_history(1);
    assert!(!ui.shell_mode);
    assert_eq!(textarea_text(&ui.textarea), "draft");
}

#[test]
pub(crate) fn composer_history_recall_respects_multiline_boundaries() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.history = vec!["older".to_string()];
    ui.textarea = textarea_with_text("line one\nline two");

    assert!(!ui.can_recall_history_previous());
    assert!(!ui.can_recall_history_next());
    ui.textarea.move_cursor(CursorMove::Top);
    assert!(ui.can_recall_history_previous());
}

#[test]
pub(crate) fn tool_only_thinking_message_does_not_create_turn_meta() {
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
    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "thinking only".to_string(),
        },
        true,
        false,
    );
    ui.apply_value_event(
            &serde_json::json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [
                        { "type": "tool_call", "id": "call_1", "name": "read", "arguments": { "path": "file.txt" } }
                    ]
                }
            }),
            false,
        );

    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
}

#[test]
pub(crate) fn tool_failure_without_answer_keeps_failure_meta() {
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
            "type": "tool_execution_end",
            "tool_name": "exec_command",
            "tool_call_id": "call_1",
            "outcome": "failed",
            "result": { "error": "boom" }
        }),
        false,
    );

    let failed_row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("failed exec_command row");
    assert!(failed_row.failed);
    assert!(!failed_row.interrupted);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_end",
            "outcome": "normal",
            "messages": []
        }),
        false,
    );
    assert!(
        ui.transcript
            .iter()
            .any(|row| { row.kind == TranscriptKind::Meta && row.text.contains("1 failure") })
    );
}

#[test]
pub(crate) fn interrupted_bash_tool_renders_interrupted_without_failure_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.interrupt_requested = true;
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
            "args": { "cmd": "find /home/kevin -name tmp.txt -type f" },
            "outcome": "aborted",
            "elapsed_ms": 4_000,
            "result": {
                "output": "(no output)",
                "exit_code": null,
                "error": "aborted",
                "truncated": false
            }
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("interrupted exec_command row");
    assert_eq!(
        row.title,
        "exec_command find /home/kevin -name tmp.txt -type f"
    );
    assert_eq!(row.text, "interrupted");
    assert!(row.interrupted);
    assert!(!row.failed);
    assert_eq!(row.tool_elapsed, Some(Duration::from_secs(4)));
    assert!(
        ui.transcript
            .iter()
            .all(|row| !(row.kind == TranscriptKind::Ran && row.text.contains("(no output)")))
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_end",
            "outcome": "aborted",
            "messages": []
        }),
        false,
    );
    let meta = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Meta)
        .expect("interrupted meta");
    assert!(meta.text.contains("interrupted"), "{meta:?}");
    assert!(!meta.text.contains("failure"), "{meta:?}");
}

#[test]
pub(crate) fn interrupted_reasoning_only_turn_meta_includes_interrupted() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro",
            "mode": "default",
            "metadata": {
                "reasoning_effort": "high",
                "elapsed_ms": 82_000
            }
        }),
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "So the situation is incomplete.".to_string(),
        },
        true,
        false,
    );
    let reasoning_row = ui.reasoning_row.expect("reasoning row");
    ui.finish_thinking_row(reasoning_row);
    ui.interrupt_requested = true;
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_end",
            "outcome": "aborted",
            "messages": []
        }),
        false,
    );
    ui.update_turn_meta(false, true, true, true);

    let meta = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Meta)
        .expect("interrupted reasoning meta");
    assert!(meta.text.contains("interrupted"), "{meta:?}");
    assert!(!meta.text.contains("failure"), "{meta:?}");
}

#[test]
pub(crate) fn interrupted_user_shell_renders_interrupted_marker() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.interrupt_requested = true;
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_name": "exec_command",
            "tool_call_id": "shell_1",
            "source": "user_shell",
            "args": { "cmd": "find /home/kevin -name tmp.txt -type f" },
            "outcome": "aborted",
            "result": {
                "output": "(no output)",
                "exit_code": null,
                "error": "aborted",
                "truncated": false
            }
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("interrupted shell row");
    assert_eq!(row.text, "interrupted");
    assert!(row.interrupted);
    assert!(!row.failed);
}
