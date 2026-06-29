#[test]
fn assistant_tool_call_text_projects_as_assistant_phase_text() {
    let mut projector = GatewayLiveProjector::default();
    let event = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "I will write the file now."},
                        {
                            "type": "tool_call",
                            "id": "call-write",
                            "name": "write",
                            "arguments": {"path": "out.md"},
                            "arguments_json": "{\"path\":\"out.md\"}",
                            "content_index": 1,
                            "call_index": 0
                        }
                    ],
                    "provider": "mock",
                    "model": "mock-model",
                    "finish_reason": "tool_calls",
                    "outcome": "normal"
                }
            })),
        )
        .expect("projected");
    match event {
        GatewayEvent::EntryCompleted { entry, .. } => {
            assert_eq!(entry.id, "live:turn-1:assistant:0");
            assert_eq!(entry.blocks.len(), 2);
            assert_eq!(entry.blocks[0].kind, TranscriptBlockKind::Text);
            assert_eq!(entry.blocks[0].order, 0);
            assert_eq!(entry.blocks[0].title, None);
            assert_eq!(
                entry.blocks[0].body.as_deref(),
                Some("I will write the file now.")
            );
            let metadata = entry.blocks[0].metadata.as_ref().expect("metadata");
            assert_eq!(metadata["projection"], "assistant_phase");
            assert_eq!(entry.blocks[1].kind, TranscriptBlockKind::File);
            assert_eq!(entry.blocks[1].order, 1);
            assert_eq!(
                entry.blocks[1].metadata.as_ref().expect("tool metadata")["tool_call_id"],
                "call-write"
            );
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn assistant_tool_call_without_text_projects_tool_without_empty_assistant_phase() {
    let mut projector = GatewayLiveProjector::default();
    let event = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool_call",
                            "id": "call-write",
                            "name": "write",
                            "arguments": {"path": "out.md"},
                            "arguments_json": "{\"path\":\"out.md\"}",
                            "content_index": 0,
                            "call_index": 0
                        }
                    ],
                    "finish_reason": "tool_calls",
                    "outcome": "normal"
                }
            })),
        )
        .expect("projected");
    match event {
        GatewayEvent::EntryCompleted { entry, .. } => {
            assert_eq!(entry.blocks.len(), 1);
            assert_eq!(entry.blocks[0].kind, TranscriptBlockKind::File);
            assert_eq!(
                entry.blocks[0].metadata.as_ref().unwrap()["tool_call_id"],
                "call-write"
            );
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn live_projector_reorders_tool_pending_after_reasoning_and_phase_text() {
    let mut projector = GatewayLiveProjector::default();
    let _ = projector.project(
        "turn-1",
        &RunStreamEvent::ReasoningDelta {
            text: "The user wants the X daily report.".to_string(),
        },
    );
    let _ = projector.project("turn-1", &RunStreamEvent::ReasoningEnd);
    let _ = projector.project(
        "turn-1",
        &RunStreamEvent::Event(json!({
            "type": "tool_call_pending",
            "tool_name": "exec_command",
            "tool_call_id": "call_fetch",
            "args": {"cmd": "python fetch.py"},
            "outcome": "normal"
        })),
    );

    let completed_message = projector
            .project(
                "turn-1",
                &RunStreamEvent::Event(json!({
                    "type": "message_end",
                    "message": {
                        "role": "assistant",
                        "content": [
                            {"type": "reasoning", "text": "The user wants the X daily report.", "content_index": 0},
                            {"type": "text", "text": "好的，开始执行 X 日报流程。先运行 `fetch.py` 抓取今日推文数据。", "content_index": 1},
                            {
                                "type": "tool_call",
                                "id": "call_fetch",
                                "name": "exec_command",
                                "arguments": {"cmd": "python fetch.py"},
                                "arguments_json": "{\"cmd\":\"python fetch.py\"}",
                                "content_index": 2,
                                "call_index": 0
                            }
                        ],
                        "finish_reason": "tool_calls",
                        "outcome": "normal"
                    }
                })),
            )
            .expect("completed message");

    let entry = gateway_entry(&completed_message);
    assert_eq!(entry.id, "live:turn-1:assistant:0");
    let metadata = entry.metadata.as_ref().expect("entry metadata");
    assert_eq!(metadata["projection"], "assistant_segment");
    assert_eq!(metadata["liveOrder"], 0);
    assert_eq!(metadata["authoritativeBlocks"], true);
    assert!(metadata["streamSeq"].as_u64().is_some());
    assert_eq!(
        entry
            .blocks
            .iter()
            .map(|block| (block.kind, block.order, block.body.as_deref().unwrap_or("")))
            .collect::<Vec<_>>(),
        vec![
            (
                TranscriptBlockKind::Reasoning,
                0,
                "The user wants the X daily report."
            ),
            (
                TranscriptBlockKind::Text,
                1,
                "好的，开始执行 X 日报流程。先运行 `fetch.py` 抓取今日推文数据。"
            ),
            (TranscriptBlockKind::Shell, 2, ""),
        ]
    );
    assert_eq!(
        entry.blocks[1].metadata.as_ref().unwrap()["projection"],
        "assistant_phase"
    );

    let running_tool = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "exec_command",
                "tool_call_id": "call_fetch",
                "result": {"session_id": 9, "exit_code": null, "output": "[x-fetch] running\n"},
                "outcome": "normal"
            })),
        )
        .expect("running tool");
    let entry = gateway_entry(&running_tool);
    assert_eq!(entry.id, "live:turn-1:assistant:0");
    assert_eq!(
        entry.metadata.as_ref().unwrap()["authoritativeBlocks"],
        false
    );
    assert_eq!(entry.blocks[2].kind, TranscriptBlockKind::Shell);
    assert_eq!(entry.blocks[2].order, 2);
    assert_eq!(entry.blocks[2].status, TranscriptBlockStatus::Running);
    assert_eq!(entry.blocks[2].body.as_deref(), Some("[x-fetch] running\n"));
    assert_eq!(
        entry.blocks[2].title.as_deref(),
        Some("exec_command python fetch.py")
    );
}

