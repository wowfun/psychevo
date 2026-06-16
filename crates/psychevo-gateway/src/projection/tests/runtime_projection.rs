use serde_json::json;

#[test]
fn run_start_projects_selected_skills() {
    let event = gateway_event_from_run_stream(
        "turn-1",
        &RunStreamEvent::Event(json!({
            "type": "run_start",
            "session_id": "thread-1",
            "selected_skills": [
                {"name": "reviewer", "path": "/tmp/reviewer/SKILL.md"}
            ]
        })),
    );
    match event.expect("run_start should project a Gateway event") {
        GatewayEvent::TurnStarted {
            thread_id,
            turn_id,
            selected_skills,
        } => {
            assert_eq!(thread_id.as_deref(), Some("thread-1"));
            assert_eq!(turn_id, "turn-1");
            assert_eq!(selected_skills.len(), 1);
            assert_eq!(selected_skills[0].name, "reviewer");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn turn_complete_projects_terminal_turn_status() {
    let event = gateway_event_from_run_stream(
        "turn-1",
        &RunStreamEvent::Event(json!({
            "type": "turn_complete",
            "session_id": "thread-1",
            "outcome": "failed",
            "error": "model service failed"
        })),
    );
    match event.expect("turn_complete should project a Gateway event") {
        GatewayEvent::TurnCompleted {
            thread_id,
            turn_id,
            turn,
            committed_entries,
        } => {
            assert_eq!(thread_id.as_deref(), Some("thread-1"));
            assert_eq!(turn_id, "turn-1");
            assert_eq!(turn.thread_id.as_deref(), Some("thread-1"));
            assert_eq!(turn.status, GatewayTurnStatus::Failed);
            assert_eq!(turn.outcome.as_deref(), Some("failed"));
            assert_eq!(
                turn.error.as_ref().map(|error| error.message.as_str()),
                Some("model service failed")
            );
            assert!(committed_entries.is_empty());
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn live_projector_streams_reasoning_before_completion() {
    let mut projector = GatewayLiveProjector::default();
    let started = projector
        .project(
            "turn-1",
            &RunStreamEvent::ReasoningDelta {
                text: "first".to_string(),
            },
        )
        .expect("started");
    let completed = projector
        .project("turn-1", &RunStreamEvent::ReasoningEnd)
        .expect("completed");

    match started {
        GatewayEvent::EntryStarted { entry, .. } => {
            assert_eq!(entry.id, "live:turn-1:assistant:0");
            assert_eq!(entry.blocks[0].body.as_deref(), Some("first"));
            assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Running);
        }
        other => panic!("unexpected started event: {other:?}"),
    }
    match completed {
        GatewayEvent::EntryUpdated { entry, .. } => {
            assert_eq!(entry.id, "live:turn-1:assistant:0");
            assert_eq!(entry.blocks[0].body.as_deref(), Some("first"));
            assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Completed);
        }
        other => panic!("unexpected completed event: {other:?}"),
    }
}

#[test]
fn live_projector_routes_scoped_child_entries_to_child_thread() {
    let mut projector = GatewayLiveProjector::new(Some("parent-thread".to_string()));

    let parent_agent = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "agent_session_start",
                "tool_call_id": "call_agent",
                "agent_name": "explore",
                "agent_description": "Read-only codebase exploration subagent.",
                "parent_session_id": "parent-thread",
                "child_session_id": "child-thread"
            })),
        )
        .expect("parent agent row");
    assert_eq!(gateway_entry(&parent_agent).thread_id, "parent-thread");
    assert_eq!(
        gateway_entry(&parent_agent).blocks[0].kind,
        TranscriptBlockKind::Agent
    );

    let child_started = projector
        .project(
            "turn-child",
            &RunStreamEvent::scoped(
                "child-thread",
                RunStreamEvent::ReasoningDelta {
                    text: "child ".to_string(),
                },
            ),
        )
        .expect("child reasoning start");
    match child_started {
        GatewayEvent::EntryStarted { entry, .. } => {
            assert_eq!(entry.thread_id, "child-thread");
            assert_eq!(entry.id, "live:turn-child:assistant:0");
            assert_eq!(entry.blocks[0].kind, TranscriptBlockKind::Reasoning);
            assert_eq!(entry.blocks[0].body.as_deref(), Some("child "));
        }
        other => panic!("unexpected child started event: {other:?}"),
    }

    let child_updated = projector
        .project(
            "turn-child",
            &RunStreamEvent::scoped(
                "child-thread",
                RunStreamEvent::ReasoningDelta {
                    text: "work".to_string(),
                },
            ),
        )
        .expect("child reasoning update");
    match child_updated {
        GatewayEvent::EntryUpdated { entry, .. } => {
            assert_eq!(entry.thread_id, "child-thread");
            assert_eq!(entry.id, "live:turn-child:assistant:0");
            assert_eq!(entry.blocks[0].body.as_deref(), Some("child work"));
        }
        other => panic!("unexpected child updated event: {other:?}"),
    }

    let child_turn = projector
        .project(
            "turn-child",
            &RunStreamEvent::scoped(
                "child-thread",
                RunStreamEvent::Event(json!({
                    "type": "turn_complete",
                    "session_id": "parent-thread",
                    "outcome": "normal"
                })),
            ),
        )
        .expect("child terminal event");
    match child_turn {
        GatewayEvent::TurnCompleted {
            thread_id, turn, ..
        } => {
            assert_eq!(thread_id.as_deref(), Some("child-thread"));
            assert_eq!(turn.thread_id.as_deref(), Some("child-thread"));
        }
        other => panic!("unexpected child terminal event: {other:?}"),
    }

    let parent_reasoning = projector
        .project(
            "turn-parent-2",
            &RunStreamEvent::ReasoningDelta {
                text: "parent only".to_string(),
            },
        )
        .expect("parent reasoning");
    match parent_reasoning {
        GatewayEvent::EntryStarted { entry, .. } => {
            assert_eq!(entry.thread_id, "parent-thread");
            assert_eq!(entry.id, "live:turn-parent-2:assistant:0");
            assert_eq!(entry.blocks[0].body.as_deref(), Some("parent only"));
        }
        other => panic!("unexpected parent event: {other:?}"),
    }
}

