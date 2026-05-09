#[test]
fn transcript_auto_follow_tracks_wrapped_streaming_content() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.last_transcript_width = 32;
    ui.last_transcript_height = 4;
    for index in 0..6 {
        ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Answer,
            "",
            format!("prior answer {index}"),
        ));
    }
    ui.scroll_to_bottom();
    let initial_bottom = ui.scroll;

    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "streaming answer ".repeat(80)
                }]
            }
        })),
        true,
        false,
    );
    ui.follow_transcript_if_needed();

    assert!(ui.scroll > initial_bottom);
    assert_eq!(ui.scroll, ui.max_transcript_scroll());

    ui.scroll_transcript(-2);
    assert!(!ui.auto_follow_transcript);
    let manual_scroll = ui.scroll;
    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "streaming answer ".repeat(120)
                }]
            }
        })),
        true,
        false,
    );
    ui.follow_transcript_if_needed();
    assert_eq!(ui.scroll, manual_scroll);

    ui.scroll_transcript(10_000);
    assert!(ui.auto_follow_transcript);
    assert_eq!(ui.scroll, ui.max_transcript_scroll());
}

#[test]
fn transcript_viewport_excludes_bottom_border_from_scroll_height() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::simple(
        TranscriptKind::Answer,
        "bottom line must be visible",
    ));

    let _buffer = draw_fullscreen_for_test(&app, &mut ui, 48, 10);

    assert_eq!(ui.last_transcript_height, 6);
}

#[test]
fn transcript_render_clears_stale_cells_after_shorter_redraw() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::simple(
        TranscriptKind::Answer,
        format!("{}TAILMARK", "wide content ".repeat(4)),
    ));
    let backend = TestBackend::new(72, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("first draw");
    assert!(buffer_text(terminal.backend().buffer()).contains("TAILMARK"));

    ui.transcript.clear();
    ui.transcript
        .push(TranscriptRow::simple(TranscriptKind::Status, "short"));
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("second draw");

    let text = buffer_text(terminal.backend().buffer());
    assert!(!text.contains("TAILMARK"), "{text}");
}

#[test]
fn history_reload_scrolls_to_end_of_multiline_markdown_answer() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let answer = "✅ **Hacker News 日报完成！**\n\n\
📄 **输出文件：** `feeds/2026-05-09/hackernews-hot-2307.md`（13.4 KB）\n\n\
**今日亮点速览：**\n\n\
| # | 话题 | 🔥 |\n\
|---|------|-----|\n\
| 1 | Google reCAPTCHA 绑定 Play Services，封杀去 Google 化 Android | ⬆️ 1249 |\n\
| 2 | David Attenborough 百岁生日 | ⬆️ 748 |\n\
| 3 | Meshtastic：LoRa 去中心化离网通信 | ⬆️ 470 |\n\
| 4 | Gowers：ChatGPT 5.5 Pro 完成博士级数学研究 | ⬆️ 467 |\n\
| 5 | WebRTC 是语音 AI 的错误选择 | ⬆️ 388 |\n\
| 6 | Cartoon Network Flash 游戏怀旧档案 | ⬆️ 375 |\n\
| 7 | AI 打破传统漏洞披露文化 | ⬆️ 367 |\n\
| 8 | Wi-Fi 全代际技术指南 | ⬆️ 296 |\n\
| 9 | HTML 在 AI 时代的非理性有效性 | ⬆️ 275 |\n\
| 10 | 重读《人月神话》——AI 是银弹吗？ | ⬆️ 246 |\n\
| 11 | AWS US-East-1 数据中心故障 | ⬆️ 243 |\n\
| 12 | io_uring ZCRX 本地提权（10 天内第 4 个 Linux LPE） | ⬆️ 199 |\n\n\
今日安全话题密度极高——第 1、7、12 条分别涉及 Google 平台控制、漏洞披露危机和 Linux 内核提权，社区讨论非常热烈。";
    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": answer}],
            "timestamp_ms": 1,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mimo-v2.5-pro",
            "provider": "xiaomi"
        }),
        None,
        None,
    );
    ui.scroll_to_bottom();

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 10);
    let text = buffer_text(&buffer);

    assert!(text.contains("| 12 | io_uring"), "{text}");
    assert!(text.contains("Linux"), "{text}");
    assert!(!text.contains("Hacker News 日报完成"), "{text}");
}

