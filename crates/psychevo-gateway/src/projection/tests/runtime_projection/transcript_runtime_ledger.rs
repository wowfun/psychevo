#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeLedgerBlock {
    turn_id: String,
    entry_id: String,
    block_id: String,
    source: String,
    tool_name: Option<String>,
    tool_call_id: Option<String>,
    status: TranscriptBlockStatus,
    order: i64,
    title: Option<String>,
    has_result: bool,
    active_elapsed_owner: bool,
}

fn runtime_ledger_from_entry(entry: &TranscriptEntry) -> Vec<RuntimeLedgerBlock> {
    entry
        .blocks
        .iter()
        .map(|block| RuntimeLedgerBlock {
            turn_id: entry.turn_id.clone().unwrap_or_default(),
            entry_id: entry.id.clone(),
            block_id: block.id.clone(),
            source: block.source.clone(),
            tool_name: block
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("tool_name"))
                .and_then(Value::as_str)
                .map(str::to_string),
            tool_call_id: block
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("tool_call_id"))
                .and_then(Value::as_str)
                .map(str::to_string),
            status: block.status,
            order: block.order,
            title: block.title.clone(),
            has_result: block.result.is_some()
                || block
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("result"))
                    .is_some_and(|result| !result.is_null()),
            active_elapsed_owner: false,
        })
        .collect()
}

fn runtime_ledger_from_event(event: &GatewayEvent) -> Vec<RuntimeLedgerBlock> {
    runtime_ledger_from_entry(gateway_entry(event))
}

fn assert_runtime_ledger_identity(checkpoint: &str, ledger: &[RuntimeLedgerBlock]) {
    let mut block_ids = std::collections::BTreeSet::new();
    let mut tool_ids = std::collections::BTreeSet::new();
    for row in ledger {
        assert!(
            block_ids.insert((row.turn_id.clone(), row.block_id.clone())),
            "{checkpoint}: duplicate block identity in ledger: {ledger:#?}"
        );
        if let (Some(tool_name), Some(tool_call_id)) = (&row.tool_name, &row.tool_call_id) {
            assert_ne!(
                tool_call_id, tool_name,
                "{checkpoint}: tool_call_id fell back to bare tool name: {ledger:#?}"
            );
            assert!(
                tool_ids.insert((row.turn_id.clone(), tool_name.clone(), tool_call_id.clone())),
                "{checkpoint}: duplicate live tool identity in ledger: {ledger:#?}"
            );
        }
    }
}

fn assert_runtime_ledger_monotonic(
    checkpoint: &str,
    previous: &[RuntimeLedgerBlock],
    next: &[RuntimeLedgerBlock],
) {
    for current in next {
        let Some(prior) = previous
            .iter()
            .find(|row| row.tool_call_id.is_some() && row.tool_call_id == current.tool_call_id)
        else {
            continue;
        };
        assert!(
            status_rank(current.status) >= status_rank(prior.status),
            "{checkpoint}: status downgraded from {:?} to {:?}: {next:#?}",
            prior.status,
            current.status
        );
        if prior.has_result {
            assert!(
                current.has_result,
                "{checkpoint}: terminal result fact disappeared: {next:#?}"
            );
        }
        if prior.title.is_some() {
            assert!(
                current.title.is_some(),
                "{checkpoint}: title disappeared from known tool block: {next:#?}"
            );
        }
    }
}

fn status_rank(status: TranscriptBlockStatus) -> u8 {
    match status {
        TranscriptBlockStatus::Pending => 0,
        TranscriptBlockStatus::Running | TranscriptBlockStatus::NeedsInput => 1,
        TranscriptBlockStatus::Completed
        | TranscriptBlockStatus::Failed
        | TranscriptBlockStatus::Cancelled => 2,
        TranscriptBlockStatus::Info => 3,
    }
}

#[test]
fn trace_replay_ledger_keeps_missing_id_tools_unique() {
    let mut projector = GatewayLiveProjector::default();
    let first = projector
        .project(
            "turn-1",
            &RunStreamEvent::value(json!({
                "type": "tool_call_pending",
                "tool_name": "exec_command",
                "args": {"cmd": "python fetch.py"},
                "outcome": "normal"
            })),
        )
        .expect("first pending tool");
    let first_ledger = runtime_ledger_from_event(&first);
    assert_runtime_ledger_identity("first pending exec", &first_ledger);

    let second = projector
        .project(
            "turn-1",
            &RunStreamEvent::value(json!({
                "type": "tool_call_pending",
                "tool_name": "exec_command",
                "args": {"cmd": "sqlite3 feeds/.cache/x.db 'select 1'"},
                "outcome": "normal"
            })),
        )
        .expect("second pending tool");
    let second_ledger = runtime_ledger_from_event(&second);

    assert_runtime_ledger_identity("second pending exec", &second_ledger);
    assert_runtime_ledger_monotonic("second pending exec", &first_ledger, &second_ledger);
    let tool_ids = second_ledger
        .iter()
        .filter_map(|row| row.tool_call_id.as_deref())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(tool_ids.len(), 2, "{second_ledger:#?}");
}

#[test]
fn trace_replay_ledger_rejects_late_message_end_downgrade() {
    let mut projector = GatewayLiveProjector::default();
    let _ = projector.project(
        "turn-1",
        &RunStreamEvent::value(json!({
            "type": "tool_call_pending",
            "tool_name": "exec_command",
            "tool_call_id": "call_fetch",
            "args": {"cmd": "python fetch.py"},
            "outcome": "normal"
        })),
    );
    let completed_tool = projector
        .project(
            "turn-1",
            &RunStreamEvent::value(json!({
                "type": "tool_execution_end",
                "tool_name": "exec_command",
                "tool_call_id": "call_fetch",
                "args": {"cmd": "python fetch.py"},
                "result": {"session_id": 7, "exit_code": 0, "output": "done\n"},
                "outcome": "normal"
            })),
        )
        .expect("completed tool");
    let completed_ledger = runtime_ledger_from_event(&completed_tool);
    assert_runtime_ledger_identity("completed exec", &completed_ledger);

    let message_end = projector
        .project(
            "turn-1",
            &RunStreamEvent::value(json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "Fetched the data.", "content_index": 0},
                        {
                            "type": "tool_call",
                            "id": "call_fetch",
                            "name": "exec_command",
                            "arguments": {"cmd": "python fetch.py"},
                            "arguments_json": "{\"cmd\":\"python fetch.py\"}",
                            "content_index": 1,
                            "call_index": 0
                        }
                    ],
                    "finish_reason": "tool_calls",
                    "outcome": "normal"
                }
            })),
        )
        .expect("message end");
    let final_ledger = runtime_ledger_from_event(&message_end);

    assert_runtime_ledger_identity("late message_end", &final_ledger);
    assert_runtime_ledger_monotonic("late message_end", &completed_ledger, &final_ledger);
    let tool = final_ledger
        .iter()
        .find(|row| row.tool_call_id.as_deref() == Some("call_fetch"))
        .expect("tool ledger row");
    assert_eq!(tool.status, TranscriptBlockStatus::Completed);
    assert!(tool.has_result, "{final_ledger:#?}");
}
