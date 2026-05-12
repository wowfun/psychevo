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

    assert!(text.contains("└────"), "{text}");
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
fn manual_down_scroll_reaches_long_markdown_bottom_with_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let table = (1..=36)
        .map(|index| format!("| {index} | **Story {index}** with a long mixed-width summary | {index} |"))
        .collect::<Vec<_>>()
        .join("\n");
    ui.transcript.push(TranscriptRow::simple(
        TranscriptKind::Answer,
        format!("# Daily report\n\n| # | Title | Score |\n|---|---|---|\n{table}\n\nBOTTOM-MARKER"),
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Meta,
        "",
        "provider/model low 7m05s 1 failure",
    ));

    let _ = draw_fullscreen_for_test(&app, &mut ui, 72, 10);
    ui.scroll = 0;
    ui.auto_follow_transcript = false;
    for _ in 0..32 {
        ui.scroll_transcript(6);
    }

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 72, 10);
    let text = buffer_text(&buffer);
    assert!(text.contains("BOTTOM-MARKER"), "{text}");
    assert!(text.contains("1 failure"), "{text}");
    assert!(!text.contains("Daily report"), "{text}");
    assert_eq!(ui.scroll, ui.max_transcript_scroll());
}

#[test]
fn manual_down_scroll_reaches_long_thinking_table_bottom() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran ls -lh /tmp/report.md",
        "-rw-r--r-- 1 user user 25K report.md",
    ));
    let table = (1..=24)
        .map(|index| {
            format!(
                "| {index} | **热门话题 {index}** - 一段较长的中文说明用来验证换行和滚动 | {} |",
                100 + index
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        format!(
            "Hacker News 日报已生成完成！ ✅\n\n**文件：** `feeds/2026-05-10/hackernews-hot-04-03.md`（25KB）\n\n### 今日 12 条热门话题速览：\n\n| # | 话题 | 💬 |\n|---|------|-----|\n{table}\n\nTHINKING_BOTTOM_MARKER"
        ),
    ));
    ui.transcript
        .last_mut()
        .expect("thinking row")
        .expanded = true;
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Meta,
        "",
        "mock/mock-model low 7m05s 1 failure",
    ));

    let _ = draw_fullscreen_for_test(&app, &mut ui, 100, 18);
    ui.scroll = 0;
    ui.auto_follow_transcript = false;
    for _ in 0..64 {
        ui.scroll_transcript(6);
    }

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 18);
    let text = buffer_text(&buffer);
    assert!(text.contains("THINKING_BOTTOM_MARKER"), "{text}");
    assert!(text.contains("1 failure"), "{text}");
    assert!(!text.contains("日报已生成"), "{text}");
    assert_eq!(ui.scroll, ui.max_transcript_scroll());
}

#[test]
fn transcript_focus_down_scrolls_selected_row_into_view() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    for index in 0..36 {
        ui.transcript.push(TranscriptRow::simple(
            TranscriptKind::Status,
            format!("focus row {index:02}"),
        ));
    }
    let _ = draw_fullscreen_for_test(&app, &mut ui, 64, 10);
    ui.focus = FocusMode::Transcript;
    ui.selected_row = Some(0);
    ui.scroll = 0;
    ui.auto_follow_transcript = false;

    ui.move_selection(18);

    assert_eq!(ui.selected_row, Some(18));
    assert!(ui.scroll > 0);
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 64, 10);
    let text = buffer_text(&buffer);
    assert!(text.contains("focus row 18"), "{text}");
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
    assert_eq!(row.text.lines().count(), 9);
    assert!(row.text.contains("... 56 more lines"));
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

    assert!(title.contains("Running cargo"));
    assert!(!title.starts_with("• "));
    assert!(title.ends_with("0s"));
    assert_eq!(UnicodeWidthStr::width(title.as_str()), 35);
}

#[test]
fn active_tool_rows_render_present_tense_without_redundant_body() {
    let started = Instant::now()
        .checked_sub(Duration::from_secs(2))
        .expect("instant");
    for (kind, stored_title, expected_title) in [
        (
            TranscriptKind::Explored,
            "Explored Cargo.toml",
            "Exploring Cargo.toml",
        ),
        (
            TranscriptKind::Ran,
            "Ran cargo test -p psychevo-cli",
            "Running cargo test -p psychevo-cli",
        ),
        (
            TranscriptKind::Changed,
            "Changed src/lib.rs",
            "Changing src/lib.rs",
        ),
    ] {
        let mut row = TranscriptRow::with_title(kind, stored_title, "running");
        row.tool_started = Some(started);

        let lines = tool_lines(&row, false, true, 80);
        let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");

        assert!(rendered.contains(expected_title), "{rendered}");
        assert!(!rendered.contains("└ running"), "{rendered}");
    }
}

