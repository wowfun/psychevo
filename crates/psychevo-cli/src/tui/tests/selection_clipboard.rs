#[test]
fn tui_mouse_capture_avoids_any_motion_tracking() {
    assert!(TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1000h"));
    assert!(TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1002h"));
    assert!(TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1006h"));
    assert!(TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1007h"));
    assert!(!TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1007l"));
    assert!(!TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1003h"));
}

#[test]
fn tui_mouse_capture_disable_restores_alternate_scroll() {
    assert!(TUI_MOUSE_CAPTURE_DISABLE_ANSI.contains("?1007l"));
    assert!(TUI_MOUSE_CAPTURE_DISABLE_ANSI.contains("?1006l"));
    assert!(TUI_MOUSE_CAPTURE_DISABLE_ANSI.contains("?1002l"));
    assert!(TUI_MOUSE_CAPTURE_DISABLE_ANSI.contains("?1000l"));
    assert!(!TUI_MOUSE_CAPTURE_DISABLE_ANSI.contains("?1003l"));
}

#[test]
fn fullscreen_enter_commands_enable_clean_alternate_screen() {
    let mut output = Vec::new();
    write_fullscreen_enter_commands(&mut output).expect("enter commands");
    let output = String::from_utf8(output).expect("utf8");
    assert!(output.contains("?1049h"));
    assert!(output.contains("?1000h"));
    assert!(output.contains("?1002h"));
    assert!(output.contains("?1006h"));
    assert!(output.contains("?1007h"));
    assert!(!output.contains("?1007l"));
    assert!(output.contains("\x1b[2J"));
    assert!(output.contains("\x1b[1;1H"));
}

#[test]
fn fullscreen_exit_commands_restore_terminal_modes() {
    let mut output = Vec::new();
    write_fullscreen_exit_commands(&mut output).expect("exit commands");
    let output = String::from_utf8(output).expect("utf8");
    assert!(output.contains("?1007l"));
    assert!(output.contains("?1006l"));
    assert!(output.contains("?1002l"));
    assert!(output.contains("?1000l"));
    assert!(output.contains("?1049l"));
    assert!(output.contains("?25h"));
}

#[test]
fn passive_mouse_motion_does_not_request_redraw() {
    assert!(!mouse_event_needs_redraw(MouseEventKind::Moved));
    assert!(mouse_event_needs_redraw(MouseEventKind::Drag(
        MouseButton::Left
    )));
    assert!(mouse_event_needs_redraw(MouseEventKind::ScrollUp));
}

#[test]
fn selection_extracts_text_from_registered_screen_lines() {
    let lines = vec![
        ScreenLine {
            region: SelectableRegion::Transcript,
            y: 1,
            cells: screen_cells_from_text(2, "hello world"),
        },
        ScreenLine {
            region: SelectableRegion::Transcript,
            y: 2,
            cells: screen_cells_from_text(2, "second line"),
        },
    ];
    let selection = SelectionState {
        anchor: Some((8, 1)),
        focus: Some((8, 2)),
        region: Some(SelectableRegion::Transcript),
    };

    assert_eq!(
        selected_text_from_lines(&lines, &selection).as_deref(),
        Some("world\nsecond")
    );
}

#[test]
fn selection_uses_rendered_wrapped_transcript_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Answer,
        "",
        "alpha beta gamma delta epsilon zeta".to_string(),
    ));

    draw_fullscreen_for_test(&app, &mut ui, 18, 8);

    let first = ui.screen_lines[0].text();
    let second = ui.screen_lines[1].text();
    ui.start_selection(0, ui.screen_lines[0].y);
    ui.update_selection(18, ui.screen_lines[1].y);

    assert_eq!(first, "alpha beta gamma");
    assert_eq!(second, "delta epsilon zeta");
    assert_eq!(ui.selected_text(), Some(format!("{first}\n{second}")));
}

#[test]
fn selection_preserves_wide_characters_from_rendered_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("中文测试abc".to_string());

    draw_fullscreen_for_test(&app, &mut ui, 24, 8);
    ui.start_selection(2, 0);
    ui.update_selection(10, 0);

    assert_eq!(ui.screen_lines[0].text(), "› 中文测试abc");
    assert_eq!(ui.selected_text().as_deref(), Some("中文测试"));
}

#[test]
fn selection_can_copy_sidebar_rendered_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    ui.refresh_sidebar(&app);

    draw_fullscreen_for_test(&app, &mut ui, 120, 10);

    let line = ui
        .screen_lines
        .iter()
        .find(|line| line.text() == "Modified Files")
        .expect("sidebar modified files line");
    let (x, y) = (line.first_x(), line.y);
    ui.start_selection(x, y);
    ui.update_selection(x + 14, y);

    assert_eq!(ui.selected_text().as_deref(), Some("Modified Files"));
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 10);
    let cell = buffer.cell((x, y)).expect("sidebar selected cell");
    assert!(cell.modifier.contains(Modifier::REVERSED));
    assert!(cell.modifier.contains(Modifier::BOLD));
}

