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
            &RunStreamEvent::value(json!({
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
                RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
        &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
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
            &RunStreamEvent::value(json!({
                "type": "tool_call_pending",
                "tool_name": "wait_agent"
            })),
        )
        .expect("wait pending");
    let pending_block_id = gateway_entry(&pending).blocks[0].id.clone();
    assert!(pending_block_id.starts_with("live:turn-parent:tool:live-temp:wait_agent:"));

    let completed = projector
        .project(
            "turn-parent",
            &RunStreamEvent::value(json!({
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
    assert_eq!(entry.blocks[0].id, "live:turn-parent:tool:call_wait_agent");
    assert_eq!(entry.blocks[0].status, TranscriptBlockStatus::Completed);
    assert_eq!(
        entry.blocks[0].metadata.as_ref().unwrap()["tool_call_id"],
        "call_wait_agent"
    );
    assert_eq!(
        entry.blocks[0].metadata.as_ref().unwrap()["result"]["message"],
        "both agents completed"
    );
}