#[test]
fn expandable_tool_title_uses_text_hint_without_brackets() {
    let content = (1..=12)
        .map(|line| format!("line {line:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Explored,
        "Explored src/long.rs",
        "line 01\nline 02\n... 10 more lines",
    );
    row.full_text = Some(content);

    let collapsed = tool_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(collapsed.contains("▸ 10 more lines"), "{collapsed}");
    assert!(!collapsed.contains("[+]"), "{collapsed}");
    assert!(!collapsed.contains("... 10 more lines"), "{collapsed}");

    row.expanded = true;
    let expanded = line_text(&tool_lines(&row, false, true, 80)[0]);
    assert!(expanded.contains("▾ collapse"), "{expanded}");
    assert!(!expanded.contains("[-]"), "{expanded}");
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
fn streaming_tool_call_after_visible_text_stays_below_answer() {
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
                        "type": "text",
                        "text": "Now let me write the complete report."
                    },
                    {
                        "type": "tool_call",
                        "id": "call_write",
                        "name": "write",
                        "arguments": {"path": "report.md", "content": "body"},
                        "arguments_json": "{\"path\":\"report.md\",\"content\":\"body\"}",
                        "arguments_error": null,
                        "content_index": 1,
                        "call_index": 0
                    }
                ]
            }
        }),
        false,
    );

    let answer = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Answer)
        .expect("answer row");
    let tool = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Changed)
        .expect("tool row");
    assert!(answer < tool);
    assert_eq!(ui.transcript[tool].title, "Changing report.md");

    ui.scroll_to_bottom();
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 8);
    let text = buffer_text(&buffer);
    assert!(text.contains("Changing report.md"), "{text}");
    assert!(!text.contains("Tool calls"), "{text}");
}

#[test]
fn message_end_text_plus_write_tool_shows_active_row_without_intermediate_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro",
            "mode": "default"
        }),
        false,
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "Now I have all the data I need. Let me write the full report:"
                    },
                    {
                        "type": "tool_call",
                        "id": "call_write_report",
                        "name": "write",
                        "arguments": {
                            "path": "/tmp/hackernews-hot-05-15.md",
                            "content": "report body"
                        },
                        "arguments_json": "{\"path\":\"/tmp/hackernews-hot-05-15.md\",\"content\":\"report body\"}",
                        "arguments_error": null,
                        "content_index": 1,
                        "call_index": 0
                    }
                ],
                "timestamp_ms": 2,
                "finish_reason": "tool_calls",
                "outcome": "normal",
                "model": "mimo-v2.5-pro",
                "provider": "xiaomi-token-plan"
            },
            "metadata": {
                "elapsed_ms": 186_260,
                "reasoning_effort": "low"
            }
        }),
        false,
    );

    let answer = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Answer)
        .expect("answer row");
    let tool = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Changed)
        .expect("tool row");
    assert!(answer < tool);
    assert_eq!(ui.transcript[tool].title, "Changing /tmp/hackernews-hot-05-15.md");
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );

    ui.scroll_to_bottom();
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 8);
    let text = buffer_text(&buffer);
    assert!(text.contains("Changing /tmp/hackernews-hot-05-15.md"), "{text}");
    assert!(!text.contains("Tool calls"), "{text}");
    assert!(!text.contains("xiaomi-token-plan/mimo-v2.5-pro"), "{text}");
}

#[test]
fn pending_write_tool_input_shows_changing_before_complete_arguments() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro",
            "mode": "default"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "Now I have all the data needed. Let me write the complete report."
                }],
                "timestamp_ms": 2,
                "outcome": "normal"
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "call_write_report",
            "tool_name": "write",
            "arguments_json": "",
            "content_index": 1,
            "call_index": 0
        }),
        false,
    );

    let answer = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Answer)
        .expect("answer row");
    let tool = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Changed)
        .expect("tool row");
    assert!(answer < tool);
    assert_eq!(ui.transcript[tool].title, "Changing files");
    assert!(ui.transcript[tool].tool_started.is_some());
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "call_write_report",
            "tool_name": "write",
            "arguments_json": "{\"path\":\"feeds/report.md\",\"content\":\"body\"}",
            "content_index": 1,
            "call_index": 0
        }),
        false,
    );
    assert_eq!(ui.transcript[tool].title, "Changing feeds/report.md");

    ui.scroll_to_bottom();
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 8);
    let text = buffer_text(&buffer);
    assert!(text.contains("Changing feeds/report.md"), "{text}");
    assert!(!text.contains("Tool calls"), "{text}");
    assert!(!text.contains("xiaomi-token-plan/mimo-v2.5-pro"), "{text}");
}

