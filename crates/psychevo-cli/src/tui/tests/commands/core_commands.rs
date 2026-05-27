#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn mode_slash_command_requires_value() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/mode");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(ui.history.last().map(String::as_str), Some("/mode"));
    assert!(ui.bottom_panel.is_none());
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/mode"
            && row.failed
            && row.text.contains("error: usage: /mode <plan|default>")
    }));
}

#[tokio::test]
pub(crate) async fn submitted_slash_command_restores_bottom_follow_after_manual_scroll() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    for index in 0..18 {
        ui.transcript.push(TranscriptRow::simple(
            TranscriptKind::Answer,
            format!("prior line {index}"),
        ));
    }
    let _ = draw_fullscreen_for_test(&app, &mut ui, 80, 10);
    assert_eq!(ui.scroll, ui.max_transcript_scroll());
    assert!(ui.scroll > 0);

    ui.scroll_transcript(-6);
    assert!(!ui.auto_follow_transcript);
    ui.textarea = textarea_with_text("/status");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");
    let _ = draw_fullscreen_for_test(&app, &mut ui, 80, 10);

    assert!(ui.auto_follow_transcript);
    assert_eq!(ui.scroll, ui.max_transcript_scroll());
    assert!(
        ui.transcript
            .iter()
            .any(|row| { row.kind == TranscriptKind::Command && row.title == "/status" })
    );
}

#[tokio::test]
pub(crate) async fn mode_slash_command_sets_mode_with_direct_value() {
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
pub(crate) async fn show_raw_toggles_persists_and_does_not_append_transcript_status() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::RawToggle)
        .await
        .expect("raw toggle");

    assert!(app.raw_visible);
    assert!(ui.raw_visible);
    assert!(ui.transcript.is_empty());
    let loaded = TuiState::load(&app.state_path).expect("load state");
    assert!(loaded.raw_visible);

    app.handle_fullscreen_command(&mut ui, SlashCommand::RawSet(false))
        .await
        .expect("raw off");

    assert!(!app.raw_visible);
    assert!(!ui.raw_visible);
    assert!(ui.transcript.is_empty());
    let loaded = TuiState::load(&app.state_path).expect("load state");
    assert!(!loaded.raw_visible);
}

#[tokio::test]
pub(crate) async fn show_raw_rejects_invalid_arguments_through_slash_entry() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/show-raw maybe");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/show-raw maybe"
            && row.failed
            && row.text.contains("error: usage: /show-raw [on|off]")
    }));
}

#[tokio::test]
pub(crate) async fn copy_command_copies_latest_answer_raw_markdown_without_transcript_row() {
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
    app.raw_visible = true;
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Answer,
        "",
        "# Raw title\n\n- `source` item".to_string(),
    ));

    app.handle_fullscreen_command(&mut ui, SlashCommand::Copy)
        .await
        .expect("copy");

    assert_eq!(
        copied.lock().expect("clipboard lock").as_slice(),
        ["# Raw title\n\n- `source` item"]
    );
    assert_eq!(
        ui.ephemeral_status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("copied latest answer Markdown")
    );
    assert!(ui.transcript.iter().all(|row| {
        row.kind != TranscriptKind::Command
            && row.kind != TranscriptKind::Status
            && row.kind != TranscriptKind::Error
    }));
}

#[tokio::test]
pub(crate) async fn ctrl_o_copies_latest_answer_raw_markdown() {
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
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Answer,
        "",
        "first answer".to_string(),
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Answer,
        "",
        "second **raw** answer".to_string(),
    ));

    let should_quit = app
        .handle_fullscreen_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        )
        .await
        .expect("ctrl-o");

    assert!(!should_quit);
    assert_eq!(
        copied.lock().expect("clipboard lock").as_slice(),
        ["second **raw** answer"]
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Command)
    );
}

#[tokio::test]
pub(crate) async fn configured_slash_alias_and_leader_shortcut_dispatch_commands() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.slash_config = parse_effective_slash_config(&serde_json::json!({
        "tui": {
            "slash_aliases": {
                "/status": ["/expr"]
            },
            "slash_keybinds": {
                "/status": "<leader>s"
            }
        }
    }))
    .expect("slash config");
    let mut ui = FullscreenUi::new(&app);
    let matches = app.slash_menu_items("/ex");
    assert_eq!(matches[0].command, "/expr");
    assert!(matches[0].description.contains("alias for /status"));

    ui.textarea = textarea_with_text("/ex");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .expect("alias tab");
    assert_eq!(textarea_text(&ui.textarea), "/expr");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("alias enter");

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command && row.title == "/expr" && row.text.contains("workdir:")
    }));

    let mut ui = FullscreenUi::new(&app);
    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .expect("leader");
    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
    )
    .await
    .expect("leader command");

    assert!(ui.history.is_empty());
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/status"
            && row.text.contains("workdir:")
    }));
}

