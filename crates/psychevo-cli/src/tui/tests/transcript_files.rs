#[test]
fn transcript_selection_toggles_expandable_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored log", "a");
    row.full_text = Some("a\nb\nc".to_string());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.selected_target = Some(TranscriptHitTarget::Row(ui.transcript[0].id));
    ui.toggle_selected();
    assert!(ui.transcript[0].expanded);
    ui.toggle_selected();
    assert!(!ui.transcript[0].expanded);
}

#[test]
fn transcript_render_blocks_keep_consecutive_tools_flat() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    for index in 0..4 {
        ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Ran,
            format!("Ran command {index}"),
            "ok",
        ));
    }

    let blocks = transcript_render_blocks(&ui);
    assert_eq!(blocks.len(), 4);
    for (index, block) in blocks.iter().enumerate() {
        assert_eq!(block.index, index);
        assert_eq!(block.target, TranscriptHitTarget::Row(ui.transcript[index].id));
    }
}

#[test]
fn transcript_render_blocks_keep_thinking_and_tools_flat() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "inspect context",
    ));
    ui.transcript
        .push(TranscriptRow::with_title(TranscriptKind::Ran, "Ran ls", "ok"));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Updated,
        "Updated report.md",
        "write normal",
    ));
    ui.transcript.push(TranscriptRow::simple(
        TranscriptKind::Answer,
        "finished",
    ));

    let blocks = transcript_render_blocks(&ui);
    assert_eq!(blocks.len(), 4);
    assert_eq!(blocks[0].index, 0);
    assert_eq!(blocks[1].index, 1);
    assert_eq!(blocks[2].index, 2);
    assert_eq!(blocks[3].index, 3);
}

#[test]
fn bash_timeout_failure_shows_timeout_before_partial_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_fetch",
            "tool_name": "bash",
            "args": {"command": "python scripts/fetch.py"},
            "result": {
                "output": "[fetch] 29 rows done\n[fetch] 1 failed",
                "exit_code": null,
                "error": "command timed out after 120 seconds",
                "truncated": false
            },
            "outcome": "failed"
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("bash row");
    assert!(row.failed);
    assert_eq!(row.title, "bash python scripts/fetch.py");
    assert!(row.text.starts_with(
        "timeout: command timed out after 120 seconds; partial output follows\n"
    ));
    assert!(row.text.contains("[fetch] 29 rows done"));
}

#[test]
fn bash_timeout_without_output_omits_no_output_placeholder() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_sleep",
            "tool_name": "bash",
            "args": {"command": "sleep 60"},
            "result": {
                "output": "(no output)",
                "exit_code": null,
                "error": "command timed out after 1 seconds",
                "truncated": false
            },
            "outcome": "failed"
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("bash row");
    assert_eq!(row.text, "timeout: command timed out after 1 seconds");
    assert!(!row.text.contains("(no output)"));
}

#[test]
fn command_row_renders_claude_style_prefixes_and_colored_context_bar() {
    let row = TranscriptRow::with_title(
        TranscriptKind::Command,
        "/context",
        "Context Usage\n[HHHHH.....]\nB base  D developer  P project  H history  C turn  U prompt  T tools  . free\ntokens: 5/10 (50.0%)",
    );

    let lines = transcript_lines(&row, false, true, 80, Path::new("/repo"), false);
    let rendered = lines.iter().map(line_text).collect::<Vec<_>>();
    assert_eq!(rendered[0], "> /context");
    assert_eq!(rendered[1], "  └  Context Usage");
    assert_eq!(rendered[2], "     [HHHHH.....]");
    assert_eq!(
        rendered[3],
        "     B base  D developer  P project  H history  C turn  U prompt  T tools  . free"
    );
    assert_eq!(rendered[4], "     tokens: 5/10 (50.0%)");

    let bar_h_style = lines[2].spans[6].style;
    let legend_h_style = lines[3].spans[10].style;
    let legend_label_style = lines[3].spans[11].style;
    assert_eq!(bar_h_style, legend_h_style);
    assert_ne!(legend_h_style, legend_label_style);
    assert_eq!(legend_label_style, tui_theme().dim_style());
    assert_eq!(lines[3].spans[11].content.as_ref(), " history");
}