#[test]
fn visible_write_preamble_creates_and_reconciles_provisional_changing_row() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "NYT is behind a paywall. Based on comments, I can still summarize the Meta article. Let me now write the report."
                }]
            }
        }),
        false,
    );

    let provisional = ui
        .transcript
        .iter()
        .position(|row| row.title == "Changing files")
        .expect("provisional row");
    assert!(ui.transcript[provisional].tool_call_id.is_none());

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "call_write_report",
            "tool_name": "write",
            "arguments_json": "{\"path\":\"feeds/report.md\",\"content\":\"body\"}",
            "content_index": 1,
            "call_index": 0
        }),
        false,
    );
    assert_eq!(ui.transcript[provisional].title, "Changing feeds/report.md");
    assert_eq!(
        ui.transcript[provisional].tool_call_id.as_deref(),
        Some("call_write_report")
    );
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Changed)
            .count(),
        1
    );
}

#[test]
fn visible_write_preamble_does_not_leave_orphan_after_non_write_tool_message() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "Now I have all the data. Let me create the output directory and write the report."
                }]
            }
        }),
        false,
    );
    assert!(ui
        .transcript
        .iter()
        .any(|row| row.title == "Changing files"));

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "Now I have all the data. Let me create the output directory and write the report."
                    },
                    {
                        "type": "tool_call",
                        "id": "call_time",
                        "name": "bash",
                        "arguments": {"command": "date -u +%H-%M"},
                        "arguments_json": "{\"command\":\"date -u +%H-%M\"}",
                        "arguments_error": null,
                        "content_index": 0,
                        "call_index": 0
                    }
                ],
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        }),
        false,
    );

    assert!(
        ui.transcript
            .iter()
            .all(|row| row.title != "Changing files"),
        "{:?}",
        ui.transcript
    );
    assert!(ui
        .transcript
        .iter()
        .any(|row| row.title == "Running date -u +%H-%M"));
}

#[test]
fn repeated_visible_write_preamble_does_not_duplicate_after_concrete_write_signal() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    let preamble = "Now let me write the full report:";

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": preamble}]
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "call_write_report",
            "tool_name": "write",
            "arguments_json": "",
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": preamble}]
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": preamble},
                    {
                        "type": "tool_call",
                        "id": "call_write_report",
                        "name": "write",
                        "arguments": {"path": "feeds/report.md", "content": "body"},
                        "arguments_json": "{\"path\":\"feeds/report.md\",\"content\":\"body\"}",
                        "arguments_error": null,
                        "content_index": 0,
                        "call_index": 0
                    }
                ],
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_write_report",
            "tool_name": "write",
            "result": {"path": "feeds/report.md", "bytes_written": 4},
            "outcome": "normal",
            "elapsed_ms": 0
        }),
        false,
    );

    assert!(ui
        .transcript
        .iter()
        .all(|row| row.title != "Changing files"));
    let changed = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Changed)
        .collect::<Vec<_>>();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].title, "Changed feeds/report.md");
}

#[test]
fn active_write_removes_confusing_failure_meta_until_it_settles() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro",
            "mode": "default"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_failed",
            "tool_name": "bash",
            "result": {"output": "failed", "exit_code": 1},
            "outcome": "failed",
            "elapsed_ms": 0
        }),
        false,
    );
    assert!(ui
        .transcript
        .iter()
        .any(|row| row.kind == TranscriptKind::Meta && row.text.contains("1 failure")));

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Now let me write the full report:"}]
            }
        }),
        false,
    );

    assert!(ui
        .transcript
        .iter()
        .any(|row| row.title == "Changing files"));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );
}

#[test]
fn reasoning_delta_removes_prior_failure_meta_while_turn_continues() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro",
            "mode": "default"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_failed",
            "tool_name": "bash",
            "result": {"output": "failed", "exit_code": 1},
            "outcome": "failed",
            "elapsed_ms": 0
        }),
        false,
    );
    assert!(ui
        .transcript
        .iter()
        .any(|row| row.kind == TranscriptKind::Meta && row.text.contains("1 failure")));

    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "Let me compose the report carefully.".to_string(),
        },
        true,
        false,
    );

    assert!(ui
        .transcript
        .iter()
        .any(|row| row.kind == TranscriptKind::Thinking));
    assert!(ui
        .transcript
        .iter()
        .all(|row| row.kind != TranscriptKind::Meta));
    assert!(ui
        .transcript
        .iter()
        .all(|row| row.title != "Changing files"));
}

#[test]
fn aborted_reasoning_only_message_does_not_recreate_failure_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro",
            "mode": "default"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_failed",
            "tool_name": "bash",
            "result": {"output": "failed", "exit_code": 1},
            "outcome": "failed",
            "elapsed_ms": 0
        }),
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "Let me compose this carefully. I'll write the full report.".to_string(),
        },
        true,
        false,
    );
    ui.apply_stream_event(RunStreamEvent::ReasoningEnd, true, false);
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [],
                "finish_reason": "aborted",
                "outcome": "aborted",
                "model": "mimo-v2.5-pro",
                "provider": "xiaomi-token-plan"
            },
            "metadata": {
                "elapsed_ms": 138_768,
                "reasoning_effort": "low"
            }
        }),
        false,
    );

    assert!(ui
        .transcript
        .iter()
        .any(|row| row.kind == TranscriptKind::Thinking));
    assert!(ui
        .transcript
        .iter()
        .all(|row| row.kind != TranscriptKind::Meta));
}