#[test]
fn live_projector_merges_agent_session_start_into_existing_agent_block() {
    let mut projector = GatewayLiveProjector::new(Some("parent-thread".to_string()));

    let started = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_start",
                "tool_name": "spawn_agent",
                "tool_call_id": "call_agent_translate",
                "args": {
                    "agent_type": "translate",
                    "task_name": "Translate user message to Chinese",
                    "prompt": "Translate the following message to Chinese: hello"
                }
            })),
        )
        .expect("agent tool start");
    assert_eq!(gateway_entry(&started).blocks.len(), 1);
    assert_eq!(
        gateway_entry(&started).blocks[0].kind,
        TranscriptBlockKind::Agent
    );
    assert_eq!(
        gateway_entry(&started).blocks[0].status,
        TranscriptBlockStatus::Running
    );

    let session_start = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "agent_session_start",
                "tool_call_id": "call_agent_translate",
                "agent_id": "agent-run-1",
                "agent_name": "translate",
                "agent_description": "Translate user message to Chinese",
                "parent_session_id": "parent-thread",
                "child_session_id": "child-thread"
            })),
        )
        .expect("agent session start");
    let entry = gateway_entry(&session_start);
    assert_eq!(entry.thread_id, "parent-thread");
    assert_eq!(entry.id, "live:turn-parent:assistant:0");
    assert_eq!(entry.blocks.len(), 1);
    let block = &entry.blocks[0];
    assert_eq!(block.id, "live:turn-parent:tool:call_agent_translate");
    assert_eq!(block.kind, TranscriptBlockKind::Agent);
    assert_eq!(block.status, TranscriptBlockStatus::Running);
    let metadata = block.metadata.as_ref().expect("metadata");
    assert_eq!(metadata["tool_call_id"], "call_agent_translate");
    assert_eq!(metadata["agent_name"], "translate");
    assert_eq!(metadata["child_session_id"], "child-thread");
    assert_eq!(metadata["result"]["child_session_id"], "child-thread");
    assert_eq!(
        metadata["args"]["prompt"],
        "Translate the following message to Chinese: hello"
    );
    assert_eq!(
        metadata["result"]["task"],
        "Translate the following message to Chinese: hello"
    );

    let completed = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "spawn_agent",
                "tool_call_id": "call_agent_translate",
                "outcome": "normal",
                "result": {
                    "agent_name": "translate",
                    "status": "completed",
                    "summary": "你好"
                }
            })),
        )
        .expect("agent completion");
    let entry = gateway_entry(&completed);
    assert_eq!(entry.blocks.len(), 1);
    let block = &entry.blocks[0];
    assert_eq!(block.id, "live:turn-parent:tool:call_agent_translate");
    assert_eq!(block.status, TranscriptBlockStatus::Completed);
    let metadata = block.metadata.as_ref().expect("metadata");
    assert_eq!(metadata["result"]["child_session_id"], "child-thread");
    assert_eq!(
        metadata["result"]["task"],
        "Translate the following message to Chinese: hello"
    );
}

