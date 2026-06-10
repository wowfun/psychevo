#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn composer_ctrl_a_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)
}

#[test]
pub(crate) fn composer_terminal_cursor_anchors_empty_input() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    let (_buffer, cursor) = draw_fullscreen_with_cursor_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    assert_eq!(cursor, (input.x, input.y));
}

#[test]
pub(crate) fn composer_terminal_cursor_anchors_normal_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("hello");

    let (_buffer, cursor) = draw_fullscreen_with_cursor_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    assert_eq!(cursor, (input.x + 5, input.y));
}

#[test]
pub(crate) fn composer_terminal_cursor_anchors_shell_mode_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.enter_shell_mode();
    ui.textarea = textarea_with_text("echo");

    let (_buffer, cursor) = draw_fullscreen_with_cursor_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    assert_eq!(cursor, (input.x + 4, input.y));
}

#[test]
pub(crate) fn composer_terminal_cursor_anchors_cjk_wide_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("你好x");

    let (_buffer, cursor) = draw_fullscreen_with_cursor_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    assert_eq!(cursor, (input.x + 5, input.y));
}

#[test]
pub(crate) fn long_single_line_composer_grows_to_wrapped_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("abcdefghijklmnopqrstuvwxyz");

    let (_buffer, cursor) = draw_fullscreen_with_cursor_for_test(&app, &mut ui, 12, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    assert_eq!(input.height, 3);
    assert_eq!(cursor, (input.x + 6, input.y + 2));
}

#[test]
pub(crate) fn empty_composer_retains_one_input_row() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    let _ = draw_fullscreen_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    assert_eq!(input.height, 1);
}

#[test]
pub(crate) fn multiline_composer_uses_logical_rows_when_wide() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("one\ntwo\nthree");

    let _ = draw_fullscreen_for_test(&app, &mut ui, 48, 12);
    let input = ui.last_composer_input_area.expect("composer input area");

    assert_eq!(input.height, 3);
}

#[test]
pub(crate) fn long_composer_height_is_capped_at_six_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text(&"abcdefghijklmnopqrstuvwxyz".repeat(4));

    let _ = draw_fullscreen_for_test(&app, &mut ui, 12, 14);
    let input = ui.last_composer_input_area.expect("composer input area");

    assert_eq!(input.height, 6);
}

#[test]
pub(crate) fn composer_terminal_cursor_stays_anchored_with_popup_above() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/sess");

    let (_buffer, cursor) = draw_fullscreen_with_cursor_for_test(&app, &mut ui, 48, 12);
    let input = ui.last_composer_input_area.expect("composer input area");

    assert!(
        !ui.last_slash_menu_areas.is_empty(),
        "slash popup should be rendered above composer"
    );
    assert_eq!(cursor, (input.x + 5, input.y));
}

#[tokio::test]
pub(crate) async fn enter_executes_first_slash_menu_suggestion() {
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
pub(crate) async fn composer_ctrl_a_selects_all_and_backspace_clears() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("hello\n世界");

    app.handle_fullscreen_key(&mut ui, composer_ctrl_a_key())
        .await
        .expect("ctrl-a");

    assert!(ui.textarea.is_selecting());
    assert_eq!(ui.textarea.selection_range(), Some(((0, 0), (1, 2))));

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .await
    .expect("backspace");

    assert_eq!(textarea_text(&ui.textarea), "");
    assert!(!ui.textarea.is_selecting());
}

#[tokio::test]
pub(crate) async fn composer_ctrl_a_delete_clears_selection() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("delete me");

    app.handle_fullscreen_key(&mut ui, composer_ctrl_a_key())
        .await
        .expect("ctrl-a");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
        .await
        .expect("delete");

    assert_eq!(textarea_text(&ui.textarea), "");
    assert!(!ui.textarea.is_selecting());
}

#[tokio::test]
pub(crate) async fn composer_ctrl_a_input_replaces_selection() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("old text");

    app.handle_fullscreen_key(&mut ui, composer_ctrl_a_key())
        .await
        .expect("ctrl-a");
    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('新'), KeyModifiers::NONE),
    )
    .await
    .expect("type");

    assert_eq!(textarea_text(&ui.textarea), "新");
    assert!(!ui.textarea.is_selecting());
}

