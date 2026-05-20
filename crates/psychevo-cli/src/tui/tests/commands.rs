#[tokio::test]
async fn mode_slash_command_requires_value() {
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
            && row.text.contains(
                "error: usage: /mode <plan|default|acceptEdits|dontAsk|bypassPermissions>",
            )
    }));
}

#[tokio::test]
async fn mode_slash_command_sets_mode_with_direct_value() {
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
async fn show_raw_toggles_persists_and_does_not_append_transcript_status() {
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
async fn show_raw_rejects_invalid_arguments_through_slash_entry() {
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
async fn copy_command_copies_latest_answer_raw_markdown_without_transcript_row() {
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
async fn ctrl_o_copies_latest_answer_raw_markdown() {
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
async fn configured_slash_alias_and_leader_shortcut_dispatch_commands() {
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
async fn configured_slash_shortcuts_do_not_fire_while_editing_or_in_panel() {
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
async fn fullscreen_status_uses_single_multiline_command_row() {
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
async fn fullscreen_help_command_opens_bottom_help_panel() {
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
    assert!(text.contains("last-provider-request can expose hidden prompts"));
    assert!(text.contains("No custom commands available"));
    assert!(!text.contains("pevo run"));
}

#[tokio::test]
async fn fullscreen_help_custom_commands_show_configured_slash_targets() {
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
async fn fullscreen_help_bottom_panel_switches_sections_and_closes() {
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
async fn fullscreen_help_rejects_arguments_and_stats_alias_opens_usage() {
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
async fn fullscreen_usage_command_opens_bottom_panel() {
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
fn usage_panel_groups_persisted_stats_rows() {
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
        ) VALUES (?1, 2, 'tool_result', 2, '{"role":"tool_result"}', 'bash')
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
            .any(|row| row.group.as_deref() == Some("Top tools") && row.label == "bash")
    );
    assert!(panel.rows.iter().any(|row| {
        row.group.as_deref() == Some("Top sessions") && row.label == "Usage session"
    }));
}

#[tokio::test]
async fn fullscreen_context_command_appends_compact_command_row() {
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
async fn fullscreen_context_command_refreshes_bottom_status_snapshot() {
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
async fn fullscreen_variant_and_upcoming_feedback_use_command_rows() {
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
async fn fullscreen_compact_queues_behind_running_turn() {
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
        } => {
            assert_eq!(session_id.as_deref(), Some(session.as_str()));
            assert_eq!(instructions.as_deref(), Some("focus on todos"));
            assert_eq!(command_echo, "/compact focus on todos");
        }
        other => panic!("unexpected queued input: {other:?}"),
    }
}

#[tokio::test]
async fn fullscreen_export_and_share_write_artifacts() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.workdir,
            "tui",
            "mock-model",
            "mock",
            Some(serde_json::json!({
                "base_url": "https://example.test/v1",
                "mode": "default",
                "model_metadata": {
                    "capabilities": {
                        "tool_call": true
                    }
                }
            })),
        )
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &session_id,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "export this prompt"}],
            "timestamp_ms": 1
        }),
    );
    insert_tui_message(
        &conn,
        &session_id,
        2,
        "assistant",
        2,
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "exported answer"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Export(crate::tui::slash::TuiExportOptions {
            path: Some("exports/session.md".to_string()),
            format: SessionExportFormat::Markdown,
            include: psychevo_runtime::SessionExportIncludeSet::default_for(
                SessionArtifactKind::Export,
            ),
        }),
    )
    .await
    .expect("export");

    let export_path = app.workdir.join("exports/session.md");
    let content = fs::read_to_string(&export_path).expect("export content");
    assert!(content.contains("export this prompt"));
    assert!(content.contains("exported answer"));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/export exports/session.md"
            && row.text.contains("exported:")
            && row.text.contains("exports/session.md")
    }));

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Export(crate::tui::slash::TuiExportOptions {
            path: Some("exports/session.json".to_string()),
            format: SessionExportFormat::Json,
            include: psychevo_runtime::SessionExportIncludeSet::parse(
                "last-provider-request",
                SessionArtifactKind::Export,
            )
            .unwrap(),
        }),
    )
    .await
    .expect("export last request json");

    let last_export_path = app.workdir.join("exports/session.json");
    let content = fs::read_to_string(&last_export_path).expect("last request export content");
    let value: Value = serde_json::from_str(&content).expect("last request export json");
    assert_eq!(
        value["last_provider_request"]["body"]["messages"][1]["content"],
        "export this prompt"
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title
                == "/export exports/session.json --format json --include last-provider-request"
            && row.text.contains("exported:")
            && row.text.contains("exports/session.json")
    }));

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Share(crate::tui::slash::TuiShareOptions {
            path: Some("share.md".to_string()),
            include: psychevo_runtime::SessionExportIncludeSet::default_for(
                SessionArtifactKind::Share,
            ),
        }),
    )
    .await
    .expect("share");

    let share_path = app.workdir.join("share.md");
    let content = fs::read_to_string(&share_path).expect("share content");
    assert!(content.contains("exported answer"));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/share share.md"
            && row.text.contains("share:")
            && row.text.contains("share.md")
    }));
}