#[test]
fn transcript_bottom_scroll_uses_paragraph_word_wrapping() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::simple(
        TranscriptKind::Answer,
        format!("{}\nBOTTOM-TARGET", "abcdefghij ".repeat(80)),
    ));
    ui.scroll_to_bottom();

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 20, 8);
    let text = buffer_text(&buffer);

    assert!(text.contains("BOTTOM-TARGET"), "{text}");
}

#[test]
fn transcript_layout_cache_reuses_row_heights_while_scrolling() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    for index in 0..80 {
        ui.transcript.push(TranscriptRow::simple(
            TranscriptKind::Answer,
            format!("row {index:02} {}", "abcdefghij ".repeat(16)),
        ));
    }
    ui.scroll_to_bottom();

    let _ = draw_fullscreen_for_test(&app, &mut ui, 48, 10);
    assert!(ui.transcript_layout.recomputed_rows > 0);
    let total_height = ui.transcript_layout.total_height;

    ui.scroll_transcript(-3);
    let _ = draw_fullscreen_for_test(&app, &mut ui, 48, 10);

    assert_eq!(ui.transcript_layout.total_height, total_height);
    assert_eq!(ui.transcript_layout.recomputed_rows, 0);
}

#[test]
fn active_tool_layout_key_tracks_elapsed_for_cache() {
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, "Running cargo test", "running");
    row.tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("instant"),
    );
    let first = transcript_layout_row_key(&row, true, true, false);

    row.tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_secs(65))
            .expect("instant"),
    );
    let later = transcript_layout_row_key(&row, true, true, false);

    assert_ne!(first, later);
}

#[test]
fn long_read_tool_output_collapses_and_preserves_full_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let content = (1..=64)
        .map(|line| format!("{line:02}: fn rendered_fixture() {{}}"))
        .collect::<Vec<_>>()
        .join("\n");

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_read_long",
            "tool_name": "read",
            "args": {"path": "src/long.rs"},
            "result": {"path": "src/long.rs", "content": content},
            "outcome": "normal"
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Explored)
        .expect("read evidence row");
    assert_eq!(row.title, "Explored src/long.rs");
    assert_eq!(row.text.lines().count(), 21);
    assert!(row.text.contains("... 44 more lines"));
    assert_eq!(row.full_text.as_deref(), Some(content.as_str()));
    assert!(row.is_expandable());
}

#[test]
fn running_tool_title_right_aligns_elapsed_duration() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran cargo test --workspace --all-targets",
        "running",
    );
    row.tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_millis(120))
            .expect("instant"),
    );

    let title = line_text(&tool_lines(&row, false, true, 36)[0]);

    assert!(title.contains("Ran cargo"));
    assert!(!title.starts_with("• "));
    assert!(title.ends_with("0s"));
    assert_eq!(UnicodeWidthStr::width(title.as_str()), 36);
}

#[test]
fn completed_tool_title_uses_fixed_elapsed_duration() {
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored src/lib.rs", "");
    row.tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_secs(5))
            .expect("instant"),
    );
    row.tool_elapsed = Some(Duration::from_millis(120));

    let title = line_text(&tool_lines(&row, false, true, 32)[0]);

    assert!(title.ends_with("0s"));
    assert!(!title.contains("5."));
}

#[test]
fn narrow_tool_title_preserves_elapsed_duration() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran cargo test --workspace --all-targets",
        "",
    );
    row.tool_elapsed = Some(Duration::from_millis(12_340));

    let title = line_text(&tool_lines(&row, false, true, 18)[0]);

    assert!(title.ends_with("12s"));
    assert!(title.contains('…'));
}

#[test]
fn completed_tool_title_formats_elapsed_minutes() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran cargo test --workspace --all-targets",
        "",
    );
    row.tool_elapsed = Some(Duration::from_millis(140_000));

    let title = line_text(&tool_lines(&row, false, true, 22)[0]);

    assert!(title.ends_with("2m20s"));
    assert!(title.contains('…'));
}