#[test]
fn command_row_defaults_open_and_toggles_details() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Command,
        "/status",
        "workdir: /repo\nmodel: mock/model",
    );

    assert!(row.is_expandable());
    let open = transcript_lines(&row, true, true, 80, Path::new("/repo"), false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert_eq!(open[0], "> /status ▾ collapse");
    assert!(open.iter().any(|line| line.contains("workdir: /repo")));

    toggle_transcript_row_details(&mut row);
    assert!(row.details_collapsed);
    let collapsed = transcript_lines(&row, false, true, 80, Path::new("/repo"), false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert_eq!(collapsed, ["> /status ▸ details"]);

    toggle_transcript_row_details(&mut row);
    assert!(!row.details_collapsed);
    let reopened = transcript_lines(&row, true, true, 80, Path::new("/repo"), false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert!(reopened.iter().any(|line| line.contains("model: mock/model")));
}

#[test]
fn command_rows_do_not_count_as_visible_messages() {
    let rows = vec![
        TranscriptRow::with_title(TranscriptKind::Command, "/status", "workdir: /repo"),
        TranscriptRow::with_title(TranscriptKind::Prompt, "", "hello"),
        TranscriptRow::with_title(TranscriptKind::Answer, "", "hi"),
    ];

    assert_eq!(visible_transcript_message_count(&rows), 2);
}

#[test]
fn long_thinking_defaults_to_row_level_collapse_without_left_rail() {
    let long = (1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let row = TranscriptRow::with_title(TranscriptKind::Thinking, "Thinking", long);
    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert!(row.text.contains("... 6 more lines"));

    let lines = thinking_lines(&row, false, true, 80);
    let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
    assert!(rendered.contains("• Thinking"), "{rendered}");
    assert!(rendered.contains("▸ 6 more lines"), "{rendered}");
    assert!(rendered.contains("  └ line 1"), "{rendered}");
    assert!(rendered.contains("... 6 more lines"), "{rendered}");
    assert!(rendered.contains("line 12"), "{rendered}");
    assert!(!rendered.contains("▌"), "{rendered}");
    assert!(!rendered.contains("Thinking:"), "{rendered}");
}

#[test]
fn long_thinking_can_expand_then_collapse_details() {
    let long = (1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut row = TranscriptRow::with_title(TranscriptKind::Thinking, "Thinking", long);

    toggle_transcript_row_details(&mut row);
    assert!(row.expanded);
    assert!(!row.details_collapsed);
    let expanded = thinking_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(expanded.contains("▾ collapse"), "{expanded}");
    assert!(expanded.contains("line 3"), "{expanded}");

    toggle_transcript_row_details(&mut row);
    assert!(!row.expanded);
    assert!(row.details_collapsed);
    let collapsed = thinking_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(collapsed.contains("▸ details"), "{collapsed}");
    assert!(!collapsed.contains("line 1"), "{collapsed}");
    assert!(!collapsed.contains("... 6 more lines"), "{collapsed}");

    toggle_transcript_row_details(&mut row);
    assert!(!row.expanded);
    assert!(!row.details_collapsed);
    let preview = thinking_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(preview.contains("▸ 6 more lines"), "{preview}");
    assert!(preview.contains("line 1"), "{preview}");
}

#[test]
fn active_thinking_row_uses_activity_marker_and_elapsed() {
    let mut row = TranscriptRow::with_title(TranscriptKind::Thinking, "Thinking", "working");
    row.tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_millis(2_500))
            .expect("instant"),
    );

    let line = thinking_lines(&row, false, true, 80)
        .into_iter()
        .next()
        .expect("thinking row");
    let marker = line.spans[0].content.as_ref();
    assert!(marker.ends_with(' '), "{marker}");
    assert_ne!(marker, "◦ ");
    let rendered = line_text(&line);
    assert!(rendered.contains("Thinking"), "{rendered}");
    assert!(rendered.contains("2s"), "{rendered}");
}

#[test]
fn completed_thinking_row_does_not_show_elapsed() {
    let mut row = TranscriptRow::with_title(TranscriptKind::Thinking, "Thinking", "done");
    row.tool_elapsed = Some(Duration::from_secs(4));

    let line = thinking_lines(&row, false, true, 80)
        .into_iter()
        .next()
        .expect("thinking row");
    let rendered = line_text(&line);
    assert!(rendered.contains("Thinking"), "{rendered}");
    assert!(!rendered.contains("4s"), "{rendered}");
}

#[test]
fn completed_thinking_row_uses_shared_evidence_title_styles() {
    let thinking = TranscriptRow::with_title(TranscriptKind::Thinking, "Thinking", "");
    let tool = TranscriptRow::with_title(TranscriptKind::Ran, "Ran ls", "");
    let thinking = thinking_lines(&thinking, false, true, 80)
        .into_iter()
        .next()
        .expect("thinking row");
    let tool = tool_lines(&tool, false, true, 80)
        .into_iter()
        .next()
        .expect("tool row");

    assert_eq!(thinking.spans[0].style, tool.spans[0].style);
    assert_eq!(thinking.spans[1].style, tool.spans[1].style);
    assert_eq!(
        thinking.spans[1].style,
        Style::default().add_modifier(Modifier::BOLD)
    );
}

#[test]
fn short_thinking_row_can_collapse_details() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "first thought\nsecond thought",
    );
    assert!(row.is_expandable());

    let open = thinking_lines(&row, true, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(open.contains("▾ collapse"), "{open}");
    assert!(open.contains("first thought"), "{open}");

    toggle_transcript_row_details(&mut row);
    assert!(row.details_collapsed);
    let collapsed = thinking_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(collapsed.contains("▸ details"), "{collapsed}");
    assert!(!collapsed.contains("first thought"), "{collapsed}");
}

#[test]
fn completed_tool_row_can_collapse_details() {
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran cargo test", "ok\nmore");
    assert!(row.is_expandable());

    let open = tool_lines(&row, true, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(open.contains("▾ collapse"), "{open}");
    assert!(open.contains("ok"), "{open}");

    toggle_transcript_row_details(&mut row);
    let collapsed = tool_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(collapsed.contains("▸ details"), "{collapsed}");
    assert!(!collapsed.contains("ok"), "{collapsed}");
}

#[test]
fn long_tool_output_uses_shared_default_collapse() {
    let output = (1..=12)
        .map(|line| format!("line {line:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran cat output.txt", output.clone());

    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert_eq!(row.text.lines().count(), 7);
    assert!(row.text.contains("... 6 more lines"));
    assert_eq!(row.full_text.as_deref(), Some(output.as_str()));

    let rendered = tool_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▸ 6 more lines"), "{rendered}");
    assert!(rendered.contains("  └ line 01"), "{rendered}");
    assert!(rendered.contains("line 12"), "{rendered}");
    assert!(rendered.contains("... 6 more lines"), "{rendered}");
}

#[test]
fn long_single_line_tool_output_collapses_by_display_width() {
    let output = format!("{{\"items\":\"{}\"}}", "x".repeat(1400));
    let row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran sqlite3 export", output.clone());

    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert!(row.full_text.is_some());
    assert!(row.text.contains('…'), "{}", row.text);

    let rendered = tool_lines(&row, false, true, 140)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▸ more output"), "{rendered}");
    assert!(rendered.len() < output.len(), "rendered output was not bounded");
}

#[test]
fn long_tool_output_collapses_by_display_tokens() {
    let output = (0..LEDGER_BODY_COLLAPSE_TOKENS + 12)
        .map(|_| "x")
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        UnicodeWidthStr::width(output.as_str()) < LEDGER_BODY_COLLAPSE_WIDTH,
        "test fixture should exercise token collapse before width collapse"
    );
    let row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran tokens", output.clone());

    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert_eq!(row.full_text.as_deref(), Some(output.as_str()));
    assert!(display_token_count(&row.text) <= LEDGER_BODY_COLLAPSE_TOKENS + 1);
    assert!(row.text.contains('…'), "{}", row.text);

    let rendered = tool_lines(&row, false, true, 120)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▸ more output"), "{rendered}");
}

#[test]
fn line_count_collapse_preview_is_bounded_by_display_tokens() {
    let line = (0..40).map(|_| "field").collect::<Vec<_>>().join(" ");
    let output = (1..=12)
        .map(|index| format!("{index}|{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran sqlite3 -separator '|'", output);

    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert!(!row.text.contains("... 6 more lines"), "{}", row.text);
    assert!(row.text.contains('…') || row.text.contains("... omitted middle"), "{}", row.text);
    assert!(
        display_token_count(&row.text) <= LEDGER_BODY_COLLAPSE_TOKENS,
        "{}",
        row.text
    );

    let rendered = tool_lines(&row, false, true, 120)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▸ more output"), "{rendered}");
    assert!(!rendered.contains("▸ 6 more lines"), "{rendered}");
}

#[test]
fn unbroken_table_output_collapses_by_display_tokens() {
    let output = "-".repeat(900);
    assert!(
        output.lines().count()
            <= LEDGER_BODY_COLLAPSE_HEAD_LINES + LEDGER_BODY_COLLAPSE_TAIL_LINES,
        "test fixture should exercise token collapse before line collapse"
    );
    assert!(
        UnicodeWidthStr::width(output.as_str()) < LEDGER_BODY_COLLAPSE_WIDTH,
        "test fixture should exercise token collapse before width collapse"
    );
    let row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran sqlite3 -column", output.clone());

    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert_eq!(row.full_text.as_deref(), Some(output.as_str()));
    assert!(row.text.contains('…'), "{}", row.text);
    assert!(row.text.len() < output.len(), "{}", row.text);

    let rendered = tool_lines(&row, false, true, 120)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▸ more output"), "{rendered}");
}

#[test]
fn expanded_tool_output_restores_full_text() {
    let output = (1..=12)
        .map(|line| format!("line {line:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran cat output.txt", output);

    toggle_transcript_row_details(&mut row);
    assert!(row.expanded);
    let rendered = tool_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▾ collapse"), "{rendered}");
    assert!(rendered.contains("line 12"), "{rendered}");
}

#[test]
fn long_command_and_long_output_expand_together() {
    let command = "cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \"SELECT id || '|' || by || '|' || text FROM comments WHERE story_id = 48073680 ORDER BY id\"";
    let output = (1..=12)
        .map(|line| format!("json row {line:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, format!("Ran {command}"), output);

    let collapsed = tool_lines(&row, false, true, 72)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(collapsed.contains("▸ 6 more lines"), "{collapsed}");
    assert!(!collapsed.contains("command: cd /home/kevin"), "{collapsed}");
    assert!(collapsed.contains("json row 12"), "{collapsed}");

    toggle_transcript_row_details(&mut row);
    let expanded = tool_lines(&row, false, true, 72)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(expanded.contains("▾ collapse"), "{expanded}");
    assert!(expanded.contains("command: cd /home/kevin/Projects/feedgarden"), "{expanded}");
    assert!(expanded.contains("json row 12"), "{expanded}");
}

#[test]
fn long_running_command_row_expands_full_command() {
    let command = "cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \"SELECT id || '|' || by || '|' || text FROM comments WHERE story_id = 48073680 ORDER BY id\"";
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, format!("Ran {command}"), "running");
    row.tool_started = Some(Instant::now());
    assert!(row.is_expandable());

    let collapsed = tool_lines(&row, false, true, 72)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(collapsed.contains("▸ command"), "{collapsed}");
    assert!(!collapsed.contains("command: cd /home/kevin"), "{collapsed}");

    toggle_transcript_row_details(&mut row);
    assert!(row.expanded);
    let expanded = tool_lines(&row, false, true, 72)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(expanded.contains("▾ collapse"), "{expanded}");
    assert!(expanded.contains("command: cd /home/kevin/Projects/feedgarden"), "{expanded}");
    assert!(!expanded.contains("  └ running"), "{expanded}");
}

#[tokio::test]
async fn ctrl_t_enters_transcript_focus_without_toggling_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored log", "a");
    row.full_text = Some("a\nb\nc".to_string());
    ui.transcript.push(row);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
        .await
        .expect("ctrl-t");

    assert_eq!(ui.focus, FocusMode::Transcript);
    assert!(ui.selected_target.is_some());
    assert!(!ui.transcript[0].expanded);
}

#[tokio::test]
async fn mouse_click_toggles_expandable_transcript_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored log", "a");
    row.full_text = Some("a\nb\nc".to_string());
    let row_id = row.id;
    ui.transcript.push(row);
    draw_fullscreen_for_test(&app, &mut ui, 80, 12);

    click_transcript_target(&mut app, &mut ui, TranscriptHitTarget::Row(row_id)).await;

    assert!(ui.transcript[0].expanded);
}

#[tokio::test]
async fn mouse_drag_selection_does_not_toggle_transcript_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.clipboard = Arc::new(|_| Ok(()));
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored log", "abcdef");
    row.full_text = Some("abcdef\nmore".to_string());
    let row_id = row.id;
    ui.transcript.push(row);
    draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let area = target_area(&ui, TranscriptHitTarget::Row(row_id));

    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(MouseEventKind::Down(MouseButton::Left), area.x + 2, area.y),
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(MouseEventKind::Drag(MouseButton::Left), area.x + 7, area.y),
    )
    .await
    .expect("mouse drag");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(MouseEventKind::Up(MouseButton::Left), area.x + 7, area.y),
    )
    .await
    .expect("mouse up");
    if app.clipboard_copies_in_flight > 0 {
        wait_for_clipboard_task(&mut app, &mut ui).await;
    }

    assert!(!ui.transcript[0].expanded);
}

