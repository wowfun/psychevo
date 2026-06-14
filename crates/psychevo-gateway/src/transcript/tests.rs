    use psychevo_runtime::{Outcome, UserContentBlock};

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
        assert_eq!(tool.result.as_ref().unwrap().result_message_seq, 2);
        assert_eq!(tool.metadata.as_ref().unwrap()["args"]["cmd"], "date");
        assert_eq!(
            tool.metadata.as_ref().unwrap()["result"]["output"],
            "today\n"
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
            .find(|block| block.title.as_deref() == Some("exec_command"))
            .expect("exec block");
        let poll = entries
            .iter()
            .flat_map(|entry| entry.blocks.iter())
            .find(|block| block.title.as_deref() == Some("write_stdin"))
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
                "workdir": "/tmp/work",
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
