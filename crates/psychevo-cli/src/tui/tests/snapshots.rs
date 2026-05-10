#[test]
fn tui_snapshot_wide_idle_minimal_chrome() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::Idle);
    assert_tui_snapshot("wide_idle_minimal_chrome", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_wide_optional_sidebar() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    assert_tui_snapshot("wide_optional_sidebar", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_narrow_idle_composer_without_sidebar() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::Idle);
    assert_tui_snapshot("narrow_idle_composer_without_sidebar", 80, 20, &app, ui);
}

#[test]
fn tui_snapshot_slash_menu_prefix_filtering() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.textarea = textarea_with_text("/mo");
    assert_tui_snapshot("slash_menu_prefix_filtering", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_file_completion_popup() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.textarea = textarea_with_text("review @src");
    ui.file_search.popup = Some(FileSearchPopupState {
        query: "src".to_string(),
        matches: vec![
            FileSearchMatch {
                path: "src".to_string(),
                kind: FileSearchMatchKind::Directory,
            },
            FileSearchMatch {
                path: "src/main.rs".to_string(),
                kind: FileSearchMatchKind::File,
            },
            FileSearchMatch {
                path: "crates/psychevo-cli/src/tui/mod.rs".to_string(),
                kind: FileSearchMatchKind::File,
            },
        ],
        selected: 1,
        waiting: false,
    });
    assert_tui_snapshot("file_completion_popup", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_model_bottom_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.bottom_panel = Some(BottomPanel::Models(
        app.model_selection_panel().expect("model panel"),
    ));
    assert_tui_snapshot("model_bottom_panel", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_variant_bottom_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let models = app.model_selection_panel().expect("model panel");
    let (other, source) = models
        .rows
        .iter()
        .find_map(|row| match &row.value {
            BottomSelectionValue::Model { model, source } if model.model == "other-model" => {
                Some((model.clone(), *source))
            }
            _ => None,
        })
        .expect("other model");
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.bottom_panel = Some(app.variant_panel(*other, source, models));
    assert_tui_snapshot("variant_bottom_panel", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_session_bottom_panel() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.bottom_panel = Some(BottomPanel::Sessions(stable_session_bottom_panel()));
    assert_tui_snapshot("session_bottom_panel", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_archived_session_action_bottom_panel() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    let mut panel = stable_archived_session_bottom_panel();
    panel.arm_action_mode();
    ui.bottom_panel = Some(BottomPanel::Sessions(panel));
    assert_tui_snapshot("archived_session_action_bottom_panel", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_running_turn_with_visible_thinking() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::RunningThinking);
    assert_tui_snapshot("running_turn_with_visible_thinking", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_completed_ledger_collapsed_tool_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::CollapsedTool);
    assert_tui_snapshot("completed_ledger_collapsed_tool_output", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_expanded_long_tool_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::ExpandedTool);
    assert_tui_snapshot("expanded_long_tool_output", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_expanded_long_command() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::ExpandedLongCommand);
    assert_tui_snapshot("expanded_long_command", 120, 18, &app, ui);
}

#[test]
fn tui_snapshot_collapsed_long_json_tool_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::CollapsedJsonTool);
    assert_tui_snapshot("collapsed_long_json_tool_output", 120, 18, &app, ui);
}

#[test]
fn tui_snapshot_long_command_folded_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::LongCommandFoldedOutput);
    assert_tui_snapshot("long_command_folded_output", 120, 18, &app, ui);
}

#[test]
fn tui_snapshot_consecutive_tool_rows_flat() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::ConsecutiveToolRows);
    assert_tui_snapshot("consecutive_tool_rows_flat", 120, 20, &app, ui);
}

#[test]
fn tui_snapshot_rich_markdown_answer() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::RichMarkdown);
    assert_tui_snapshot("rich_markdown_answer", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_active_write_after_visible_answer() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::ActiveWriteAfterAnswer);
    assert_tui_snapshot("active_write_after_visible_answer", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_active_visible_write_preamble() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::ActiveVisibleWritePreamble);
    assert_tui_snapshot("active_visible_write_preamble", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_history_pending_write_call() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": "NYT is behind a paywall. Based on comments, I can still summarize the Meta article. Let me now write the report."
                },
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
            "timestamp_ms": 1,
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
    for row in &mut ui.transcript {
        if row.title.starts_with("Changing ") {
            row.tool_started = Some(
                Instant::now()
                    .checked_sub(Duration::from_secs(2))
                    .expect("instant"),
            );
        }
    }
    assert_tui_snapshot("history_pending_write_call", 120, 18, &app, ui);
}

#[test]
fn tui_snapshot_active_write_suppresses_failure_meta() {
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
    for row in &mut ui.transcript {
        if row.title.starts_with("Changing ") {
            row.tool_started = Some(
                Instant::now()
                    .checked_sub(Duration::from_secs(2))
                    .expect("instant"),
            );
        }
    }
    assert_tui_snapshot("active_write_suppresses_failure_meta", 120, 18, &app, ui);
}

#[test]
fn tui_snapshot_reasoning_suppresses_failure_meta() {
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
            text: "The current time is approximately 15:20 UTC. Let me compose the full report now. I have all 10 stories and their comments.\n\nLet me compose this carefully.".to_string(),
        },
        true,
        false,
    );
    assert_tui_snapshot("reasoning_suppresses_failure_meta", 120, 18, &app, ui);
}

#[test]
fn tui_snapshot_active_reasoning_write() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::ActiveReasoningWrite);
    assert_tui_snapshot("active_reasoning_write", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_long_markdown_meta_bottom_scroll() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::LongMarkdownBottom);
    assert_tui_snapshot("long_markdown_meta_bottom_scroll", 100, 18, &app, ui);
}

#[test]
fn tui_snapshot_long_thinking_markdown_meta_bottom_scroll() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::LongThinkingMarkdownBottom);
    assert_tui_snapshot("long_thinking_markdown_meta_bottom_scroll", 100, 18, &app, ui);
}

#[test]
fn tui_snapshot_long_thinking_markdown_expanded_meta_bottom_scroll() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::LongThinkingMarkdownExpandedBottom);
    assert_tui_snapshot(
        "long_thinking_markdown_expanded_meta_bottom_scroll",
        100,
        18,
        &app,
        ui,
    );
}

#[test]
fn tui_snapshot_debug_meta_with_usage_metadata() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.debug = true;
    let ui = fixture_ui(&app, FixtureKind::DebugMeta);
    assert_tui_snapshot("debug_meta_with_usage_metadata", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_failure_tool_error_turn_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::FailureMeta);
    assert_tui_snapshot("failure_tool_error_turn_meta", 120, 24, &app, ui);
}
