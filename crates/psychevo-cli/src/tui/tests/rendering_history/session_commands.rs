use crate::tui::tests::fixtures::test_app;
use crate::tui::{
    FullscreenUi, KeyCode, KeyEvent, KeyModifiers, RunningTask, SlashCommand, StartThreadRequest,
    TranscriptKind, textarea_with_text,
};
use tempfile::tempdir;

#[tokio::test]
pub(crate) async fn fullscreen_rename_updates_session_title_and_sidebar() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let mut request = StartThreadRequest::new(&app.cwd);
    request.source = "tui".to_string();
    let thread = app
        .runtime
        .client()
        .start_thread(request)
        .await
        .expect("Thread");
    let session_id = thread.id().to_string();
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
    let summary = thread.summary().await.expect("Thread summary");
    assert_eq!(summary.title.as_deref(), Some("Better Session Title"));
}

#[tokio::test]
pub(crate) async fn obsolete_thinking_command_submits_as_prompt_in_fullscreen() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/thinking");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Prompt && row.text == "/thinking")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("unknown slash command"))
    );
    if let Some(running) = ui.running.take() {
        running.control.abort();
        if let RunningTask::Agent(task) = running.task {
            let _ = task.await;
        }
    }
}
