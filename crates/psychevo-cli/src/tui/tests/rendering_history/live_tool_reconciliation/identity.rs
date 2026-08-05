use super::{RuntimeToolLedgerRow, assert_runtime_tool_ledger, runtime_tool_ledger};
use crate::tui::FullscreenUi;
use crate::tui::tests::fixtures::{buffer_text, draw_fullscreen_for_test, test_app};
use crate::tui::tests::runtime_turn_event;
use serde_json::Value;
use tempfile::tempdir;

#[tokio::test]
async fn later_assistant_message_reusing_tool_position_keeps_both_calls() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);

    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call-exec",
                    "name": "exec_command",
                    "arguments": {"cmd": "echo first"},
                    "content_index": 1,
                    "call_index": 1
                }],
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        })),
        true,
        false,
    );
    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "call-write",
            "tool_name": "write",
            "arguments_json": "{\"path\":\"report.md\",\"content\":\"done\"}",
            "content_index": 1,
            "call_index": 1
        })),
        true,
        false,
    );
    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call-write",
                    "name": "write",
                    "arguments": {"path": "report.md", "content": "done"},
                    "content_index": 1,
                    "call_index": 1
                }],
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        })),
        true,
        false,
    );

    assert_eq!(
        runtime_tool_ledger(&ui),
        vec![
            RuntimeToolLedgerRow {
                turn_id: None,
                entry_id: None,
                block_id: None,
                source: None,
                tool_name: Some("exec_command".to_string()),
                tool_call_id: Some("call-exec".to_string()),
                status: "running",
                order: 0,
                title: "exec_command echo first".to_string(),
                has_result: true,
                active_elapsed_owner: true,
            },
            RuntimeToolLedgerRow {
                turn_id: None,
                entry_id: None,
                block_id: None,
                source: None,
                tool_name: Some("write".to_string()),
                tool_call_id: Some("call-write".to_string()),
                status: "running",
                order: 1,
                title: "write report.md".to_string(),
                has_result: true,
                active_elapsed_owner: true,
            },
        ]
    );
}

#[tokio::test]
async fn idless_parallel_same_name_calls_keep_distinct_positions() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);

    for content_index in [1, 2] {
        ui.apply_turn_event(
            runtime_turn_event(serde_json::json!({
                "type": "tool_call_pending",
                "tool_name": "write",
                "arguments_json": "",
                "content_index": content_index,
                "call_index": 0
            })),
            true,
            false,
        );
    }

    assert_eq!(
        runtime_tool_ledger(&ui),
        vec![
            RuntimeToolLedgerRow {
                turn_id: None,
                entry_id: None,
                block_id: None,
                source: None,
                tool_name: Some("write".to_string()),
                tool_call_id: None,
                status: "running",
                order: 0,
                title: "write".to_string(),
                has_result: true,
                active_elapsed_owner: true,
            },
            RuntimeToolLedgerRow {
                turn_id: None,
                entry_id: None,
                block_id: None,
                source: None,
                tool_name: Some("write".to_string()),
                tool_call_id: None,
                status: "running",
                order: 1,
                title: "write".to_string(),
                has_result: true,
                active_elapsed_owner: true,
            },
        ]
    );
}

