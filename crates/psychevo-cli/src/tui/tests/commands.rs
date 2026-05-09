#[tokio::test]
async fn mode_slash_command_requires_value() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/mode");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(ui.history.last().map(String::as_str), Some("/mode"));
    assert!(ui.bottom_panel.is_none());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Error
                && row.text.contains("usage: /mode <plan|default>"))
    );
}

#[tokio::test]
async fn mode_slash_command_sets_mode_with_direct_value() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/mode plan");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(ui.history.last().map(String::as_str), Some("/mode plan"));
    assert_eq!(app.current_mode, RunMode::Plan);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Error)
    );
}

#[tokio::test]
async fn fullscreen_status_uses_single_multiline_status_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Status)
        .await
        .expect("status");

    let status_rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Status)
        .collect::<Vec<_>>();
    assert_eq!(status_rows.len(), 1);
    assert!(status_rows[0].text.contains("workdir:"));
    assert!(status_rows[0].text.contains("\nmodel: mock/model\n"));
    assert!(status_rows[0].text.contains("\nvariant: high\n"));
    assert!(status_rows[0].text.contains("\ndebug: off"));
}

#[tokio::test]
async fn fullscreen_skills_command_lists_dynamic_entries_and_inserts_skill_marker() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    fs::create_dir_all(app.workdir.join(".git")).expect("git marker");
    let skill_dir = app.home.join("skills").join("helper");
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: helper\ndescription: Helps with focused edits\n---\n\nFollow the helper workflow.\n",
    )
    .expect("skill");

    let matches = app.slash_menu_items("/skill:h");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].command, "/skill:helper");

    let mut ui = FullscreenUi::new(&app);
    app.handle_fullscreen_command(&mut ui, SlashCommand::Skills)
        .await
        .expect("skills");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Status
                && row.text.contains("helper: Helps with focused edits"))
    );

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::SkillInvoke {
            name: "helper".to_string(),
            args: "apply it to src/lib.rs".to_string(),
        },
    )
    .await
    .expect("skill invoke");
    assert_eq!(textarea_text(&ui.textarea), "$helper apply it to src/lib.rs");
}

#[tokio::test]
async fn fullscreen_undo_restores_prompt_and_redo_restores_transcript() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&app.workdir)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = app.workdir.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("session");
    app.current_session = Some(session_id.clone());

    let before_first = test_track_snapshot(&app, &session_id);
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        1,
        "user",
        "first prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "first prompt"}],
            "timestamp_ms": 1
        }),
        Some(serde_json::json!({"undo": {"pre_snapshot": before_first}})),
    );
    fs::write(&file, "after first\n").expect("after first");
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        2,
        "assistant",
        "first answer",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "first answer"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
    );
    let before_second = test_track_snapshot(&app, &session_id);
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        3,
        "user",
        "second prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "second prompt"}],
            "timestamp_ms": 3
        }),
        Some(serde_json::json!({"undo": {"pre_snapshot": before_second}})),
    );
    fs::write(&file, "after second\n").expect("after second");
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        4,
        "assistant",
        "second answer",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "second answer"}],
            "timestamp_ms": 4,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui)
        .expect("load history");
    ui.textarea = textarea_with_text("/undo");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("undo");

    assert_eq!(textarea_text(&ui.textarea), "second prompt");
    assert_eq!(fs::read_to_string(&file).expect("file"), "after first\n");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "first answer")
    );
    assert!(ui.transcript.iter().all(|row| row.text != "second answer"));
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Status && row.text.contains("prompt restored"))
    );

    ui.textarea = textarea_with_text("/redo");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("redo");

    assert_eq!(textarea_text(&ui.textarea), "");
    assert_eq!(fs::read_to_string(&file).expect("file"), "after second\n");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "second answer")
    );
}

#[tokio::test]
async fn fullscreen_new_command_resets_session_without_status_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("previous prompt".to_string());

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");

    assert_eq!(app.current_session, None);
    assert_eq!(app.current_session_title, None);
    assert!(app.force_new_once);
    assert!(ui.transcript.is_empty());
    assert!(ui.terminal_clear_requested);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Status)
    );
}

#[tokio::test]
async fn mouse_click_can_execute_slash_menu_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/mo");
    ui.last_slash_menu_areas = vec![(
        1,
        Rect {
            x: 0,
            y: 2,
            width: 16,
            height: 1,
        },
    )];

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse");

    assert_eq!(ui.history.last().map(String::as_str), Some("/mode"));
}

#[tokio::test]
async fn mouse_wheel_scrolls_transcript_inside_tui() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.last_transcript_width = 80;
    ui.last_transcript_height = 4;
    for index in 0..12 {
        ui.transcript.push(TranscriptRow::simple(
            TranscriptKind::Answer,
            format!("line {index}"),
        ));
    }
    ui.scroll_to_bottom();
    let bottom = ui.scroll;

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("scroll");

    assert!(ui.scroll < bottom);
    assert!(!ui.auto_follow_transcript);
}