#[tokio::test]
pub(crate) async fn configured_slash_shortcuts_do_not_fire_while_editing_or_in_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.slash_config = parse_effective_slash_config(&serde_json::json!({
        "tui": {
            "slash_keybinds": {
                "/status": ["<leader>s", "alt+s"]
            }
        }
    }))
    .expect("slash config");
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("draft");

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT),
    )
    .await
    .expect("shortcut while editing");

    assert!(ui.transcript.is_empty());
    assert_eq!(textarea_text(&ui.textarea), "draft");

    let mut ui = FullscreenUi::new(&app);
    ui.bottom_panel = Some(BottomPanel::Help(app.help_panel()));
    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .expect("leader in panel");
    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
    )
    .await
    .expect("s in panel");

    assert!(ui.transcript.is_empty());
    assert!(matches!(ui.bottom_panel, Some(BottomPanel::Help(_))));
}

#[tokio::test]
pub(crate) async fn fullscreen_status_uses_single_multiline_command_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Status)
        .await
        .expect("status");

    let status_rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Command)
        .collect::<Vec<_>>();
    assert_eq!(status_rows.len(), 1);
    assert_eq!(status_rows[0].title, "/status");
    assert!(status_rows[0].text.contains("workdir:"));
    assert!(status_rows[0].text.contains("\nmodel: mock/model\n"));
    assert!(status_rows[0].text.contains("\nvariant: high\n"));
    assert!(!status_rows[0].text.contains("\nthinking:"));
    assert!(status_rows[0].text.contains("\ndebug: off"));
}

#[tokio::test]
pub(crate) async fn fullscreen_help_command_opens_bottom_help_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Help)
        .await
        .expect("help");

    let Some(BottomPanel::Help(panel)) = &ui.bottom_panel else {
        panic!("help panel missing");
    };
    assert_eq!(panel.tab, HelpTab::General);
    assert!(
        panel
            .sections
            .custom_commands
            .contains(&"No custom commands available".to_string())
    );
    assert!(ui.transcript.is_empty());

    let text = app.help_status_text();
    assert!(text.contains("General\n"));
    assert!(text.contains("\nCommands\n"));
    assert!(text.contains("\nCustom commands\n"));
    assert!(text.contains("Ctrl+B - toggle sidebar"));
    assert!(text.contains("/usage - local usage and cost (aliases: /stats)"));
    assert!(text.contains("Reads persisted SQLite accounting and cost estimates"));
    assert!(text.contains(
        "/export [path] [-f|--format markdown|json] [-i|--include list] - write session export"
    ));
    assert!(text.contains("last-provider-response is normalized"));
    assert!(text.contains("No custom commands available"));
    assert!(!text.contains("pevo run"));
}

#[tokio::test]
pub(crate) async fn fullscreen_help_custom_commands_show_configured_slash_targets() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.slash_config = parse_effective_slash_config(&serde_json::json!({
        "tui": {
            "leader_key": "ctrl+x",
            "slash_aliases": {
                "/export -i lpr -f json": ["/expr"]
            },
            "slash_keybinds": {
                "/export -i lpr -f json": "<leader>e"
            }
        }
    }))
    .expect("slash config");
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Help)
        .await
        .expect("help");

    let Some(BottomPanel::Help(panel)) = &ui.bottom_panel else {
        panic!("help panel missing");
    };
    assert!(panel.sections.custom_commands.iter().any(|row| {
        row == "/export -i lpr -f json - write session export (aliases: /expr) (shortcuts: <leader>e)"
    }));
    assert!(
        !panel
            .sections
            .custom_commands
            .contains(&"No custom commands available".to_string())
    );
}