#[tokio::test]
async fn unmatched_completion_does_not_replace_active_call_at_same_position() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);

    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call-active",
                    "name": "exec_command",
                    "arguments": {"cmd": "sleep 30"},
                    "content_index": 1,
                    "call_index": 1
                }],
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        })),
        true,
        false,
    );
    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-orphan",
            "tool_name": "exec_command",
            "content_index": 1,
            "call_index": 1,
            "args": {"cmd": "echo orphan"},
            "result": {"output": "orphan\n", "exit_code": 0},
            "outcome": "normal",
            "elapsed_ms": 5
        })),
        true,
        false,
    );

    assert_eq!(
        runtime_tool_ledger(&ui),
        vec![
            RuntimeToolLedgerRow {
                turn_id: None,
                entry_id: None,
                block_id: None,
                source: None,
                tool_name: Some("exec_command".to_string()),
                tool_call_id: Some("call-active".to_string()),
                status: "running",
                order: 0,
                title: "exec_command sleep 30".to_string(),
                has_result: true,
                active_elapsed_owner: true,
            },
            RuntimeToolLedgerRow {
                turn_id: None,
                entry_id: None,
                block_id: None,
                source: None,
                tool_name: Some("exec_command".to_string()),
                tool_call_id: Some("call-orphan".to_string()),
                status: "completed",
                order: 1,
                title: "exec_command echo orphan".to_string(),
                has_result: true,
                active_elapsed_owner: false,
            },
        ]
    );
}

#[tokio::test]
async fn idless_completion_does_not_replace_stable_active_call_at_same_position() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);

    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call-active",
                    "name": "exec_command",
                    "arguments": {"cmd": "sleep 30"},
                    "content_index": 1,
                    "call_index": 1
                }],
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        })),
        true,
        false,
    );
    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "tool_execution_end",
            "tool_name": "read",
            "content_index": 1,
            "call_index": 1,
            "args": {"path": "notes.md"},
            "result": {"content": "done"},
            "outcome": "normal",
            "elapsed_ms": 5
        })),
        true,
        false,
    );

    assert_eq!(
        runtime_tool_ledger(&ui)
            .iter()
            .map(|row| {
                (
                    row.tool_call_id.as_deref(),
                    row.tool_name.as_deref(),
                    row.title.as_str(),
                    row.status,
                    row.order,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (
                Some("call-active"),
                Some("exec_command"),
                "exec_command sleep 30",
                "running",
                0,
            ),
            (None, Some("read"), "read notes.md", "completed", 1),
        ]
    );
}

#[tokio::test]
async fn pending_call_with_new_stable_id_does_not_adopt_occupied_position() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);

    for (tool_call_id, cmd) in [("call-first", "echo first"), ("call-second", "echo second")] {
        ui.apply_turn_event(
            runtime_turn_event(serde_json::json!({
                "type": "tool_call_pending",
                "tool_call_id": tool_call_id,
                "tool_name": "exec_command",
                "arguments_json": serde_json::to_string(&serde_json::json!({"cmd": cmd}))
                    .expect("arguments"),
                "content_index": 1,
                "call_index": 1
            })),
            true,
            false,
        );
    }

    let rows = runtime_tool_ledger(&ui);
    assert_runtime_tool_ledger("conflicting stable ids at one position", &rows);
    assert_eq!(
        rows.iter()
            .map(|row| (
                row.tool_call_id.as_deref(),
                row.title.as_str(),
                row.status,
                row.order,
            ))
            .collect::<Vec<_>>(),
        vec![
            (Some("call-first"), "exec_command echo first", "running", 0),
            (
                Some("call-second"),
                "exec_command echo second",
                "running",
                1,
            ),
        ]
    );
}

#[tokio::test]
async fn execution_start_with_new_stable_id_does_not_adopt_occupied_position() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);

    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call-preview",
                    "name": "exec_command",
                    "arguments": {"cmd": "sleep 30"},
                    "content_index": 1,
                    "call_index": 1
                }],
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        })),
        true,
        false,
    );
    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call-start",
            "tool_name": "exec_command",
            "content_index": 1,
            "call_index": 1,
            "args": {"cmd": "echo started"}
        })),
        true,
        false,
    );

    assert_eq!(
        runtime_tool_ledger(&ui)
            .iter()
            .map(|row| (row.tool_call_id.as_deref(), row.title.as_str(), row.order))
            .collect::<Vec<_>>(),
        vec![
            (Some("call-preview"), "exec_command sleep 30", 0),
            (Some("call-start"), "exec_command echo started", 1),
        ]
    );
}