#[test]
fn visible_write_preamble_provisional_row_is_removed_without_tool_call() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "Now I have all the data needed. Let me write the complete report."
                }]
            }
        }),
        false,
    );
    assert!(ui
        .transcript
        .iter()
        .any(|row| row.title == "Changing files"));

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "Actually, here is the final answer without writing."
                }],
                "timestamp_ms": 2,
                "finish_reason": "stop",
                "outcome": "normal"
            }
        }),
        false,
    );

    assert!(ui
        .transcript
        .iter()
        .all(|row| row.title != "Changing files"));
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
fn completed_live_tool_elapsed_keeps_visible_active_duration_for_all_tool_phases() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let cases = vec![
        (
            "read",
            serde_json::json!({"path": "Cargo.toml"}),
            serde_json::json!({"path": "Cargo.toml", "content": "body"}),
            TranscriptKind::Explored,
            "Explored Cargo.toml",
        ),
        (
            "search",
            serde_json::json!({"query": "Changing"}),
            serde_json::json!({"query": "Changing", "matches": []}),
            TranscriptKind::Explored,
            "Explored search Changing",
        ),
        (
            "bash",
            serde_json::json!({"command": "cargo test -p psychevo-cli"}),
            serde_json::json!({"output": "ok"}),
            TranscriptKind::Ran,
            "Ran cargo test -p psychevo-cli",
        ),
        (
            "write",
            serde_json::json!({"path": "report.md", "content": "body"}),
            serde_json::json!({"path": "report.md", "bytes_written": 4}),
            TranscriptKind::Changed,
            "Changed report.md",
        ),
        (
            "edit",
            serde_json::json!({"path": "report.md", "old": "a", "new": "b"}),
            serde_json::json!({"path": "report.md", "replacements": 1}),
            TranscriptKind::Changed,
            "Changed report.md",
        ),
    ];

    for (tool, args, result, expected_kind, expected_title) in cases {
        let mut ui = FullscreenUi::new(&app);
        let tool_call_id = format!("call_{tool}");
        ui.apply_value_event(
            &serde_json::json!({
                "type": "tool_execution_start",
                "tool_call_id": tool_call_id,
                "tool_name": tool,
                "args": args
            }),
            false,
        );
        let active_idx = ui
            .transcript
            .iter()
            .position(active_tool_row)
            .expect("active tool row");
        ui.transcript[active_idx].tool_started = Some(
            Instant::now()
                .checked_sub(Duration::from_secs(16))
                .expect("instant"),
        );
        ui.apply_value_event(
            &serde_json::json!({
                "type": "tool_execution_end",
                "tool_call_id": format!("call_{tool}"),
                "tool_name": tool,
                "result": result,
                "outcome": "normal",
                "elapsed_ms": 0
            }),
            false,
        );

        let row = ui
            .transcript
            .iter()
            .find(|row| row.kind == expected_kind)
            .expect("completed tool row");
        assert_eq!(row.title, expected_title, "{tool}");
        let elapsed = row.tool_elapsed.expect("elapsed");
        assert!(elapsed >= Duration::from_secs(16), "{tool}: {elapsed:?}");
        assert!(elapsed < Duration::from_secs(17), "{tool}: {elapsed:?}");
        assert!(row.tool_started.is_none(), "{tool}");
    }
}

#[test]
fn interrupted_pending_tool_row_stops_timer_as_interrupted() {
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
    assert_eq!(row.title, "Changed src/lib.rs");
    assert_eq!(row.text, "interrupted");
    assert!(row.interrupted);
    assert!(!row.failed);
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
fn history_reasoning_only_final_message_gets_turn_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [{"type": "reasoning", "text": "final folded report"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mimo-v2.5-pro",
            "provider": "xiaomi-token-plan"
        }),
        None,
        Some(&serde_json::json!({
            "elapsed_ms": 425_887,
            "reasoning_effort": "low"
        })),
    );

    assert_eq!(ui.transcript[0].kind, TranscriptKind::Thinking);
    assert_eq!(ui.transcript[0].text, "final folded report");
    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Meta)
        .expect("meta row");
    assert_eq!(row.text, "xiaomi-token-plan/mimo-v2.5-pro low  7m05s");
}

#[test]
fn history_aborted_reasoning_only_message_does_not_get_turn_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "reasoning",
                "text": "Let me compose this carefully. I'll write the full report."
            }],
            "timestamp_ms": 2,
            "finish_reason": "aborted",
            "outcome": "aborted",
            "model": "mimo-v2.5-pro",
            "provider": "xiaomi-token-plan"
        }),
        None,
        Some(&serde_json::json!({
            "elapsed_ms": 138_768,
            "reasoning_effort": "low"
        })),
    );

    assert_eq!(ui.transcript[0].kind, TranscriptKind::Thinking);
    assert!(ui
        .transcript
        .iter()
        .all(|row| row.kind != TranscriptKind::Meta));
}