#[test]
fn selected_answer_uses_single_line_focus_marker() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let row = TranscriptRow::simple(
        TranscriptKind::Answer,
        "first line\nsecond line\nthird line",
    );

    let rendered = answer_lines(&row, true, true, 80, &app.workdir, false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();

    assert!(rendered.first().is_some_and(|line| line.starts_with("› ")), "{rendered:?}");
    assert!(!rendered.iter().skip(1).any(|line| line.starts_with("> ")), "{rendered:?}");
    assert!(!rendered.iter().skip(1).any(|line| line.starts_with("› ")), "{rendered:?}");
}

#[test]
fn selected_thinking_and_tool_rows_use_single_line_focus_marker() {
    let thinking = TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "first thought\nsecond thought",
    );
    let thinking = thinking_lines(&thinking, true, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert!(thinking.first().is_some_and(|line| line.starts_with("› Thinking")), "{thinking:?}");
    assert!(thinking.get(1).is_some_and(|line| line.starts_with("  └ ")), "{thinking:?}");
    assert!(!thinking.iter().skip(1).any(|line| line.starts_with("> ")), "{thinking:?}");

    let tool = TranscriptRow::with_title(TranscriptKind::Ran, "Ran ls", "ok\nmore");
    let tool = tool_lines(&tool, true, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert!(tool.first().is_some_and(|line| line.starts_with("› bash ls")), "{tool:?}");
    assert!(tool.get(1).is_some_and(|line| line.starts_with("  └ ")), "{tool:?}");
    assert!(!tool.iter().skip(1).any(|line| line.starts_with("> ")), "{tool:?}");
}

#[test]
fn status_rows_use_quiet_notice_marker_and_hide_default_title() {
    let row = TranscriptRow::simple(
        TranscriptKind::Status,
        "mode: plan\nskill loaded: reviewer",
    );
    let lines = status_lines(&row, false, true, 80);
    let rendered = lines.iter().map(line_text).collect::<Vec<_>>();

    assert_eq!(rendered, vec!["· mode: plan", "  skill loaded: reviewer"]);
    assert_eq!(lines[0].spans[0].style, tui_theme().dim_style());
    assert!(!rendered.iter().any(|line| line.contains("Status")), "{rendered:?}");
    assert!(!rendered.iter().any(|line| line.contains("▌")), "{rendered:?}");

    let selected = status_lines(&row, true, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert_eq!(selected, vec!["› mode: plan", "  skill loaded: reviewer"]);
    assert!(!selected.iter().skip(1).any(|line| line.starts_with("› ")), "{selected:?}");
}

#[test]
fn status_tool_rows_keep_title_hint_and_tree_detail() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "Tool view_skill",
        "# X Daily\n... 174 more lines",
    );
    row.full_text = Some("# X Daily\nfull skill content".to_string());

    let rendered = status_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();

    assert!(rendered[0].starts_with("· Tool view_skill"), "{rendered:?}");
    assert!(rendered[0].contains("▸ 174 more lines"), "{rendered:?}");
    assert_eq!(rendered[1], "  └ # X Daily");
    assert!(!rendered.iter().any(|line| line.contains("... 174 more lines")), "{rendered:?}");
    assert!(!rendered.iter().any(|line| line.contains("▌")), "{rendered:?}");
}

