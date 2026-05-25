#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn failed_tool_end_does_not_create_intermediate_turn_meta() {
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
            "type": "tool_execution_start",
            "tool_call_id": "call_sqlite",
            "tool_name": "exec_command",
            "args": {"cmd": "sqlite3 -json feeds.db"}
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_sqlite",
            "tool_name": "exec_command",
            "args": {"cmd": "sqlite3 -json feeds.db"},
            "result": {"output": "[]", "exit_code": 1},
            "outcome": "failed"
        }),
        false,
    );

    let failed_tool = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("failed tool row");
    assert!(failed_tool.failed);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );

    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "The query failed; I will continue with available context.".to_string(),
        },
        true,
        false,
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );

    ui.apply_stream_event(RunStreamEvent::ReasoningEnd, true, false);
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "I continued after the failed query."}],
                "timestamp_ms": 3,
                "finish_reason": "stop",
                "outcome": "normal",
                "model": "mimo-v2.5-pro",
                "provider": "xiaomi-token-plan"
            },
            "metadata": {"elapsed_ms": 1_200}
        }),
        false,
    );

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Meta && row.text.contains("1 failure")),
        "{:?}",
        ui.transcript
    );
}

#[test]
pub(crate) fn pending_write_tool_input_shows_updating_before_complete_arguments() {
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
        .position(|row| row.kind == TranscriptKind::Updated)
        .expect("tool row");
    assert!(answer < tool);
    assert_eq!(ui.transcript[tool].title, "write");
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
    assert_eq!(ui.transcript[tool].title, "write feeds/report.md");

    ui.scroll_to_bottom();
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 8);
    let text = buffer_text(&buffer);
    assert!(text.contains("write feeds/report.md"), "{text}");
    assert!(!text.contains("Tool calls"), "{text}");
    assert!(!text.contains("xiaomi-token-plan/mimo-v2.5-pro"), "{text}");
}

#[test]
pub(crate) fn visible_write_preamble_creates_and_reconciles_provisional_updating_row() {
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
        .position(|row| row.title == "write")
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
    assert_eq!(ui.transcript[provisional].title, "write feeds/report.md");
    assert_eq!(
        ui.transcript[provisional].tool_call_id.as_deref(),
        Some("call_write_report")
    );
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Updated)
            .count(),
        1
    );
}

#[test]
pub(crate) fn visible_write_preamble_does_not_leave_orphan_after_non_write_tool_message() {
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
    assert!(ui.transcript.iter().any(|row| row.title == "write"));

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
                        "name": "exec_command",
                        "arguments": {"cmd": "date -u +%H-%M"},
                        "arguments_json": "{\"cmd\":\"date -u +%H-%M\"}",
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
        ui.transcript.iter().all(|row| row.title != "write"),
        "{:?}",
        ui.transcript
    );
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.title == "exec_command date -u +%H-%M")
    );
}

#[test]
pub(crate) fn repeated_visible_write_preamble_does_not_duplicate_after_concrete_write_signal() {
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

    assert!(ui.transcript.iter().all(|row| row.title != "write"));
    let updated = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Updated)
        .collect::<Vec<_>>();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].title, "write feeds/report.md");
}

#[test]
pub(crate) fn active_write_keeps_failed_tool_meta_suppressed_until_final_answer() {
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
            "tool_name": "exec_command",
            "result": {"output": "failed", "exit_code": 1},
            "outcome": "failed",
            "elapsed_ms": 0
        }),
        false,
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
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

    assert!(ui.transcript.iter().any(|row| row.title == "write"));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );
}

#[test]
pub(crate) fn reasoning_delta_keeps_failed_tool_meta_suppressed_while_turn_continues() {
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
            "tool_name": "exec_command",
            "result": {"output": "failed", "exit_code": 1},
            "outcome": "failed",
            "elapsed_ms": 0
        }),
        false,
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );

    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "Let me compose the report carefully.".to_string(),
        },
        true,
        false,
    );

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Thinking)
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
    assert!(ui.transcript.iter().all(|row| row.title != "write"));
}