#[test]
fn history_tool_call_reasoning_message_does_not_get_turn_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "reasoning", "text": "I need to inspect a file."},
                {
                    "type": "tool_call",
                    "id": "call_read",
                    "name": "read",
                    "arguments": {"path": "Cargo.toml"},
                    "arguments_json": "{\"path\":\"Cargo.toml\"}",
                    "arguments_error": null,
                    "content_index": 1,
                    "call_index": 0
                }
            ],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
        Some(&serde_json::json!({"elapsed_ms": 230})),
    );

    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
    assert!(ui
        .transcript
        .iter()
        .any(|row| row.title == "Exploring Cargo.toml"));
}

#[test]
fn history_aborted_tool_calls_render_interrupted_without_live_timer() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "reasoning", "text": "Let me continue fetching the remaining stories."},
                {
                    "type": "tool_call",
                    "id": "call_story",
                    "name": "bash",
                    "arguments": {
                        "command": "cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \"SELECT content FROM stories WHERE id = 48074265;\" 2>&1 | head -c 3000",
                        "timeout": 10
                    },
                    "arguments_json": "{\"command\":\"cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \\\"SELECT content FROM stories WHERE id = 48074265;\\\" 2>&1 | head -c 3000\",\"timeout\":10}",
                    "arguments_error": null,
                    "content_index": 1,
                    "call_index": 0
                }
            ],
            "timestamp_ms": 2,
            "finish_reason": "aborted",
            "outcome": "aborted",
            "model": "mimo-v2.5-pro",
            "provider": "xiaomi-token-plan"
        }),
        None,
        Some(&serde_json::json!({
            "elapsed_ms": 34_653,
            "reasoning_effort": "low"
        })),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("interrupted bash row");
    assert!(row.title.starts_with("Ran cd /home/kevin/Projects/feedgarden"));
    assert!(!row.title.starts_with("Running "));
    assert_eq!(row.text, "interrupted");
    assert!(row.interrupted);
    assert!(!row.failed);
    assert_eq!(row.tool_elapsed, Some(Duration::from_millis(34_653)));
    assert!(row.tool_started.is_none());
    assert!(ui.tool_rows.is_empty());
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
}

#[test]
fn history_text_plus_tool_call_message_shows_active_row_without_turn_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Now I have all the data I need."},
                {
                    "type": "tool_call",
                    "id": "call_write_report",
                    "name": "write",
                    "arguments": {
                        "path": "/tmp/hackernews-hot-05-15.md",
                        "content": "report body"
                    },
                    "arguments_json": "{\"path\":\"/tmp/hackernews-hot-05-15.md\",\"content\":\"report body\"}",
                    "arguments_error": null,
                    "content_index": 1,
                    "call_index": 0
                }
            ],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mimo-v2.5-pro",
            "provider": "xiaomi-token-plan"
        }),
        None,
        Some(&serde_json::json!({
            "elapsed_ms": 186_260,
            "reasoning_effort": "low"
        })),
    );

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer)
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Changed
            && row.title == "Changing /tmp/hackernews-hot-05-15.md"
            && row.tool_started.is_some()
    }));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
}

#[test]
fn history_aborted_tool_result_renders_interrupted_without_failure_style() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_find",
                "name": "bash",
                "arguments": {
                    "command": "find /home/kevin -name tmp.txt -type f",
                    "timeout": 10
                },
                "arguments_json": "{\"command\":\"find /home/kevin -name tmp.txt -type f\",\"timeout\":10}",
                "arguments_error": null,
                "content_index": 0,
                "call_index": 0
            }],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
        None,
    );
    ui.push_history_message(
        &serde_json::json!({
            "role": "tool_result",
            "tool_name": "bash",
            "tool_call_id": "call_find",
            "content": "{\"output\":\"(no output)\",\"exit_code\":null,\"error\":\"aborted\",\"truncated\":false}",
            "is_error": true
        }),
        None,
        Some(&serde_json::json!({ "elapsed_ms": 4_000 })),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("interrupted history row");
    assert_eq!(row.title, "Ran find /home/kevin -name tmp.txt -type f");
    assert_eq!(row.text, "interrupted");
    assert!(row.interrupted);
    assert!(!row.failed);
    assert_eq!(row.tool_elapsed, Some(Duration::from_secs(4)));
    assert!(row.tool_started.is_none());
}

