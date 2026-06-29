#[test]
fn live_projector_merges_write_stdin_polls_into_yielded_exec_command() {
    let mut projector = GatewayLiveProjector::default();
    let pending = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_call_pending",
                "tool_name": "exec_command",
                "tool_call_id": "call_exec",
                "args": {"cmd": "python fetch.py"},
                "outcome": "normal"
            })),
        )
        .expect("pending exec");
    match pending {
        GatewayEvent::EntryStarted { entry, .. } => {
            let block = &entry.blocks[0];
            assert_eq!(block.title.as_deref(), Some("exec_command python fetch.py"));
            assert_eq!(block.status, TranscriptBlockStatus::Pending);
            let metadata = block.metadata.as_ref().expect("metadata");
            assert_eq!(metadata["args"]["cmd"], "python fetch.py");
        }
        other => panic!("unexpected pending event: {other:?}"),
    }

    let yielded = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "exec_command",
                "tool_call_id": "call_exec",
                "result": {"session_id": 7, "exit_code": null, "output": "first\n"},
                "outcome": "normal"
            })),
        )
        .expect("yielded exec");
    assert_exec_event(
        &yielded,
        "call_exec",
        TranscriptBlockStatus::Running,
        "first\n",
        Some("exec_command python fetch.py"),
        None,
    );

    let hidden_poll = projector.project(
        "turn-1",
        &RunStreamEvent::Event(json!({
            "type": "tool_call_pending",
            "tool_name": "write_stdin",
            "tool_call_id": "call_poll",
            "args": {"session_id": 7, "yield_time_ms": 60000},
            "outcome": "normal"
        })),
    );
    assert!(hidden_poll.is_none());

    let polled = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "write_stdin",
                "tool_call_id": "call_poll",
                "result": {"session_id": null, "exit_code": null, "output": "second\n"},
                "outcome": "normal"
            })),
        )
        .expect("poll result");
    assert_exec_event(
        &polled,
        "call_exec",
        TranscriptBlockStatus::Running,
        "first\nsecond\n",
        Some("exec_command python fetch.py"),
        None,
    );

    let _ = projector.project(
        "turn-1",
        &RunStreamEvent::Event(json!({
            "type": "tool_call_pending",
            "tool_name": "write_stdin",
            "tool_call_id": "call_done",
            "args": {"session_id": 7, "yield_time_ms": 60000},
            "outcome": "normal"
        })),
    );
    let completed = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "write_stdin",
                "tool_call_id": "call_done",
                "result": {"session_id": null, "exit_code": 0, "output": "third\n"},
                "outcome": "normal"
            })),
        )
        .expect("completion");
    assert_exec_event(
        &completed,
        "call_exec",
        TranscriptBlockStatus::Completed,
        "first\nsecond\nthird\n",
        Some("exec_command python fetch.py"),
        Some(0),
    );
}

#[test]
fn live_projector_merges_background_exec_session_finish_into_yielded_exec_command() {
    let mut projector = GatewayLiveProjector::default();
    let _ = projector.project(
        "turn-1",
        &RunStreamEvent::Event(json!({
            "type": "tool_call_pending",
            "tool_name": "exec_command",
            "tool_call_id": "call_exec",
            "args": {"cmd": "python fetch.py"},
            "outcome": "normal"
        })),
    );
    let yielded = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "exec_command",
                "tool_call_id": "call_exec",
                "result": {"session_id": 7, "exit_code": null, "output": "start\n"},
                "outcome": "normal"
            })),
        )
        .expect("yielded exec");
    assert_exec_event(
        &yielded,
        "call_exec",
        TranscriptBlockStatus::Running,
        "start\n",
        Some("exec_command python fetch.py"),
        None,
    );

    let delta = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "exec_session_output_delta",
                "session_id": 7,
                "tool_call_id": "call_exec",
                "seq": 0,
                "output": "done\n"
            })),
        )
        .expect("background output");
    assert_exec_event(
        &delta,
        "call_exec",
        TranscriptBlockStatus::Running,
        "start\ndone\n",
        Some("exec_command python fetch.py"),
        None,
    );

    let finished = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "exec_session_finished",
                "session_id": 7,
                "tool_call_id": "call_exec",
                "exit_code": 0,
                "elapsed_ms": 501,
                "interrupted": false
            })),
        )
        .expect("background finish");
    assert_exec_event(
        &finished,
        "call_exec",
        TranscriptBlockStatus::Completed,
        "start\ndone\n",
        Some("exec_command python fetch.py"),
        Some(0),
    );
}

#[test]
fn live_projector_hides_unmatched_successful_write_stdin_but_keeps_failed_one() {
    let mut projector = GatewayLiveProjector::default();
    let success = projector.project(
        "turn-1",
        &RunStreamEvent::Event(json!({
            "type": "tool_execution_end",
            "tool_name": "write_stdin",
            "tool_call_id": "call_poll",
            "result": {"session_id": null, "exit_code": null, "output": "late\n"},
            "outcome": "normal"
        })),
    );
    assert!(success.is_none());

    let failed = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "write_stdin",
                "tool_call_id": "call_poll",
                "result": {"session_id": null, "exit_code": 1, "output": "error\n"},
                "outcome": "failed"
            })),
        )
        .expect("failed");
    match failed {
        GatewayEvent::EntryCompleted { entry, .. } => {
            assert_eq!(entry.id, "live:turn-1:assistant:0");
            assert_eq!(entry.blocks[0].title.as_deref(), Some("write_stdin"));
            assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Failed);
        }
        other => panic!("unexpected failed event: {other:?}"),
    }
}

#[test]
fn live_projector_hides_assistant_write_stdin_tool_call_block() {
    let mut projector = GatewayLiveProjector::default();
    let hidden = projector.project(
        "turn-1",
        &RunStreamEvent::Event(json!({
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
}
