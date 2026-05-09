fn draw_fullscreen_for_test(
    app: &TuiApp,
    ui: &mut FullscreenUi<'_>,
    width: u16,
    height: u16,
) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, ui))
        .expect("draw");
    terminal.backend().buffer().clone()
}

async fn drain_fullscreen_until_idle(app: &mut TuiApp, ui: &mut FullscreenUi<'_>) {
    for _ in 0..200 {
        app.drain_fullscreen_events(ui).await.expect("drain events");
        if ui.running.is_none() && ui.queued_inputs.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("fullscreen work did not become idle");
}

fn test_app(temp: &tempfile::TempDir) -> TuiApp {
    let home = temp.path().join("home");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&workdir).expect("workdir");
    let workdir = workdir.canonicalize().expect("canonical");
    let (clipboard_result_tx, clipboard_result_rx) = std::sync::mpsc::channel();
    TuiApp {
        env_map: BTreeMap::new(),
        home: home.clone(),
        state_path: home.join("tui-state.json"),
        state: TuiState::default(),
        db_path: home.join("state.db"),
        config_path: None,
        workdir: workdir.clone(),
        workdir_key: workdir.display().to_string(),
        current_session: Some("1234567890abcdef".to_string()),
        current_session_title: Some("Review sidebar polish".to_string()),
        force_new_once: false,
        current_model: Some("mock/model".to_string()),
        current_variant: Some("high".to_string()),
        selected_model: None,
        current_mode: RunMode::Build,
        no_skills: false,
        skill_inputs: Vec::new(),
        thinking_visible: true,
        clipboard: Arc::new(|_| Ok(())),
        renderer: TuiRenderer::new(false),
        debug: false,
        had_error: false,
        model_catalog: ModelCatalogCache::default(),
        clipboard_result_tx,
        clipboard_result_rx,
        clipboard_copies_in_flight: 0,
    }
}

#[derive(Debug, Clone, Copy)]
enum FixtureKind {
    Idle,
    RunningThinking,
    CollapsedTool,
    ExpandedTool,
    DebugMeta,
    FailureMeta,
}

fn fixture_ui<'a>(app: &TuiApp, kind: FixtureKind) -> FullscreenUi<'a> {
    let mut ui = FullscreenUi::new(app);
    ui.sidebar = stable_sidebar();
    match kind {
        FixtureKind::Idle => {}
        FixtureKind::RunningThinking => {
            ui.transcript.clear();
            ui.push_user("Inspect the CLI rendering path.".to_string());
            ui.start_assistant();
            ui.apply_value_event(
                &serde_json::json!({
                    "type": "run_start",
                    "provider": "mock",
                    "model": "mock-model",
                    "mode": "default",
                    "context_limit": 64000
                }),
                false,
            );
            ui.turn_started = None;
            ui.apply_stream_event(
                RunStreamEvent::ReasoningDelta {
                    text: "Read the TUI renderer and identify stable evidence blocks.".to_string(),
                },
                true,
                false,
            );
            ui.running_elapsed_override = Some(Duration::from_secs(12));
            ui.transcript.push(TranscriptRow::with_title(
                TranscriptKind::Explored,
                "Explored crates/psychevo-cli/src/tui.rs",
                "running",
            ));
        }
        FixtureKind::CollapsedTool | FixtureKind::ExpandedTool => {
            ui.transcript.clear();
            push_completed_turn(&mut ui, kind);
        }
        FixtureKind::DebugMeta => {
            ui.transcript.clear();
            push_completed_turn(&mut ui, kind);
            ui.sidebar_hidden = true;
        }
        FixtureKind::FailureMeta => {
            ui.transcript.clear();
            push_failure_turn(&mut ui);
        }
    }
    ui.sidebar = stable_sidebar();
    ui
}

fn stable_sidebar() -> SidebarSnapshot {
    SidebarSnapshot {
        title: "Review sidebar polish".to_string(),
        session: "12345678".to_string(),
        workdir: "/repo/psychevo".to_string(),
        branch: "main".to_string(),
        tokens: Some(12_000),
        context_percent: Some(18.8),
        message_count: 2,
        tool_count: 1,
        changed_files: vec![
            "M crates/psychevo-cli/src/tui.rs".to_string(),
            "?? specs/210-pevo-tui/testing.md".to_string(),
        ],
    }
}

fn stable_session_bottom_panel() -> BottomSelectionPanel {
    BottomSelectionPanel::new_sessions(
        SessionListView::Active,
        vec![
            BottomSelectionRow {
                label: "Implement model picker".to_string(),
                description: Some("mock/mock-model  messages=5".to_string()),
                detail: Some("12:10".to_string()),
                group: Some("2026-05-06".to_string()),
                search_text: "session-a Implement model picker mock mock-model tui".to_string(),
                is_current: true,
                is_default: false,
                style: BottomRowStyle::Normal,
                footer: None,
                value: BottomSelectionValue::Session("session-a".to_string()),
            },
            BottomSelectionRow {
                label: "Review session pane".to_string(),
                description: Some("mock/other-model  messages=3".to_string()),
                detail: Some("09:44".to_string()),
                group: Some("2026-05-05".to_string()),
                search_text: "session-b Review session pane mock other-model run".to_string(),
                is_current: false,
                is_default: false,
                style: BottomRowStyle::Normal,
                footer: None,
                value: BottomSelectionValue::Session("session-b".to_string()),
            },
        ],
    )
}

