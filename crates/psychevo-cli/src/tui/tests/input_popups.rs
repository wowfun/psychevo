#[tokio::test]
async fn enter_executes_first_slash_menu_suggestion() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/sess");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(ui.history.last().map(String::as_str), Some("/sessions"));
    assert!(matches!(ui.bottom_panel, Some(BottomPanel::Sessions(_))));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Error)
    );
}

#[tokio::test]
async fn slash_menu_selection_can_choose_mode_over_model() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/mo");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(ui.history.last().map(String::as_str), Some("/mode"));
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Error
                && row.text.contains("usage: /mode <plan|default>"))
    );
}

#[tokio::test]
async fn slash_menu_up_down_wrap_between_first_and_last_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/mo");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .await
        .expect("up");
    assert_eq!(ui.slash_menu_selected, 1);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");
    assert_eq!(ui.slash_menu_selected, 0);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");
    assert_eq!(ui.slash_menu_selected, 1);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");
    assert_eq!(ui.slash_menu_selected, 0);
}

#[tokio::test]
async fn file_popup_keyboard_navigation_wraps_and_inserts_selection() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("read @sr");
    ui.file_search.popup = Some(FileSearchPopupState {
        query: "sr".to_string(),
        matches: vec![
            FileSearchMatch {
                path: "src".to_string(),
                kind: FileSearchMatchKind::Directory,
            },
            FileSearchMatch {
                path: "src/main.rs".to_string(),
                kind: FileSearchMatchKind::File,
            },
        ],
        selected: 0,
        waiting: false,
    });

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .await
        .expect("up");
    assert_eq!(ui.file_search.popup.as_ref().expect("popup").selected, 1);
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");
    assert_eq!(ui.file_search.popup.as_ref().expect("popup").selected, 0);
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(textarea_text(&ui.textarea), "read src/main.rs ");
    assert!(ui.file_search.popup.is_none());
}

#[tokio::test]
async fn file_popup_esc_dismisses_until_token_changes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("read @sr");
    ui.file_search.popup = Some(FileSearchPopupState {
        query: "sr".to_string(),
        matches: vec![FileSearchMatch {
            path: "src/main.rs".to_string(),
            kind: FileSearchMatchKind::File,
        }],
        selected: 0,
        waiting: false,
    });

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc");
    assert!(ui.file_search.popup.is_none());
    ui.sync_file_popup(&app.workdir);
    assert!(ui.file_search.popup.is_none());

    ui.textarea = textarea_with_text("read @s");
    ui.sync_file_popup(&app.workdir);
    assert!(ui.file_search.popup.is_some());
}

#[test]
fn file_popup_is_hidden_while_bottom_panel_is_open() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("read @src");
    ui.file_search.popup = Some(FileSearchPopupState {
        query: "src".to_string(),
        matches: vec![FileSearchMatch {
            path: "src/main.rs".to_string(),
            kind: FileSearchMatchKind::File,
        }],
        selected: 0,
        waiting: false,
    });
    ui.bottom_panel = Some(BottomPanel::Models(
        app.model_selection_panel().expect("panel"),
    ));

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 20);
    let text = buffer_text(&buffer);

    assert!(!text.contains("files"), "{text}");
    assert!(text.contains("Select Model"), "{text}");
}

#[tokio::test]
async fn mouse_click_inserts_file_popup_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("open @fi");
    ui.file_search.popup = Some(FileSearchPopupState {
        query: "fi".to_string(),
        matches: vec![FileSearchMatch {
            path: "file.txt".to_string(),
            kind: FileSearchMatchKind::File,
        }],
        selected: 0,
        waiting: false,
    });
    ui.last_file_popup_areas = vec![(
        0,
        Rect {
            x: 0,
            y: 3,
            width: 30,
            height: 1,
        },
    )];

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse");

    assert_eq!(textarea_text(&ui.textarea), "open file.txt ");
    assert!(ui.file_search.popup.is_none());
}

#[tokio::test]
async fn skill_popup_keyboard_selection_inserts_marker_without_submitting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_skill(&app, "helper", "Helps with focused edits");
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("use $he");
    app.sync_skill_popup(&mut ui);

    assert!(ui.skill_popup_visible());
    assert_eq!(
        ui.skill_search
            .popup
            .as_ref()
            .expect("popup")
            .matches[0]
            .name,
        "helper"
    );

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(textarea_text(&ui.textarea), "use $helper ");
    assert!(ui.skill_search.popup.is_none());
    assert!(ui.history.is_empty());
    assert!(ui.running.is_none());
}

#[tokio::test]
async fn skill_popup_esc_dismisses_until_token_changes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_skill(&app, "helper", "Helps with focused edits");
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("use $he");
    app.sync_skill_popup(&mut ui);
    assert!(ui.skill_popup_visible());

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc");
    assert!(ui.skill_search.popup.is_none());
    app.sync_skill_popup(&mut ui);
    assert!(ui.skill_search.popup.is_none());

    ui.textarea = textarea_with_text("use $h");
    app.sync_skill_popup(&mut ui);
    assert!(ui.skill_popup_visible());
}

#[tokio::test]
async fn slash_skill_selection_inserts_marker_without_submitting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_skill(&app, "helper", "Helps with focused edits");
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/skill:h");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(textarea_text(&ui.textarea), "$helper ");
    assert!(ui.history.is_empty());
    assert!(ui.running.is_none());
}

#[tokio::test]
async fn marker_only_prompt_sends_on_next_enter_after_skill_selection() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_skill(&app, "helper", "Helps with focused edits");
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("$he");
    app.sync_skill_popup(&mut ui);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("insert");
    assert_eq!(textarea_text(&ui.textarea), "$helper ");
    assert!(ui.history.is_empty());

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("submit");
    assert_eq!(ui.history.last().map(String::as_str), Some("$helper "));
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Prompt && row.text == "$helper ")
    );
    if let Some(running) = ui.running.take() {
        running.control.abort();
        if let RunningTask::Agent(task) = running.task {
            let _ = task.await;
        }
    }
}

#[tokio::test]
async fn mouse_click_inserts_skill_popup_row_without_submitting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("use $he");
    ui.skill_search.popup = Some(SkillSearchPopupState {
        query: "he".to_string(),
        matches: vec![SkillSearchMatch {
            name: "helper".to_string(),
            description: "Helps with focused edits".to_string(),
        }],
        selected: 0,
    });
    ui.last_skill_popup_areas = vec![(
        0,
        Rect {
            x: 0,
            y: 3,
            width: 30,
            height: 1,
        },
    )];

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse");

    assert_eq!(textarea_text(&ui.textarea), "use $helper ");
    assert!(ui.skill_search.popup.is_none());
    assert!(ui.history.is_empty());
    assert!(ui.running.is_none());
}

#[test]
fn run_start_selected_skills_adds_transcript_status() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "mock",
            "model": "mock-model",
            "mode": "default",
            "selected_skills": [{"name": "reviewer", "path": "/tmp/reviewer/SKILL.md"}]
        }),
        false,
    );

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Status
                && row.text.contains("skill loaded: reviewer"))
    );
}

fn write_tui_skill(app: &TuiApp, name: &str, description: &str) {
    let dir = app.home.join("skills").join(name);
    std::fs::create_dir_all(&dir).expect("skill dir");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description:?}\n---\n\nUse this skill.\n"),
    )
    .expect("skill");
}