#[test]
fn streaming_tool_call_creates_pending_changing_row_before_execution() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
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
        }),
        false,
    );

    assert_eq!(ui.transcript.len(), 1);
    assert_eq!(ui.transcript[0].kind, TranscriptKind::Changed);
    assert_eq!(ui.transcript[0].title, "Changing files");
    assert_eq!(ui.transcript[0].text, "preparing");
    assert!(ui.transcript[0].tool_started.is_some());
    assert!(ui
        .transcript
        .iter()
        .all(|row| row.kind != TranscriptKind::Answer));
}

#[test]
fn streaming_tool_call_migrates_position_key_to_tool_id_without_duplicate() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
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
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
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
                    "content_index": 0,
                    "call_index": 0
                }]
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_write",
            "tool_name": "write",
            "args": {"path": "report.md", "content": "body"}
        }),
        false,
    );

    let rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Changed)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Changing report.md");
    assert_eq!(rows[0].text, "running");
    assert_eq!(rows[0].tool_call_id.as_deref(), Some("call_write"));
}

#[test]
fn streaming_tool_completion_reuses_pending_row_as_completed_evidence() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
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
                    "content_index": 0,
                    "call_index": 0
                }]
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_write",
            "tool_name": "write",
            "result": {"path": "report.md", "bytes_written": 4},
            "outcome": "normal",
            "elapsed_ms": 65_000
        }),
        false,
    );

    assert_eq!(ui.transcript.len(), 1);
    let row = &ui.transcript[0];
    assert_eq!(row.kind, TranscriptKind::Changed);
    assert_eq!(row.title, "Changed report.md");
    assert!(row.text.contains("write normal"));
    assert_eq!(row.tool_elapsed, Some(Duration::from_millis(65_000)));
    assert!(row.tool_started.is_none());
}

#[test]
fn interrupted_pending_tool_row_stops_timer_as_failed() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_edit",
                    "name": "edit",
                    "arguments": {"path": "src/lib.rs"},
                    "arguments_json": "{\"path\":\"src/lib.rs\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }]
            }
        }),
        false,
    );

    ui.finish_turn();

    let row = &ui.transcript[0];
    assert_eq!(row.title, "Changing src/lib.rs");
    assert_eq!(row.text, "interrupted");
    assert!(row.failed);
    assert!(row.tool_elapsed.is_some());
    assert!(row.tool_started.is_none());
}

#[test]
fn parallel_streaming_tool_calls_create_independent_active_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_call",
                        "id": "call_read",
                        "name": "read",
                        "arguments": {"path": "Cargo.toml"},
                        "arguments_json": "{\"path\":\"Cargo.toml\"}",
                        "arguments_error": null,
                        "content_index": 0,
                        "call_index": 0
                    },
                    {
                        "type": "tool_call",
                        "id": "call_search",
                        "name": "search",
                        "arguments": {"query": "format_duration"},
                        "arguments_json": "{\"query\":\"format_duration\"}",
                        "arguments_error": null,
                        "content_index": 1,
                        "call_index": 0
                    }
                ]
            }
        }),
        false,
    );

    let titles = ui
        .transcript
        .iter()
        .map(|row| row.title.as_str())
        .collect::<Vec<_>>();
    assert_eq!(titles, ["Exploring Cargo.toml", "Exploring search format_duration"]);
}

#[test]
fn sequential_streaming_tool_calls_reuse_position_without_overwriting_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_first",
                    "name": "bash",
                    "arguments": {"command": "echo one"},
                    "arguments_json": "{\"command\":\"echo one\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }]
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_first",
            "tool_name": "bash",
            "result": {"output": "one", "exit_code": 0},
            "outcome": "normal",
            "elapsed_ms": 10
        }),
        false,
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_second",
                    "name": "write",
                    "arguments": {"path": "report.md", "content": "body"},
                    "arguments_json": "{\"path\":\"report.md\",\"content\":\"body\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }]
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_second",
            "tool_name": "write",
            "result": {"path": "report.md", "bytes_written": 4},
            "outcome": "normal",
            "elapsed_ms": 10
        }),
        false,
    );

    let titles = ui
        .transcript
        .iter()
        .filter(|row| {
            matches!(
                row.kind,
                TranscriptKind::Explored | TranscriptKind::Ran | TranscriptKind::Changed
            )
        })
        .map(|row| row.title.as_str())
        .collect::<Vec<_>>();
    assert_eq!(titles, ["Ran echo one", "Changed report.md"]);
}