#[test]
fn clarify_status_results_use_shared_status_notice_renderer() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "Questions 1/1 answered",
        "Which mode should we use?\nanswer: Fast\nnote: include tests",
    );
    row.tool_name = Some("clarify".to_string());

    let rendered = status_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();

    assert_eq!(rendered[0], "· Questions 1/1 answered");
    assert_eq!(rendered[1], "  └ Which mode should we use?");
    assert_eq!(rendered[2], "    answer: Fast");
    assert_eq!(rendered[3], "    note: include tests");
    assert!(!rendered.iter().any(|line| line.starts_with("• Questions")), "{rendered:?}");
    assert!(!rendered.iter().any(|line| line.starts_with("  • ")), "{rendered:?}");
}

async fn click_transcript_target(
    app: &mut TuiApp,
    ui: &mut FullscreenUi<'_>,
    target: TranscriptHitTarget,
) {
    let area = target_area(ui, target);
    app.handle_fullscreen_mouse(
        ui,
        mouse_event(MouseEventKind::Down(MouseButton::Left), area.x + 1, area.y),
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        ui,
        mouse_event(MouseEventKind::Up(MouseButton::Left), area.x + 1, area.y),
    )
    .await
    .expect("mouse up");
}

fn target_area(ui: &FullscreenUi<'_>, target: TranscriptHitTarget) -> Rect {
    ui.last_entry_areas
        .iter()
        .find_map(|(entry_target, area)| (*entry_target == target).then_some(*area))
        .expect("target area")
}

fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn compact_duration_formatter_uses_whole_seconds_and_minutes() {
    assert_eq!(format_duration_compact(Duration::from_millis(0)), "0s");
    assert_eq!(format_duration_compact(Duration::from_millis(999)), "0s");
    assert_eq!(format_duration_compact(Duration::from_millis(1_000)), "1s");
    assert_eq!(format_duration_compact(Duration::from_millis(59_999)), "59s");
    assert_eq!(format_duration_compact(Duration::from_millis(60_000)), "1m00s");
    assert_eq!(format_duration_compact(Duration::from_millis(65_000)), "1m05s");
    assert_eq!(format_duration_compact(Duration::from_millis(140_000)), "2m20s");
}

#[test]
fn streaming_tool_calls_parse_id_position_and_complete_arguments() {
    let event = serde_json::json!({
        "type": "message_update",
        "message": {
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_write",
                "name": "write",
                "arguments": {"path": "report.md", "content": "body"},
                "arguments_json": "{\"path\":\"report.md\",\"content\":\"body\"}",
                "arguments_error": null,
                "content_index": 3,
                "call_index": 2
            }]
        }
    });

    let calls = streaming_tool_calls_from_event(&event);

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id.as_deref(), Some("call_write"));
    assert_eq!(calls[0].position_key, "pos:3:2");
    assert_eq!(calls[0].tool_name, "write");
    assert_eq!(calls[0].args["path"], "report.md");
}