#[tokio::test]
pub(crate) async fn fullscreen_help_bottom_panel_switches_sections_and_closes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Help)
        .await
        .expect("help");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .expect("tab");
    let Some(BottomPanel::Help(panel)) = &ui.bottom_panel else {
        panic!("help panel missing");
    };
    assert_eq!(panel.tab, HelpTab::Commands);

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .expect("right");
    let Some(BottomPanel::Help(panel)) = &ui.bottom_panel else {
        panic!("help panel missing");
    };
    assert_eq!(panel.tab, HelpTab::CustomCommands);

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .expect("esc");
    assert!(ui.bottom_panel.is_none());
}

#[tokio::test]
pub(crate) async fn fullscreen_help_rejects_arguments_and_stats_alias_opens_usage() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    SqliteStore::open(&app.db_path).expect("store");
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/help now");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("help args");

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/help now"
            && row.failed
            && row.text.contains("error: /help does not accept arguments")
    }));

    ui.textarea = textarea_with_text("/usage");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("usage");

    assert!(matches!(ui.bottom_panel, Some(BottomPanel::Stats(_))));

    ui.bottom_panel = None;
    ui.textarea = textarea_with_text("/stats");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("stats alias");

    assert!(matches!(ui.bottom_panel, Some(BottomPanel::Stats(_))));
}

#[tokio::test]
pub(crate) async fn fullscreen_usage_command_opens_bottom_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    SqliteStore::open(&app.db_path).expect("store");
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Usage)
        .await
        .expect("usage");

    let Some(BottomPanel::Stats(panel)) = &ui.bottom_panel else {
        panic!("usage panel missing");
    };
    assert_eq!(panel.title, "Usage");
    assert!(panel.rows.iter().any(|row| row.label == "Totals"));
    assert!(
        panel
            .rows
            .iter()
            .any(|row| row.label == "Cache and reasoning")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Command)
    );
}

#[test]
pub(crate) fn usage_panel_groups_persisted_stats_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "model", "mock", None)
        .expect("session");
    store
        .set_session_title(&session_id, "Usage session")
        .expect("title");
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    conn.execute(
        r#"
        INSERT INTO messages (
            session_id, session_seq, role, timestamp_ms, message_json,
            content_text, model, provider, context_input_tokens,
            billable_input_tokens, billable_output_tokens, reasoning_tokens,
            cache_read_tokens, cache_write_tokens, reported_total_tokens,
            estimated_cost_nanodollars, pricing_source
        ) VALUES (
            ?1, 1, 'assistant', 1, '{"role":"assistant","content":[]}',
            'done', 'model', 'mock', 321, 200, 80, 21, 20, 7, 321, NULL, NULL
        )
        "#,
        rusqlite::params![&session_id],
    )
    .expect("assistant message");
    conn.execute(
        r#"
        INSERT INTO messages (
            session_id, session_seq, role, timestamp_ms, message_json, tool_name
        ) VALUES (?1, 2, 'tool_result', 2, '{"role":"tool_result"}', 'exec_command')
        "#,
        rusqlite::params![&session_id],
    )
    .expect("tool result");

    let panel = app.stats_panel().expect("usage panel");

    assert_eq!(panel.title, "Usage");
    assert!(panel.rows.iter().any(|row| {
        row.label == "Cache and reasoning"
            && row
                .description
                .as_deref()
                .is_some_and(|text| text.contains("21 reasoning"))
    }));
    assert!(panel.rows.iter().any(|row| row.label == "Unknown pricing"));
    assert!(panel.rows.iter().any(|row| {
        row.group.as_deref() == Some("Provider / model") && row.label == "mock/model"
    }));
    assert!(
        panel
            .rows
            .iter()
            .any(|row| row.group.as_deref() == Some("Top tools") && row.label == "exec_command")
    );
    assert!(panel.rows.iter().any(|row| {
        row.group.as_deref() == Some("Top sessions") && row.label == "Usage session"
    }));
}

#[tokio::test]
pub(crate) async fn fullscreen_context_command_appends_compact_command_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.last_context_snapshot = Some(test_context_snapshot());
    ui.last_transcript_width = 79;

    app.handle_fullscreen_command(&mut ui, SlashCommand::Context)
        .await
        .expect("context");

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Command && row.title == "/context")
        .expect("context row");
    assert!(row.text.starts_with("Context Usage\n"));
    assert!(row.text.contains("scope: last provider request"));
    assert!(!row.text.contains("bar:"));
    let bar_line = row
        .text
        .lines()
        .find(|line| line.starts_with('['))
        .expect("bar line");
    let cell_count = bar_line.len().saturating_sub(2);
    assert_eq!(cell_count, 70);
    assert_eq!(cell_count % 5, 0);
    assert!((50..=100).contains(&cell_count));
    assert!(row.text.contains(
        "\nB base  D developer  P project  H history  C turn  U prompt  T tools  . free\n\n"
    ));
    assert!(row.text.contains("\ninput_history:"));
    assert!(!row.text.contains("\nmessages:"));
    assert!(!row.text.contains("unavailable"));
}

