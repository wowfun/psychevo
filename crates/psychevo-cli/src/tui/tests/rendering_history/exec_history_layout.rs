#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn completed_streaming_write_stdin_poll_placeholder_is_removed() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, "exec_command long fetch", "");
    row.tool_name = Some("exec_command".to_string());
    row.tool_call_id = Some("call_exec".to_string());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.exec_session_rows.insert(0, 0);

    ui.apply_stream_event(
        RunStreamEvent::value(serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_poll",
                    "name": "write_stdin",
                    "arguments_json": "{\"session_id\":",
                    "content_index": 0,
                    "call_index": 0
                }]
            }
        })),
        true,
        false,
    );
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.tool_name.as_deref() == Some("write_stdin"))
    );

    ui.apply_stream_event(
        RunStreamEvent::value(serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_poll",
                    "name": "write_stdin",
                    "arguments_json": "{\"session_id\":0,\"yield_time_ms\":30000}",
                    "content_index": 0,
                    "call_index": 0
                }]
            }
        })),
        true,
        false,
    );

    assert_eq!(ui.transcript.len(), 1);
    assert_eq!(ui.transcript[0].tool_name.as_deref(), Some("exec_command"));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.tool_name.as_deref() != Some("write_stdin"))
    );
}

#[test]
pub(crate) fn non_empty_stdin_renders_as_compact_terminal_interaction() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, "exec_command read", "");
    row.tool_name = Some("exec_command".to_string());
    row.tool_call_id = Some("call_exec".to_string());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.exec_session_rows.insert(7, 0);

    ui.apply_stream_event(
        RunStreamEvent::value(serde_json::json!({
            "type": "exec_session_stdin",
            "session_id": 7,
            "tool_call_id": "call_exec",
            "write_tool_call_id": "call_stdin",
            "chars": "hello\n"
        })),
        true,
        false,
    );

    assert_eq!(ui.transcript.len(), 2);
    assert_eq!(ui.transcript[1].title, "stdin 7");
    assert_eq!(ui.transcript[1].text, "hello\n");
    assert_ne!(ui.transcript[1].title, "write_stdin 7");
}

#[test]
pub(crate) fn history_replay_merges_exec_command_and_write_stdin_chunks() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    let exec_args = serde_json::json!({"cmd": "long running"});
    ui.push_history_active_tool_call(
        &serde_json::json!({"timestamp_ms": 1}),
        HistoryToolCall {
            id: "call_exec".to_string(),
            name: "exec_command".to_string(),
            active_title: active_tool_title(
                "exec_command",
                &serde_json::json!({"args": exec_args.clone()}),
            ),
            completed_title: tool_title(
                "exec_command",
                &serde_json::json!({"args": exec_args.clone()}),
            ),
            args: exec_args,
        },
    );
    ui.push_history_tool_result(
        &serde_json::json!({
            "tool_name": "exec_command",
            "tool_call_id": "call_exec",
            "content": serde_json::to_string(&serde_json::json!({
                "chunk_id": 0,
                "wall_time_seconds": 0.25,
                "exit_code": null,
                "session_id": 99,
                "original_token_count": 1,
                "output": "start"
            })).expect("json"),
            "is_error": false
        }),
        Some(&serde_json::json!({"elapsed_ms": 250})),
    );

    let write_args = serde_json::json!({"session_id": 99, "chars": ""});
    ui.push_history_active_tool_call(
        &serde_json::json!({"timestamp_ms": 2}),
        HistoryToolCall {
            id: "call_poll".to_string(),
            name: "write_stdin".to_string(),
            active_title: active_tool_title(
                "write_stdin",
                &serde_json::json!({"args": write_args.clone()}),
            ),
            completed_title: tool_title(
                "write_stdin",
                &serde_json::json!({"args": write_args.clone()}),
            ),
            args: write_args,
        },
    );
    ui.push_history_tool_result(
        &serde_json::json!({
            "tool_name": "write_stdin",
            "tool_call_id": "call_poll",
            "content": serde_json::to_string(&serde_json::json!({
                "chunk_id": 1,
                "wall_time_seconds": 1.0,
                "exit_code": 0,
                "session_id": null,
                "original_token_count": 1,
                "output": "done"
            })).expect("json"),
            "is_error": false
        }),
        Some(&serde_json::json!({"elapsed_ms": 1000})),
    );

    assert_eq!(ui.transcript.len(), 1);
    assert_eq!(ui.transcript[0].text, "startdone");
    assert_eq!(
        ui.transcript[0].tool_elapsed,
        Some(Duration::from_millis(1250))
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.title.contains("write_stdin"))
    );
}

#[test]
pub(crate) fn transcript_viewport_excludes_bottom_border_from_scroll_height() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::simple(
        TranscriptKind::Answer,
        "bottom line must be visible",
    ));

    let _buffer = draw_fullscreen_for_test(&app, &mut ui, 48, 10);

    assert_eq!(ui.last_transcript_height, 7);
}

