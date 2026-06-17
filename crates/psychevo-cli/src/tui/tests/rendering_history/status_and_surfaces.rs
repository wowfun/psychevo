#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn history_tool_result_updates_rehydrated_pending_write_row() {
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

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.title == "write feeds/2026-05-10/hackernews-hot-06-42.md")
    );

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

    let updated = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Updated)
        .collect::<Vec<_>>();
    assert_eq!(updated.len(), 1);
    assert_eq!(
        updated[0].title,
        "write feeds/2026-05-10/hackernews-hot-06-42.md"
    );
    assert_eq!(updated[0].tool_elapsed, Some(Duration::from_millis(0)));
    assert!(updated[0].tool_started.is_none());
    assert!(ui.tool_rows.is_empty());
}

#[test]
pub(crate) fn live_reasoning_only_final_message_gets_turn_meta() {
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
pub(crate) fn bottom_context_usage_stays_visible_while_model_answers_without_usage() {
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
pub(crate) fn bottom_status_line_renders_minimal_workdir_branch_and_context() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar.branch = "main".to_string();
    ui.last_context_snapshot = Some(test_context_snapshot());

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 12);
    let text = buffer_text(&buffer);

    assert!(
        text.contains("mock/model  high  ~50/100 (50.0%) estimated · ~/work · main"),
        "{text}"
    );
    assert!(!text.contains("workdir:"), "{text}");
    assert!(!text.contains("context:"), "{text}");
}

#[test]
pub(crate) fn bottom_status_context_hides_missing_branch_and_unknown_limit() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar.branch = "(none)".to_string();
    ui.last_context_snapshot = Some(test_context_snapshot());

    let text = bottom_status_context_for_width(&app, &ui, 80).expect("status context");
    assert_eq!(text, "~50/100 (50.0%) estimated · ~/work");

    let mut snapshot = test_context_snapshot();
    snapshot.context_limit = None;
    snapshot.total.percent = None;
    ui.last_context_snapshot = Some(snapshot);
    let text = bottom_status_context_for_width(&app, &ui, 80).expect("status context");
    assert_eq!(text, "~/work");
}

#[test]
pub(crate) fn bottom_status_context_uses_live_input_usage_before_snapshot() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar.branch = "main".to_string();
    ui.sidebar_tokens = Some(29_800);
    ui.sidebar_context_limit = Some(1_000_000);

    let text = bottom_status_context_for_width(&app, &ui, 80).expect("status context");

    assert_eq!(text, "29.8k/1.0M (3.0%) · ~/work · main");
}

#[test]
pub(crate) fn bottom_status_context_hides_branch_before_context_usage() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar.branch = "main".to_string();
    ui.last_context_snapshot = Some(test_context_snapshot());

    let text = bottom_status_context_for_width(&app, &ui, 36).expect("status context");

    assert_eq!(text, "~50/100 (50.0%) estimated · ~/work");
}

