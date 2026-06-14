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