#[test]
fn sidebar_omits_context_section_and_footer_chrome() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_mode = RunMode::Plan;
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    ui.refresh_sidebar(&app);

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 18);
    let text = buffer_text(&buffer);

    assert!(text.contains("Review sidebar polish"));
    assert!(text.contains("Modified Files"));
    for omitted in [
        "Context",
        "workdir:",
        "branch:",
        "messages:",
        "tool calls:",
        "tokens:",
        "context:",
        "cost:",
        "source: tui",
        "mode: plan",
        "Footer",
        "local facts only",
    ] {
        assert!(
            !text.contains(omitted),
            "sidebar should omit {omitted:?}:\n{text}"
        );
    }
}

#[test]
fn sidebar_render_clears_stale_terminal_cells() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    ui.sidebar_tokens = Some(36_019);
    ui.refresh_sidebar(&app);
    ui.sidebar.changed_files = vec![
        "?? .gitignore".to_string(),
        "?? .opencode/".to_string(),
        "?? .psychevo/".to_string(),
    ];

    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| {
            let lines = (0..24)
                .map(|_| Line::from("g".repeat(120)))
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(lines),
                Rect {
                    x: 0,
                    y: 0,
                    width: 120,
                    height: 24,
                },
            );
        })
        .expect("pollute frame");

    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    let text = buffer_text(&buffer);
    let sidebar_x = 120 - 42;

    assert!(text.contains("Modified Files"), "{text}");
    assert!(!text.contains("tokens:"), "{text}");
    assert!(!text.contains("gokens"), "{text}");
    assert_eq!(
        buffer
            .cell((sidebar_x + 4, 21))
            .expect("blank sidebar cell")
            .symbol(),
        " "
    );
}

#[test]
fn multiline_transcript_selection_ignores_same_row_sidebar_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Answer,
            "",
            "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau"
                .to_string(),
        ));
    ui.refresh_sidebar(&app);

    draw_fullscreen_for_test(&app, &mut ui, 120, 10);
    let transcript_rows = ui
        .screen_lines
        .iter()
        .filter(|line| line.region == SelectableRegion::Transcript)
        .take(2)
        .map(|line| (line.first_x(), line.y, line.text()))
        .collect::<Vec<_>>();
    assert_eq!(transcript_rows.len(), 2);
    let sidebar_row = ui
        .screen_lines
        .iter()
        .find(|line| line.region == SelectableRegion::Sidebar && line.y == transcript_rows[0].1)
        .map(|line| (line.first_x(), line.y, line.text()))
        .expect("same-row sidebar text");

    ui.start_selection(transcript_rows[0].0, transcript_rows[0].1);
    ui.update_selection(78, transcript_rows[1].1);
    let selected = ui.selected_text().expect("selected text");

    assert!(selected.contains("alpha beta gamma"));
    assert!(selected.contains("lambda"));
    assert!(
        !selected.contains(&sidebar_row.2),
        "selected text should not include same-row sidebar text: {selected:?}"
    );
    assert!(!selected.contains("Context"));

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 10);
    let sidebar_cell = buffer
        .cell((sidebar_row.0, sidebar_row.1))
        .expect("sidebar cell");
    assert!(!sidebar_cell.modifier.contains(Modifier::REVERSED));
    assert_ne!(sidebar_cell.bg, TUI_SELECTION_BG);
}

