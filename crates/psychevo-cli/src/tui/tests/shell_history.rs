#[test]
fn shell_escape_parser_accepts_leading_space_and_preserves_command() {
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
fn shell_escape_history_survives_session_history_replacement() {
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
async fn esc_clears_empty_shell_mode_composer() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("!");

    let should_quit = app
        .handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc");

    assert!(!should_quit);
    assert_eq!(textarea_text(&ui.textarea), "");
    assert!(ui.running.is_none());
}

#[test]
fn status_line_marks_shell_mode_for_bang_input() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("!printf shell");

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);

    assert!(text.contains("mock/model  high  shell"), "{text}");
}

#[test]
fn running_status_line_shows_spinner_elapsed_and_esc_hint() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.running_elapsed_override = Some(Duration::from_secs(12));

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);

    assert!(text.contains("mock/model  high"), "{text}");
    assert!(text.contains("12s · Esc"), "{text}");
    assert!(!text.contains("Working"), "{text}");

    ui.running_elapsed_override = Some(Duration::from_secs(140));
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);

    assert!(text.contains("2m20s · Esc"), "{text}");
}

#[tokio::test]
async fn esc_interrupts_running_turn_without_transcript_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
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
async fn esc_dismisses_slash_menu_before_interrupting_running_turn() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
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
async fn shifted_one_key_enters_shell_mode() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::SHIFT),
    )
    .await
    .expect("bang key");

    assert_eq!(textarea_text(&ui.textarea), "!");
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("mock/model  high  shell"), "{text}");
}

#[tokio::test]
async fn fullscreen_user_shell_runs_locally_and_drains_queued_shell_escape() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
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
            .any(|row| { row.title == "Ran printf shell-one" && row.text == "shell-one" })
    );
    assert!(
        ran_rows
            .iter()
            .any(|row| { row.title == "Ran printf shell-two" && row.text == "shell-two" })
    );
    assert!(!app.had_error);
}

#[test]
fn composer_history_recall_preserves_draft() {
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
fn composer_history_recall_respects_multiline_boundaries() {
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
fn tool_only_thinking_message_does_not_create_turn_meta() {
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
fn tool_failure_without_answer_keeps_failure_meta() {
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
            "tool_name": "bash",
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
        .expect("failed bash row");
    assert!(failed_row.failed);
    assert!(!failed_row.interrupted);
    assert!(
        ui.transcript
            .iter()
            .any(|row| { row.kind == TranscriptKind::Meta && row.text.contains("1 failure") })
    );
}

#[test]
fn interrupted_bash_tool_renders_interrupted_without_failure_meta() {
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
            "tool_name": "bash",
            "tool_call_id": "call_1",
            "args": { "command": "find /home/kevin -name tmp.txt -type f" },
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
        .expect("interrupted bash row");
    assert_eq!(row.title, "Ran find /home/kevin -name tmp.txt -type f");
    assert_eq!(row.text, "interrupted");
    assert!(row.interrupted);
    assert!(!row.failed);
    assert_eq!(row.tool_elapsed, Some(Duration::from_secs(4)));
    assert!(
        ui.transcript
            .iter()
            .all(|row| !(row.kind == TranscriptKind::Ran && row.text.contains("(no output)")))
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
fn interrupted_user_shell_renders_interrupted_marker() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.interrupt_requested = true;
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_name": "bash",
            "tool_call_id": "shell_1",
            "source": "user_shell",
            "args": { "command": "find /home/kevin -name tmp.txt -type f" },
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
