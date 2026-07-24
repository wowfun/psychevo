#[allow(unused_imports)]
pub(crate) use super::*;

fn bind_native(app: &TuiApp, session_id: &str) {
    let cwd = app.cwd.display().to_string();
    app.state_runtime
        .create_gateway_runtime_binding(psychevo_runtime::state::GatewayRuntimeBindingInput {
            thread_id: session_id,
            agent_ref: None,
            agent_fingerprint: "test-agent",
            agent_definition_json: "null",
            runtime_ref: "native",
            backend_kind: "native",
            native_kind: "native",
            native_session_id: Some(session_id),
            cwd: &cwd,
            profile_fingerprint: "test-profile",
            profile_revision: "1",
            profile_config_json: "{}",
            adapter_kind: "native",
            adapter_revision: "test",
            ownership: psychevo_runtime::state::GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("Native binding");
}

fn persisted_history_message(app: &TuiApp, session_id: &str) -> i64 {
    app.state_runtime
        .append_message_with_undo_snapshot_metadata_and_context_evidence(
            session_id,
            &psychevo_agent_core::Message::User {
                content: vec![
                    psychevo_agent_core::UserContentBlock::text("before hidden context after"),
                    psychevo_agent_core::UserContentBlock::image_url(
                        "https://example.test/history.png",
                    ),
                ],
                timestamp_ms: 1,
            },
            Some(serde_json::json!({
                psychevo_runtime::types::EDITABLE_INPUT_METADATA_KEY: {
                    "version": 1,
                    "parts": [
                        {"type": "text", "text": "before "},
                        {"type": "image", "imageBlockIndex": 0},
                        {"type": "text", "text": " after"}
                    ]
                }
            })),
            Some("before [Image #1] after".to_string()),
            &[],
        )
        .expect("persist history message")
}

#[test]
pub(crate) fn tui_prompt_metadata_keeps_text_image_order_in_exact_envelope() {
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
        "before [Image #1] middle [Image #2] after".to_string(),
        &attachments,
        &cwd,
    )
    .expect("metadata");
    assert_eq!(
        metadata.editable_input.expect("exact envelope").parts,
        vec![
            psychevo_runtime::types::StoredEditableInputPart::Text {
                text: "before ".to_string(),
            },
            psychevo_runtime::types::StoredEditableInputPart::Image {
                image_block_index: 0,
            },
            psychevo_runtime::types::StoredEditableInputPart::Text {
                text: " middle ".to_string(),
            },
            psychevo_runtime::types::StoredEditableInputPart::Image {
                image_block_index: 1,
            },
            psychevo_runtime::types::StoredEditableInputPart::Text {
                text: " after".to_string(),
            },
        ]
    );
}

#[tokio::test]
pub(crate) async fn persisted_user_row_keyboard_and_mouse_open_same_message_actions() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let session_id = app
        .state_runtime
        .create_session_with_metadata(&app.cwd, "tui", "model", "mock", None)
        .expect("session");
    bind_native(&app, &session_id);
    persisted_history_message(&app, &session_id);
    app.current_session = Some(session_id);
    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");
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

#[test]
pub(crate) fn point_fork_editor_preserves_ordered_images_and_prefills_empty_child() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let source = app
        .state_runtime
        .create_session_with_metadata(&app.cwd, "tui", "model", "mock", None)
        .expect("source");
    bind_native(&app, &source);
    let message_seq = persisted_history_message(&app, &source);
    app.current_session = Some(source.clone());
    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    app.begin_history_message_edit(
        &mut ui,
        format!("message:{message_seq}"),
        HistoryMessageAction::Fork,
    )
    .expect("begin fork edit");
    assert_eq!(textarea_text(&ui.textarea), "before [Image #1] after");
    assert_eq!(ui.pending_images.len(), 1);
    ui.set_composer_text("edited [Image #1] tail");
    assert!(
        app.submit_history_message_edit(&mut ui)
            .expect("point fork")
    );

    let child = app.current_session.clone().expect("child");
    assert_ne!(child, source);
    assert!(
        app.state_runtime
            .load_messages(&child)
            .expect("child messages")
            .is_empty()
    );
    assert_eq!(textarea_text(&ui.textarea), "edited [Image #1] tail");
    assert_eq!(ui.pending_images.len(), 1);
    assert!(ui.sidebar.title.contains("forked from"));
    assert_eq!(
        app.state_runtime
            .session_metadata(&child)
            .expect("metadata")
            .and_then(|metadata| metadata.get("forkedFromThreadId").cloned()),
        Some(serde_json::json!(source))
    );
}

#[test]
pub(crate) fn unchanged_tui_update_is_a_structural_no_op() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let source = app
        .state_runtime
        .create_session_with_metadata(&app.cwd, "tui", "model", "mock", None)
        .expect("source");
    bind_native(&app, &source);
    let message_seq = persisted_history_message(&app, &source);
    app.current_session = Some(source.clone());
    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");
    app.begin_history_message_edit(
        &mut ui,
        format!("message:{message_seq}"),
        HistoryMessageAction::UpdateAndRun,
    )
    .expect("begin update");

    assert!(
        app.submit_history_message_edit(&mut ui)
            .expect("unchanged update")
    );
    assert!(ui.history_message_edit.is_none());
    assert!(
        app.state_runtime
            .session_revert_state(&source)
            .expect("revert")
            .is_none()
    );
    assert!(ui.running.is_none());
}

#[tokio::test]
pub(crate) async fn sessions_action_f_creates_full_root_fork() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let source = app
        .state_runtime
        .create_session_with_metadata(&app.cwd, "tui", "model", "mock", None)
        .expect("source");
    bind_native(&app, &source);
    persisted_history_message(&app, &source);
    app.current_session = Some(source.clone());
    let mut ui = FullscreenUi::new(&app);
    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .expect("arm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
    )
    .expect("fork");

    let child = app.current_session.clone().expect("child");
    assert_ne!(child, source);
    assert_eq!(
        app.state_runtime
            .load_messages(&child)
            .expect("messages")
            .len(),
        1
    );
    assert_eq!(
        app.state_runtime
            .session_summary(&child)
            .expect("summary")
            .and_then(|summary| summary.parent_session_id),
        None
    );
    let sessions = app
        .session_selection_panel(SessionListView::Active)
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