#[test]
fn streaming_tool_calls_keep_partial_arguments_as_null() {
    let event = serde_json::json!({
        "type": "message_update",
        "message": {
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "",
                "name": "write",
                "arguments": null,
                "arguments_json": "{\"path\":\"report.md\"",
                "arguments_error": "EOF while parsing",
                "content_index": 0,
                "call_index": 0
            }]
        }
    });

    let calls = streaming_tool_calls_from_event(&event);

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, None);
    assert_eq!(calls[0].position_key, "pos:0:0");
    assert!(calls[0].args.is_null());
}

#[test]
fn turn_meta_omits_tokens_and_uses_prefixless_debug_parts() {
    let usage = serde_json::json!({
        "input_tokens": 2,
        "output_tokens": 3,
        "total_tokens": 5
    });
    let default = turn_meta_text(TurnMetaProjection {
        mode: "default",
        provider: "provider",
        model: "model",
        started: None,
        usage: Some(&usage),
        metadata: None,
        accounting: None,
        failures: 0,
        interrupted: false,
        debug: false,
    });
    assert_eq!(default, "provider/model");
    let metadata = serde_json::json!({"provider_response_id":"resp"});
    let debug = turn_meta_text(TurnMetaProjection {
        mode: "plan",
        provider: "provider",
        model: "model",
        started: None,
        usage: Some(&usage),
        metadata: Some(&metadata),
        accounting: None,
        failures: 0,
        interrupted: false,
        debug: true,
    });
    assert!(debug.contains("usage 2 input"));
    assert!(debug.contains("3 output"));
    assert!(debug.contains("metadata response resp"));
    assert!(debug.ends_with("plan"));
    assert!(!debug.contains('='));
}