#[test]
fn live_projector_treats_background_agent_running_result_as_handoff() {
    let mut projector = GatewayLiveProjector::new(Some("parent-thread".to_string()));

    let pending = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_call_pending",
                "tool_name": "spawn_agent",
                "tool_call_id": "call_agent_translate",
                "arguments": {
                    "agent_type": "translate",
                    "task_name": "translate-en-to-zh"
                }
            })),
        )
        .expect("agent pending");
    assert_eq!(
        gateway_entry(&pending).blocks[0].status,
        TranscriptBlockStatus::Pending
    );

    let handoff = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "spawn_agent",
                "tool_call_id": "call_agent_translate",
                "outcome": "normal",
                "elapsed_ms": 24,
                "result": {
                    "agent_name": "translate",
                    "agent_description": "Detect the source language automatically.",
                    "task_name": "translate-en-to-zh",
                    "task": "Please translate this English text into Chinese.",
                    "status": "running",
                    "session_id": "child-thread",
                    "child_session_id": "child-thread"
                }
            })),
        )
        .expect("background agent handoff");
    assert!(
        matches!(handoff, GatewayEvent::EntryUpdated { .. }),
        "{handoff:?}"
    );
    let entry = gateway_entry(&handoff);
    assert_eq!(entry.blocks.len(), 1);
    let block = &entry.blocks[0];
    assert_eq!(block.id, "live:turn-parent:tool:call_agent_translate");
    assert_eq!(block.kind, TranscriptBlockKind::Agent);
    assert_eq!(block.status, TranscriptBlockStatus::Running);
    let metadata = block.metadata.as_ref().expect("metadata");
    assert_eq!(metadata["tool_call_id"], "call_agent_translate");
    assert_eq!(metadata["result"]["status"], "running");
    assert_eq!(metadata["result"]["background"], serde_json::Value::Null);
    assert_eq!(metadata["result"]["child_session_id"], "child-thread");

    let session_start = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "agent_session_start",
                "tool_call_id": "call_agent_translate",
                "agent_id": "agent-run-1",
                "agent_name": "translate",
                "agent_description": "Detect the source language automatically.",
                "task_name": "translate-en-to-zh",
                "task": "Please translate this English text into Chinese.",
                "parent_session_id": "parent-thread",
                "child_session_id": "child-thread",
                "background": true
            })),
        )
        .expect("agent session start");
    let entry = gateway_entry(&session_start);
    assert_eq!(entry.blocks.len(), 1);
    let block = &entry.blocks[0];
    assert_eq!(block.id, "live:turn-parent:tool:call_agent_translate");
    assert_eq!(block.status, TranscriptBlockStatus::Running);
    let metadata = block.metadata.as_ref().expect("metadata");
    assert_eq!(metadata["result"]["child_session_id"], "child-thread");
    assert_eq!(metadata["result"]["task_name"], "translate-en-to-zh");
}

#[test]
fn live_projector_does_not_alias_parallel_spawn_agent_execution_by_agent_name() {
    let mut projector = GatewayLiveProjector::new(Some("parent-thread".to_string()));
    let cn_args = json!({
        "agent_type": "translate",
        "task_name": "cn_to_en",
        "message": "Translate this Chinese sentence to English."
    });
    let en_args = json!({
        "agent_type": "translate",
        "task_name": "en_to_cn",
        "message": "Translate this English sentence to Chinese."
    });

    let pending_cn = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_call_pending",
                "tool_name": "spawn_agent",
                "tool_call_id": "call-cn",
                "args": cn_args,
                "content_index": 0,
                "call_index": 0
            })),
        )
        .expect("cn pending");
    assert_eq!(gateway_entry(&pending_cn).blocks.len(), 1);
    assert_agent_block_task(gateway_entry(&pending_cn), "call-cn", "cn_to_en");

    let start_en = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_start",
                "tool_name": "spawn_agent",
                "tool_call_id": "call-en",
                "args": en_args
            })),
        )
        .expect("en start");
    let entry = gateway_entry(&start_en);
    assert_eq!(entry.blocks.len(), 2, "{entry:#?}");
    assert_agent_block_task(entry, "call-cn", "cn_to_en");
    assert_agent_block_task(entry, "call-en", "en_to_cn");

    let session_start_en = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "agent_session_start",
                "tool_call_id": "call-en",
                "agent_name": "translate",
                "task_name": "en_to_cn",
                "child_session_id": "child-en"
            })),
        )
        .expect("en session start");
    let entry = gateway_entry(&session_start_en);
    assert_eq!(entry.blocks.len(), 2, "{entry:#?}");
    assert_agent_block_task(entry, "call-cn", "cn_to_en");
    assert_agent_block_task(entry, "call-en", "en_to_cn");
    assert_agent_block_child(entry, "call-en", "child-en");
    assert_agent_block_has_no_child(entry, "call-cn");

    let handoff_cn = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "spawn_agent",
                "tool_call_id": "call-cn",
                "outcome": "normal",
                "result": {
                    "agent_name": "translate",
                    "task_name": "cn_to_en",
                    "status": "running",
                    "background": true,
                    "child_session_id": "child-cn"
                }
            })),
        )
        .expect("cn handoff");
    let entry = gateway_entry(&handoff_cn);
    assert_eq!(entry.blocks.len(), 2, "{entry:#?}");
    assert_agent_block_task(entry, "call-cn", "cn_to_en");
    assert_agent_block_child(entry, "call-cn", "child-cn");
    assert_agent_block_task(entry, "call-en", "en_to_cn");
    assert_agent_block_child(entry, "call-en", "child-en");
}