#[tokio::test]
pub(crate) async fn fullscreen_context_command_refreshes_bottom_status_snapshot() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    app.last_context_snapshot = Some(test_context_snapshot());
    ui.last_context_snapshot = None;

    app.handle_fullscreen_command(&mut ui, SlashCommand::Context)
        .await
        .expect("context");

    assert!(ui.last_context_snapshot.is_some());
    let text = bottom_status_context_for_width(&app, &ui, 80).expect("status context");
    assert!(text.contains("~50/100 (50.0%) estimated"), "{text}");
}

#[tokio::test]
pub(crate) async fn fullscreen_variant_and_upcoming_feedback_use_command_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/variant low");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("variant");

    assert_eq!(app.current_variant.as_deref(), Some("low"));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/variant low"
            && row.text == "variant: low"
    }));

    app.handle_fullscreen_command(&mut ui, SlashCommand::Upcoming("compact".to_string()))
        .await
        .expect("upcoming");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/compact"
            && row.text == "/compact is upcoming; no session changes made"
    }));
}

#[tokio::test]
pub(crate) async fn fullscreen_compact_queues_behind_running_turn() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session = store
        .create_session_with_metadata(&app.workdir, "tui", "model", "mock", None)
        .expect("session");
    app.current_session = Some(session.clone());
    let mut ui = FullscreenUi::new(&app);
    attach_pending_agent_running(&mut ui);

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Compact(Some("focus on todos".to_string())),
    )
    .await
    .expect("compact");

    assert_eq!(ui.queued_inputs.len(), 1);
    match ui.queued_inputs.front().expect("queued") {
        QueuedInput::Compact {
            session_id,
            instructions,
            command_echo,
            ..
        } => {
            assert_eq!(session_id.as_deref(), Some(session.as_str()));
            assert_eq!(instructions.as_deref(), Some("focus on todos"));
            assert_eq!(command_echo, "/compact focus on todos");
        }
        other => panic!("unexpected queued input: {other:?}"),
    }
}

#[tokio::test]
pub(crate) async fn running_enter_steers_without_immediate_transcript_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    attach_pending_agent_running(&mut ui);
    ui.textarea = textarea_with_text("revise the current answer");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(ui.pending_steers.len(), 1);
    assert!(ui.queued_inputs.is_empty());
    assert_eq!(textarea_text(&ui.textarea), "");
    assert!(ui.ephemeral_status.is_none());
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Prompt)
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| { !(row.kind == TranscriptKind::Status && row.title == "Pending steer") })
    );

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("pending steer"));
    assert!(text.contains("revise the current answer"));
    assert!(!text.contains("steer 1"));
    assert!(!ui.last_pending_input_action_areas.is_empty());

    let pending_id = ui.pending_steers[0].id.as_u64();
    ui.apply_stream_event_for_session(
        RunStreamEvent::Event(serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "user",
                "content": [{"text": "revise the current answer"}],
                "timestamp_ms": 1
            },
            "metadata": {
                "pending_input": {
                    "id": pending_id,
                    "kind": "steer"
                }
            }
        })),
        app.thinking_visible,
        app.debug,
        app.current_session.as_deref(),
    );

    assert!(ui.pending_steers.is_empty());
    assert!(
        ui.transcript
            .iter()
            .all(|row| { !(row.kind == TranscriptKind::Status && row.title == "Pending steer") })
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Prompt && row.text == "revise the current answer"
    }));
}