#[test]
fn turn_meta_omits_accounting_cost() {
    let accounting = serde_json::json!({
        "estimated_cost_nanodollars": 42_000,
        "pricing_source": "catalog"
    });
    let meta = turn_meta_text(TurnMetaProjection {
        mode: "default",
        provider: "provider",
        model: "model",
        started: None,
        usage: None,
        metadata: None,
        accounting: Some(&accounting),
        failures: 0,
        interrupted: false,
        debug: false,
    });

    assert_eq!(meta, "provider/model");
    assert!(!meta.contains("cost"));
}

#[test]
fn turn_meta_prefers_completed_elapsed_metadata() {
    let metadata = serde_json::json!({"elapsed_ms": 120});
    let stale_started = Instant::now()
        .checked_sub(Duration::from_secs(5))
        .expect("instant");

    let meta = turn_meta_text(TurnMetaProjection {
        mode: "default",
        provider: "provider",
        model: "model",
        started: Some(stale_started),
        usage: None,
        metadata: Some(&metadata),
        accounting: None,
        failures: 0,
        interrupted: false,
        debug: true,
    });

    assert!(meta.contains("0s"));
    assert!(!meta.contains("5."));
    assert!(!meta.contains("metadata elapsed"));
}

#[test]
fn turn_meta_formats_persisted_elapsed_minutes() {
    let metadata = serde_json::json!({"elapsed_ms": 65_000});

    let meta = turn_meta_text(TurnMetaProjection {
        mode: "default",
        provider: "provider",
        model: "model",
        started: None,
        usage: None,
        metadata: Some(&metadata),
        accounting: None,
        failures: 0,
        interrupted: false,
        debug: false,
    });

    assert_eq!(meta, "provider/model  1m05s");
}

#[test]
fn turn_meta_places_variant_after_model_and_filters_debug_duplicate() {
    let metadata = serde_json::json!({
        "elapsed_ms": 120,
        "reasoning_effort": "high",
        "provider_response_id": "resp"
    });
    let usage = serde_json::json!({"input_tokens": 2});

    let meta = turn_meta_text(TurnMetaProjection {
        mode: "plan",
        provider: "provider",
        model: "model",
        started: None,
        usage: Some(&usage),
        metadata: Some(&metadata),
        accounting: None,
        failures: 1,
        interrupted: false,
        debug: true,
    });

    assert_eq!(
        meta,
        "provider/model high  0s  1 failure  usage 2 input  metadata response resp  plan"
    );
}

#[test]
fn slash_completion_completes_command_prefixes() {
    assert_eq!(slash_completion("/he").as_deref(), Some("/help"));
    assert_eq!(slash_completion("/ren").as_deref(), Some("/rename"));
    assert_eq!(slash_completion("/rn"), None);
    assert_eq!(slash_completion("/mo").as_deref(), Some("/mode"));
    assert_eq!(slash_completion("/model"), None);
    assert_eq!(slash_completion("hello"), None);
    assert_eq!(slash_completion("/he\nthere"), None);
}

#[test]
fn file_token_detection_covers_boundaries_and_unicode() {
    let cases = vec![
        ("@", 0, 1, Some("")),
        ("@file.txt", 0, 4, Some("file.txt")),
        ("hello @world test", 0, 8, Some("world")),
        (
            "@icons/icon@2x.png",
            0,
            "@icons/icon@2x.png".chars().count(),
            Some("icons/icon@2x.png"),
        ),
        (
            "test　@İstanbul",
            0,
            "test　@İstanbul".chars().count(),
            Some("İstanbul"),
        ),
        ("foo@bar", 0, "foo@bar".chars().count(), None),
        ("@ hello", 0, 2, None),
        (
            "first @one\nsecond @two",
            1,
            "second @two".chars().count(),
            Some("two"),
        ),
    ];

    for (input, row, col, expected) in cases {
        let textarea = textarea_with_lines_and_cursor(
            input.split('\n').map(ToString::to_string).collect(),
            row,
            col,
        );
        let actual = current_file_token(&textarea).map(|token| token.query);
        assert_eq!(
            actual.as_deref(),
            expected,
            "input={input:?} row={row} col={col}"
        );
    }
}