#[tokio::test]
async fn active_selection_highlights_rendered_buffer_and_esc_clears() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("copy me".to_string());
    ui.start_selection(2, 0);
    ui.update_selection(6, 0);

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 32, 8);
    let start = buffer.cell((2, 0)).expect("highlight start");
    assert!(start.modifier.contains(Modifier::REVERSED));
    assert!(start.modifier.contains(Modifier::BOLD));
    assert_ne!(start.bg, TUI_SELECTION_BG);
    let end = buffer.cell((5, 0)).expect("highlight end");
    assert!(end.modifier.contains(Modifier::REVERSED));
    assert!(end.modifier.contains(Modifier::BOLD));
    assert_ne!(end.bg, TUI_SELECTION_BG);
    let outside = buffer.cell((6, 0)).expect("outside highlight");
    assert!(!outside.modifier.contains(Modifier::REVERSED));

    let should_quit = app
        .handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc");

    assert!(!should_quit);
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 32, 8);
    assert!(
        !buffer
            .cell((2, 0))
            .expect("cleared")
            .modifier
            .contains(Modifier::REVERSED)
    );
}

#[test]
fn osc52_sequence_encodes_clipboard_text() {
    assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    assert_eq!(
        osc52_sequence_with_passthrough("hello", false).expect("osc52"),
        "\x1b]52;c;aGVsbG8=\x07"
    );
}

#[test]
fn osc52_sequence_encodes_cjk_clipboard_text_as_utf8() {
    assert_eq!(base64_encode("中文测试".as_bytes()), "5Lit5paH5rWL6K+V");
    assert_eq!(
        osc52_sequence_with_passthrough("中文测试", false).expect("osc52"),
        "\x1b]52;c;5Lit5paH5rWL6K+V\x07"
    );
}

#[test]
fn osc52_sequence_rejects_oversized_clipboard_payload() {
    let text = "x".repeat(100_001);

    assert!(osc52_sequence_with_passthrough(&text, false).is_err());
}

#[test]
fn wsl_clipboard_detection_uses_kernel_markers_without_env() {
    assert!(is_probably_wsl_from(
        Some("Linux version 6.6.87.2-microsoft-standard-WSL2"),
        None,
        false,
        false,
    ));
    assert!(is_probably_wsl_from(
        None,
        Some("6.6.87.2-microsoft-standard-WSL2"),
        false,
        false,
    ));
    assert!(!is_probably_wsl_from(
        Some("Linux version 6.6.87-generic"),
        Some("6.6.87-generic"),
        false,
        false,
    ));
}

#[test]
fn wsl_clipboard_candidates_try_powershell_then_clip_exe() {
    let candidates = local_clipboard_commands_for(false, false, true, true);

    assert_eq!(
        candidates.first().map(|candidate| candidate.command),
        Some("powershell.exe")
    );
    assert_eq!(
        candidates.get(1).map(|candidate| candidate.command),
        Some("clip.exe")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "wl-copy")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "xclip")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "xsel")
    );
}

#[test]
fn linux_wayland_clipboard_candidates_try_wl_copy_before_x11() {
    let candidates = local_clipboard_commands_for(false, false, false, true);

    assert_eq!(
        candidates.first().map(|candidate| candidate.command),
        Some("wl-copy")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "xclip")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "xsel")
    );
}

#[test]
fn linux_x11_clipboard_candidates_fall_back_to_xclip_and_xsel() {
    let candidates = local_clipboard_commands_for(false, false, false, false);

    assert_eq!(
        candidates.first().map(|candidate| candidate.command),
        Some("xclip")
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.command == "wl-copy")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "xsel")
    );
}

#[test]
fn clipboard_backend_reports_failure_when_all_backends_fail() {
    let candidates = local_clipboard_commands_for(false, false, true, false);
    let mut tried = Vec::new();

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: false,
            tmux_session: false,
        },
        candidates,
        |candidate, _| {
            tried.push(candidate.command);
            Ok(false)
        },
        |_| panic!("local clipboard fallback should not use tmux"),
        |_| Err(io::Error::other("osc blocked")),
    );

    let err = result.expect_err("clipboard failure");
    let message = err.to_string();
    assert_eq!(tried.first().copied(), Some("powershell.exe"));
    assert_eq!(tried.get(1).copied(), Some("clip.exe"));
    assert!(message.contains("powershell.exe unavailable"));
    assert!(message.contains("clip.exe unavailable"));
    assert!(message.contains("OSC52: osc blocked"));
}