#[tokio::test]
pub(crate) async fn pending_preview_shows_steer_and_queue_above_composer_without_status_counts() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    attach_pending_agent_running(&mut ui);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Steer("nudge now".to_string()))
        .await
        .expect("steer");
    app.handle_fullscreen_command(&mut ui, SlashCommand::Queue("next turn".to_string()))
        .await
        .expect("queue");

    assert_eq!(ui.pending_steers.len(), 1);
    assert_eq!(ui.queued_inputs.len(), 1);

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 16);
    let text = buffer_text(&buffer);
    assert!(text.contains("pending steer"));
    assert!(text.contains("nudge now"));
    assert!(text.contains("pending queue"));
    assert!(text.contains("next turn"));
    assert!(text.contains("[edit]"));
    assert!(text.contains("[undo]"));
    assert!(!text.contains("steer 1"));
    assert!(!text.contains("queue 1"));

    let composer_area = ui.last_composer_area.expect("composer area");
    assert!(
        ui.last_pending_input_action_areas
            .iter()
            .all(|(_, _, area)| area.y < composer_area.y)
    );

    ui.textarea = textarea_with_text("/");
    let _ = draw_fullscreen_for_test(&app, &mut ui, 80, 24);
    let slash_bottom = ui
        .last_slash_menu_areas
        .iter()
        .map(|(_, area)| area.y.saturating_add(area.height))
        .max()
        .expect("slash menu areas");
    let pending_top = ui
        .last_pending_input_action_areas
        .iter()
        .map(|(_, _, area)| area.y)
        .min()
        .expect("pending action areas");
    let composer_area = ui.last_composer_area.expect("composer area");
    assert!(pending_top >= slash_bottom);
    assert!(pending_top < composer_area.y);
}

#[tokio::test]
pub(crate) async fn diff_command_opens_overlay_and_renders_untracked_diff_without_transcript_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    init_git_repo_for_diff_test(&app.workdir);
    fs::write(app.workdir.join("notes.txt"), "hello from diff\n").expect("write");
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Diff)
        .await
        .expect("diff command");

    assert!(app.diff_task.is_some());
    assert!(ui.transcript.is_empty());
    let overlay = ui.diff_overlay.as_ref().expect("diff overlay");
    assert_eq!(overlay.title, "D I F F");
    assert_eq!(overlay_text(overlay), "computing diff");

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 18);
    let text = buffer_text(&buffer);
    assert!(text.contains("D I F F"), "{text}");
    assert!(text.contains("computing diff"), "{text}");

    drain_diff_task_for_test(&mut app, &mut ui).await;

    assert!(app.diff_task.is_none());
    assert!(ui.transcript.is_empty());
    let overlay = ui.diff_overlay.as_ref().expect("diff overlay");
    let text = overlay_text(overlay);
    assert!(text.contains("diff --git"), "{text}");
    assert!(text.contains("notes.txt"), "{text}");
    assert!(text.contains("+hello from diff"), "{text}");
}

#[tokio::test]
pub(crate) async fn diff_command_shows_empty_message_for_clean_workspace() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    init_git_repo_for_diff_test(&app.workdir);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Diff)
        .await
        .expect("diff command");
    drain_diff_task_for_test(&mut app, &mut ui).await;

    let overlay = ui.diff_overlay.as_ref().expect("diff overlay");
    assert_eq!(overlay_text(overlay), "No changes detected.");
    assert!(ui.transcript.is_empty());
}

#[tokio::test]
pub(crate) async fn diff_overlay_scrolls_and_closes_with_keys() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.diff_overlay = Some(DiffOverlay::from_lines(
        (0..80)
            .map(|index| Line::from(format!("diff line {index}")))
            .collect(),
    ));

    let _ = draw_fullscreen_for_test(&app, &mut ui, 100, 18);
    let viewport_height = ui.last_diff_overlay_area.expect("overlay area").height;

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
    )
    .await
    .expect("page down");
    let overlay = ui.diff_overlay.as_ref().expect("diff overlay");
    assert!(overlay.scroll > 0);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
        .await
        .expect("end");
    let overlay = ui.diff_overlay.as_ref().expect("diff overlay");
    assert_eq!(overlay.scroll, overlay.max_scroll(viewport_height));

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("escape");
    assert!(ui.diff_overlay.is_none());
    assert!(ui.last_diff_overlay_area.is_none());
}

async fn drain_diff_task_for_test(app: &mut TuiApp, ui: &mut FullscreenUi<'_>) {
    for _ in 0..100 {
        if app.drain_diff_task(ui).await.expect("drain diff task") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("diff task did not finish");
}

fn overlay_text(overlay: &DiffOverlay) -> String {
    overlay
        .lines
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn init_git_repo_for_diff_test(path: &Path) {
    let output = StdCommand::new("git")
        .arg("init")
        .current_dir(path)
        .output()
        .expect("git init");
    assert!(
        output.status.success(),
        "git init failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