fn stable_archived_session_bottom_panel() -> BottomSelectionPanel {
    BottomSelectionPanel::new_sessions(
        SessionListView::Archived,
        vec![BottomSelectionRow {
            label: "Archived refactor branch".to_string(),
            description: Some("mock/mock-model  messages=7".to_string()),
            detail: Some("18:22".to_string()),
            group: Some("2026-05-01".to_string()),
            search_text: "session-archived Archived refactor branch mock mock-model tui"
                .to_string(),
            is_current: false,
            is_default: false,
            style: BottomRowStyle::Normal,
            footer: None,
            value: BottomSelectionValue::Session("session-archived".to_string()),
        }],
    )
}

fn push_completed_turn(ui: &mut FullscreenUi<'_>, kind: FixtureKind) {
    ui.push_user("Summarize the TUI snapshot harness.".to_string());
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "Check layout boundaries, style roles, and expandable evidence.",
    ));
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Explored,
        "Explored crates/psychevo-cli/src/tui.rs",
        long_tool_output()
            .lines()
            .take(collapsed_fixture_lines(kind))
            .collect::<Vec<_>>()
            .join("\n")
            + &format!("\n... {} more lines", 24 - collapsed_fixture_lines(kind)),
    );
    row.full_text = Some(long_tool_output());
    if matches!(kind, FixtureKind::ExpandedTool) {
        row.expanded = true;
        ui.focus = FocusMode::Transcript;
        ui.selected_row = Some(2);
        ui.auto_follow_transcript = false;
    }
    ui.transcript.push(row);
    ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Answer,
            "",
            "The harness snapshots stable buffer text and style roles, then leaves real terminal screenshots as diagnostics.",
        ));
    let debug = matches!(kind, FixtureKind::DebugMeta);
    let usage = if debug {
        serde_json::json!({
            "input_tokens": 120,
            "total_tokens": 177
        })
    } else {
        serde_json::json!({
        "input_tokens": 120,
        "output_tokens": 45,
        "reasoning_tokens": 12,
        "total_tokens": 177
        })
    };
    let metadata = if debug {
        serde_json::json!({
            "elapsed_ms": 2500,
            "provider_response_id": "resp_snapshot",
            "reasoning_effort": "high"
        })
    } else {
        serde_json::json!({
            "elapsed_ms": 2500,
            "provider_response_id": "resp_snapshot",
            "reasoning_effort": "high",
            "system_fingerprint": "fp_mock"
        })
    };
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Meta,
        "",
        turn_meta_text(TurnMetaProjection {
            mode: "default",
            provider: "mock",
            model: "mock-model",
            started: None,
            usage: Some(&usage),
            metadata: Some(&metadata),
            failures: 0,
            debug,
        }),
    ));
}

fn push_failure_turn(ui: &mut FullscreenUi<'_>) {
    ui.push_user("Run a command that fails.".to_string());
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran cargo test -p psychevo-cli",
        "exit_code=101\ncompile error: fixture failure",
    );
    row.failed = true;
    ui.transcript.push(row);
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Answer,
        "",
        "The run failed before producing a clean validation result.",
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Meta,
        "",
        "mock/mock-model  1 failure",
    ));
}

fn long_tool_output() -> String {
    (1..=24)
        .map(|line| format!("{line:02}: crates/psychevo-cli/src/tui.rs evidence row"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collapsed_fixture_lines(kind: FixtureKind) -> usize {
    match kind {
        FixtureKind::ExpandedTool => 20,
        _ => 4,
    }
}

fn assert_tui_snapshot(
    name: &str,
    width: u16,
    height: u16,
    app: &TuiApp,
    mut ui: FullscreenUi<'_>,
) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let text = buffer_text(buffer);
    let styles = buffer_style_text(buffer);
    let combined = format!(
        "fixture={name}\nsize={width}x{height}\n\n--- text ---\n{text}\n--- styles ---\n{styles}"
    );
    write_snapshot_diagnostics(name, &text, &styles, &combined);
    let snapshot_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/snapshots");
    insta::with_settings!({ prepend_module_to_snapshot => false, snapshot_path => snapshot_path }, {
        insta::assert_snapshot!(name, combined);
    });
}

fn write_snapshot_diagnostics(name: &str, text: &str, styles: &str, combined: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pevo-tui-snapshots")
        .join(name);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = fs::write(dir.join("text.txt"), text);
    let _ = fs::write(dir.join("styles.txt"), styles);
    let _ = fs::write(dir.join("combined.txt"), combined);
    let _ = fs::write(
        dir.join("metadata.json"),
        serde_json::json!({
            "fixture": name,
            "source": "ratatui TestBackend",
            "golden": "insta snapshot"
        })
        .to_string(),
    );
}

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    let area = *buffer.area();
    let mut text = String::new();
    for y in area.y..area.y + area.height {
        let mut line = String::new();
        for x in area.x..area.x + area.width {
            line.push_str(buffer.cell((x, y)).expect("cell").symbol());
        }
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

fn buffer_style_text(buffer: &ratatui::buffer::Buffer) -> String {
    let area = *buffer.area();
    let mut text = String::new();
    for y in area.y..area.y + area.height {
        let mut line = String::new();
        let mut last = None;
        for x in area.x..area.x + area.width {
            let cell = buffer.cell((x, y)).expect("cell");
            if last != Some(cell.fg) {
                last = Some(cell.fg);
                line.push_str(style_marker(cell.fg));
            }
            line.push_str(cell.symbol());
        }
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

fn style_marker(color: Color) -> &'static str {
    if color == TUI_MAGENTA || color == Color::Magenta {
        "[magenta]"
    } else if color == TUI_CYAN || color == Color::Cyan {
        "[cyan]"
    } else if color == Color::Green {
        "[green]"
    } else if color == TUI_RED || color == Color::Red {
        "[red]"
    } else if color == TUI_DIM || color == Color::DarkGray {
        "[dim]"
    } else if color == TUI_PAPER {
        "[paper]"
    } else {
        "[default]"
    }
}
