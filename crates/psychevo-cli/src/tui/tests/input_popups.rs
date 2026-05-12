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
async fn paste_event_inserts_full_path_without_dropping_chars() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let pasted = "/home/kevin/Projects/psychevo/.local/data/subagents-vs-agent-teams-dark.txt\r\n描述该文件";

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
async fn standalone_pasted_image_source_adds_pending_attachment() {
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
async fn pasted_prompt_with_embedded_image_path_remains_text() {
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
async fn missing_standalone_image_paste_is_inserted_as_text() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let missing = app.workdir.join("missing.avif");
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_event(&mut ui, CrosstermEvent::Paste(missing.display().to_string()))
        .await
        .expect("paste");

    assert_eq!(textarea_text(&ui.textarea), missing.display().to_string());
    assert!(ui.pending_images.is_empty());
    assert!(ui.ephemeral_status.is_none());
}

#[tokio::test]
async fn cjk_prompt_with_relative_image_name_pastes_as_text() {
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
async fn image_slash_command_adds_attachment_and_restores_prompt() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let image = app.workdir.join("image one.png");
    fs::write(&image, [1, 2, 3]).expect("image");
    let mut ui = FullscreenUi::new(&app);
    let command = format!("/image \"{}\" describe it", image.display());

    app.submit_fullscreen_text(&mut ui, command.clone(), true)
        .await
        .expect("image command");

    assert_eq!(ui.history.last().map(String::as_str), Some(command.as_str()));
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
async fn image_slash_command_error_uses_command_row() {
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
async fn leading_absolute_image_path_submits_as_prompt_not_slash_command() {
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
async fn embedded_absolute_image_path_submits_as_text() {
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
    assert!(ui.transcript.iter().all(|row| {
        !(row.kind == TranscriptKind::Meta && row.text.contains("attachments"))
    }));
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
async fn image_only_submit_uses_pending_attachment_and_clears_composer() {
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
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Prompt && row.text == "[Image #1]"
    }));
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

#[tokio::test]
async fn deleted_image_placeholder_unbinds_pending_attachment() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let image = app.workdir.join("image.png");
    fs::write(&image, tiny_png_bytes()).expect("image");
    let mut ui = FullscreenUi::new(&app);
    ui.add_pending_image(ImageInput::LocalPath(image));
    ui.textarea = textarea_with_text("describe only");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("submit");

    assert!(ui.pending_images.is_empty());
    assert!(ui.running.is_some());
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Prompt && row.text == "describe only"
    }));
    assert!(ui.transcript.iter().all(|row| {
        !(row.kind == TranscriptKind::Meta && row.text.contains("attachments"))
    }));
    if let Some(running) = ui.running.take() {
        running.control.abort();
        if let RunningTask::Agent(task) = running.task {
            let _ = task.await;
        }
    }
}

#[test]
fn adding_image_after_deleted_placeholder_keeps_placeholder_unique() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let image = app.workdir.join("image.png");
    fs::write(&image, tiny_png_bytes()).expect("image");
    let mut ui = FullscreenUi::new(&app);
    let first = ui.add_pending_image(ImageInput::LocalPath(image.clone()));
    ui.textarea = textarea_with_text(&first);
    let second = ui.add_pending_image(ImageInput::LocalPath(image.clone()));
    ui.textarea = textarea_with_text(&format!("{first} {second}"));
    ui.textarea = textarea_with_text(&second);

    let third = ui.add_pending_image(ImageInput::LocalPath(image));

    assert_eq!(second, "[Image #2]");
    assert_eq!(third, "[Image #3]");
    assert_eq!(
        ui.pending_images
            .iter()
            .map(|attachment| attachment.placeholder.as_str())
            .collect::<Vec<_>>(),
        vec!["[Image #2]", "[Image #3]"]
    );
}

#[tokio::test]
async fn new_command_clears_pending_images_and_ephemeral_status() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let image = app.workdir.join("image.png");
    fs::write(&image, tiny_png_bytes()).expect("image");
    let mut ui = FullscreenUi::new(&app);
    let placeholder = ui.add_pending_image(ImageInput::LocalPath(image));
    ui.textarea = textarea_with_text(&placeholder);
    ui.set_ephemeral_status("temporary");

    app.submit_fullscreen_text(&mut ui, "/new".to_string(), true)
        .await
        .expect("new");

    assert!(ui.pending_images.is_empty());
    assert!(ui.ephemeral_status.is_none());
}

#[tokio::test]
async fn unknown_slash_command_still_reports_bounded_error() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.submit_fullscreen_text(&mut ui, "/unknown".to_string(), true)
        .await
        .expect("submit");

    assert_eq!(ui.history.last().map(String::as_str), Some("/unknown"));
    assert!(ui.running.is_none());
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.failed
            && row.text.contains("unknown slash command: /unknown")
    }));
}

fn tiny_png_bytes() -> &'static [u8] {
    &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
        8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 15,
        4, 0, 9, 251, 3, 253, 5, 67, 69, 202, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ]
}

#[test]
fn fullscreen_terminal_commands_toggle_bracketed_paste() {
    let mut out = Vec::new();
    write_fullscreen_enter_commands(&mut out).expect("enter commands");
    assert!(String::from_utf8_lossy(&out).contains("\x1b[?2004h"));

    out.clear();
    write_fullscreen_exit_commands(&mut out).expect("exit commands");
    assert!(String::from_utf8_lossy(&out).contains("\x1b[?2004l"));
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
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/mode"
            && row.failed
            && row.text.contains("error: usage: /mode <plan|default>")
    }));
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
    ui.bottom_panel = Some(BottomPanel::Models(ModelPanel::new(
        app.model_selection_panel().expect("panel"),
    )));

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 20);
    let text = buffer_text(&buffer);

    assert!(!text.contains("files"), "{text}");
    assert!(text.contains("Model   Models    Info"), "{text}");
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
