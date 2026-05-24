#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn fullscreen_rename_updates_session_title_and_sidebar() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "model", "provider", None)
        .expect("session");
    app.current_session = Some(session_id.clone());
    app.current_session_title = None;
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Rename("  Better\nSession   Title  ".to_string()),
    )
    .await
    .expect("rename");

    assert_eq!(
        app.current_session_title.as_deref(),
        Some("Better Session Title")
    );
    assert_eq!(ui.sidebar.title, "Better Session Title");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/rename Better Session Title"
            && row.text == "session renamed: Better Session Title"
    }));
    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some("Better Session Title"));
}

#[tokio::test]
pub(crate) async fn obsolete_thinking_command_is_unknown_in_fullscreen() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/thinking");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/thinking"
            && row.failed
            && row.text.contains("error: unknown slash command: /thinking")
    }));
}
