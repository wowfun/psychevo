#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn fullscreen_btw_opens_hidden_side_and_ctrl_c_deletes_it() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent");
    insert_tui_message_with_metadata(
        &app.db_path,
        &parent,
        1,
        "user",
        "parent prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "parent prompt"}],
            "timestamp_ms": 1
        }),
        None,
    );
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    app.handle_fullscreen_command(&mut ui, SlashCommand::Btw(None))
        .await
        .expect("btw");

    let side = app
        .side_conversation
        .as_ref()
        .expect("side state")
        .side_thread_id
        .clone();
    assert_eq!(app.current_session.as_deref(), Some(side.as_str()));
    assert!(ui.transcript.is_empty());
    assert!(
        app.tui_sessions(SessionListView::Active)
            .expect("sessions")
            .iter()
            .all(|session| session.summary.id != side)
    );

    app.handle_fullscreen_command(&mut ui, SlashCommand::Refresh)
        .await
        .expect("refresh rejected");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/refresh"
            && row.failed
            && row.text.contains("unavailable inside a /btw side chat")
    }));

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Compact(Some("focus current task".to_string())),
    )
    .await
    .expect("compact rejected");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/compact focus current task"
            && row.failed
            && row.text.contains("unavailable inside a /btw side chat")
    }));

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    )
    .await
    .expect("ctrl-c");

    assert_eq!(app.current_session.as_deref(), Some(parent.as_str()));
    assert!(app.side_conversation.is_none());
    assert!(
        SqliteStore::open(&app.db_path)
            .expect("store")
            .session_summary(&side)
            .expect("summary")
            .is_none()
    );
    assert!(
        ui.ephemeral_status
            .as_ref()
            .is_some_and(|status| { status.text.contains("returned from /btw") })
    );
}

#[tokio::test]
pub(crate) async fn fullscreen_btw_detaches_running_parent() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent");
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    attach_pending_agent_running(&mut ui);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Btw(None))
        .await
        .expect("btw");

    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);
    assert_eq!(
        ui.auxiliary_agent_tasks[0].session_id.as_deref(),
        Some(parent.as_str())
    );
    assert!(
        app.side_parent_status_label(&ui)
            .is_some_and(|label| label.contains("main running"))
    );

    for task in &ui.auxiliary_agent_tasks {
        task.control.abort();
    }
}

#[tokio::test]
pub(crate) async fn fullscreen_refresh_cleans_orphan_side_conversations() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent");
    let side = store
        .create_child_session_with_metadata(
            &parent,
            &app.workdir,
            TUI_SIDE_CONVERSATION_SESSION_SOURCE,
            "mock-model",
            "mock",
            Some(serde_json::json!({SIDE_CONVERSATION_METADATA_KEY: {"ephemeral": true}})),
        )
        .expect("side");
    app.current_session = Some(parent);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Refresh)
        .await
        .expect("refresh");
    for _ in 0..10 {
        if app.drain_side_cleanup_task(&mut ui).await.expect("drain") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert!(
        SqliteStore::open(&app.db_path)
            .expect("store")
            .session_summary(&side)
            .expect("summary")
            .is_none()
    );
    assert!(
        ui.ephemeral_status
            .as_ref()
            .is_some_and(|status| { status.text.contains("side cleanup deleted 1") })
    );
}

#[tokio::test]
pub(crate) async fn mouse_click_can_execute_slash_menu_row() {
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
pub(crate) async fn mouse_wheel_scrolls_transcript_inside_tui() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    for index in 0..12 {
        ui.transcript.push(TranscriptRow::simple(
            TranscriptKind::Answer,
            format!("line {index}"),
        ));
    }
    let _ = draw_fullscreen_for_test(&app, &mut ui, 80, 10);
    let bottom = ui.scroll;
    let transcript_area = ui.last_transcript_area.expect("transcript area");

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: transcript_area.x,
            row: transcript_area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("scroll");

    assert!(ui.scroll < bottom);
    assert!(!ui.auto_follow_transcript);
}

#[tokio::test]
pub(crate) async fn mouse_wheel_in_transcript_does_not_recall_composer_history() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.history = vec!["older prompt".to_string()];
    for index in 0..18 {
        ui.transcript.push(TranscriptRow::simple(
            TranscriptKind::Answer,
            format!("line {index}"),
        ));
    }
    let _ = draw_fullscreen_for_test(&app, &mut ui, 80, 10);
    let transcript_area = ui.last_transcript_area.expect("transcript area");
    ui.scroll = 0;
    ui.auto_follow_transcript = false;

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: transcript_area.x,
            row: transcript_area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("scroll");

    assert!(ui.scroll > 0);
    assert_eq!(ui.history_index, None);
    assert_eq!(textarea_text(&ui.textarea), "");
}