#[test]
fn live_projector_aliases_runtime_tool_id_to_matching_authoritative_tool_call() {
    let mut projector = GatewayLiveProjector::default();
    let completed_message = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "先运行 fetch.py。", "content_index": 0},
                        {
                            "type": "tool_call",
                            "id": "model_call_fetch",
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
        .expect("completed message");
    assert_eq!(gateway_entry(&completed_message).blocks.len(), 2);

    let running_tool = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "exec_command",
                "tool_call_id": "runtime_call_fetch",
                "args": {"cmd": "python fetch.py"},
                "result": {"session_id": 9, "exit_code": null, "output": "[x-fetch] running\n"},
                "outcome": "normal"
            })),
        )
        .expect("running tool");

    let entry = gateway_entry(&running_tool);
    assert_eq!(entry.id, "live:turn-1:assistant:0");
    assert_eq!(
        entry
            .blocks
            .iter()
            .map(|block| block.kind)
            .collect::<Vec<_>>(),
        vec![TranscriptBlockKind::Text, TranscriptBlockKind::Shell]
    );
    let block = &entry.blocks[1];
    assert_eq!(block.id, "live:turn-1:tool:model_call_fetch");
    assert_eq!(
        block.metadata.as_ref().unwrap()["tool_call_id"],
        "model_call_fetch"
    );
    assert_eq!(block.status, TranscriptBlockStatus::Running);
    assert_eq!(block.body.as_deref(), Some("[x-fetch] running\n"));
}

#[test]
fn live_projector_projects_completed_agent_as_openable_agent_block() {
    let mut projector = GatewayLiveProjector::new(Some("thread-1".to_string()));
    let _ = projector.project(
        "turn-1",
        &RunStreamEvent::Event(json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "tool_call_id": "call_agent",
            "args": {"agent_type": "explore", "prompt": "Inspect the project"},
            "outcome": "normal"
        })),
    );

    let completed = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "spawn_agent",
                "tool_call_id": "call_agent",
                "result": {
                    "id": "run-agent",
                    "agent_name": "explore",
                    "task_name": "Inspect",
                    "task": "Inspect the project",
                    "status": "completed",
                    "session_id": "child-thread",
                    "child_session_id": "child-thread",
                    "parent_session_id": "thread-1",
                    "summary": "Project inspected."
                },
                "outcome": "normal"
            })),
        )
        .expect("completed agent");

    let entry = gateway_entry(&completed);
    assert_eq!(entry.thread_id, "thread-1");
    let block = entry
        .blocks
        .iter()
        .find(|block| {
            block
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("tool_call_id"))
                .and_then(Value::as_str)
                == Some("call_agent")
        })
        .expect("agent block");
    assert_eq!(block.kind, TranscriptBlockKind::Agent);
    assert_eq!(block.status, TranscriptBlockStatus::Completed);
    let metadata = block.metadata.as_ref().expect("metadata");
    assert_eq!(metadata["tool_name"], "spawn_agent");
    assert_eq!(metadata["result"]["child_session_id"], "child-thread");
    assert_eq!(metadata["result"]["session_id"], "child-thread");
    assert_eq!(metadata["args"]["agent_type"], "explore");
}

#[test]
fn live_projector_authoritative_message_end_preserves_runtime_reasoning() {
    let mut projector = GatewayLiveProjector::default();
    let _ = projector.project(
        "turn-1",
        &RunStreamEvent::ReasoningDelta {
            text: "This runtime stream is real reasoning.".to_string(),
        },
    );
    let _ = projector.project("turn-1", &RunStreamEvent::ReasoningEnd);
    let _ = projector.project(
        "turn-1",
        &RunStreamEvent::Event(json!({
            "type": "tool_call_pending",
            "tool_name": "exec_command",
            "tool_call_id": "call_fetch",
            "args": {"cmd": "python fetch.py"},
            "outcome": "normal"
        })),
    );

    let completed = projector
        .project(
            "turn-1",
            &RunStreamEvent::Event(json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "好的，开始执行 X 日报流程。", "content_index": 0},
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
        .expect("completed message");

    let entry = gateway_entry(&completed);
    assert_eq!(entry.id, "live:turn-1:assistant:0");
    assert_eq!(
        entry.metadata.as_ref().unwrap()["authoritativeBlocks"],
        true
    );
    assert_eq!(
        entry
            .blocks
            .iter()
            .map(|block| block.kind)
            .collect::<Vec<_>>(),
        vec![
            TranscriptBlockKind::Reasoning,
            TranscriptBlockKind::Text,
            TranscriptBlockKind::Shell
        ]
    );
    assert_eq!(
        entry.blocks[0].body.as_deref(),
        Some("This runtime stream is real reasoning.")
    );
    assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Completed);
    assert_eq!(
        entry.blocks[0].metadata.as_ref().unwrap()["origin"],
        "run_stream_reasoning"
    );
    assert_eq!(
        entry.blocks[1].body.as_deref(),
        Some("好的，开始执行 X 日报流程。")
    );
    assert_eq!(entry.blocks[2].order, 1);
    assert_eq!(entry.blocks[2].status, TranscriptBlockStatus::Pending);
    assert_eq!(
        entry.blocks[2].title.as_deref(),
        Some("exec_command python fetch.py")
    );
}