#[test]
pub(crate) fn transcript_render_clears_stale_cells_after_shorter_redraw() {
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
pub(crate) fn history_reload_scrolls_to_end_of_multiline_markdown_answer() {
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
pub(crate) fn transcript_bottom_scroll_uses_paragraph_word_wrapping() {
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
pub(crate) fn transcript_layout_cache_reuses_row_heights_while_scrolling() {
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
pub(crate) fn manual_down_scroll_reaches_long_markdown_bottom_with_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let table = (1..=36)
        .map(|index| {
            format!("| {index} | **Story {index}** with a long mixed-width summary | {index} |")
        })
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
pub(crate) fn manual_down_scroll_reaches_long_thinking_table_bottom() {
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
    ui.transcript.last_mut().expect("thinking row").expanded = true;
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
pub(crate) fn transcript_focus_down_scrolls_selected_row_into_view() {
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
pub(crate) fn active_tool_layout_key_tracks_elapsed_for_cache() {
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
pub(crate) fn long_read_tool_output_collapses_and_preserves_full_text() {
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
    assert_eq!(row.title, "read src/long.rs");
    assert_eq!(row.text.lines().count(), 7);
    assert!(row.text.contains("... 58 more lines"));
    assert_eq!(row.full_text.as_deref(), Some(content.as_str()));
    assert!(row.is_expandable());
}

#[test]
pub(crate) fn running_tool_title_right_aligns_elapsed_duration() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        "exec_command cargo test --workspace --all-targets",
        "running",
    );
    row.tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_millis(1_200))
            .expect("instant"),
    );

    let title = line_text(&tool_lines(&row, false, true, 36)[0]);

    assert!(title.contains("exec_command cargo"));
    assert!(!title.starts_with("• "));
    assert!(title.ends_with("1s"));
    assert_eq!(UnicodeWidthStr::width(title.as_str()), 35);
}

#[test]
pub(crate) fn running_tool_title_hides_subsecond_elapsed_duration() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        "exec_command cargo test --workspace --all-targets",
        "running",
    );
    row.tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_millis(120))
            .expect("instant"),
    );

    let title = line_text(&tool_lines(&row, false, true, 36)[0]);

    assert!(title.contains("exec_command cargo"));
    assert!(!title.contains("0s"));
}

#[test]
pub(crate) fn active_tool_rows_render_present_tense_without_redundant_body() {
    let started = Instant::now()
        .checked_sub(Duration::from_secs(2))
        .expect("instant");
    for (kind, stored_title, expected_title) in [
        (
            TranscriptKind::Explored,
            "read Cargo.toml",
            "read Cargo.toml",
        ),
        (
            TranscriptKind::Ran,
            "exec_command cargo test -p psychevo-cli",
            "exec_command cargo test -p psychevo-cli",
        ),
        (
            TranscriptKind::Updated,
            "write src/lib.rs",
            "write src/lib.rs",
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
pub(crate) fn expandable_tool_title_uses_text_hint_without_brackets() {
    let content = (1..=12)
        .map(|line| format!("line {line:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Explored,
        "read src/long.rs",
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
pub(crate) fn completed_tool_title_uses_fixed_elapsed_duration() {
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored src/lib.rs", "");
    row.tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_secs(5))
            .expect("instant"),
    );
    row.tool_elapsed = Some(Duration::from_millis(120));

    let title = line_text(&tool_lines(&row, false, true, 32)[0]);

    assert!(!title.contains("0s"));
    assert!(!title.contains("5."));
}

#[test]
pub(crate) fn narrow_tool_title_preserves_elapsed_duration() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        "exec_command cargo test --workspace --all-targets",
        "",
    );
    row.tool_elapsed = Some(Duration::from_millis(12_340));

    let title = line_text(&tool_lines(&row, false, true, 18)[0]);

    assert!(title.ends_with("12s"));
    assert!(title.contains('…'));
}

#[test]
pub(crate) fn completed_tool_title_formats_elapsed_minutes() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        "exec_command cargo test --workspace --all-targets",
        "",
    );
    row.tool_elapsed = Some(Duration::from_millis(140_000));

    let title = line_text(&tool_lines(&row, false, true, 22)[0]);

    assert!(title.ends_with("2m20s"));
    assert!(title.contains('…'));
}

#[test]
pub(crate) fn streaming_tool_call_creates_pending_updating_row_before_execution() {
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
    assert_eq!(ui.transcript[0].kind, TranscriptKind::Updated);
    assert_eq!(ui.transcript[0].title, "write");
    assert_eq!(ui.transcript[0].text, "preparing");
    assert!(ui.transcript[0].tool_started.is_some());
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Answer)
    );
}

#[test]
pub(crate) fn streaming_tool_call_after_visible_text_stays_below_preamble() {
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

    let preamble = ui
        .transcript
        .iter()
        .position(|row| {
            row.kind == TranscriptKind::Thinking
                && row.title == "Thinking"
                && row.text == "Now let me write the complete report."
        })
        .expect("preamble row");
    let tool = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Updated)
        .expect("tool row");
    assert!(preamble < tool);
    assert_eq!(ui.transcript[tool].title, "write report.md");
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Answer)
    );

    ui.scroll_to_bottom();
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 8);
    let text = buffer_text(&buffer);
    assert!(text.contains("write report.md"), "{text}");
    assert!(!text.contains("Preamble"), "{text}");
    assert!(!text.contains("Tool calls"), "{text}");
}

#[test]
pub(crate) fn message_end_text_plus_write_tool_shows_active_row_without_intermediate_meta() {
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

    let preamble = ui
        .transcript
        .iter()
        .position(|row| {
            row.kind == TranscriptKind::Thinking
                && row.title == "Thinking"
                && row.text == "Now I have all the data I need. Let me write the full report:"
        })
        .expect("preamble row");
    let tool = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Updated)
        .expect("tool row");
    assert!(preamble < tool);
    assert_eq!(
        ui.transcript[tool].title,
        "write /tmp/hackernews-hot-05-15.md"
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Answer)
    );
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
    assert!(
        text.contains("write /tmp/hackernews-hot-05-15.md"),
        "{text}"
    );
    assert!(!text.contains("Tool calls"), "{text}");
    assert!(!text.contains("xiaomi-token-plan/mimo-v2.5-pro"), "{text}");
}