#[tokio::test]
pub(crate) async fn composer_ctrl_a_paste_replaces_selection() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("old text");

    app.handle_fullscreen_key(&mut ui, composer_ctrl_a_key())
        .await
        .expect("ctrl-a");
    app.handle_fullscreen_event(&mut ui, CrosstermEvent::Paste("new\ntext".to_string()))
        .await
        .expect("paste");

    assert_eq!(textarea_text(&ui.textarea), "new\ntext");
    assert!(!ui.textarea.is_selecting());
}

#[tokio::test]
pub(crate) async fn composer_ctrl_a_empty_composer_is_noop() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_key(&mut ui, composer_ctrl_a_key())
        .await
        .expect("ctrl-a");

    assert_eq!(textarea_text(&ui.textarea), "");
    assert!(!ui.textarea.is_selecting());
}

#[tokio::test]
pub(crate) async fn composer_ctrl_a_shell_mode_deletes_command_without_exiting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.enter_shell_mode();
    ui.textarea = textarea_with_text("printf shell");

    app.handle_fullscreen_key(&mut ui, composer_ctrl_a_key())
        .await
        .expect("ctrl-a");
    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .await
    .expect("backspace");

    assert!(ui.shell_mode);
    assert_eq!(textarea_text(&ui.textarea), "");

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .await
    .expect("empty shell backspace");

    assert!(!ui.shell_mode);
}

#[tokio::test]
pub(crate) async fn composer_ctrl_a_bottom_panel_keeps_focus() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("draft");
    ui.bottom_panel = Some(BottomPanel::Help(app.help_panel()));

    app.handle_fullscreen_key(&mut ui, composer_ctrl_a_key())
        .await
        .expect("ctrl-a");

    assert!(matches!(ui.bottom_panel, Some(BottomPanel::Help(_))));
    assert_eq!(textarea_text(&ui.textarea), "draft");
    assert!(!ui.textarea.is_selecting());
}

#[tokio::test]
pub(crate) async fn composer_ctrl_a_esc_cancels_selection() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("draft");

    app.handle_fullscreen_key(&mut ui, composer_ctrl_a_key())
        .await
        .expect("ctrl-a");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc");

    assert_eq!(textarea_text(&ui.textarea), "draft");
    assert!(!ui.textarea.is_selecting());
    assert!(ui.running.is_none());
}

#[test]
pub(crate) fn composer_ctrl_a_selection_is_highlighted() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("highlight");
    ui.textarea.select_all();

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");
    let first_text_cell = (input.x, input.y);
    let cell = buffer.cell(first_text_cell).expect("selected char");

    assert!(cell.modifier.contains(Modifier::REVERSED));
    assert!(cell.modifier.contains(Modifier::BOLD));
    assert_ne!(cell.bg, TUI_SELECTION_BG);
}

#[tokio::test]
pub(crate) async fn composer_mouse_selection_selects_text_without_clipboard_copy() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let copied = Arc::new(Mutex::new(Vec::new()));
    let copied_for_sink = Arc::clone(&copied);
    app.clipboard = Arc::new(move |text| {
        copied_for_sink
            .lock()
            .expect("clipboard lock")
            .push(text.to_string());
        Ok(())
    });
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("delete me");
    draw_fullscreen_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(MouseEventKind::Down(MouseButton::Left), input.x, input.y),
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            input.x.saturating_add(6),
            input.y,
        ),
    )
    .await
    .expect("mouse drag");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            input.x.saturating_add(6),
            input.y,
        ),
    )
    .await
    .expect("mouse up");

    assert!(ui.textarea.is_selecting());
    assert_eq!(ui.textarea.selection_range(), Some(((0, 0), (0, 6))));
    assert_eq!(ui.selection, SelectionState::default());
    assert_eq!(app.clipboard_copies_in_flight, 0);
    assert!(copied.lock().expect("clipboard lock").is_empty());
}

#[tokio::test]
pub(crate) async fn composer_mouse_selection_backspace_and_delete_clear_selection() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("delete me");
    draw_fullscreen_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(MouseEventKind::Down(MouseButton::Left), input.x, input.y),
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            input.x.saturating_add(9),
            input.y,
        ),
    )
    .await
    .expect("mouse drag");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            input.x.saturating_add(9),
            input.y,
        ),
    )
    .await
    .expect("mouse up");

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .await
    .expect("backspace");
    assert_eq!(textarea_text(&ui.textarea), "");
    assert!(!ui.textarea.is_selecting());

    ui.textarea = textarea_with_text("delete me");
    ui.textarea.select_all();
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
        .await
        .expect("delete");

    assert_eq!(textarea_text(&ui.textarea), "");
    assert!(!ui.textarea.is_selecting());
}