#[test]
fn history_bash_timeout_renders_timeout_before_partial_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_fetch",
                "name": "bash",
                "arguments": {
                    "command": "python scripts/fetch.py",
                    "timeout": 120
                },
                "arguments_json": "{\"command\":\"python scripts/fetch.py\",\"timeout\":120}",
                "arguments_error": null,
                "content_index": 0,
                "call_index": 0
            }],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
        None,
    );
    ui.push_history_message(
        &serde_json::json!({
            "role": "tool_result",
            "tool_name": "bash",
            "tool_call_id": "call_fetch",
            "content": "{\"output\":\"[fetch] 29 rows done\",\"exit_code\":null,\"error\":\"command timed out after 120 seconds\",\"truncated\":false}",
            "is_error": true
        }),
        None,
        Some(&serde_json::json!({ "elapsed_ms": 120_000 })),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("timeout history row");
    assert_eq!(row.title, "Ran python scripts/fetch.py");
    assert!(row.failed);
    assert!(row.text.starts_with(
        "timeout: command timed out after 120 seconds; partial output follows\n"
    ));
    assert!(row.text.contains("[fetch] 29 rows done"));
}

#[test]
fn history_answer_turn_meta_omits_accounting_cost() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message_with_accounting(
        &serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "done"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
        Some(&serde_json::json!({ "elapsed_ms": 42 })),
        Some(&serde_json::json!({ "estimated_cost_nanodollars": 99_000 })),
    );

    let meta = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Meta)
        .expect("history meta row");
    assert_eq!(meta.text, "mock/mock-model  0s");
    assert!(!meta.text.contains("cost"));
}

#[test]
fn history_user_image_display_metadata_renders_prompt_and_attachment_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "user",
            "content": [
                {"type": "local_image", "path": "/tmp/image.png"},
                {"type": "text", "text": "describe it"}
            ],
            "timestamp_ms": 1
        }),
        None,
        Some(&serde_json::json!({
            "tui_display": {
                "content_text": "[Image #1] describe it",
                "attachments": [
                    {
                        "kind": "image",
                        "placeholder": "[Image #1]",
                        "source": "image.png"
                    }
                ]
            }
        })),
    );

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Prompt && row.text == "[Image #1] describe it"
    }));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Meta
            && row.text == "attachments\nimage 1: image.png"
    }));
}

#[test]
fn legacy_history_image_blocks_render_as_attachment_meta_not_prompt_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "user",
            "content": [
                {"type": "local_image", "path": "/tmp/image.png"},
                {"type": "text", "text": "describe it"}
            ],
            "timestamp_ms": 1
        }),
        None,
        None,
    );

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Prompt && row.text == "describe it"
    }));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Meta
            && row.text == "attachments\nimage 1: /tmp/image.png"
    }));
}

#[test]
fn history_tool_result_updates_rehydrated_pending_write_row() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "text", "text": "NYT is behind a paywall. Let me now write the report."},
                {
                    "type": "tool_call",
                    "id": "call_write_report",
                    "name": "write",
                    "arguments": {
                        "path": "feeds/2026-05-10/hackernews-hot-06-42.md",
                        "content": "report body"
                    },
                    "arguments_json": "{\"path\":\"feeds/2026-05-10/hackernews-hot-06-42.md\",\"content\":\"report body\"}",
                    "arguments_error": null,
                    "content_index": 1,
                    "call_index": 0
                }
            ],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mimo-v2.5-pro",
            "provider": "xiaomi-token-plan"
        }),
        None,
        Some(&serde_json::json!({
            "elapsed_ms": 174_093,
            "reasoning_effort": "low"
        })),
    );

    assert!(ui
        .transcript
        .iter()
        .any(|row| row.title == "Changing feeds/2026-05-10/hackernews-hot-06-42.md"));

    ui.push_history_message(
        &serde_json::json!({
            "role": "tool_result",
            "tool_call_id": "call_write_report",
            "tool_name": "write",
            "content": "{\"bytes_written\":26779,\"dirs_created\":false,\"error\":null,\"path\":\"feeds/2026-05-10/hackernews-hot-06-42.md\"}",
            "is_error": false,
            "timestamp_ms": 3
        }),
        None,
        Some(&serde_json::json!({"elapsed_ms": 0})),
    );

    let changed = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Changed)
        .collect::<Vec<_>>();
    assert_eq!(changed.len(), 1);
    assert_eq!(
        changed[0].title,
        "Changed feeds/2026-05-10/hackernews-hot-06-42.md"
    );
    assert_eq!(changed[0].tool_elapsed, Some(Duration::from_millis(0)));
    assert!(changed[0].tool_started.is_none());
    assert!(ui.tool_rows.is_empty());
}

#[test]
fn live_reasoning_only_final_message_gets_turn_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro",
            "mode": "default"
        }),
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "final folded report".to_string(),
        },
        true,
        false,
    );
    ui.apply_stream_event(RunStreamEvent::ReasoningEnd, true, false);
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [],
                "timestamp_ms": 2,
                "finish_reason": "stop",
                "outcome": "normal",
                "model": "mimo-v2.5-pro",
                "provider": "xiaomi-token-plan"
            },
            "metadata": {
                "elapsed_ms": 425_887,
                "reasoning_effort": "low"
            }
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Meta)
        .expect("meta row");
    assert_eq!(row.text, "xiaomi-token-plan/mimo-v2.5-pro low  7m05s");
}

