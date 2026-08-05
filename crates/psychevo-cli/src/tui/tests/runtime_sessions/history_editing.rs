use crate::tui::tests::fixtures::{draw_fullscreen_for_test, test_app};
use crate::tui::{
    BottomPanel, BottomSelectionValue, FocusMode, FullscreenUi, HistoryMessageAction, ImageInput,
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    PendingImageAttachment, SessionListView, SlashCommand, StartThreadRequest, TranscriptHitTarget,
    TranscriptKind, TuiApp, TurnRequest, prompt_display_metadata, textarea_text,
};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::tempdir;

async fn start_bound_thread(app: &TuiApp) -> String {
    let mut start = StartThreadRequest::new(&app.cwd);
    start.source = "tui".to_string();
    let handle = app
        .runtime
        .client()
        .start_thread_with_turn(
            start,
            TurnRequest::new("history editing fixture")
                .with_model(Some("mock/model".to_string()), None),
        )
        .await
        .expect("bound Thread acceptance");
    let thread_id = handle.receipt().thread_id.clone();
    handle.interrupt();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle.wait())
        .await
        .expect("bound Thread fixture must settle");
    let conn = rusqlite::Connection::open(&app.db_path).expect("history fixture connection");
    conn.execute(
        "DELETE FROM messages WHERE session_id = ?1",
        rusqlite::params![&thread_id],
    )
    .expect("clear binding fixture message");
    thread_id
}

async fn persisted_history_message(app: &TuiApp, session_id: &str) -> i64 {
    let message = psychevo::application::Message::User {
        content: vec![
            psychevo::application::UserContentBlock::text("before hidden context after"),
            psychevo::application::UserContentBlock::image_url("https://example.test/history.png"),
        ],
        timestamp_ms: 1,
    };
    let metadata = serde_json::json!({
        psychevo::application::EDITABLE_INPUT_METADATA_KEY: {
            "version": 1,
            "parts": [
                {"type": "text", "text": "before "},
                {"type": "image", "imageBlockIndex": 0},
                {"type": "text", "text": " after"}
            ]
        }
    });
    let conn = rusqlite::Connection::open(&app.db_path).expect("history fixture connection");
    conn.execute(
        r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json,
                content_text, metadata_json
            ) VALUES (?1, 1, 'user', 1, ?2, ?3, ?4)
        "#,
        rusqlite::params![
            session_id,
            serde_json::to_string(&message).expect("message JSON"),
            "before [Image #1] after",
            metadata.to_string(),
        ],
    )
    .expect("persist history message");
    1
}

#[tokio::test]
pub(crate) async fn tui_prompt_metadata_keeps_text_image_order_in_exact_envelope() {
    let cwd = PathBuf::from("/workspace");
    let attachments = vec![
        PendingImageAttachment {
            placeholder: "[Image #1]".to_string(),
            image: ImageInput::LocalPath(cwd.join("one.png")),
        },
        PendingImageAttachment {
            placeholder: "[Image #2]".to_string(),
            image: ImageInput::ImageUrl("https://example.test/two.png".to_string()),
        },
    ];
    let metadata = prompt_display_metadata(
        "before [Image #1] middle [Image #2] after",
        &attachments,
        &cwd,
    )
    .expect("metadata");
    assert_eq!(
        metadata.editable_input.expect("exact envelope").parts,
        vec![
            psychevo::application::StoredEditableInputPart::Text {
                text: "before ".to_string(),
            },
            psychevo::application::StoredEditableInputPart::Image {
                image_block_index: 0,
            },
            psychevo::application::StoredEditableInputPart::Text {
                text: " middle ".to_string(),
            },
            psychevo::application::StoredEditableInputPart::Image {
                image_block_index: 1,
            },
            psychevo::application::StoredEditableInputPart::Text {
                text: " after".to_string(),
            },
        ]
    );
}