#[test]
pub(crate) fn directory_display_uses_home_prefix_and_display_width_truncation() {
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
pub(crate) fn last_context_input_token_count_uses_input_tokens_when_later_usage_arrives() {
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
pub(crate) fn bottom_status_session_observability_degrades_with_width() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_tokens = Some(900);
    ui.sidebar_context_limit = Some(1_000);
    ui.session_usage_summary = Some(SessionUsageSummary {
        session_id: "session".to_string(),
        provider: "mock".to_string(),
        model: "mock-model".to_string(),
        message_count: 1,
        assistant_message_count: 1,
        context_input_tokens: 2_000,
        billable_input_tokens: 1_000,
        billable_output_tokens: 500,
        reasoning_tokens: 0,
        cache_read_tokens: 1_000,
        cache_write_tokens: 0,
        reported_total_tokens: 2_500,
        estimated_cost_nanodollars: 10_000_000,
        cost_status: "estimated".to_string(),
        estimated_pricing_count: 1,
        free_pricing_count: 0,
        included_pricing_count: 0,
        unknown_pricing_count: 0,
        cache_read_percent: Some(50.0),
    });

    let medium = bottom_status_context_for_width(&app, &ui, 32).expect("medium status");
    assert_eq!(medium, "900/1.0k (90.0%) · cache 50%");
    let narrow = bottom_status_context_for_width(&app, &ui, 20).expect("narrow status");
    assert_eq!(narrow, "900/1.0k (90.0%)");
}

#[test]
pub(crate) fn last_context_input_token_count_ignores_total_tokens_without_input_tokens() {
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
pub(crate) fn live_tool_call_reasoning_message_does_not_get_turn_meta() {
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
pub(crate) fn reasoning_only_write_message_waits_for_typed_tool_call() {
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

    assert!(
        ui.transcript
            .iter()
            .all(|row| row.tool_name.as_deref() != Some("write")),
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
    let updating = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Updated)
        .expect("updating row");
    assert!(thinking < updating);
    assert_eq!(
        ui.transcript[updating].title,
        "write /tmp/hackernews-hot-05-39.md"
    );
    assert!(ui.transcript[updating].tool_started.is_some());
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
pub(crate) fn hidden_thinking_write_intent_does_not_create_provisional_updating() {
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

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Thinking)
    );
    assert!(ui.transcript.iter().all(|row| row.title != "write"));
}

#[test]
pub(crate) fn visible_thinking_run_text_waits_for_typed_command_call() {
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
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.tool_name.as_deref() != Some("exec_command")),
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
                    "id": "call_wc",
                    "name": "exec_command",
                    "arguments": {"cmd": "wc -c report.md"},
                    "arguments_json": "{\"cmd\":\"wc -c report.md\"}",
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
    assert_eq!(rows[0].title, "exec_command wc -c report.md");
    assert_eq!(rows[0].tool_call_id.as_deref(), Some("call_wc"));
}

#[test]
pub(crate) fn prompt_block_uses_full_width_background_without_left_rail() {
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
pub(crate) fn composer_and_prompt_share_full_width_surface() {
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
    let composer_y = 8;

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
}

#[test]
pub(crate) fn wrapped_prompt_rows_keep_full_width_background_for_wide_text() {
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
pub(crate) fn empty_composer_uses_one_surface_row() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let backend = TestBackend::new(48, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let composer_y = 8;

    assert_eq!(
        buffer
            .cell((0, composer_y))
            .expect("composer marker")
            .symbol(),
        "›"
    );
    assert_eq!(
        buffer.cell((0, composer_y)).expect("composer row start").bg,
        TUI_SURFACE_BG
    );
    assert_eq!(
        buffer.cell((47, composer_y)).expect("composer row end").bg,
        TUI_SURFACE_BG
    );
}

#[test]
pub(crate) fn thinking_new_paragraphs_do_not_use_label_width_indent() {
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
pub(crate) fn bash_tool_title_uses_actual_first_command_line() {
    let title = tool_title(
        "exec_command",
        &serde_json::json!({
            "args": {"cmd": "cargo test -p psychevo-cli\ncargo fmt"}
        }),
    );
    assert_eq!(title, "exec_command cargo test -p psychevo-cli");
}

#[test]
pub(crate) fn bash_tool_title_skips_leading_shell_comments() {
    let title = tool_title(
        "exec_command",
        &serde_json::json!({
            "args": {
                "cmd": "\n# Try webcache for the NYT article\ncurl -sL https://example.com | python3 -c 'print(1)'"
            }
        }),
    );
    assert_eq!(
        title,
        "exec_command curl -sL https://example.com | python3 -c 'print(1)'"
    );

    let active = active_tool_title(
        "exec_command",
        &serde_json::json!({
            "args": {
                "cmd": "  # Get all comments with full text\npython3 -c 'print(42)'"
            }
        }),
    );
    assert_eq!(active, "exec_command python3 -c 'print(42)'");
}

#[test]
pub(crate) fn fullscreen_bash_title_survives_tool_end_without_args() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_bash",
            "tool_name": "exec_command",
            "args": {"cmd": "cargo test -p psychevo-cli\ncargo fmt"}
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_bash",
            "tool_name": "exec_command",
            "result": {"output": "ok", "exit_code": 0},
            "outcome": "normal"
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("exec_command row");
    assert_eq!(row.title, "exec_command cargo test -p psychevo-cli");
    assert_ne!(row.title, "exec_command command");
}

#[test]
pub(crate) fn history_tool_result_reuses_persisted_bash_command_title() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let assistant = serde_json::json!({
        "role": "assistant",
        "content": [{
            "type": "tool_call",
            "id": "call_bash",
            "name": "exec_command",
            "arguments": {
                "cmd": "find . -maxdepth 2\nprintf done"
            },
            "arguments_json": "{\"cmd\":\"find . -maxdepth 2\\nprintf done\"}",
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
        "tool_name": "exec_command",
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
        .expect("history exec_command row");
    assert_eq!(row.title, "exec_command find . -maxdepth 2");
}

#[tokio::test]
pub(crate) async fn sidebar_toggle_persists_visibility() {
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