#[tokio::test]
pub(crate) async fn composer_mouse_selection_input_and_paste_replace_selection() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("old text");
    draw_fullscreen_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(MouseEventKind::Down(MouseButton::Left), input.x, input.y),
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            input.x.saturating_add(8),
            input.y,
        ),
    )
    .await
    .expect("mouse drag");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            input.x.saturating_add(8),
            input.y,
        ),
    )
    .await
    .expect("mouse up");

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('新'), KeyModifiers::NONE),
    )
    .await
    .expect("type");
    assert_eq!(textarea_text(&ui.textarea), "新");
    assert!(!ui.textarea.is_selecting());

    ui.textarea = textarea_with_text("old text");
    ui.textarea.select_all();
    app.handle_fullscreen_event(&mut ui, CrosstermEvent::Paste("new\ntext".to_string()))
        .await
        .expect("paste");

    assert_eq!(textarea_text(&ui.textarea), "new\ntext");
    assert!(!ui.textarea.is_selecting());
}

#[tokio::test]
pub(crate) async fn composer_mouse_selection_maps_cjk_width_to_text_columns() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("你好ab");
    draw_fullscreen_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(MouseEventKind::Down(MouseButton::Left), input.x, input.y),
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            input.x.saturating_add(4),
            input.y,
        ),
    )
    .await
    .expect("mouse drag");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            input.x.saturating_add(4),
            input.y,
        ),
    )
    .await
    .expect("mouse up");

    assert_eq!(ui.textarea.selection_range(), Some(((0, 0), (0, 2))));
}

#[tokio::test]
pub(crate) async fn composer_mouse_click_moves_cursor_and_clears_old_selection() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("abcdef");
    ui.textarea.select_all();
    draw_fullscreen_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            input.x.saturating_add(3),
            input.y,
        ),
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            input.x.saturating_add(3),
            input.y,
        ),
    )
    .await
    .expect("mouse up");

    assert_eq!(ui.textarea.cursor(), (0, 3));
    assert!(!ui.textarea.is_selecting());
}

#[tokio::test]
pub(crate) async fn composer_mouse_selection_shell_mode_keeps_shell_state() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.enter_shell_mode();
    ui.textarea = textarea_with_text("echo hi");
    draw_fullscreen_for_test(&app, &mut ui, 48, 10);
    let input = ui.last_composer_input_area.expect("composer input area");

    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(MouseEventKind::Down(MouseButton::Left), input.x, input.y),
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            input.x.saturating_add(7),
            input.y,
        ),
    )
    .await
    .expect("mouse drag");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            input.x.saturating_add(7),
            input.y,
        ),
    )
    .await
    .expect("mouse up");

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .await
    .expect("backspace");

    assert_eq!(textarea_text(&ui.textarea), "");
    assert!(ui.shell_mode);
}

#[tokio::test]
pub(crate) async fn paste_event_inserts_full_path_without_dropping_chars() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let pasted =
        "/home/kevin/Projects/psychevo/.local/data/subagents-vs-agent-teams-dark.txt\r\n描述该文件";

    let outcome = app
        .handle_fullscreen_event(&mut ui, CrosstermEvent::Paste(pasted.to_string()))
        .await
        .expect("paste");

    assert!(outcome.needs_draw);
    assert_eq!(
        textarea_text(&ui.textarea),
        "/home/kevin/Projects/psychevo/.local/data/subagents-vs-agent-teams-dark.txt\n描述该文件"
    );
}

#[tokio::test]
pub(crate) async fn standalone_pasted_image_source_adds_pending_attachment() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let image = app.workdir.join("image.png");
    fs::write(&image, [1, 2, 3]).expect("image");
    let mut ui = FullscreenUi::new(&app);

    let outcome = app
        .handle_fullscreen_event(&mut ui, CrosstermEvent::Paste(image.display().to_string()))
        .await
        .expect("paste");

    assert!(outcome.needs_draw);
    assert_eq!(textarea_text(&ui.textarea), "[Image #1] ");
    assert_eq!(
        ui.pending_images,
        vec![PendingImageAttachment {
            placeholder: "[Image #1]".to_string(),
            image: ImageInput::LocalPath(image),
        }]
    );
    assert!(ui.ephemeral_status.is_none());
}