#[test]
fn local_clipboard_emits_osc52_before_native_commands() {
    let calls = std::cell::RefCell::new(Vec::new());

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: false,
            tmux_session: false,
        },
        vec![ClipboardCommand {
            command: "remote-copy",
            args: NO_ARGS,
        }],
        |candidate, _| {
            calls.borrow_mut().push(candidate.command);
            Ok(true)
        },
        |_| panic!("local clipboard should not use tmux"),
        |text| {
            calls.borrow_mut().push("OSC52");
            assert_eq!(text, "hello");
            Ok(())
        },
    );

    assert!(result.is_ok());
    assert_eq!(calls.into_inner(), ["OSC52", "remote-copy"]);
}

#[test]
fn ssh_clipboard_skips_remote_native_commands_and_uses_osc52() {
    let mut local_calls = 0;
    let mut tmux_calls = 0;
    let mut osc_text = None;

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: true,
            tmux_session: false,
        },
        local_clipboard_commands_for(false, false, true, true),
        |_, _| {
            local_calls += 1;
            Ok(true)
        },
        |_| {
            tmux_calls += 1;
            Ok(())
        },
        |text| {
            osc_text = Some(text.to_string());
            Ok(())
        },
    );

    assert!(result.is_ok());
    assert_eq!(local_calls, 0);
    assert_eq!(tmux_calls, 0);
    assert_eq!(osc_text.as_deref(), Some("hello"));
}

#[test]
fn ssh_tmux_clipboard_emits_osc52_and_tmux_load_buffer() {
    let mut local_calls = 0;
    let mut tmux_text = None;
    let mut osc_text = None;

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: true,
            tmux_session: true,
        },
        local_clipboard_commands_for(false, false, false, false),
        |_, _| {
            local_calls += 1;
            Ok(true)
        },
        |text| {
            tmux_text = Some(text.to_string());
            Ok(())
        },
        |text| {
            osc_text = Some(text.to_string());
            Ok(())
        },
    );

    assert!(result.is_ok());
    assert_eq!(local_calls, 0);
    assert_eq!(osc_text.as_deref(), Some("hello"));
    assert_eq!(tmux_text.as_deref(), Some("hello"));
}

#[test]
fn ssh_tmux_clipboard_succeeds_when_tmux_fails_after_osc52() {
    let mut local_calls = 0;
    let mut tmux_calls = 0;
    let mut osc_text = None;

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: true,
            tmux_session: true,
        },
        local_clipboard_commands_for(false, false, false, false),
        |_, _| {
            local_calls += 1;
            Ok(true)
        },
        |_| {
            tmux_calls += 1;
            Err(io::Error::other("tmux unavailable"))
        },
        |text| {
            osc_text = Some(text.to_string());
            Ok(())
        },
    );

    assert!(result.is_ok());
    assert_eq!(local_calls, 0);
    assert_eq!(tmux_calls, 1);
    assert_eq!(osc_text.as_deref(), Some("hello"));
}

#[test]
fn ssh_tmux_clipboard_succeeds_when_osc52_fails_but_tmux_succeeds() {
    let mut local_calls = 0;
    let mut tmux_text = None;
    let mut osc_calls = 0;

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: true,
            tmux_session: true,
        },
        local_clipboard_commands_for(false, false, false, false),
        |_, _| {
            local_calls += 1;
            Ok(true)
        },
        |text| {
            tmux_text = Some(text.to_string());
            Ok(())
        },
        |_| {
            osc_calls += 1;
            Err(io::Error::other("osc blocked"))
        },
    );

    assert!(result.is_ok());
    assert_eq!(local_calls, 0);
    assert_eq!(osc_calls, 1);
    assert_eq!(tmux_text.as_deref(), Some("hello"));
}