#[tokio::test]
async fn fullscreen_skills_command_lists_dynamic_entries_and_inserts_skill_marker() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    fs::create_dir_all(app.workdir.join(".git")).expect("git marker");
    let skill_dir = app.home.join("skills").join("helper");
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: helper\ndescription: Helps with focused edits\n---\n\nFollow the helper workflow.\n",
    )
    .expect("skill");

    let matches = app.slash_menu_items("/skill:h");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].command, "/skill:helper");

    let mut ui = FullscreenUi::new(&app);
    app.handle_fullscreen_command(&mut ui, SlashCommand::Skills)
        .await
        .expect("skills");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/skills"
            && row.text.contains("helper: Helps with focused edits")
    }));

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::SkillInvoke {
            name: "helper".to_string(),
            args: "apply it to src/lib.rs".to_string(),
        },
    )
    .await
    .expect("skill invoke");
    assert_eq!(
        textarea_text(&ui.textarea),
        "$helper apply it to src/lib.rs"
    );
}

fn test_context_snapshot() -> ContextSnapshot {
    let mut categories = BTreeMap::new();
    categories.insert(
        "base_policy".to_string(),
        ContextCategory {
            label: "Base policy".to_string(),
            tokens: 10,
            estimated: true,
            status: "estimated".to_string(),
            percent: Some(10.0),
            details: Value::Null,
        },
    );
    categories.insert(
        "history".to_string(),
        ContextCategory {
            label: "History".to_string(),
            tokens: 40,
            estimated: true,
            status: "estimated".to_string(),
            percent: Some(40.0),
            details: serde_json::json!({
                "roles": {"user": {"count": 1, "tokens": 40}},
            }),
        },
    );
    categories.insert(
        "free_space".to_string(),
        ContextCategory {
            label: "Free space".to_string(),
            tokens: 50,
            estimated: true,
            status: "derived".to_string(),
            percent: Some(50.0),
            details: Value::Null,
        },
    );
    ContextSnapshot {
        event_type: "context_snapshot".to_string(),
        scope: ContextScope::LastProviderRequest,
        status: "estimated".to_string(),
        session_id: Some("session".to_string()),
        provider: "mock".to_string(),
        model: "model".to_string(),
        mode: Some("default".to_string()),
        context_limit: Some(100),
        tokenizer: ContextTokenizer {
            encoding: "o200k_base".to_string(),
            source: "fallback".to_string(),
            fallback: true,
        },
        total: ContextTotal {
            tokens: 50,
            estimated_tokens: 50,
            estimated: true,
            source: "estimate".to_string(),
            percent: Some(50.0),
        },
        categories,
        advice: Vec::new(),
    }
}