#[test]
fn bottom_context_usage_stays_visible_while_model_answers_without_usage() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.last_context_snapshot = Some(test_context_snapshot());
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Streaming answer without usage yet."}]
            }
        }),
        false,
    );
    ui.refresh_sidebar(&app);

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 18);
    let text = buffer_text(&buffer);
    assert!(text.contains("~50/100 (50.0%) estimated"), "{text}");
    assert!(!text.contains("tokens:"), "{text}");

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Final answer without usage."}],
                "finish_reason": "stop",
                "outcome": "normal"
            }
        }),
        false,
    );
    ui.refresh_sidebar(&app);

    assert!(ui.last_context_snapshot.is_some());
}

#[test]
fn bottom_status_line_renders_minimal_workdir_branch_and_context() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar.branch = "main".to_string();
    ui.last_context_snapshot = Some(test_context_snapshot());

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 12);
    let text = buffer_text(&buffer);

    assert!(
        text.contains("mock/model  high  ~/work · main · ~50/100 (50.0%) estimated"),
        "{text}"
    );
    assert!(!text.contains("workdir:"), "{text}");
    assert!(!text.contains("context:"), "{text}");
}

#[test]
fn bottom_status_context_hides_missing_branch_and_unknown_limit() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar.branch = "(none)".to_string();
    ui.last_context_snapshot = Some(test_context_snapshot());

    let text = bottom_status_context_for_width(&app, &ui, 80).expect("status context");
    assert_eq!(text, "~/work · ~50/100 (50.0%) estimated");

    let mut snapshot = test_context_snapshot();
    snapshot.context_limit = None;
    snapshot.total.percent = None;
    ui.last_context_snapshot = Some(snapshot);
    let text = bottom_status_context_for_width(&app, &ui, 80).expect("status context");
    assert_eq!(text, "~/work");
}

#[test]
fn bottom_status_context_uses_live_input_usage_before_snapshot() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar.branch = "main".to_string();
    ui.sidebar_tokens = Some(29_800);
    ui.sidebar_context_limit = Some(1_000_000);

    let text = bottom_status_context_for_width(&app, &ui, 80).expect("status context");

    assert_eq!(text, "~/work · main · 29.8k/1.0M (3.0%)");
}

#[test]
fn bottom_status_context_hides_branch_before_context_usage() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar.branch = "main".to_string();
    ui.last_context_snapshot = Some(test_context_snapshot());

    let text = bottom_status_context_for_width(&app, &ui, 36).expect("status context");

    assert_eq!(text, "~/work · ~50/100 (50.0%) estimated");
}

#[test]
fn directory_display_uses_home_prefix_and_display_width_truncation() {
    let home = Path::new("/home/kevin");

    assert_eq!(
        format_directory_display_with_home(Path::new("/home/kevin"), Some(home), 80),
        "~"
    );
    assert_eq!(
        format_directory_display_with_home(
            Path::new("/home/kevin/Projects/psychevo"),
            Some(home),
            80
        ),
        "~/Projects/psychevo"
    );
    assert_eq!(
        format_directory_display_with_home(Path::new("/opt/work"), Some(home), 80),
        "/opt/work"
    );

    let truncated = format_directory_display_with_home(
        Path::new("/home/kevin/项目/非常非常长的目录名/work"),
        Some(home),
        12,
    );
    assert!(truncated.contains('…'), "{truncated}");
    assert!(
        UnicodeWidthStr::width(truncated.as_str()) <= 12,
        "{truncated}"
    );
}

#[test]
fn last_context_input_token_count_uses_input_tokens_when_later_usage_arrives() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_tokens = Some(12_345);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Final answer with usage."}],
                "finish_reason": "stop",
                "outcome": "normal"
            },
            "usage": {
                "input_tokens": 20_000,
                "output_tokens": 3_456,
                "total_tokens": 23_456
            }
        }),
        false,
    );

    assert_eq!(ui.sidebar_tokens, Some(20_000));
}

#[test]
fn last_context_input_token_count_ignores_total_tokens_without_input_tokens() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_tokens = Some(12_345);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Final answer with total-only usage."}],
                "finish_reason": "stop",
                "outcome": "normal"
            },
            "usage": {"total_tokens": 23_456}
        }),
        false,
    );

    assert_eq!(ui.sidebar_tokens, Some(12_345));
}