#[test]
fn history_tool_result_restores_elapsed_duration() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "tool_result",
            "tool_call_id": "call_read",
            "tool_name": "read",
            "content": "{\"path\":\"src/lib.rs\",\"content\":\"done\"}",
            "is_error": false,
            "timestamp_ms": 2
        }),
        None,
        Some(&serde_json::json!({"elapsed_ms": 230})),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Explored)
        .expect("tool row");
    assert_eq!(row.tool_elapsed, Some(Duration::from_millis(230)));
    assert!(line_text(&tool_lines(row, false, true, 32)[0]).ends_with("0s"));
}

#[test]
fn history_meta_uses_persisted_variant_not_current_variant() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_variant = Some("xhigh".to_string());
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "answer"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
        Some(&serde_json::json!({
            "elapsed_ms": 230,
            "reasoning_effort": "high"
        })),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Meta)
        .expect("meta row");
    assert_eq!(row.text, "mock/mock-model high  0s");
    assert!(!row.text.contains("xhigh"));
}

#[test]
fn prompt_block_uses_full_width_background_without_left_rail() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("inspect prompt styling".to_string());
    let backend = TestBackend::new(48, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "›");
    assert_eq!(buffer.cell((0, 0)).expect("cell").bg, TUI_SURFACE_BG);
    assert_eq!(buffer.cell((47, 0)).expect("cell").bg, TUI_SURFACE_BG);
    assert_ne!(buffer.cell((0, 0)).expect("cell").symbol(), "▌");
}

#[test]
fn composer_and_prompt_share_full_width_surface() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("match the composer surface".to_string());
    let backend = TestBackend::new(48, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let composer_y = 7;

    assert_eq!(buffer.cell((0, 0)).expect("prompt marker").symbol(), "›");
    assert_eq!(
        buffer
            .cell((0, composer_y))
            .expect("composer marker")
            .symbol(),
        "›"
    );
    assert_eq!(buffer.cell((0, 0)).expect("prompt bg").bg, TUI_SURFACE_BG);
    assert_eq!(
        buffer.cell((47, 0)).expect("prompt trailing bg").bg,
        TUI_SURFACE_BG
    );
    assert_eq!(
        buffer.cell((0, composer_y)).expect("composer bg").bg,
        TUI_SURFACE_BG
    );
    assert_eq!(
        buffer
            .cell((47, composer_y))
            .expect("composer trailing bg")
            .bg,
        TUI_SURFACE_BG
    );
    assert_ne!(
        buffer
            .cell((0, composer_y))
            .expect("composer rail")
            .symbol(),
        "│"
    );
    assert_eq!(
        buffer
            .cell((0, composer_y + 1))
            .expect("composer row bg")
            .bg,
        TUI_SURFACE_BG
    );
    assert_eq!(
        buffer
            .cell((47, composer_y + 1))
            .expect("composer row trailing bg")
            .bg,
        TUI_SURFACE_BG
    );
}

#[test]
fn wrapped_prompt_rows_keep_full_width_background_for_wide_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("中文测试中文测试中文测试中文测试".to_string());
    let backend = TestBackend::new(24, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer.cell((0, 0)).expect("first marker").symbol(), "›");
    assert_eq!(
        buffer.cell((0, 1)).expect("continuation marker").symbol(),
        " "
    );
    for y in 0..=1 {
        assert_eq!(
            buffer.cell((0, y)).expect("row start").bg,
            TUI_SURFACE_BG,
            "row {y} start"
        );
        assert_eq!(
            buffer.cell((23, y)).expect("row end").bg,
            TUI_SURFACE_BG,
            "row {y} end"
        );
    }
}