#[tokio::test]
pub(crate) async fn persisted_user_row_keyboard_and_mouse_open_same_message_actions() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let session_id = start_bound_thread(&app).await;
    persisted_history_message(&app, &session_id).await;
    app.current_session = Some(session_id);
    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui)
        .await
        .expect("history");
    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Prompt)
        .expect("prompt");
    let target = TranscriptHitTarget::Row(row.id);

    draw_fullscreen_for_test(&app, &mut ui, 90, 24);
    assert!(ui.target_visible(target), "target must be visible");
    assert_eq!(
        ui.transcript
            .iter()
            .find(|row| row.id == row.id)
            .and_then(|row| row.transcript_entry_id.as_deref()),
        Some("message:1")
    );
    ui.focus = FocusMode::Transcript;
    ui.selected_target = Some(target);
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("keyboard actions");
    let Some(BottomPanel::AgentActions(panel)) = &ui.bottom_panel else {
        panic!("message actions panel");
    };
    assert_eq!(panel.title, "Message Actions");
    assert_eq!(
        panel
            .rows
            .iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        ["Edit", "Fork"]
    );

    ui.bottom_panel = None;
    draw_fullscreen_for_test(&app, &mut ui, 90, 24);
    let area = ui
        .last_entry_areas
        .iter()
        .find_map(|(candidate, area)| (*candidate == target).then_some(*area))
        .expect("prompt area");
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_fullscreen_mouse(
            &mut ui,
            MouseEvent {
                kind,
                column: area.x.saturating_add(1),
                row: area.y,
                modifiers: KeyModifiers::NONE,
            },
        )
        .await
        .expect("mouse actions");
    }
    assert!(matches!(
        ui.bottom_panel,
        Some(BottomPanel::AgentActions(_))
    ));
}

#[tokio::test]
pub(crate) async fn point_fork_editor_preserves_ordered_images_and_prefills_empty_child() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let source = start_bound_thread(&app).await;
    let message_seq = persisted_history_message(&app, &source).await;
    app.current_session = Some(source.clone());
    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui)
        .await
        .expect("history");

    app.begin_history_message_edit(
        &mut ui,
        format!("message:{message_seq}"),
        HistoryMessageAction::Fork,
    )
    .await
    .expect("begin fork edit");
    assert_eq!(textarea_text(&ui.textarea), "before [Image #1] after");
    assert_eq!(ui.pending_images.len(), 1);
    ui.set_composer_text("edited [Image #1] tail");
    assert!(
        app.submit_history_message_edit(&mut ui)
            .await
            .expect("point fork")
    );

    let child = app.current_session.clone().expect("child");
    assert_ne!(child, source);
    assert!(
        app.runtime
            .client()
            .resume_thread(&child)
            .await
            .expect("child Thread")
            .history()
            .latest(Some(200))
            .await
            .expect("child history")
            .items
            .is_empty()
    );
    assert_eq!(textarea_text(&ui.textarea), "edited [Image #1] tail");
    assert_eq!(ui.pending_images.len(), 1);
    assert!(ui.sidebar.title.contains("forked from"));
    assert_eq!(
        app.runtime
            .client()
            .resume_thread(&child)
            .await
            .expect("child Thread")
            .summary()
            .await
            .expect("child summary")
            .forked_from_thread_id
            .as_deref(),
        Some(source.as_str())
    );
}

#[tokio::test]
pub(crate) async fn unchanged_tui_update_is_a_structural_no_op() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let source = start_bound_thread(&app).await;
    let message_seq = persisted_history_message(&app, &source).await;
    app.current_session = Some(source.clone());
    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui)
        .await
        .expect("history");
    app.begin_history_message_edit(
        &mut ui,
        format!("message:{message_seq}"),
        HistoryMessageAction::UpdateAndRun,
    )
    .await
    .expect("begin update");

    assert!(
        app.submit_history_message_edit(&mut ui)
            .await
            .expect("unchanged update")
    );
    assert!(ui.history_message_edit.is_none());
    assert!(
        app.runtime
            .gateway()
            .history_editing_state(&source)
            .await
            .expect("history editing state")
            .is_none()
    );
    assert!(ui.running.is_none());
}

#[tokio::test]
pub(crate) async fn sessions_action_f_creates_full_root_fork() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let source = start_bound_thread(&app).await;
    persisted_history_message(&app, &source).await;
    app.current_session = Some(source.clone());
    let mut ui = FullscreenUi::new(&app);
    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .await
    .expect("arm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
    )
    .await
    .expect("fork");

    let child = app.current_session.clone().expect("child");
    assert_ne!(child, source);
    assert_eq!(
        app.runtime
            .client()
            .resume_thread(&child)
            .await
            .expect("child Thread")
            .history()
            .latest(Some(200))
            .await
            .expect("child history")
            .items
            .len(),
        1
    );
    assert_eq!(
        app.runtime
            .client()
            .resume_thread(&child)
            .await
            .expect("child Thread")
            .summary()
            .await
            .expect("child summary")
            .parent_thread_id,
        None
    );
    let sessions = app
        .session_selection_panel(SessionListView::Active)
        .await
        .expect("sessions");
    let child_row = sessions
        .rows
        .iter()
        .find(|row| {
            matches!(&row.value, BottomSelectionValue::Session(session_id) if session_id == &child)
        })
        .expect("child row");
    assert!(
        child_row
            .description
            .as_deref()
            .is_some_and(|description| description.contains("forked from"))
    );
}