#[tokio::test]
pub(crate) async fn standalone_pasted_url_remains_text() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let pasted = "https://example.com/token-plan/mimo-v2.5-pro";

    let outcome = app
        .handle_fullscreen_event(&mut ui, CrosstermEvent::Paste(pasted.to_string()))
        .await
        .expect("paste");

    assert!(outcome.needs_draw);
    assert_eq!(textarea_text(&ui.textarea), pasted);
    assert!(ui.pending_images.is_empty());
    assert!(ui.ephemeral_status.is_none());
}

#[tokio::test]
pub(crate) async fn pasted_prompt_with_embedded_image_path_remains_text() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let image = app.workdir.join("img1.avif");
    fs::write(&image, [1, 2, 3]).expect("image");
    let mut ui = FullscreenUi::new(&app);
    let pasted = format!("描述这张图片的内容：{}\r\n", image.display());

    let outcome = app
        .handle_fullscreen_event(&mut ui, CrosstermEvent::Paste(pasted))
        .await
        .expect("paste");

    assert!(outcome.needs_draw);
    assert_eq!(
        textarea_text(&ui.textarea),
        format!("描述这张图片的内容：{}\n", image.display())
    );
    assert!(ui.pending_images.is_empty());
    assert!(ui.ephemeral_status.is_none());
}

#[tokio::test]
pub(crate) async fn missing_standalone_image_paste_is_inserted_as_text() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let missing = app.workdir.join("missing.avif");
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_event(
        &mut ui,
        CrosstermEvent::Paste(missing.display().to_string()),
    )
    .await
    .expect("paste");

    assert_eq!(textarea_text(&ui.textarea), missing.display().to_string());
    assert!(ui.pending_images.is_empty());
    assert!(ui.ephemeral_status.is_none());
}

#[tokio::test]
pub(crate) async fn cjk_prompt_with_relative_image_name_pastes_as_text() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let pasted = "描述这张图片的内容：img1.avif";

    app.handle_fullscreen_event(&mut ui, CrosstermEvent::Paste(pasted.to_string()))
        .await
        .expect("paste");

    assert_eq!(textarea_text(&ui.textarea), pasted);
    assert!(ui.pending_images.is_empty());
    assert!(ui.ephemeral_status.is_none());
}

#[tokio::test]
pub(crate) async fn image_slash_command_adds_attachment_and_restores_prompt() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let image = app.workdir.join("image one.png");
    fs::write(&image, [1, 2, 3]).expect("image");
    let mut ui = FullscreenUi::new(&app);
    let command = format!("/image \"{}\" describe it", image.display());

    app.submit_fullscreen_text(&mut ui, command.clone(), true)
        .await
        .expect("image command");

    assert_eq!(
        ui.history.last().map(String::as_str),
        Some(command.as_str())
    );
    assert_eq!(
        ui.pending_images,
        vec![PendingImageAttachment {
            placeholder: "[Image #1]".to_string(),
            image: ImageInput::LocalPath(image),
        }]
    );
    assert_eq!(textarea_text(&ui.textarea), "[Image #1] describe it");
    assert!(ui.running.is_none());
}

#[tokio::test]
pub(crate) async fn image_slash_command_error_uses_command_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.submit_fullscreen_text(&mut ui, "/image missing.png".to_string(), true)
        .await
        .expect("image command");

    assert!(ui.running.is_none());
    assert!(ui.pending_images.is_empty());
    assert!(ui.ephemeral_status.is_none());
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.failed
            && row.text.contains("image path does not exist")
    }));
}

#[tokio::test]
pub(crate) async fn leading_absolute_image_path_submits_as_prompt_not_slash_command() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let image = app.workdir.join("image.avif");
    fs::write(&image, [1, 2, 3]).expect("image");
    let mut ui = FullscreenUi::new(&app);
    let prompt = format!("{} 描述该图片", image.display());

    let should_quit = app
        .submit_fullscreen_text(&mut ui, prompt.clone(), true)
        .await
        .expect("submit");

    assert!(!should_quit);
    assert_eq!(ui.history.last().map(String::as_str), Some(prompt.as_str()));
    assert!(ui.running.is_some());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Prompt && row.text == prompt)
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