#[test]
fn ssh_tmux_clipboard_reports_osc52_and_tmux_failures() {
    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: true,
            tmux_session: true,
        },
        local_clipboard_commands_for(false, false, false, false),
        |_, _| panic!("ssh clipboard should not use remote native commands"),
        |_| Err(io::Error::other("tmux unavailable")),
        |_| Err(io::Error::other("osc blocked")),
    );

    let message = result.expect_err("clipboard failure").to_string();
    assert!(message.contains("OSC52: osc blocked"));
    assert!(message.contains("tmux: tmux unavailable"));
}

#[test]
fn tmux_clipboard_ready_rejects_disabled_or_missing_forwarding() {
    assert!(tmux_clipboard_copy_ready(
        || Ok("external\n".to_string()),
        || Ok("193: Ms: (string) \\033]52;%p1%s;%p2%s\\a\n".to_string()),
    )
    .is_ok());
    assert_eq!(
        tmux_clipboard_copy_ready(
            || Ok("off\n".to_string()),
            || panic!("tmux info should not be queried when forwarding is disabled"),
        )
        .expect_err("disabled forwarding")
        .to_string(),
        "tmux clipboard forwarding is disabled"
    );
    assert_eq!(
        tmux_clipboard_copy_ready(
            || Ok("external\n".to_string()),
            || Ok("193: Ms: [missing]\n".to_string()),
        )
        .expect_err("missing Ms")
        .to_string(),
        "tmux clipboard forwarding is unavailable: missing Ms capability"
    );
}

#[tokio::test]
async fn mouse_drag_copies_selected_text_through_clipboard_sink() {
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
    ui.push_user("copy this line".to_string());
    draw_fullscreen_for_test(&app, &mut ui, 48, 10);

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 6,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse drag");
    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 6,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse up");

    assert_eq!(app.clipboard_copies_in_flight, 1);
    wait_for_clipboard_task(&mut app, &mut ui).await;
    assert_eq!(copied.lock().expect("clipboard lock").as_slice(), ["copy"]);
    assert_eq!(ui.selection, SelectionState::default());
}

#[tokio::test]
async fn mouse_up_clipboard_failure_clears_selection_without_quitting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.clipboard = Arc::new(|_| Err(io::Error::other("blocked")));
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("copy this line".to_string());
    draw_fullscreen_for_test(&app, &mut ui, 48, 10);

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 6,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse drag");
    let should_quit = app
        .handle_fullscreen_mouse(
            &mut ui,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 6,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
        )
        .await
        .expect("mouse up");

    assert!(!should_quit);
    assert_eq!(ui.selection, SelectionState::default());
    assert_eq!(app.clipboard_copies_in_flight, 1);
    wait_for_clipboard_task(&mut app, &mut ui).await;
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Error && row.text.contains("copy failed: blocked")
    }));
}

async fn wait_for_clipboard_task(app: &mut TuiApp, ui: &mut FullscreenUi<'_>) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if app.drain_finished_clipboard_copies(ui) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("clipboard task should finish");
}

#[tokio::test]
async fn ctrl_c_copies_active_selection_without_quitting() {
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
    ui.push_screen_line(0, 0, "selected text");
    ui.start_selection(0, 0);
    ui.update_selection(8, 0);

    let should_quit = app
        .handle_fullscreen_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        )
        .await
        .expect("ctrl-c");

    assert!(!should_quit);
    assert!(!ui.quit_requested);
    assert_eq!(
        copied.lock().expect("clipboard lock").as_slice(),
        ["selected"]
    );
    assert_eq!(ui.selection, SelectionState::default());
}

#[tokio::test]
async fn clipboard_failure_during_ctrl_c_is_consumed_without_quitting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.clipboard = Arc::new(|_| Err(io::Error::other("blocked")));
    let mut ui = FullscreenUi::new(&app);
    ui.push_screen_line(0, 0, "selected text");
    ui.start_selection(0, 0);
    ui.update_selection(8, 0);

    let should_quit = app
        .handle_fullscreen_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        )
        .await
        .expect("ctrl-c");

    assert!(!should_quit);
    assert!(!ui.quit_requested);
    assert_eq!(ui.selection, SelectionState::default());
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Error && row.text.contains("copy failed: blocked")
    }));
}
