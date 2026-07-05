#[test]
fn assistant_tool_call_text_projects_as_assistant_phase_text() {
    let mut projector = GatewayLiveProjector::default();
    let event = projector
        .project(
            "turn-1",
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
        &RunStreamEvent::value(json!({
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
                &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
fn live_projector_does_not_collapse_missing_tool_call_ids_by_tool_name() {
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
    let first_entry = gateway_entry(&first);
    let first_block = first_entry.blocks.first().expect("first block");
    let first_id = first_block
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("tool_call_id"))
        .and_then(Value::as_str)
        .expect("first tool id")
        .to_string();

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
    let second_entry = gateway_entry(&second);
    let second_block = second_entry.blocks.last().expect("second block");
    let second_id = second_block
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("tool_call_id"))
        .and_then(Value::as_str)
        .expect("second tool id")
        .to_string();

    assert_ne!(first_id, "exec_command");
    assert_ne!(second_id, "exec_command");
    assert_ne!(first_id, second_id);
    assert_eq!(second_entry.blocks.len(), 2);
    assert!(second_entry.blocks.iter().any(|block| block.id == first_block.id));
    assert!(second_entry.blocks.iter().any(|block| block.id == second_block.id));
}

#[test]
fn live_projector_message_end_tool_call_does_not_downgrade_completed_tool_block() {
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
    assert_exec_event(
        &completed_tool,
        "call_fetch",
        TranscriptBlockStatus::Completed,
        "done\n",
        Some("exec_command python fetch.py"),
        Some(0),
    );

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
    let entry = gateway_entry(&message_end);
    let block = entry
        .blocks
        .iter()
        .find(|block| {
            block
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("tool_call_id"))
                .and_then(Value::as_str)
                == Some("call_fetch")
        })
        .expect("tool block");
    assert_eq!(block.status, TranscriptBlockStatus::Completed);
    assert!(block.body.as_deref().is_some_and(|body| body.contains("done")));
    let metadata = block.metadata.as_ref().expect("metadata");
    assert_eq!(metadata["result"]["exit_code"], 0);
    assert_eq!(metadata["result"]["output"], "done\n");
}

#[test]
fn live_projector_projects_completed_agent_as_openable_agent_block() {
    let mut projector = GatewayLiveProjector::new(Some("thread-1".to_string()));
    let _ = projector.project(
        "turn-1",
        &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
        &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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

#[test]
fn live_projector_projects_message_update_before_terminal_message_end() {
    let mut projector = GatewayLiveProjector::default();

    let update = projector
        .project(
            "turn-1",
            &RunStreamEvent::value(json!({
                "type": "message_update",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "streaming hello"}
                    ],
                    "finish_reason": null,
                    "outcome": "normal"
                }
            })),
        )
        .expect("streaming message update");

    let (turn_id, entry) = match update {
        GatewayEvent::EntryStarted { turn_id, entry }
        | GatewayEvent::EntryUpdated { turn_id, entry } => (turn_id, entry),
        other => panic!("message_update must project to a live transcript entry: {other:?}"),
    };
    assert_eq!(turn_id, "turn-1");
    assert_eq!(entry.role, TranscriptEntryRole::Assistant);
    assert_eq!(entry.status, TranscriptBlockStatus::Running);
    assert_eq!(entry.blocks[0].kind, TranscriptBlockKind::Text);
    assert_eq!(entry.blocks[0].body.as_deref(), Some("streaming hello"));
    assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Running);

    let terminal = projector
        .project(
            "turn-1",
            &RunStreamEvent::value(json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "streaming hello done"}
                    ],
                    "finish_reason": "stop",
                    "outcome": "normal"
                }
            })),
        )
        .expect("terminal message end");

    let GatewayEvent::EntryCompleted { turn_id, entry } = terminal else {
        panic!("message_end must project to entryCompleted");
    };
    assert_eq!(turn_id, "turn-1");
    assert_eq!(entry.status, TranscriptBlockStatus::Completed);
    assert_eq!(entry.blocks[0].body.as_deref(), Some("streaming hello done"));
    assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Completed);
}

#[test]
fn live_projector_replaces_running_message_update_snapshot_when_content_positions_shift() {
    let mut projector = GatewayLiveProjector::default();
    let table = "3 个翻译 Agent 已并发启动 ✅，等待它们返回结果：\n\n| # | 任务 | Agent ID | 状态 |\n|---|------|----------|------|\n| 1 | 中→英 | `translate_cn_to_en` | 🔄 运行中 |\n| 2 | 英→中 | `translate_en_to_cn` | 🔄 运行中 |\n| 3 | 日→中 | `translate_ja_to_cn` | 🔄 运行中 |\n\n正在等待所有 Agent 完成...";

    let first_update = projector
        .project(
            "turn-1",
            &RunStreamEvent::value(json!({
                "type": "message_update",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": table}
                    ],
                    "finish_reason": null,
                    "outcome": "normal"
                }
            })),
        )
        .expect("first update");
    assert_eq!(
        gateway_entry(&first_update)
            .blocks
            .iter()
            .map(|block| block.kind)
            .collect::<Vec<_>>(),
        vec![TranscriptBlockKind::Text]
    );

    let shifted_update = projector
        .project(
            "turn-1",
            &RunStreamEvent::value(json!({
                "type": "message_update",
                "message": {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "reasoning",
                            "text": "All three agents have been spawned and are running concurrently."
                        },
                        {"type": "text", "text": table},
                        {
                            "type": "tool_call",
                            "id": "call_wait_agent",
                            "name": "wait_agent",
                            "arguments": {"timeout_ms": 30000},
                            "arguments_json": "{\"timeout_ms\":30000}",
                            "content_index": 0,
                            "call_index": 0
                        }
                    ],
                    "finish_reason": null,
                    "outcome": "normal"
                }
            })),
        )
        .expect("shifted update");

    let entry = gateway_entry(&shifted_update);
    assert_eq!(entry.id, "live:turn-1:assistant:0");
    assert_eq!(
        entry
            .blocks
            .iter()
            .map(|block| (block.kind, block.body.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            (
                TranscriptBlockKind::Reasoning,
                Some("All three agents have been spawned and are running concurrently.")
            ),
            (TranscriptBlockKind::Text, Some(table)),
            (TranscriptBlockKind::ToolCall, None),
        ]
    );
    assert_eq!(
        entry
            .blocks
            .iter()
            .filter(|block| block.kind == TranscriptBlockKind::Text && block.body.as_deref() == Some(table))
            .count(),
        1
    );
    assert_eq!(entry.blocks[2].title.as_deref(), Some("wait_agent"));
}