#[tokio::test]
pub(crate) async fn leading_absolute_markdown_path_submits_as_prompt_not_slash_command() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let path = app
        .workdir
        .join("docs")
        .join("evaluation")
        .join("README.md");
    fs::create_dir_all(path.parent().expect("parent")).expect("docs dir");
    fs::write(&path, "# Evaluation\n").expect("markdown");
    let mut ui = FullscreenUi::new(&app);
    let prompt = path.display().to_string();
    ui.textarea = textarea_with_text(&prompt);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("submit");

    assert_eq!(ui.history.last().map(String::as_str), Some(prompt.as_str()));
    assert!(ui.running.is_some());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Prompt && row.text == prompt)
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

#[tokio::test]
pub(crate) async fn unknown_slash_input_submits_as_prompt() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);
    let prompt = "/made-up explain this".to_string();

    app.submit_fullscreen_text(&mut ui, prompt.clone(), true)
        .await
        .expect("submit");

    assert_eq!(ui.history.last().map(String::as_str), Some(prompt.as_str()));
    assert!(ui.running.is_some());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Prompt && row.text == prompt)
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

#[tokio::test]
pub(crate) async fn uninstalled_dynamic_slash_input_submits_as_prompt() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);
    let prompt = "/helper apply it to src/lib.rs".to_string();

    app.submit_fullscreen_text(&mut ui, prompt.clone(), true)
        .await
        .expect("submit");

    assert_eq!(ui.history.last().map(String::as_str), Some(prompt.as_str()));
    assert!(ui.running.is_some());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Prompt && row.text == prompt)
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("unknown skill or bundle"))
    );
    if let Some(running) = ui.running.take() {
        running.control.abort();
        if let RunningTask::Agent(task) = running.task {
            let _ = task.await;
        }
    }
}

#[tokio::test]
pub(crate) async fn known_slash_command_argument_errors_remain_command_errors() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.submit_fullscreen_text(&mut ui, "/model set mock/model".to_string(), true)
        .await
        .expect("submit");

    assert!(ui.running.is_none());
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command && row.failed && row.text.contains("usage: /model")
    }));
}

#[tokio::test]
pub(crate) async fn embedded_absolute_image_path_submits_as_text() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let image = app.workdir.join("img1.png");
    fs::write(&image, tiny_png_bytes()).expect("image");
    let mut ui = FullscreenUi::new(&app);
    let prompt = format!("描述这张图片的内容：{}", image.display());

    app.submit_fullscreen_text(&mut ui, prompt.clone(), true)
        .await
        .expect("submit");

    assert_eq!(ui.history.last().map(String::as_str), Some(prompt.as_str()));
    assert!(ui.running.is_some());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Prompt && row.text == prompt)
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| { !(row.kind == TranscriptKind::Meta && row.text.contains("attachments")) })
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("image path does not exist"))
    );
    if let Some(running) = ui.running.take() {
        running.control.abort();
        if let RunningTask::Agent(task) = running.task {
            let _ = task.await;
        }
    }
}

#[tokio::test]
pub(crate) async fn image_only_submit_uses_pending_attachment_and_clears_composer() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let image = app.workdir.join("image.png");
    fs::write(&image, tiny_png_bytes()).expect("image");
    let mut ui = FullscreenUi::new(&app);
    let placeholder = ui.add_pending_image(ImageInput::LocalPath(image.clone()));
    ui.textarea = textarea_with_text(&placeholder);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("submit image");

    assert!(ui.pending_images.is_empty());
    assert_eq!(textarea_text(&ui.textarea), "");
    assert!(ui.running.is_some());
    assert!(
        ui.transcript
            .iter()
            .any(|row| { row.kind == TranscriptKind::Prompt && row.text == "[Image #1]" })
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Meta
            && row.text.contains("attachments")
            && row.text.contains("image 1: image.png")
    }));
    if let Some(running) = ui.running.take() {
        running.control.abort();
        if let RunningTask::Agent(task) = running.task {
            let _ = task.await;
        }
    }
}
