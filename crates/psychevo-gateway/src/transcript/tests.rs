    use psychevo_runtime::{Outcome, UserContentBlock};

    #[test]
    fn terminal_projection_keeps_failed_and_interrupted_turns_visible() {
        let entries = project_turn_terminal_entries(&[
            GatewayTurnTerminalRecord {
                turn_id: "turn-ok".to_string(),
                thread_id: "thread-1".to_string(),
                status: "completed".to_string(),
                outcome: Some("normal".to_string()),
                error_message: None,
                started_at_ms: Some(1),
                completed_at_ms: 2,
                metadata: None,
            },
            GatewayTurnTerminalRecord {
                turn_id: "turn-failed".to_string(),
                thread_id: "thread-1".to_string(),
                status: "failed".to_string(),
                outcome: Some("failed".to_string()),
                error_message: Some("model service failed".to_string()),
                started_at_ms: Some(3),
                completed_at_ms: 4,
                metadata: Some(json!({"source": "test"})),
            },
            GatewayTurnTerminalRecord {
                turn_id: "turn-interrupted".to_string(),
                thread_id: "thread-1".to_string(),
                status: "interrupted".to_string(),
                outcome: Some("aborted".to_string()),
                error_message: None,
                started_at_ms: Some(5),
                completed_at_ms: 6,
                metadata: None,
            },
        ]);

        assert_eq!(
            entries.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(),
            vec!["turn:turn-failed:terminal", "turn:turn-interrupted:terminal"]
        );
        assert_eq!(entries[0].role, TranscriptEntryRole::Diagnostic);
        assert_eq!(entries[0].status, TranscriptBlockStatus::Failed);
        assert_eq!(
            entries[0].blocks[0].body.as_deref(),
            Some("model service failed")
        );
        assert_eq!(
            entries[0].metadata.as_ref().unwrap()["terminal"]["source"],
            "test"
        );
        assert_eq!(entries[1].status, TranscriptBlockStatus::Cancelled);
        assert_eq!(
            entries[1].blocks[0].title.as_deref(),
            Some("Turn interrupted")
        );
    }

    #[test]
    fn terminal_reconciliation_marks_yielded_exec_block_failed_after_turn_failure() {
        let summaries = vec![
            summary(
                1,
                Message::User {
                    content: vec![UserContentBlock::text("$x-daily")],
                    timestamp_ms: 1,
                },
            ),
            summary(
                2,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_exec",
                        "exec_command",
                        json!({"cmd": "python .agents/skills/x-daily/scripts/fetch.py"}),
                    )],
                    timestamp_ms: 2,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                3,
                Message::ToolResult {
                    tool_call_id: "call_exec".to_string(),
                    tool_name: "exec_command".to_string(),
                    content: "{\"session_id\":7,\"exit_code\":null,\"output\":\"\"}".to_string(),
                    is_error: false,
                    timestamp_ms: 3,
                },
            ),
        ];
        let terminal = GatewayTurnTerminalRecord {
            turn_id: "turn-failed".to_string(),
            thread_id: "thread-1".to_string(),
            status: "failed".to_string(),
            outcome: Some("failed".to_string()),
            error_message: Some("provider failed".to_string()),
            started_at_ms: Some(1),
            completed_at_ms: 4,
            metadata: Some(json!({
                "source": "gateway",
                "firstCommittedSeq": 2
            })),
        };
        let mut entries = project_transcript_entries("thread-1", &summaries);
        assert_eq!(entries[1].blocks[0].status, TranscriptBlockStatus::Running);

        reconcile_terminal_bounded_running_blocks(&mut entries, &[terminal]);

        assert_eq!(entries[1].status, TranscriptBlockStatus::Failed);
        assert_eq!(entries[1].blocks[0].status, TranscriptBlockStatus::Failed);
    }

    #[test]
    fn projector_preserves_assistant_block_order_and_attaches_tool_result() {
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![
                        AssistantBlock::Reasoning {
                            text: "think first".to_string(),
                            provider_evidence: None,
                        },
                        AssistantBlock::Text {
                            text: "I will run date.".to_string(),
                        },
                        tool_call("call_exec", "exec_command", json!({"cmd": "date"})),
                    ],
                    timestamp_ms: 10,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_exec".to_string(),
                    tool_name: "exec_command".to_string(),
                    content: "{\"exit_code\":0,\"output\":\"today\\n\"}".to_string(),
                    is_error: false,
                    timestamp_ms: 20,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_seq, Some(1));
        assert_eq!(
            entries[0]
                .blocks
                .iter()
                .map(|block| block.kind)
                .collect::<Vec<_>>(),
            vec![
                TranscriptBlockKind::Reasoning,
                TranscriptBlockKind::Text,
                TranscriptBlockKind::Shell,
            ]
        );
        assert_eq!(entries[0].blocks[0].body.as_deref(), Some("think first"));
        assert_eq!(
            entries[0].blocks[1].body.as_deref(),
            Some("I will run date.")
        );
        assert_eq!(
            entries[0].blocks[1].metadata.as_ref().unwrap()["projection"],
            "assistant_phase"
        );
        let tool = &entries[0].blocks[2];
        assert_eq!(tool.status, TranscriptBlockStatus::Completed);
        assert_eq!(tool.title.as_deref(), Some("exec_command date"));
        assert_eq!(tool.result.as_ref().unwrap().result_message_seq, 2);
        assert_eq!(tool.metadata.as_ref().unwrap()["args"]["cmd"], "date");
        assert_eq!(
            tool.metadata.as_ref().unwrap()["result"]["output"],
            "today\n"
        );
    }

    #[test]
    fn projector_preserves_hosted_web_lifecycle_and_clickable_sources() {
        let entries = project_transcript_entries("thread-1", &[summary(1, Message::Assistant {
            content: vec![
                AssistantBlock::ProviderTool(psychevo_runtime::ProviderToolBlock {
                    id: "ws_1".into(), name: "web_search".into(),
                    action: Some(json!({"type":"search","query":"rust news"})), status: "completed".into(),
                }),
                AssistantBlock::Source(psychevo_ai::AssistantSource::UrlCitation(psychevo_ai::UrlCitationSource {
                    url: "https://example.com/rust".into(), title: "Rust".into(), start_index: Some(0), end_index: Some(4),
                })),
            ], timestamp_ms: 10, finish_reason: Some("completed".into()), outcome: Outcome::Normal,
            model: Some("gpt-5".into()), provider: Some("openai".into()),
        })]);
        assert_eq!(entries[0].blocks.len(), 2);
        assert_eq!(entries[0].blocks[0].kind, TranscriptBlockKind::Web);
        assert_eq!(entries[0].blocks[0].title.as_deref(), Some("Searched the web"));
        assert_eq!(entries[0].blocks[1].metadata.as_ref().unwrap()["projection"], "url_citation");
        assert_eq!(entries[0].blocks[1].metadata.as_ref().unwrap()["url"], "https://example.com/rust");
    }

    #[test]
    fn projector_decodes_wrapped_local_web_search_result_for_committed_transcript() {
        let envelope = json!({
            "query": "Rust async cancellation",
            "provider": "exa",
            "execution_owner": "runtime",
            "payload": {
                "type": "results",
                "items": [{
                    "title": "Async cancellation in Rust",
                    "url": "https://example.test/rust-async",
                    "text": "Cancellation requires an explicit ownership boundary."
                }]
            },
            "truncated": false,
            "error": null
        });
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_search",
                        "web_search",
                        json!({"query": "Rust async cancellation"}),
                    )],
                    timestamp_ms: 1,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("mock-model".to_string()),
                    provider: Some("mock".to_string()),
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_search".to_string(),
                    tool_name: "web_search".to_string(),
                    content: format!(
                        "<external_untrusted_web_search>\n{}\n</external_untrusted_web_search>",
                        envelope
                    ),
                    is_error: false,
                    timestamp_ms: 2,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);
        let block = &entries[0].blocks[0];
        assert_eq!(block.kind, TranscriptBlockKind::Web);
        assert_eq!(
            block.metadata.as_ref().unwrap()["result"]["payload"]["items"][0]["title"],
            "Async cancellation in Rust"
        );
        let public_result: Value = serde_json::from_str(
            &block.result.as_ref().expect("tool result").content,
        )
        .expect("decoded public tool result");
        assert_eq!(
            public_result["payload"]["items"][0]["title"],
            "Async cancellation in Rust"
        );
        assert!(
            !block
                .body
                .as_deref()
                .unwrap_or_default()
                .contains("external_untrusted_web_search")
        );
    }

    #[test]
    fn projector_derives_failed_write_preview_without_changing_persisted_values() {
        let arguments = json!({"path": "report.md", "content": "unfinished body"});
        let result_content = json!({"error": "permission denied"}).to_string();
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call("call-write", "write", arguments.clone())],
                    timestamp_ms: 1,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call-write".to_string(),
                    tool_name: "write".to_string(),
                    content: result_content.clone(),
                    is_error: true,
                    timestamp_ms: 2,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);
        let block = &entries[0].blocks[0];
        let metadata = block.metadata.as_ref().expect("write metadata");
        assert_eq!(metadata["args"], arguments);
        assert_eq!(
            metadata["write_argument_preview"],
            json!({
                "phase": "failed",
                "path": "report.md",
                "text": "unfinished body",
                "bytes_seen": 15,
                "lines_seen": 1,
                "omitted_bytes": 0,
                "truncated": false,
            })
        );
        assert_eq!(
            block.result.as_ref().expect("write result").content,
            result_content
        );
        match &summaries[0].message {
            Message::Assistant { content, .. } => match &content[0] {
                AssistantBlock::ToolCall(call) => assert_eq!(call.arguments, arguments),
                other => panic!("unexpected assistant block: {other:?}"),
            },
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn projector_does_not_attach_write_preview_to_successful_history() {
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call(
                        "call-write",
                        "write",
                        json!({"path": "report.md", "content": "complete body"}),
                    )],
                    timestamp_ms: 1,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call-write".to_string(),
                    tool_name: "write".to_string(),
                    content: json!({"path": "report.md", "bytes_written": 13}).to_string(),
                    is_error: false,
                    timestamp_ms: 2,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);
        let metadata = entries[0].blocks[0]
            .metadata
            .as_ref()
            .expect("write metadata");
        assert!(metadata.get("write_argument_preview").is_none());
    }

    #[test]
    fn projector_materializes_acp_plan_metadata_as_a_display_only_status_block() {
        let mut assistant = summary(
            1,
            Message::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "Plan accepted.".to_string(),
                }],
                timestamp_ms: 10,
                finish_reason: Some("end_turn".to_string()),
                outcome: Outcome::Normal,
                model: Some("OpenCode".to_string()),
                provider: Some("acp:opencode".to_string()),
            },
        );
        assistant.metadata = Some(json!({
            "acp": {
                "messageIds": ["assistant-1"],
                "origin": "live",
                "turnId": "turn-plan",
                "plan": {
                    "body": "- [x] Inspect repo\n- [~] Persist the latest plan",
                    "update": {
                        "sessionUpdate": "plan",
                        "entries": [
                            {"content": "Inspect repo", "priority": "high", "status": "completed"},
                            {"content": "Persist the latest plan", "priority": "high", "status": "in_progress"}
                        ]
                    }
                }
            }
        }));

        let entries = project_transcript_entries("thread-1", &[assistant]);

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0]
                .blocks
                .iter()
                .map(|block| block.kind)
                .collect::<Vec<_>>(),
            vec![TranscriptBlockKind::Text, TranscriptBlockKind::Status]
        );
        let plan = &entries[0].blocks[1];
        assert_eq!(plan.id, "turn:turn-plan:acp-peer-plan");
        assert_eq!(plan.status, TranscriptBlockStatus::Completed);
        assert_eq!(plan.title.as_deref(), Some("Plan"));
        assert_eq!(
            plan.body.as_deref(),
            Some("- [x] Inspect repo\n- [~] Persist the latest plan")
        );
        assert_eq!(plan.metadata.as_ref().unwrap()["projection"], "acp_peer_plan");
        assert_eq!(plan.metadata.as_ref().unwrap()["origin"], "acp_peer");
        assert_eq!(plan.metadata.as_ref().unwrap()["source"], "acp_peer");
        assert_eq!(plan.metadata.as_ref().unwrap()["turnId"], "turn-plan");
        assert_eq!(
            plan.metadata.as_ref().unwrap()["plan"]["entries"][1]["content"],
            "Persist the latest plan"
        );
    }

    #[test]
    fn projector_promotes_generated_image_tool_result_to_artifact_block() {
        let content = json!({
            "status": "completed",
            "mediaKind": "generated_image",
            "artifactId": "img_test",
            "prompt": "a red cube",
            "savedPath": "/tmp/psychevo/media/generated/img_test.png",
            "displayUrl": "/_gateway/media/img_test",
            "agentVisibleSource": "psychevo-media://img_test",
            "mimeType": "image/png",
            "provider": "fake",
            "model": "fake-image"
        });
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_image",
                        "image_generate",
                        json!({"prompt": "a red cube"}),
                    )],
                    timestamp_ms: 10,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_image".to_string(),
                    tool_name: "image_generate".to_string(),
                    content: content.to_string(),
                    is_error: false,
                    timestamp_ms: 20,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);

        assert_eq!(entries.len(), 1);
        let block = &entries[0].blocks[0];
        assert_eq!(block.kind, TranscriptBlockKind::Artifact);
        assert_eq!(block.artifact_ids, vec!["img_test".to_string()]);
        assert_eq!(block.title.as_deref(), Some("Generated image"));
        assert!(block.body.as_deref().unwrap_or_default().contains("a red cube"));
        assert_eq!(
            block.metadata.as_ref().unwrap()["result"]["displayUrl"],
            "/_gateway/media/img_test"
        );
    }

    #[test]
    fn projector_titles_pending_exec_command_from_arguments() {
        let command = "sqlite3 /home/kevin/Projects/feedgarden/feeds/.cache/hn.db \"SELECT id, title FROM stories ORDER BY score DESC;\"";
        let summaries = vec![summary(
            1,
            Message::Assistant {
                content: vec![tool_call(
                    "call_exec",
                    "exec_command",
                    json!({"cmd": command, "yield_time_ms": 1000}),
                )],
                timestamp_ms: 10,
                finish_reason: Some("tool_calls".to_string()),
                outcome: Outcome::Normal,
                model: Some("model".to_string()),
                provider: Some("provider".to_string()),
            },
        )];

        let entries = project_transcript_entries("thread-1", &summaries);
        let block = &entries[0].blocks[0];

        assert_eq!(block.status, TranscriptBlockStatus::Pending);
        assert_eq!(
            block.title.as_deref(),
            Some(format!("exec_command {command}").as_str())
        );
        assert_eq!(block.metadata.as_ref().unwrap()["tool_call_id"], "call_exec");
        assert_eq!(block.metadata.as_ref().unwrap()["args"]["cmd"], command);
        assert_eq!(
            block.detail.as_deref(),
            Some(json!({"cmd": command, "yield_time_ms": 1000}).to_string().as_str())
        );
    }

    #[test]
    fn projector_reloads_agent_tool_calls_as_agent_blocks() {
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_agent",
                        "spawn_agent",
                        json!({"agent_type": "Planck", "task_name": "inspect", "message": "Inspect"}),
                    )],
                    timestamp_ms: 10,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_agent".to_string(),
                    tool_name: "spawn_agent".to_string(),
                    content: serde_json::to_string(&json!({
                        "agent_name": "Planck",
                        "child_session_id": "child-thread",
                        "parent_session_id": "thread-1",
                        "task_name": "Inspect"
                    }))
                    .expect("agent result json"),
                    is_error: false,
                    timestamp_ms: 20,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);
        let block = &entries[0].blocks[0];

        assert_eq!(block.kind, TranscriptBlockKind::Agent);
        assert_eq!(block.status, TranscriptBlockStatus::Completed);
        assert_eq!(block.metadata.as_ref().unwrap()["result"]["child_session_id"], "child-thread");
        assert_eq!(block.result.as_ref().unwrap().result_message_seq, 2);
    }

    #[test]
    fn projector_reloads_agent_tool_prompt_into_result_metadata() {
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_agent",
                        "spawn_agent",
                        json!({
                            "agent_type": "translate",
                            "task_name": "translate_user_message_to_chinese",
                            "message": "Translate the following message to Chinese: hello"
                        }),
                    )],
                    timestamp_ms: 10,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_agent".to_string(),
                    tool_name: "spawn_agent".to_string(),
                    content: serde_json::to_string(&json!({
                        "agent_name": "translate",
                        "child_session_id": "child-thread",
                        "parent_session_id": "thread-1",
                        "status": "completed",
                        "summary": "你好"
                    }))
                    .expect("agent result json"),
                    is_error: false,
                    timestamp_ms: 20,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);
        let block = &entries[0].blocks[0];
        let metadata = block.metadata.as_ref().expect("metadata");

        assert_eq!(block.kind, TranscriptBlockKind::Agent);
        assert_eq!(block.status, TranscriptBlockStatus::Completed);
        assert_eq!(
            metadata["result"]["task"],
            "Translate the following message to Chinese: hello"
        );
        assert_eq!(metadata["result"]["child_session_id"], "child-thread");
        assert_eq!(metadata["result"]["session_id"], "child-thread");
    }

    #[test]
    fn agent_edge_enrichment_restores_committed_child_session_target() {
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_agent_zh",
                        "spawn_agent",
                        json!({
                            "agent_type": "translate",
                            "task_name": "zh_to_en",
                            "message": "Translate 你好 to English"
                        }),
                    )],
                    timestamp_ms: 10,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_agent_zh".to_string(),
                    tool_name: "spawn_agent".to_string(),
                    content: serde_json::to_string(&json!({
                        "agent_id": "agent-run-1",
                        "agent_name": "translate",
                        "task_name": "zh-to-en",
                        "status": "completed",
                        "summary": "hello"
                    }))
                    .expect("agent result json"),
                    is_error: false,
                    timestamp_ms: 20,
                },
            ),
        ];
        let mut entries = project_transcript_entries("parent-thread", &summaries);
        let edges = vec![agent_edge(
            "parent-thread",
            "child-thread",
            json!({
                "agent": {
                    "id": "agent-run-1",
                    "name": "translate",
                    "task_name": "zh-to-en",
                    "task": "Translate 你好 to English",
                    "parent_tool_call_id": "call_agent_zh"
                }
            }),
        )];

        enrich_agent_blocks_from_edges(&mut entries, &edges);

        let metadata = entries[0].blocks[0].metadata.as_ref().expect("metadata");
        assert_eq!(metadata["result"]["child_session_id"], "child-thread");
        assert_eq!(metadata["result"]["session_id"], "child-thread");
        assert_eq!(metadata["result"]["parent_session_id"], "parent-thread");
        assert_eq!(metadata["result"]["agent_id"], "agent-run-1");
        assert_eq!(metadata["result"]["agent_name"], "translate");
        assert_eq!(
            entries[0].blocks[0]
                .result
                .as_ref()
                .unwrap()
                .metadata
                .as_ref()
                .unwrap()["result"]["child_session_id"],
            "child-thread"
        );
    }

    #[test]
    fn agent_edge_enrichment_does_not_make_failed_agent_blocks_openable() {
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_agent_bad",
                        "spawn_agent",
                        json!({
                            "agent_type": "translate",
                            "task_name": "zh_to_en",
                            "message": "Translate 你好 to English"
                        }),
                    )],
                    timestamp_ms: 10,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_agent_bad".to_string(),
                    tool_name: "spawn_agent".to_string(),
                    content: serde_json::to_string(&json!({
                        "error": "unknown agent"
                    }))
                    .expect("agent result json"),
                    is_error: true,
                    timestamp_ms: 20,
                },
            ),
        ];
        let mut entries = project_transcript_entries("parent-thread", &summaries);
        let edges = vec![agent_edge(
            "parent-thread",
            "child-thread",
            json!({
                "agent": {
                    "id": "agent-run-1",
                    "name": "translate",
                    "task_name": "zh-to-en",
                    "parent_tool_call_id": "call_agent_bad"
                }
            }),
        )];

        enrich_agent_blocks_from_edges(&mut entries, &edges);

        let block = &entries[0].blocks[0];
        assert_eq!(block.status, TranscriptBlockStatus::Failed);
        assert!(block.metadata.as_ref().unwrap()["result"]["child_session_id"].is_null());
        assert!(
            block
                .result
                .as_ref()
                .and_then(|result| result.metadata.as_ref())
                .is_none_or(|metadata| metadata["result"]["child_session_id"].is_null())
        );
    }

    #[test]
    fn projector_does_not_create_top_level_entry_for_unmatched_tool_result() {
        let entries = project_transcript_entries(
            "thread-1",
            &[summary(
                1,
                Message::ToolResult {
                    tool_call_id: "missing".to_string(),
                    tool_name: "exec_command".to_string(),
                    content: "orphan".to_string(),
                    is_error: true,
                    timestamp_ms: 10,
                },
            )],
        );

        assert!(entries.is_empty());
    }

    #[test]
    fn projector_merges_write_stdin_poll_into_exec_command_block() {
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_exec",
                        "exec_command",
                        json!({"cmd": "printf first"}),
                    )],
                    timestamp_ms: 1,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_exec".to_string(),
                    tool_name: "exec_command".to_string(),
                    content: "{\"session_id\":7,\"exit_code\":null,\"output\":\"first\\n\"}"
                        .to_string(),
                    is_error: false,
                    timestamp_ms: 2,
                },
            ),
            summary(
                3,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_poll",
                        "write_stdin",
                        json!({"session_id": 7, "yield_time_ms": 60000}),
                    )],
                    timestamp_ms: 3,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                4,
                Message::ToolResult {
                    tool_call_id: "call_poll".to_string(),
                    tool_name: "write_stdin".to_string(),
                    content: "{\"session_id\":null,\"exit_code\":0,\"output\":\"second\\n\"}"
                        .to_string(),
                    is_error: false,
                    timestamp_ms: 4,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);
        let exec = entries
            .iter()
            .flat_map(|entry| entry.blocks.iter())
            .find(|block| block.title.as_deref() == Some("exec_command printf first"))
            .expect("exec block");
        let poll = entries
            .iter()
            .flat_map(|entry| entry.blocks.iter())
            .find(|block| block.title.as_deref() == Some("write_stdin 7"))
            .expect("write_stdin block");

        assert_eq!(exec.status, TranscriptBlockStatus::Completed);
        assert_eq!(
            exec.metadata.as_ref().unwrap()["result"]["output"],
            "first\nsecond\n"
        );
        assert_eq!(exec.metadata.as_ref().unwrap()["result"]["exit_code"], 0);
        assert_eq!(poll.metadata.as_ref().unwrap()["hidden"], true);
    }

    #[test]
    fn projector_keeps_exec_command_invocation_title_when_result_display_is_json() {
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_exec",
                        "exec_command",
                        json!({"cmd": "sqlite3 feeds/.cache/x.db \"SELECT date FROM daily_picks;\""}),
                    )],
                    timestamp_ms: 1,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_exec".to_string(),
                    tool_name: "exec_command".to_string(),
                    content: serde_json::to_string(&json!({
                        "chunk_id": 0,
                        "exit_code": 0,
                        "output": "2072155613925437769|fchollet|Francois Chollet\n",
                        "display": "{\"chunk_id\":0,\"exit_code\":0,\"output\":\"2072155613925437769|fchollet|Francois Chollet\\n\"}"
                    }))
                    .expect("result json"),
                    is_error: false,
                    timestamp_ms: 2,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);
        let block = entries
            .iter()
            .flat_map(|entry| entry.blocks.iter())
            .find(|block| block.metadata.as_ref().unwrap()["tool_call_id"] == "call_exec")
            .expect("exec block");

        assert_eq!(
            block.title.as_deref(),
            Some("exec_command sqlite3 feeds/.cache/x.db \"SELECT date FROM daily_picks;\"")
        );
        assert_eq!(
            block.metadata.as_ref().unwrap()["args"]["cmd"],
            "sqlite3 feeds/.cache/x.db \"SELECT date FROM daily_picks;\""
        );
        assert_ne!(
            block.title.as_deref(),
            block
                .metadata
                .as_ref()
                .unwrap()
                .get("display")
                .and_then(Value::as_str)
        );
        assert!(block.metadata.as_ref().unwrap().get("display").is_none());
        assert!(
            block.metadata.as_ref().unwrap()["result"]["display"]
                .as_str()
                .unwrap_or_default()
                .contains("chunk_id")
        );
    }

    #[test]
    fn projector_promotes_acp_peer_result_display_title() {
        let summaries = vec![
            summary(
                1,
                Message::Assistant {
                    content: vec![tool_call(
                        "call_visual",
                        "exec_command",
                        json!({"cmd": "echo done"}),
                    )],
                    timestamp_ms: 1,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                2,
                Message::ToolResult {
                    tool_call_id: "call_visual".to_string(),
                    tool_name: "exec_command".to_string(),
                    content: serde_json::to_string(&json!({
                        "source": "acp_peer",
                        "display": "Run visual tool",
                        "output": "done\n"
                    }))
                    .expect("result json"),
                    is_error: false,
                    timestamp_ms: 2,
                },
            ),
        ];

        let entries = project_transcript_entries("thread-1", &summaries);
        let block = entries
            .iter()
            .flat_map(|entry| entry.blocks.iter())
            .find(|block| block.metadata.as_ref().unwrap()["tool_call_id"] == "call_visual")
            .expect("tool block");

        assert_eq!(block.title.as_deref(), Some("Run visual tool"));
        assert_eq!(
            block.metadata.as_ref().unwrap()["display"],
            "Run visual tool"
        );
        assert_eq!(block.metadata.as_ref().unwrap()["source"], "acp_peer");
    }

    #[test]
    fn projector_keeps_selected_skill_metadata_on_user_entry() {
        let mut user = summary(
            1,
            Message::User {
                content: vec![UserContentBlock::text("$x-daily")],
                timestamp_ms: 1,
            },
        );
        user.metadata = Some(json!({
            "prompt_prefix": {
                "selected_skills": [{"name": "x-daily", "path": "/tmp/x/SKILL.md"}]
            }
        }));

        let entries = project_transcript_entries("thread-1", &[user]);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].role, TranscriptEntryRole::User);
        assert_eq!(
            entries[0].metadata.as_ref().unwrap()["prompt_prefix"]["selected_skills"][0]["name"],
            "x-daily"
        );
        assert_eq!(
            entries[0].blocks[0].metadata.as_ref().unwrap()["prompt_prefix"]["selected_skills"][0]
                ["path"],
            "/tmp/x/SKILL.md"
        );
    }

    #[test]
    fn projector_reloads_user_shell_metadata_as_shell_evidence() {
        let mut user = summary(
            1,
            Message::User {
                content: vec![UserContentBlock::text(
                    "<user_shell_command><command>printf ok</command></user_shell_command>",
                )],
                timestamp_ms: 1,
            },
        );
        user.metadata = Some(json!({
            "user_shell": {
                "command": "printf ok",
                "cwd": "/tmp/work",
                "outcome": "normal",
                "is_error": false,
                "exit_code": 0,
                "truncated": false,
                "elapsed_ms": 12,
                "result": {
                    "exit_code": 0,
                    "output": "ok"
                }
            }
        }));

        let entries = project_transcript_entries("thread-1", &[user]);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].role, TranscriptEntryRole::User);
        assert_eq!(entries[0].blocks.len(), 1);
        let block = &entries[0].blocks[0];
        assert_eq!(block.kind, TranscriptBlockKind::Shell);
        assert_eq!(block.source, "runtime.user_shell");
        assert_eq!(block.title.as_deref(), Some("exec_command"));
        assert_eq!(block.metadata.as_ref().unwrap()["args"]["cmd"], "printf ok");
        assert_eq!(block.metadata.as_ref().unwrap()["result"]["output"], "ok");
        assert!(
            !block
                .body
                .as_deref()
                .unwrap_or_default()
                .contains("<user_shell_command>")
        );
    }

    #[test]
    fn committed_turn_projection_filters_by_first_message_sequence() {
        let summaries = vec![
            summary(
                1,
                Message::User {
                    content: vec![UserContentBlock::text("old")],
                    timestamp_ms: 1,
                },
            ),
            summary(
                2,
                Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "new".to_string(),
                    }],
                    timestamp_ms: 2,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
        ];

        let entries = project_committed_turn_entries("thread-1", &summaries, 2);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_seq, Some(2));
        assert_eq!(entries[0].blocks[0].body.as_deref(), Some("new"));
    }

    #[test]
    fn committed_turn_projection_stamps_turn_identity_and_assistant_segment_order() {
        let summaries = vec![
            summary(
                1,
                Message::User {
                    content: vec![UserContentBlock::text("old")],
                    timestamp_ms: 1,
                },
            ),
            summary(
                2,
                Message::User {
                    content: vec![UserContentBlock::text("new prompt")],
                    timestamp_ms: 2,
                },
            ),
            summary(
                3,
                Message::Assistant {
                    content: vec![AssistantBlock::Reasoning {
                        text: "thinking".to_string(),
                        provider_evidence: None,
                    }],
                    timestamp_ms: 3,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            summary(
                4,
                Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "final answer".to_string(),
                    }],
                    timestamp_ms: 4,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
        ];

        let entries = project_committed_turn_window_entries(
            "thread-1",
            &summaries,
            TurnProjectionWindow {
                turn_id: "turn-1",
                first_committed_seq: 2,
            },
        );

        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.message_seq, entry.turn_id.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                (Some(2), Some("turn-1")),
                (Some(3), Some("turn-1")),
                (Some(4), Some("turn-1")),
            ]
        );
        assert_eq!(entries[1].metadata.as_ref().unwrap()["liveOrder"], 0);
        assert_eq!(entries[2].metadata.as_ref().unwrap()["liveOrder"], 1);
    }

    #[test]
    fn projector_hides_side_inherited_parent_context() {
        let mut inherited = summary(
            1,
            Message::User {
                content: vec![UserContentBlock::text("parent history")],
                timestamp_ms: 1,
            },
        );
        let mut metadata = serde_json::Map::new();
        metadata.insert(
            psychevo_runtime::SIDE_INHERITED_METADATA_KEY.to_string(),
            json!({
                "hidden": true,
                "parent_session_id": "parent-thread",
            }),
        );
        inherited.metadata = Some(Value::Object(metadata));
        let side_local = summary(
            2,
            Message::User {
                content: vec![UserContentBlock::text("side prompt")],
                timestamp_ms: 2,
            },
        );

        let entries = project_transcript_entries("side-thread", &[inherited, side_local]);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_seq, Some(2));
        assert_eq!(entries[0].blocks[0].body.as_deref(), Some("side prompt"));
    }

    fn summary(session_seq: i64, message: Message) -> TuiMessageSummary {
        TuiMessageSummary {
            session_seq,
            message,
            usage: None,
            metadata: None,
            accounting: None,
        }
    }

    fn tool_call(id: &str, name: &str, arguments: Value) -> AssistantBlock {
        let arguments_json = arguments.to_string();
        serde_json::from_value(json!({
            "type": "tool_call",
            "id": id,
            "name": name,
            "arguments": arguments,
            "arguments_json": arguments_json,
            "arguments_error": null,
            "content_index": 0,
            "call_index": 0
        }))
        .expect("tool call block")
    }

    fn agent_edge(parent: &str, child: &str, metadata: Value) -> AgentEdgeRecord {
        AgentEdgeRecord {
            parent_session_id: parent.to_string(),
            child_session_id: child.to_string(),
            status: psychevo_runtime::AgentEdgeStatus::Closed,
            created_at_ms: 1,
            updated_at_ms: 2,
            metadata: Some(metadata),
        }
    }