#[tokio::test]
async fn write_stdin_cleanup_does_not_delete_different_call_at_same_position() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);

    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call-exec",
            "tool_name": "exec_command",
            "args": {"cmd": "long-running"}
        })),
        true,
        false,
    );
    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "exec_session_yielded",
            "session_id": 42,
            "tool_call_id": "call-exec",
            "cmd": "long-running"
        })),
        true,
        false,
    );
    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "call-poll-existing",
            "tool_name": "write_stdin",
            "arguments_json": "{\"session_id\":",
            "content_index": 1,
            "call_index": 1
        })),
        true,
        false,
    );
    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "call-poll-new",
            "tool_name": "write_stdin",
            "arguments_json": "{\"session_id\":42,\"chars\":\"\"}",
            "content_index": 1,
            "call_index": 1
        })),
        true,
        false,
    );

    assert_eq!(
        runtime_tool_ledger(&ui)
            .iter()
            .map(|row| (row.tool_call_id.as_deref(), row.tool_name.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            (Some("call-exec"), Some("exec_command")),
            (Some("call-poll-existing"), Some("write_stdin")),
        ]
    );
}

#[tokio::test]
async fn persisted_session_fault_trace_keeps_primary_tool_order() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);

    apply_tool_message(
        &mut ui,
        vec![tool_call(
            "call_db1eb62c5de64e59a6359f30",
            "exec_command",
            serde_json::json!({"cmd": "fetch-session"}),
            1,
            1,
        )],
    );
    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "exec_session_yielded",
            "session_id": 0,
            "tool_call_id": "call_db1eb62c5de64e59a6359f30",
            "cmd": "fetch-session"
        })),
        true,
        false,
    );
    complete_tool(
        &mut ui,
        "call_db1eb62c5de64e59a6359f30",
        "exec_command",
        serde_json::json!({"cmd": "fetch-session"}),
        serde_json::json!({"session_id": 0, "exit_code": null, "output": ""}),
        1,
        1,
    );

    apply_tool_message(
        &mut ui,
        vec![tool_call(
            "call_f2ceeb0b1c424d5799d695c2",
            "write_stdin",
            serde_json::json!({"session_id": 0, "yield_time_ms": 60_000}),
            1,
            1,
        )],
    );
    complete_tool(
        &mut ui,
        "call_f2ceeb0b1c424d5799d695c2",
        "write_stdin",
        serde_json::json!({"session_id": 0, "yield_time_ms": 60_000}),
        serde_json::json!({"session_id": 0, "exit_code": 0, "output": "fetched"}),
        1,
        1,
    );

    apply_tool_message(
        &mut ui,
        vec![
            tool_call(
                "call_07c6859241d947d28b643889",
                "exec_command",
                serde_json::json!({"cmd": "query-daily"}),
                1,
                1,
            ),
            tool_call(
                "call_16a93d34914a4a3cbd495011",
                "read",
                serde_json::json!({"path": "users.json"}),
                2,
                2,
            ),
        ],
    );
    complete_tool(
        &mut ui,
        "call_07c6859241d947d28b643889",
        "exec_command",
        serde_json::json!({"cmd": "query-daily"}),
        serde_json::json!({"exit_code": 0, "output": "rows"}),
        1,
        1,
    );
    complete_tool(
        &mut ui,
        "call_16a93d34914a4a3cbd495011",
        "read",
        serde_json::json!({"path": "users.json"}),
        serde_json::json!({"content": "{}"}),
        2,
        2,
    );

    for (tool_call_id, cmd) in [
        ("call_cb0906270a0a4a1eb6061ed6", "timestamp-session"),
        ("call_00e3be11176146458ff2bd11", "prepare-output"),
    ] {
        apply_tool_message(
            &mut ui,
            vec![tool_call(
                tool_call_id,
                "exec_command",
                serde_json::json!({"cmd": cmd}),
                1,
                1,
            )],
        );
        complete_tool(
            &mut ui,
            tool_call_id,
            "exec_command",
            serde_json::json!({"cmd": cmd}),
            serde_json::json!({"exit_code": 0, "output": "ok"}),
            1,
            1,
        );
    }

    apply_tool_message(
        &mut ui,
        vec![tool_call(
            "call_b81eb59a155a4020865e1e2c",
            "write",
            serde_json::json!({"path": "report.md", "content": "done"}),
            1,
            1,
        )],
    );
    complete_tool(
        &mut ui,
        "call_b81eb59a155a4020865e1e2c",
        "write",
        serde_json::json!({"path": "report.md", "content": "done"}),
        serde_json::json!({"path": "report.md"}),
        1,
        1,
    );

    let rows = runtime_tool_ledger(&ui);
    assert_runtime_tool_ledger("persisted session fault trace", &rows);
    assert_eq!(
        rows.iter()
            .map(|row| {
                (
                    row.tool_call_id.as_deref(),
                    row.tool_name.as_deref(),
                    row.title.as_str(),
                    row.status,
                    row.order,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (
                Some("call_db1eb62c5de64e59a6359f30"),
                Some("exec_command"),
                "exec_command fetch-session",
                "completed",
                0,
            ),
            (
                Some("call_07c6859241d947d28b643889"),
                Some("exec_command"),
                "exec_command query-daily",
                "completed",
                1,
            ),
            (
                Some("call_16a93d34914a4a3cbd495011"),
                Some("read"),
                "read users.json",
                "completed",
                2,
            ),
            (
                Some("call_cb0906270a0a4a1eb6061ed6"),
                Some("exec_command"),
                "exec_command timestamp-session",
                "completed",
                3,
            ),
            (
                Some("call_00e3be11176146458ff2bd11"),
                Some("exec_command"),
                "exec_command prepare-output",
                "completed",
                4,
            ),
            (
                Some("call_b81eb59a155a4020865e1e2c"),
                Some("write"),
                "write report.md",
                "completed",
                5,
            ),
        ]
    );
    assert!(
        rows.iter()
            .all(|row| row.tool_name.as_deref() != Some("write_stdin"))
    );

    ui.scroll_to_bottom();
    let rendered = buffer_text(&draw_fullscreen_for_test(&app, &mut ui, 100, 18));
    let mut previous = None;
    for title in [
        "exec_command fetch-session",
        "exec_command query-daily",
        "read users.json",
        "exec_command timestamp-session",
        "exec_command prepare-output",
        "write report.md",
    ] {
        let position = rendered.find(title).unwrap_or_else(|| {
            panic!("missing `{title}` from narrow transcript render:\n{rendered}")
        });
        assert!(
            previous.is_none_or(|previous| previous < position),
            "visible tool title order diverged at `{title}`:\n{rendered}"
        );
        previous = Some(position);
    }
}

fn apply_tool_message(ui: &mut FullscreenUi<'_>, content: Vec<Value>) {
    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": content,
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        })),
        true,
        false,
    );
}

fn tool_call(id: &str, name: &str, arguments: Value, content_index: u64, call_index: u64) -> Value {
    serde_json::json!({
        "type": "tool_call",
        "id": id,
        "name": name,
        "arguments": arguments,
        "content_index": content_index,
        "call_index": call_index
    })
}

fn complete_tool(
    ui: &mut FullscreenUi<'_>,
    tool_call_id: &str,
    tool_name: &str,
    args: Value,
    result: Value,
    content_index: u64,
    call_index: u64,
) {
    ui.apply_turn_event(
        runtime_turn_event(serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": tool_call_id,
            "tool_name": tool_name,
            "args": args,
            "result": result,
            "outcome": "normal",
            "elapsed_ms": 5,
            "content_index": content_index,
            "call_index": call_index
        })),
        true,
        false,
    );
}
