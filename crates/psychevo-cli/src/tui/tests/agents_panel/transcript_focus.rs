#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn ctrl_t_focuses_transcript_and_space_toggles_expandable_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::LongThinkingMarkdownBottom);
    let thinking_index = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    assert!(!ui.transcript[thinking_index].expanded);

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
    )
    .await
    .expect("focus transcript");
    assert_eq!(ui.focus, FocusMode::Transcript);
    assert!(ui.selected_target.is_some());

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    )
    .await
    .expect("toggle thinking");
    assert!(ui.transcript[thinking_index].expanded);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("return composer");
    assert_eq!(ui.focus, FocusMode::Composer);
}

pub(crate) fn write_home_tui_agent(app: &TuiApp, name: &str, description: &str) {
    let dir = app.home.join("agents");
    std::fs::create_dir_all(&dir).expect("home agent dir");
    std::fs::write(
        dir.join(format!("{name}.md")),
        format!("---\ndescription: {description:?}\n---\n\nUse this agent.\n"),
    )
    .expect("home agent");
}

pub(crate) async fn click_transcript_test_target(
    app: &mut TuiApp,
    ui: &mut FullscreenUi<'_>,
    target: TranscriptHitTarget,
) {
    let area = transcript_test_target_area(ui, target);
    app.handle_fullscreen_mouse(
        ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 1,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        ui,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: area.x + 1,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse up");
}

pub(crate) fn transcript_test_target_area(
    ui: &FullscreenUi<'_>,
    target: TranscriptHitTarget,
) -> Rect {
    ui.last_entry_areas
        .iter()
        .find_map(|(entry_target, area)| (*entry_target == target).then_some(*area))
        .expect("target area")
}