#[test]
fn live_tool_call_reasoning_message_does_not_get_turn_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "mock",
            "model": "mock-model",
            "mode": "default"
        }),
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "I need to inspect a file.".to_string(),
        },
        true,
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_read",
                    "name": "read",
                    "arguments": {"path": "Cargo.toml"},
                    "arguments_json": "{\"path\":\"Cargo.toml\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }],
                "timestamp_ms": 2,
                "finish_reason": "tool_calls",
                "outcome": "normal",
                "model": "mock-model",
                "provider": "mock"
            },
            "metadata": {"elapsed_ms": 230}
        }),
        false,
    );

    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
}

#[test]
fn reasoning_only_write_message_shows_active_changing_without_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro",
            "mode": "default"
        }),
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "Let me compose the full report now. I have all the data. Let me write it out."
                .to_string(),
        },
        true,
        false,
    );

    let provisional = ui
        .transcript
        .iter()
        .position(|row| row.title == "Changing files")
        .expect("provisional changing row");
    assert!(ui.transcript[provisional].tool_started.is_some());
    assert_eq!(ui.transcript[provisional].tool_call_id, None);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.title != "Changing /tmp/hackernews-hot-05-39.md"),
        "{:?}",
        ui.transcript
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_write_report",
                    "name": "write",
                    "arguments": {
                        "path": "/tmp/hackernews-hot-05-39.md",
                        "content": "report body"
                    },
                    "arguments_json": "{\"path\":\"/tmp/hackernews-hot-05-39.md\",\"content\":\"report body\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }],
                "timestamp_ms": 2,
                "finish_reason": "tool_calls",
                "outcome": "normal",
                "model": "mimo-v2.5-pro",
                "provider": "xiaomi-token-plan"
            },
            "metadata": {
                "elapsed_ms": 190_546,
                "reasoning_effort": "low"
            }
        }),
        false,
    );

    let thinking = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    let changing = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Changed)
        .expect("changing row");
    assert!(thinking < changing);
    assert_eq!(
        ui.transcript[changing].title,
        "Changing /tmp/hackernews-hot-05-39.md"
    );
    assert!(ui.transcript[changing].tool_started.is_some());
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Answer)
    );
}

#[test]
fn hidden_thinking_write_intent_does_not_create_provisional_changing() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();

    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "Let me compose the full report now. Let me write it out.".to_string(),
        },
        false,
        false,
    );

    assert!(ui
        .transcript
        .iter()
        .any(|row| row.kind == TranscriptKind::Thinking));
    assert!(ui
        .transcript
        .iter()
        .all(|row| row.title != "Changing files"));
}

#[test]
fn visible_thinking_run_intent_creates_and_reconciles_running_command() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();

    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "Let me run a quick command to verify the file size.".to_string(),
        },
        true,
        false,
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Ran
            && row.title == "Running command"
            && row.tool_started.is_some()
    }));

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_wc",
                    "name": "bash",
                    "arguments": {"command": "wc -c report.md"},
                    "arguments_json": "{\"command\":\"wc -c report.md\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }],
                "timestamp_ms": 2,
                "finish_reason": "tool_calls",
                "outcome": "normal",
                "model": "mimo-v2.5-pro",
                "provider": "xiaomi-token-plan"
            }
        }),
        false,
    );

    let rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Ran)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Running wc -c report.md");
    assert_eq!(rows[0].tool_call_id.as_deref(), Some("call_wc"));
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
    let lines = thinking_lines(&row, false, true, 80);

    assert_eq!(lines.len(), 4);
    assert!(line_text(&lines[0]).contains("Thinking"));
    assert_eq!(lines[1].spans[0].content.as_ref(), "  └ ");
    assert_eq!(lines[1].spans[1].content.as_ref(), "First paragraph.");
    assert_eq!(lines[3].spans[0].content.as_ref(), "    ");
    assert_eq!(lines[3].spans[1].content.as_ref(), "Second paragraph.");
    assert!(!line_text(&lines[0]).contains("Thinking:"));
    assert!(!line_text(&lines[0]).contains("▌"));
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
fn bash_tool_title_skips_leading_shell_comments() {
    let title = tool_title(
        "bash",
        &serde_json::json!({
            "args": {
                "command": "\n# Try webcache for the NYT article\ncurl -sL https://example.com | python3 -c 'print(1)'"
            }
        }),
    );
    assert_eq!(
        title,
        "Ran curl -sL https://example.com | python3 -c 'print(1)'"
    );

    let active = active_tool_title(
        "bash",
        &serde_json::json!({
            "args": {
                "command": "  # Get all comments with full text\npython3 -c 'print(42)'"
            }
        }),
    );
    assert_eq!(active, "Running python3 -c 'print(42)'");
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
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/rename Better Session Title"
            && row.text == "session renamed: Better Session Title"
    }));
    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some("Better Session Title"));
}

#[tokio::test]
async fn obsolete_thinking_command_is_unknown_in_fullscreen() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/thinking");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/thinking"
            && row.failed
            && row.text.contains("error: unknown slash command: /thinking")
    }));
}