#[test]
fn empty_composer_uses_two_surface_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let backend = TestBackend::new(48, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let composer_y = 7;

    assert_eq!(
        buffer
            .cell((0, composer_y))
            .expect("composer marker")
            .symbol(),
        "›"
    );
    for y in composer_y..=composer_y + 1 {
        assert_eq!(
            buffer.cell((0, y)).expect("composer row start").bg,
            TUI_SURFACE_BG
        );
        assert_eq!(
            buffer.cell((47, y)).expect("composer row end").bg,
            TUI_SURFACE_BG
        );
    }
}

#[test]
fn thinking_new_paragraphs_do_not_use_label_width_indent() {
    let row = TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "First paragraph.\n\nSecond paragraph.",
    );
    let lines = thinking_lines(&row, false, true);

    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].spans[0].content.as_ref(), "▌ ");
    assert_eq!(lines[0].spans[1].content.as_ref(), "Thinking: ");
    assert_eq!(lines[0].spans[2].content.as_ref(), "First paragraph.");
    assert_eq!(lines[2].spans[0].content.as_ref(), "▌ ");
    assert_eq!(lines[2].spans[1].content.as_ref(), "Second paragraph.");
}

#[test]
fn bash_tool_title_uses_actual_first_command_line() {
    let title = tool_title(
        "bash",
        &serde_json::json!({
            "args": {"command": "cargo test -p psychevo-cli\ncargo fmt"}
        }),
    );
    assert_eq!(title, "Ran cargo test -p psychevo-cli");
}

#[test]
fn fullscreen_bash_title_survives_tool_end_without_args() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_bash",
            "tool_name": "bash",
            "args": {"command": "cargo test -p psychevo-cli\ncargo fmt"}
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_bash",
            "tool_name": "bash",
            "result": {"output": "ok", "exit_code": 0},
            "outcome": "normal"
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("bash row");
    assert_eq!(row.title, "Ran cargo test -p psychevo-cli");
    assert_ne!(row.title, "Ran command");
}

#[test]
fn history_tool_result_reuses_persisted_bash_command_title() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let assistant = serde_json::json!({
        "role": "assistant",
        "content": [{
            "type": "tool_call",
            "id": "call_bash",
            "name": "bash",
            "arguments": {
                "command": "find . -maxdepth 2\nprintf done"
            },
            "arguments_json": "{\"command\":\"find . -maxdepth 2\\nprintf done\"}",
            "arguments_error": null,
            "content_index": 0,
            "call_index": 0
        }],
        "timestamp_ms": 1,
        "finish_reason": "tool_calls",
        "outcome": "normal"
    });
    let tool_result = serde_json::json!({
        "role": "tool_result",
        "tool_call_id": "call_bash",
        "tool_name": "bash",
        "content": "{\"output\":\"ok\"}",
        "is_error": false,
        "timestamp_ms": 2
    });

    ui.push_history_message(&assistant, None, None);
    ui.push_history_message(&tool_result, None, None);

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("history bash row");
    assert_eq!(row.title, "Ran find . -maxdepth 2");
}

#[tokio::test]
async fn sidebar_toggle_persists_visibility() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    assert!(!ui.sidebar_enabled());

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
    )
    .await
    .expect("show sidebar");
    assert!(ui.sidebar_enabled());
    let loaded = TuiState::load(&app.state_path).expect("load visible state");
    assert!(loaded.sidebar_visible);

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
    )
    .await
    .expect("hide sidebar");
    assert!(!ui.sidebar_enabled());
    let loaded = TuiState::load(&app.state_path).expect("load hidden state");
    assert!(!loaded.sidebar_visible);
}

#[tokio::test]
async fn fullscreen_rename_updates_session_title_and_sidebar() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "model", "provider", None)
        .expect("session");
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
    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some("Better Session Title"));
}

#[tokio::test]
async fn removed_thinking_command_renders_bounded_error_in_fullscreen() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/thinking");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert!(
        ui.transcript.iter().any(|row| {
            row.kind == TranscriptKind::Error && row.text.contains("/show-thinking")
        })
    );
}
