#[allow(unused_imports)]
use super::*;

#[test]
pub(crate) fn late_agent_pending_after_background_handoff_does_not_mark_interrupted() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let weak_pending = serde_json::json!({
        "agent_type": "translate"
    });
    let cn_pending = serde_json::json!({
        "agent_type": "translate",
        "task_name": "cn-to-en"
    });
    let en_pending = serde_json::json!({
        "agent_type": "translate",
        "task_name": "en-to-cn"
    });

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": weak_pending,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": weak_pending,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-cn",
            "tool_name": "spawn_agent",
            "content_index": 0,
            "call_index": 0,
            "result": {
                "agent_name": "translate",
                "task": "cn-to-en",
                "status": "running",
                "background": true,
                "child_session_id": "child-cn"
            },
            "outcome": "normal",
            "elapsed_ms": 9
        }),
        false,
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": cn_pending,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    let rows = agent_rows(&ui);
    assert_eq!(
        rows.iter()
            .filter(|row| row.title == "translate(cn-to-en)")
            .count(),
        1,
        "{:#?}",
        ui.transcript
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-en",
            "tool_name": "spawn_agent",
            "content_index": 1,
            "call_index": 1,
            "result": {
                "agent_name": "translate",
                "task": "en-to-cn",
                "status": "running",
                "background": true,
                "child_session_id": "child-en"
            },
            "outcome": "normal",
            "elapsed_ms": 11
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": en_pending,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );
    ui.turn_outcome = Some(Outcome::Normal);
    ui.finish_turn();

    let rows = agent_rows(&ui);
    assert_eq!(rows.len(), 2, "{:#?}", ui.transcript);
    assert!(rows.iter().all(|row| !row.interrupted), "{rows:#?}");
    assert!(
        rows.iter().all(|row| row
            .agent_target
            .as_deref()
            .is_some_and(|id| id.starts_with("child-"))),
        "{rows:#?}"
    );
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
        let ledger = runtime_tool_ledger(&ui);
        assert_runtime_tool_ledger(&format!("{tool} completed"), &ledger);
        assert!(
            ledger.iter().all(|row| !row.active_elapsed_owner),
            "{tool}: {ledger:#?}"
        );
    }
}

#[test]
pub(crate) fn stale_running_exec_after_completion_does_not_retake_elapsed_ownership() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_exec",
            "tool_name": "exec_command",
            "args": {"cmd": "sleep 1 && echo done"}
        }),
        false,
    );
    let active_idx = ui
        .transcript
        .iter()
        .position(active_tool_row)
        .expect("active exec row");
    ui.transcript[active_idx].tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_secs(12))
            .expect("instant"),
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_exec",
            "tool_name": "exec_command",
            "result": {"session_id": 7, "exit_code": 0, "output": "done\n"},
            "outcome": "normal",
            "elapsed_ms": 0
        }),
        false,
    );
    let completed_ledger = runtime_tool_ledger(&ui);
    assert_runtime_tool_ledger("exec completed", &completed_ledger);

    let changed = ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_exec",
            "tool_name": "exec_command",
            "result": {"session_id": 7, "exit_code": null, "output": "stale running\n"},
            "outcome": "normal",
            "elapsed_ms": 30_000
        }),
        false,
    );
    assert!(!changed);
    let after = runtime_tool_ledger(&ui);
    assert_runtime_tool_ledger("stale exec running after completion", &after);
    assert_eq!(after.len(), 1, "{after:#?}");
    assert_eq!(after[0].tool_call_id.as_deref(), Some("call_exec"));
    assert_eq!(after[0].status, "completed");
    assert!(!after[0].active_elapsed_owner, "{after:#?}");
    assert!(after[0].has_result, "{after:#?}");
}