#[tokio::test]
pub(crate) async fn mouse_wheel_in_composer_or_status_is_ignored() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.history = vec!["older prompt".to_string()];
    for index in 0..18 {
        ui.transcript.push(TranscriptRow::simple(
            TranscriptKind::Answer,
            format!("line {index}"),
        ));
    }
    let _ = draw_fullscreen_for_test(&app, &mut ui, 80, 10);
    let composer_area = ui.last_composer_area.expect("composer area");
    let status_area = ui.last_status_area.expect("status area");
    ui.scroll = 0;
    ui.auto_follow_transcript = false;

    for area in [composer_area, status_area] {
        app.handle_fullscreen_mouse(
            &mut ui,
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: area.x,
                row: area.y,
                modifiers: KeyModifiers::NONE,
            },
        )
        .await
        .expect("scroll");
    }

    assert_eq!(ui.scroll, 0);
    assert_eq!(ui.history_index, None);
    assert_eq!(textarea_text(&ui.textarea), "");
}

#[tokio::test]
pub(crate) async fn mouse_wheel_routes_between_bottom_panel_and_transcript_by_hover() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    for index in 0..18 {
        ui.transcript.push(TranscriptRow::simple(
            TranscriptKind::Answer,
            format!("line {index}"),
        ));
    }
    app.handle_fullscreen_command(&mut ui, SlashCommand::Help)
        .await
        .expect("help");
    let _ = draw_fullscreen_for_test(&app, &mut ui, 80, 24);
    let transcript_area = ui.last_transcript_area.expect("transcript area");
    let panel_area = ui.last_bottom_panel_area.expect("bottom panel area");
    ui.scroll = 0;
    ui.auto_follow_transcript = false;

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: panel_area.x,
            row: panel_area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("panel scroll");

    let Some(BottomPanel::Help(panel)) = &ui.bottom_panel else {
        panic!("help panel");
    };
    assert_eq!(panel.scroll, 3);
    assert_eq!(ui.scroll, 0);

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: transcript_area.x,
            row: transcript_area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("transcript scroll");

    let Some(BottomPanel::Help(panel)) = &ui.bottom_panel else {
        panic!("help panel");
    };
    assert_eq!(panel.scroll, 3);
    assert!(ui.scroll > 0);
}

#[tokio::test]
pub(crate) async fn empty_composer_down_without_active_history_does_not_scroll_or_recall() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.history = vec!["older prompt".to_string()];
    for index in 0..18 {
        ui.transcript.push(TranscriptRow::simple(
            TranscriptKind::Answer,
            format!("line {index}"),
        ));
    }
    let _ = draw_fullscreen_for_test(&app, &mut ui, 80, 10);
    let bottom = ui.scroll;
    ui.auto_follow_transcript = false;
    ui.textarea = new_textarea();

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");

    assert_eq!(ui.scroll, bottom);
    assert!(!ui.auto_follow_transcript);
    assert_eq!(ui.history_index, None);
    assert_eq!(textarea_text(&ui.textarea), "");
}

#[tokio::test]
pub(crate) async fn empty_composer_up_recalls_latest_prompt_without_scrolling_transcript() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.history = vec!["older prompt".to_string(), "latest prompt".to_string()];
    for index in 0..18 {
        ui.transcript.push(TranscriptRow::simple(
            TranscriptKind::Answer,
            format!("line {index}"),
        ));
    }
    let _ = draw_fullscreen_for_test(&app, &mut ui, 80, 10);
    let bottom = ui.scroll;
    let auto_follow = ui.auto_follow_transcript;
    assert!(bottom > 0);
    ui.textarea = new_textarea();

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .await
        .expect("up");

    assert_eq!(ui.scroll, bottom);
    assert_eq!(ui.auto_follow_transcript, auto_follow);
    assert_eq!(ui.history_index, Some(1));
    assert_eq!(textarea_text(&ui.textarea), "latest prompt");
}

#[tokio::test]
pub(crate) async fn non_empty_composer_up_recalls_prompt_history_and_down_restores_draft() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.history = vec!["older prompt".to_string()];
    ui.textarea = textarea_with_text("draft");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .await
        .expect("up");

    assert_eq!(ui.history_index, Some(0));
    assert_eq!(textarea_text(&ui.textarea), "older prompt");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");

    assert_eq!(ui.history_index, None);
    assert_eq!(textarea_text(&ui.textarea), "draft");
}
