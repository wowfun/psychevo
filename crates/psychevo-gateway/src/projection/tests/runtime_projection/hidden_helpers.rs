#[test]
fn live_projector_hidden_assistant_message_end_closes_segment() {
    let mut projector = GatewayLiveProjector::default();
    let _ = projector.project(
        "turn-1",
        &RunStreamEvent::ReasoningDelta {
            text: "The command is still running.".to_string(),
        },
    );
    let _ = projector.project("turn-1", &RunStreamEvent::ReasoningEnd);
    let hidden = projector.project(
        "turn-1",
        &RunStreamEvent::value(json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_call",
                        "id": "call_poll",
                        "name": "write_stdin",
                        "arguments": {"session_id": 7, "yield_time_ms": 60000},
                        "arguments_json": "{\"session_id\":7,\"yield_time_ms\":60000}",
                        "content_index": 0,
                        "call_index": 0
                    }
                ],
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        })),
    );
    assert!(hidden.is_none());

    let next = projector
        .project(
            "turn-1",
            &RunStreamEvent::ReasoningDelta {
                text: "fetch.py completed.".to_string(),
            },
        )
        .expect("next reasoning");
    match next {
        GatewayEvent::EntryStarted { entry, .. } => {
            assert_eq!(entry.id, "live:turn-1:assistant:1");
            assert_eq!(entry.blocks[0].body.as_deref(), Some("fetch.py completed."));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

fn gateway_entry(event: &GatewayEvent) -> &TranscriptEntry {
    match event {
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. } => entry,
        other => panic!("unexpected event: {other:?}"),
    }
}

fn agent_block<'a>(entry: &'a TranscriptEntry, tool_call_id: &str) -> Option<&'a TranscriptBlock> {
    entry.blocks.iter().find(|block| {
        block.kind == TranscriptBlockKind::Agent
            && block
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("tool_call_id"))
                .and_then(Value::as_str)
                == Some(tool_call_id)
    })
}

fn assert_agent_block_task(entry: &TranscriptEntry, tool_call_id: &str, expected_task_name: &str) {
    let block = agent_block(entry, tool_call_id).expect("agent block");
    let metadata = block.metadata.as_ref().expect("metadata");
    assert_eq!(metadata["args"]["task_name"], expected_task_name);
    if let Some(result_task_name) = metadata
        .get("result")
        .and_then(|result| result.get("task_name"))
        .filter(|value| !value.is_null())
    {
        assert_eq!(result_task_name, expected_task_name);
    }
}

fn assert_agent_block_child(entry: &TranscriptEntry, tool_call_id: &str, expected_child_id: &str) {
    let block = agent_block(entry, tool_call_id).expect("agent block");
    let metadata = block.metadata.as_ref().expect("metadata");
    assert_eq!(
        metadata
            .get("child_thread_id")
            .or_else(|| metadata.get("child_session_id"))
            .or_else(|| metadata.get("session_id"))
            .or_else(|| {
                metadata
                    .get("result")
                    .and_then(|result| result.get("child_thread_id"))
            })
            .or_else(|| {
                metadata
                    .get("result")
                    .and_then(|result| result.get("child_session_id"))
            })
            .or_else(|| metadata.get("result").and_then(|result| result.get("session_id"))),
        Some(&json!(expected_child_id))
    );
}

fn assert_agent_block_has_no_child(entry: &TranscriptEntry, tool_call_id: &str) {
    let block = agent_block(entry, tool_call_id).expect("agent block");
    let metadata = block.metadata.as_ref().expect("metadata");
    assert!(
        metadata.get("child_thread_id").is_none_or(Value::is_null)
            && metadata.get("child_session_id").is_none_or(Value::is_null)
            && metadata.get("session_id").is_none_or(Value::is_null)
            && metadata
                .get("result")
                .and_then(|result| result.get("child_thread_id"))
                .is_none_or(Value::is_null)
            && metadata
                .get("result")
                .and_then(|result| result.get("child_session_id"))
                .is_none_or(Value::is_null)
            && metadata
                .get("result")
                .and_then(|result| result.get("session_id"))
                .is_none_or(Value::is_null),
        "{metadata:#?}"
    );
}

fn assert_exec_event(
    event: &GatewayEvent,
    expected_tool_call_id: &str,
    expected_status: TranscriptBlockStatus,
    expected_output: &str,
    expected_title: Option<&str>,
    expected_exit_code: Option<i64>,
) {
    let entry = gateway_entry(event);
    assert_eq!(entry.id, "live:turn-1:assistant:0");
    let block = entry
        .blocks
        .iter()
        .find(|block| {
            block
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("tool_call_id"))
                .and_then(Value::as_str)
                == Some(expected_tool_call_id)
        })
        .expect("tool block");
    assert_eq!(block.title.as_deref(), expected_title);
    assert_eq!(block.status, expected_status);
    let metadata = block.metadata.as_ref().expect("metadata");
    assert_eq!(metadata["tool_name"], "exec_command");
    assert_eq!(metadata["tool_call_id"], expected_tool_call_id);
    assert_eq!(metadata["args"]["cmd"], "python fetch.py");
    assert_eq!(metadata["result"]["output"], expected_output);
    match expected_exit_code {
        Some(exit_code) => assert_eq!(metadata["result"]["exit_code"], exit_code),
        None => assert!(
            metadata["result"]
                .get("exit_code")
                .is_none_or(Value::is_null),
            "{metadata:?}"
        ),
    }
}