#[test]
fn live_projector_upgrades_spawn_agent_position_id_without_metadata_mix() {
    let mut projector = GatewayLiveProjector::new(Some("parent-thread".to_string()));
    let args = json!({
        "agent_type": "translate",
        "task_name": "cn_to_en",
        "message": "Translate this Chinese sentence to English."
    });

    let pending = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_call_pending",
                "tool_name": "spawn_agent",
                "arguments": args,
                "content_index": 0,
                "call_index": 0
            })),
        )
        .expect("generated-id pending");
    assert_agent_block_task(gateway_entry(&pending), "spawn_agent@0:0:0", "cn_to_en");

    let start = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_start",
                "tool_name": "spawn_agent",
                "tool_call_id": "call-cn",
                "args": args,
                "content_index": 0,
                "call_index": 0
            })),
        )
        .expect("resolved start");
    let entry = gateway_entry(&start);
    assert_eq!(entry.blocks.len(), 1, "{entry:#?}");
    assert!(agent_block(entry, "spawn_agent@0:0:0").is_none(), "{entry:#?}");
    assert_agent_block_task(entry, "call-cn", "cn_to_en");

    let handoff = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "spawn_agent",
                "tool_call_id": "call-cn",
                "outcome": "normal",
                "result": {
                    "agent_name": "translate",
                    "task_name": "cn_to_en",
                    "status": "running",
                    "background": true,
                    "child_session_id": "child-cn"
                }
            })),
        )
        .expect("handoff");
    let entry = gateway_entry(&handoff);
    assert_eq!(entry.blocks.len(), 1, "{entry:#?}");
    assert_agent_block_task(entry, "call-cn", "cn_to_en");
    assert_agent_block_child(entry, "call-cn", "child-cn");
}

#[test]
fn live_projector_keeps_parallel_spawn_agent_pending_without_ids_separate_by_position() {
    let mut projector = GatewayLiveProjector::new(Some("parent-thread".to_string()));
    let cn_args = json!({
        "agent_type": "translate",
        "task_name": "cn_to_en",
        "message": "Translate this Chinese sentence to English."
    });
    let en_args = json!({
        "agent_type": "translate",
        "task_name": "en_to_cn",
        "message": "Translate this English sentence to Chinese."
    });

    let _ = projector.project(
        "turn-parent",
        &RunStreamEvent::Event(json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": cn_args,
            "content_index": 0,
            "call_index": 0
        })),
    );
    let pending = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_call_pending",
                "tool_name": "spawn_agent",
                "arguments": en_args,
                "content_index": 1,
                "call_index": 1
            })),
        )
        .expect("second pending");

    let entry = gateway_entry(&pending);
    assert_eq!(entry.blocks.len(), 2, "{entry:#?}");
    assert_agent_block_task(entry, "spawn_agent@0:0:0", "cn_to_en");
    assert_agent_block_task(entry, "spawn_agent@0:1:1", "en_to_cn");
}

#[test]
fn live_projector_merges_unidentified_wait_agent_placeholder() {
    let mut projector = GatewayLiveProjector::new(Some("parent-thread".to_string()));

    let pending = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_call_pending",
                "tool_name": "wait_agent"
            })),
        )
        .expect("wait pending");
    assert_eq!(
        gateway_entry(&pending).blocks[0].id,
        "live:turn-parent:tool:wait_agent"
    );

    let completed = projector
        .project(
            "turn-parent",
            &RunStreamEvent::Event(json!({
                "type": "tool_execution_end",
                "tool_name": "wait_agent",
                "tool_call_id": "call_wait_agent",
                "outcome": "normal",
                "elapsed_ms": 30_000,
                "result": {
                    "message": "both agents completed",
                    "timed_out": false
                }
            })),
        )
        .expect("wait completed");
    let entry = gateway_entry(&completed);
    assert_eq!(entry.blocks.len(), 1);
    assert_eq!(entry.blocks[0].id, "live:turn-parent:tool:wait_agent");
    assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Completed);
    assert_eq!(
        entry.blocks[0].metadata.as_ref().unwrap()["tool_call_id"],
        "wait_agent"
    );
    assert_eq!(
        entry.blocks[0].metadata.as_ref().unwrap()["result"]["message"],
        "both agents completed"
    );
}

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