#[test]
pub(crate) fn visible_answer_update_completes_active_thinking_without_reasoning_end() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();

    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "I have enough context and will answer now.".to_string(),
        },
        true,
        false,
    );

    let thinking = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    assert!(ui.transcript[thinking].tool_started.is_some());

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Here is the answer."}]
            }
        }),
        false,
    );

    assert_eq!(ui.reasoning_row, None);
    assert!(ui.transcript[thinking].tool_started.is_none());
    assert!(ui.transcript[thinking].tool_elapsed.is_some());
    let answer = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Answer)
        .expect("answer row");
    assert!(thinking < answer);

    let rendered = thinking_lines(&ui.transcript[thinking], false, true, 80)
        .into_iter()
        .next()
        .map(|line| line_text(&line))
        .expect("thinking row");
    assert!(!rendered.contains("0s"), "{rendered}");
}

#[test]
pub(crate) fn aborted_reasoning_only_message_does_not_recreate_failure_meta() {
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
            "tool_name": "exec_command",
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

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Thinking)
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
}

#[test]
pub(crate) fn visible_write_preamble_provisional_row_is_removed_without_tool_call() {
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
    assert!(ui.transcript.iter().any(|row| row.title == "write"));

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

    assert!(ui.transcript.iter().all(|row| row.title != "write"));
}

#[test]
pub(crate) fn streaming_tool_call_migrates_position_key_to_tool_id_without_duplicate() {
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
        .filter(|row| row.kind == TranscriptKind::Updated)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "write report.md");
    assert_eq!(rows[0].text, "running");
    assert_eq!(rows[0].tool_call_id.as_deref(), Some("call_write"));
}

#[test]
pub(crate) fn streaming_tool_completion_reuses_pending_row_as_completed_evidence() {
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
    assert_eq!(row.kind, TranscriptKind::Updated);
    assert_eq!(row.title, "write report.md");
    assert!(row.text.contains("write normal"));
    assert_eq!(row.tool_elapsed, Some(Duration::from_millis(65_000)));
    assert!(row.tool_started.is_none());
}

#[test]
pub(crate) fn late_preamble_message_end_after_tool_completion_does_not_create_temp_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "deepseek",
            "model": "deepseek-v4-pro",
            "mode": "default"
        }),
        false,
    );
    let preamble = "I'll query the local cache before answering.";
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
            "tool_call_id": "call_sqlite",
            "tool_name": "exec_command",
            "arguments_json": "{\"cmd\":\"sqlite3 feeds.db\"}",
            "content_index": 1,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_sqlite",
            "tool_name": "exec_command",
            "args": {"cmd": "sqlite3 feeds.db"},
            "result": {"output": "row 1", "exit_code": 0},
            "outcome": "normal",
            "elapsed_ms": 0
        }),
        false,
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": preamble}],
                "timestamp_ms": 2,
                "finish_reason": "stop",
                "outcome": "normal",
                "provider": "deepseek",
                "model": "deepseek-v4-pro"
            },
            "metadata": {"elapsed_ms": 0}
        }),
        false,
    );

    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );

    let final_answer = "The cached rows point to the API docs article.";
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": final_answer}]
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": final_answer}],
                "timestamp_ms": 3,
                "finish_reason": "stop",
                "outcome": "normal",
                "provider": "deepseek",
                "model": "deepseek-v4-pro"
            },
            "metadata": {"elapsed_ms": 120_000}
        }),
        false,
    );

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Meta && row.text.contains("deepseek/deepseek-v4-pro")
    }));
}

#[test]
pub(crate) fn completed_live_tool_elapsed_keeps_visible_active_duration_for_all_tool_phases() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let cases = vec![
        (
            "read",
            serde_json::json!({"path": "Cargo.toml"}),
            serde_json::json!({"path": "Cargo.toml", "content": "body"}),
            TranscriptKind::Explored,
            "read Cargo.toml",
        ),
        (
            "exec_command",
            serde_json::json!({"cmd": "cargo test -p psychevo-cli"}),
            serde_json::json!({"output": "ok"}),
            TranscriptKind::Ran,
            "exec_command cargo test -p psychevo-cli",
        ),
        (
            "write",
            serde_json::json!({"path": "report.md", "content": "body"}),
            serde_json::json!({"path": "report.md", "bytes_written": 4}),
            TranscriptKind::Updated,
            "write report.md",
        ),
        (
            "edit",
            serde_json::json!({"path": "report.md", "old": "a", "new": "b"}),
            serde_json::json!({"path": "report.md", "replacements": 1}),
            TranscriptKind::Updated,
            "edit report.md",
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
