#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn extension_tool_records_render_with_generic_tool_style() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);

    for (tool, args, expected_title) in [
        (
            "custom_list",
            serde_json::json!({"path": "src"}),
            "custom_list src",
        ),
        (
            "custom_search",
            serde_json::json!({"query": "needle"}),
            "custom_search needle",
        ),
    ] {
        let mut ui = FullscreenUi::new(&app);
        ui.apply_value_event(
            &serde_json::json!({
                "type": "tool_execution_start",
                "tool_call_id": format!("call_{tool}"),
                "tool_name": tool,
                "args": args
            }),
            false,
        );

        let row = ui
            .transcript
            .iter()
            .find(|row| row.tool_name.as_deref() == Some(tool))
            .expect("active row");
        assert_eq!(row.kind, TranscriptKind::Ran, "{tool}");
        assert_eq!(row.title, expected_title);

        ui.apply_value_event(
            &serde_json::json!({
                "type": "tool_execution_end",
                "tool_call_id": format!("call_{tool}"),
                "tool_name": tool,
                "result": {"query": "needle", "matches": []},
                "outcome": "normal",
                "elapsed_ms": 1
            }),
            false,
        );

        let row = ui
            .transcript
            .iter()
            .find(|row| row.tool_name.as_deref() == Some(tool))
            .expect("completed row");
        assert_eq!(row.kind, TranscriptKind::Ran, "{tool}");
        assert_eq!(row.title, expected_title);
        assert!(!row.title.starts_with("Tool "), "{tool}");
    }
}

#[test]
pub(crate) fn web_fetch_result_defaults_to_summary_with_content_as_detail() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_fetch",
            "tool_name": "web_fetch",
            "args": {"url": "https://example.test/docs"},
            "result": {
                "url": "https://example.test/docs",
                "final_url": "https://example.test/docs",
                "status": 200,
                "content_type": "text/html",
                "content": "first fetched line\nsecond fetched line\nthird fetched line",
                "original_bytes": 1200,
                "output_bytes": 80,
                "truncated": false
            },
            "outcome": "normal",
            "elapsed_ms": 12
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.tool_name.as_deref() == Some("web_fetch"))
        .expect("web_fetch row");
    assert_eq!(row.kind, TranscriptKind::Explored);
    assert_eq!(row.title, "web_fetch https://example.test/docs");
    assert!(
        row.text.contains("web_fetch normal: status=200"),
        "{}",
        row.text
    );
    assert!(!row.text.contains("first fetched line"), "{}", row.text);
    assert!(
        row.full_text
            .as_deref()
            .is_some_and(|full| full.contains("first fetched line")),
        "{row:?}"
    );
}

#[test]
pub(crate) fn tool_display_snapshot_controls_extension_tool_projection() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let display = serde_json::json!({
        "category": "update",
        "title_arg_keys": ["target"],
        "title_result_keys": ["target"],
        "summary_keys": ["status"],
        "body_keys": ["content"],
        "body_policy": "summary"
    });

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_custom",
            "tool_name": "custom_publish",
            "args": {"target": "draft.md"},
            "display": display
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.tool_name.as_deref() == Some("custom_publish"))
        .expect("custom row");
    assert_eq!(row.kind, TranscriptKind::Updated);
    assert_eq!(row.title, "custom_publish draft.md");
}

#[test]
pub(crate) fn interrupted_pending_tool_row_stops_timer_as_interrupted() {
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
    assert_eq!(row.title, "edit src/lib.rs");
    assert_eq!(row.text, "interrupted");
    assert!(row.interrupted);
    assert!(!row.failed);
    assert!(row.tool_elapsed.is_some());
    assert!(row.tool_started.is_none());
}

#[test]
pub(crate) fn parallel_streaming_tool_calls_create_independent_active_rows() {
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
                        "id": "call_exec",
                        "name": "exec_command",
                        "arguments": {"cmd": "rg format_duration"},
                        "arguments_json": "{\"cmd\":\"rg format_duration\"}",
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
    assert_eq!(
        titles,
        ["read Cargo.toml", "exec_command rg format_duration"]
    );
}