#[tokio::test]
async fn fullscreen_undo_restores_prompt_and_redo_restores_transcript() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&app.workdir)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = app.workdir.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("session");
    app.current_session = Some(session_id.clone());

    let before_first = test_track_snapshot(&app, &session_id);
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        1,
        "user",
        "first prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "first prompt"}],
            "timestamp_ms": 1
        }),
        Some(serde_json::json!({"undo": {"pre_snapshot": before_first}})),
    );
    fs::write(&file, "after first\n").expect("after first");
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        2,
        "assistant",
        "first answer",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "first answer"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
    );
    let before_second = test_track_snapshot(&app, &session_id);
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        3,
        "user",
        "second prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "second prompt"}],
            "timestamp_ms": 3
        }),
        Some(serde_json::json!({"undo": {"pre_snapshot": before_second}})),
    );
    fs::write(&file, "after second\n").expect("after second");
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        4,
        "assistant",
        "second answer",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "second answer"}],
            "timestamp_ms": 4,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui)
        .expect("load history");
    ui.textarea = textarea_with_text("/undo");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("undo");

    assert_eq!(textarea_text(&ui.textarea), "second prompt");
    assert_eq!(fs::read_to_string(&file).expect("file"), "after first\n");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "first answer")
    );
    assert!(ui.transcript.iter().all(|row| row.text != "second answer"));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/undo"
            && row.text.contains("prompt restored")
    }));

    ui.textarea = textarea_with_text("/redo");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("redo");

    assert_eq!(textarea_text(&ui.textarea), "");
    assert_eq!(fs::read_to_string(&file).expect("file"), "after second\n");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "second answer")
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command && row.title == "/redo" && row.text.contains("redone")
    }));
}

#[tokio::test]
async fn fullscreen_new_command_resets_session_without_status_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("previous prompt".to_string());

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");

    assert_eq!(app.current_session, None);
    assert_eq!(app.current_session_title, None);
    assert!(app.force_new_once);
    assert!(ui.transcript.is_empty());
    assert!(ui.terminal_clear_requested);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Status)
    );
}

#[tokio::test]
async fn fullscreen_reload_context_points_to_refresh() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ReloadContextDeprecated)
        .await
        .expect("reload deprecated");

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/reload-context"
            && row.failed
            && row.text.contains("use /refresh")
    }));
}

#[tokio::test]
async fn fullscreen_btw_opens_hidden_side_and_ctrl_c_deletes_it() {
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
        .btw_side
        .as_ref()
        .expect("side state")
        .side_session
        .clone();
    assert_eq!(app.current_session.as_deref(), Some(side.as_str()));
    assert!(ui.transcript.is_empty());
    assert!(
        store
            .list_sessions_for_workdir_with_sources(&app.workdir, TUI_SESSION_SOURCES)
            .expect("sessions")
            .iter()
            .all(|summary| summary.id != side)
    );

    app.handle_fullscreen_command(&mut ui, SlashCommand::Refresh)
        .await
        .expect("refresh rejected");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/refresh"
            && row.failed
            && row
                .text
                .contains("unavailable inside a /btw side conversation")
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
            && row
                .text
                .contains("unavailable inside a /btw side conversation")
    }));

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    )
    .await
    .expect("ctrl-c");

    assert_eq!(app.current_session.as_deref(), Some(parent.as_str()));
    assert!(app.btw_side.is_none());
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
async fn fullscreen_btw_detaches_running_parent() {
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
        app.btw_parent_status_label(&ui)
            .is_some_and(|label| label.contains("main running"))
    );

    for task in &ui.auxiliary_agent_tasks {
        task.control.abort();
    }
}

#[tokio::test]
async fn fullscreen_refresh_cleans_orphan_side_sessions() {
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
            TUI_SIDE_SESSION_SOURCE,
            "mock-model",
            "mock",
            Some(serde_json::json!({BTW_SIDE_METADATA_KEY: {"ephemeral": true}})),
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
async fn mouse_click_can_execute_slash_menu_row() {
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
async fn mouse_wheel_scrolls_transcript_inside_tui() {
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
async fn mouse_wheel_in_transcript_does_not_recall_composer_history() {
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
async fn mouse_wheel_in_composer_or_status_is_ignored() {
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
async fn mouse_wheel_routes_between_bottom_panel_and_transcript_by_hover() {
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
async fn empty_composer_down_without_active_history_does_not_scroll_or_recall() {
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
async fn empty_composer_up_recalls_latest_prompt_without_scrolling_transcript() {
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
async fn non_empty_composer_up_recalls_prompt_history_and_down_restores_draft() {
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