#[test]
fn file_token_replacement_quotes_paths_with_spaces() {
    let mut textarea = textarea_with_text("open @src");
    assert!(replace_current_file_token(&mut textarea, "src/main.rs"));
    assert_eq!(textarea_text(&textarea), "open src/main.rs ");

    let mut textarea = textarea_with_text("open @docs");
    assert!(replace_current_file_token(
        &mut textarea,
        "docs/reference notes.md"
    ));
    assert_eq!(
        textarea_text(&textarea),
        "open \"docs/reference notes.md\" "
    );
}

#[test]
fn file_search_returns_workdir_relative_paths_and_respects_gitignore() {
    let temp = tempdir().expect("temp");
    let root = temp.path();
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("main");
    fs::write(root.join(".hidden.rs"), "hidden\n").expect("hidden");
    fs::write(root.join("ignored.txt"), "ignored\n").expect("ignored");
    fs::write(root.join(".gitignore"), "ignored.txt\n").expect("gitignore");
    fs::create_dir_all(root.join(".git/objects")).expect("git dir");
    fs::write(root.join(".git/config"), "private\n").expect("git config");
    let cancel = AtomicBool::new(false);

    let src_matches = search_workdir_files(root, "src", &cancel);
    assert_eq!(
        src_matches.first(),
        Some(&FileSearchMatch {
            path: "src".to_string(),
            kind: FileSearchMatchKind::Directory,
        })
    );
    assert!(src_matches.iter().any(|entry| entry.path == "src/main.rs"));

    let ignored_matches = search_workdir_files(root, "ignored", &cancel);
    assert!(ignored_matches.is_empty(), "{ignored_matches:#?}");

    let hidden_matches = search_workdir_files(root, "hidden", &cancel);
    assert_eq!(hidden_matches.len(), 1);
    assert_eq!(hidden_matches[0].path, ".hidden.rs");

    let git_matches = search_workdir_files(root, "config", &cancel);
    assert!(
        git_matches
            .iter()
            .all(|entry| !entry.path.starts_with(".git/")),
        "{git_matches:#?}"
    );
}

#[test]
fn stale_file_search_results_are_ignored() {
    let mut state = FileSearchState::new();
    state.generation = 2;
    state.popup = Some(FileSearchPopupState {
        query: "new".to_string(),
        matches: Vec::new(),
        selected: 0,
        waiting: true,
    });
    state
        .tx
        .send(FileSearchResult {
            generation: 1,
            query: "old".to_string(),
            matches: vec![FileSearchMatch {
                path: "old.rs".to_string(),
                kind: FileSearchMatchKind::File,
            }],
        })
        .expect("send stale");
    state
        .tx
        .send(FileSearchResult {
            generation: 2,
            query: "new".to_string(),
            matches: vec![FileSearchMatch {
                path: "new.rs".to_string(),
                kind: FileSearchMatchKind::File,
            }],
        })
        .expect("send current");

    state.drain_results();

    let popup = state.popup.expect("popup");
    assert_eq!(
        popup.matches,
        vec![FileSearchMatch {
            path: "new.rs".to_string(),
            kind: FileSearchMatchKind::File,
        }]
    );
    assert!(!popup.waiting);
}

#[test]
fn bottom_panel_row_right_aligns_detail_with_wide_title() {
    let row = BottomSelectionRow {
        label: "当前模式询问".to_string(),
        description: Some("deepseek/deepseek-v4-pro  messages=2".to_string()),
        detail: Some("08:50".to_string()),
        group: None,
        search_text: String::new(),
        is_current: false,
        is_default: false,
        style: BottomRowStyle::Normal,
        footer: None,
        value: BottomSelectionValue::Session("session-a".to_string()),
    };

    let width = 54;
    let text = line_text(&bottom_panel_row(
        &row,
        false,
        width,
        false,
        Duration::default(),
    ));

    assert!(text.ends_with("08:50"));
    assert_eq!(UnicodeWidthStr::width(text.as_str()), usize::from(width));
}
