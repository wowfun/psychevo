#[allow(unused_imports)]
use super::*;

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
pub(crate) fn visible_write_preamble_waits_for_typed_tool_call() {
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

    assert!(
        ui.transcript
            .iter()
            .all(|row| row.tool_name.as_deref() != Some("write")),
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
    let tool = ui
        .transcript
        .iter()
        .position(|row| row.title == "write feeds/report.md")
        .expect("typed write row");
    assert_eq!(
        ui.transcript[tool].tool_call_id.as_deref(),
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
pub(crate) fn tool_call_message_end_converts_visible_answer_to_preamble_in_place() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    let preamble = "The article fetch failed. Let me query comments.";

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
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == preamble),
        "{:?}",
        ui.transcript
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "call_comments",
            "tool_name": "exec_command",
            "arguments_json": "{\"cmd\":\"sqlite3 -json hn.db 'select * from comments'\"}",
            "content_index": 1,
            "call_index": 0
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
                        "id": "call_comments",
                        "name": "exec_command",
                        "arguments": {"cmd": "sqlite3 -json hn.db 'select * from comments'"},
                        "arguments_json": "{\"cmd\":\"sqlite3 -json hn.db 'select * from comments'\"}",
                        "arguments_error": null,
                        "content_index": 1,
                        "call_index": 0
                    }
                ],
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        }),
        false,
    );

    let preamble_row = ui
        .transcript
        .iter()
        .position(|row| row.title == "Thinking" && row.text == preamble)
        .expect("preamble row");
    let tool_row = ui
        .transcript
        .iter()
        .position(|row| row.tool_call_id.as_deref() == Some("call_comments"))
        .expect("tool row");
    assert!(preamble_row < tool_row, "{:?}", ui.transcript);
    assert_eq!(ui.transcript[preamble_row].kind, TranscriptKind::Thinking);
    assert!(
        ui.transcript
            .iter()
            .all(|row| !(row.kind == TranscriptKind::Answer && row.text == preamble)),
        "{:?}",
        ui.transcript
    );
}

#[test]
pub(crate) fn tool_call_preamble_stays_between_reasoning_and_tool_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();

    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "The comments are too many. Let me query them in batches.".to_string(),
        },
        true,
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Now query all story comments."},
                    {
                        "type": "tool_call",
                        "id": "call_comments",
                        "name": "exec_command",
                        "arguments": {"cmd": "sqlite3 -json hn.db 'select * from comments'"},
                        "arguments_json": "{\"cmd\":\"sqlite3 -json hn.db 'select * from comments'\"}",
                        "arguments_error": null,
                        "content_index": 1,
                        "call_index": 0
                    }
                ],
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        }),
        false,
    );

    let reasoning = ui
        .transcript
        .iter()
        .position(|row| row.title == "Thinking" && row.text.contains("comments are too many"))
        .expect("reasoning");
    let preamble = ui
        .transcript
        .iter()
        .position(|row| row.title == "Thinking" && row.text == "Now query all story comments.")
        .expect("preamble");
    let tool = ui
        .transcript
        .iter()
        .position(|row| row.tool_call_id.as_deref() == Some("call_comments"))
        .expect("tool");
    assert!(reasoning < preamble, "{:?}", ui.transcript);
    assert!(preamble < tool, "{:?}", ui.transcript);
}

#[test]
pub(crate) fn visible_write_preamble_does_not_create_orphan_for_non_write_tool_message() {
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
    let write_rows = ui
        .transcript
        .iter()
        .filter(|row| {
            row.kind == TranscriptKind::Updated && row.tool_name.as_deref() == Some("write")
        })
        .collect::<Vec<_>>();
    assert_eq!(write_rows.len(), 1, "{:?}", ui.transcript);
    assert_eq!(write_rows[0].title, "write feeds/report.md");
    assert_eq!(
        write_rows[0].tool_call_id.as_deref(),
        Some("call_write_report")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.title != "exec_command date -u +%H-%M")
    );
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

    assert!(
        ui.transcript
            .iter()
            .all(|row| row.tool_name.as_deref() != Some("write")),
        "{:?}",
        ui.transcript
    );
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
pub(crate) fn visible_write_preamble_without_tool_call_never_creates_tool_row() {
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
    assert!(ui.transcript.iter().all(|row| row.title != "write"));

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