#[test]
pub(crate) fn sequential_streaming_tool_calls_reuse_position_without_overwriting_rows() {
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
                    "name": "exec_command",
                    "arguments": {"cmd": "echo one"},
                    "arguments_json": "{\"cmd\":\"echo one\"}",
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
            "tool_name": "exec_command",
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
                TranscriptKind::Explored | TranscriptKind::Ran | TranscriptKind::Updated
            )
        })
        .map(|row| row.title.as_str())
        .collect::<Vec<_>>();
    assert_eq!(titles, ["exec_command echo one", "write report.md"]);
}

#[test]
pub(crate) fn history_tool_result_restores_elapsed_duration() {
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
    let title = line_text(&tool_lines(row, false, true, 32)[0]);
    assert!(title.contains("read src/lib.rs"), "{title}");
    assert!(!title.contains("0s"), "{title}");
}

#[test]
pub(crate) fn history_meta_uses_persisted_variant_not_current_variant() {
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
pub(crate) fn history_reasoning_only_final_message_gets_turn_meta() {
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
pub(crate) fn history_aborted_reasoning_only_message_does_not_get_turn_meta() {
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
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
}

#[test]
pub(crate) fn history_tool_call_reasoning_message_does_not_get_turn_meta() {
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
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.title == "read Cargo.toml")
    );
}

#[test]
pub(crate) fn history_aborted_tool_calls_render_interrupted_without_live_timer() {
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
                    "name": "exec_command",
                    "arguments": {
                        "cmd": "cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \"SELECT content FROM stories WHERE id = 48074265;\" 2>&1 | head -c 3000",
                        "timeout": 10
                    },
                    "arguments_json": "{\"cmd\":\"cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \\\"SELECT content FROM stories WHERE id = 48074265;\\\" 2>&1 | head -c 3000\",\"timeout\":10}",
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
        .expect("interrupted exec_command row");
    assert!(
        row.title
            .starts_with("exec_command cd /home/kevin/Projects/feedgarden")
    );
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
pub(crate) fn history_text_plus_tool_call_message_shows_active_row_without_turn_meta() {
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
        row.kind == TranscriptKind::Updated
            && row.title == "write /tmp/hackernews-hot-05-15.md"
            && row.tool_started.is_some()
    }));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
}

#[test]
pub(crate) fn history_aborted_tool_result_renders_interrupted_without_failure_style() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_find",
                "name": "exec_command",
                "arguments": {
                    "cmd": "find /home/kevin -name tmp.txt -type f",
                    "timeout": 10
                },
                "arguments_json": "{\"cmd\":\"find /home/kevin -name tmp.txt -type f\",\"timeout\":10}",
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
            "tool_name": "exec_command",
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
    assert_eq!(
        row.title,
        "exec_command find /home/kevin -name tmp.txt -type f"
    );
    assert_eq!(row.text, "interrupted");
    assert!(row.interrupted);
    assert!(!row.failed);
    assert_eq!(row.tool_elapsed, Some(Duration::from_secs(4)));
    assert!(row.tool_started.is_none());
}

#[test]
pub(crate) fn history_bash_timeout_renders_timeout_before_partial_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_fetch",
                "name": "exec_command",
                "arguments": {
                    "cmd": "python scripts/fetch.py",
                    "timeout": 120
                },
                "arguments_json": "{\"cmd\":\"python scripts/fetch.py\",\"timeout\":120}",
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
            "tool_name": "exec_command",
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
    assert_eq!(row.title, "exec_command python scripts/fetch.py");
    assert!(row.failed);
    assert!(
        row.text
            .starts_with("timeout: command timed out after 120 seconds; partial output follows\n")
    );
    assert!(row.text.contains("[fetch] 29 rows done"));
}

#[test]
pub(crate) fn history_answer_turn_meta_omits_accounting_cost() {
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
pub(crate) fn history_user_image_display_metadata_renders_prompt_and_attachment_meta() {
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

    assert!(
        ui.transcript.iter().any(|row| {
            row.kind == TranscriptKind::Prompt && row.text == "[Image #1] describe it"
        })
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Meta && row.text == "attachments\nimage 1: image.png"
    }));
}

#[test]
pub(crate) fn legacy_history_image_blocks_render_as_attachment_meta_not_prompt_text() {
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

    assert!(
        ui.transcript
            .iter()
            .any(|row| { row.kind == TranscriptKind::Prompt && row.text == "describe it" })
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Meta && row.text == "attachments\nimage 1: /tmp/image.png"
    }));
}
