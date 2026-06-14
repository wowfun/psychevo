#[cfg(test)]
mod tests {
    use super::*;
    use psychevo_gateway::TranscriptEntryRole;
    use psychevo_runtime::command_registry::{
        SlashCommandEffect, SlashCommandParse, SlashCommandSurface,
        available_slash_commands_for_surface, parse_slash_command_line, slash_invocation_effect,
    };
    use psychevo_runtime::{
        SessionExportInclude, WorkspaceDiffFileStatus, WorkspaceDiffTruncation,
    };

    #[test]
    fn acp_advertises_diff_and_allows_it_during_active_turns() {
        let available = available_slash_commands_for_surface(
            acp_command_capabilities(),
            true,
            &[],
            ACP_COMMAND_ADVERTISEMENT_LIMIT,
        );
        assert!(
            available
                .commands
                .iter()
                .any(|command| command.name == "diff"),
            "{available:?}"
        );

        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/diff") else {
            panic!("expected /diff to parse");
        };
        let effect = slash_invocation_effect(
            &invocation,
            acp_command_capabilities(),
            SlashCommandSurface::Acp,
            true,
        )
        .expect("slash effect");
        assert_eq!(effect, SlashCommandEffect::Diff);
    }

    #[test]
    fn acp_advertises_undo_redo_when_idle() {
        let available = available_slash_commands_for_surface(
            acp_command_capabilities(),
            false,
            &[],
            ACP_COMMAND_ADVERTISEMENT_LIMIT,
        );
        let names = available
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"undo"), "{names:?}");
        assert!(names.contains(&"redo"), "{names:?}");

        let SlashCommandParse::Known(undo) = parse_slash_command_line("/undo") else {
            panic!("expected /undo to parse");
        };
        let effect = slash_invocation_effect(
            &undo,
            acp_command_capabilities(),
            SlashCommandSurface::Acp,
            false,
        )
        .expect("undo effect");
        assert_eq!(effect, SlashCommandEffect::Undo);

        let active = available_slash_commands_for_surface(
            acp_command_capabilities(),
            true,
            &[],
            ACP_COMMAND_ADVERTISEMENT_LIMIT,
        );
        assert!(!active.commands.iter().any(|command| command.name == "undo"));
        assert!(!active.commands.iter().any(|command| command.name == "redo"));
    }

    #[test]
    fn acp_export_parses_last_provider_response_include() {
        let parsed = parse_artifact_args(
            "out.json -f json -i last-provider-response",
            SessionArtifactKind::Export,
        )
        .expect("export args");
        assert_eq!(parsed.format, Some(SessionExportFormat::Json));
        assert!(parsed.path.as_deref() == Some(Path::new("out.json")));
        assert!(parsed.include.is_some_and(|include| {
            include.contains(SessionExportInclude::LastProviderResponse)
        }));

        let share = parse_artifact_args(
            "share.md -i last-provider-response",
            SessionArtifactKind::Share,
        );
        assert!(share.is_err());
        assert!(
            parse_artifact_args("out.json -i last-raw-response", SessionArtifactKind::Export)
                .is_err()
        );
    }

    #[test]
    fn diff_tool_call_update_uses_structured_diff_without_text_fallback() {
        let diff = sample_workspace_diff();
        let (start, completed) = diff_tool_call_updates("slash_diff_test", &diff);

        match start {
            SessionUpdate::ToolCall(call) => {
                assert_eq!(call.title, "Workspace diff");
                assert_eq!(call.kind, ToolKind::Read);
                assert_eq!(call.status, ToolCallStatus::InProgress);
                assert_eq!(
                    call.raw_input
                        .as_ref()
                        .and_then(|value| value.get("command"))
                        .and_then(Value::as_str),
                    Some("/diff")
                );
                assert!(call.content.is_empty());
            }
            SessionUpdate::AgentMessageChunk(_) => panic!("diff must not use assistant text"),
            other => panic!("unexpected start update: {other:?}"),
        }

        match completed {
            SessionUpdate::ToolCallUpdate(update) => {
                assert_eq!(update.fields.title.as_deref(), Some("Workspace diff"));
                assert_eq!(update.fields.kind, Some(ToolKind::Read));
                assert_eq!(update.fields.status, Some(ToolCallStatus::Completed));
                let content = update.fields.content.expect("diff content");
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ToolCallContent::Diff(diff) => {
                        assert_eq!(diff.path, PathBuf::from("src/lib.rs"));
                        assert_eq!(diff.old_text.as_deref(), Some("old body\n"));
                        assert_eq!(diff.new_text, "new body\n");
                    }
                    other => panic!("unexpected content: {other:?}"),
                }

                let raw = update.fields.raw_output.expect("raw output");
                assert_eq!(raw.get("status").and_then(Value::as_str), Some("ok"));
                assert_eq!(raw.get("file_count").and_then(Value::as_u64), Some(1));
                assert_eq!(
                    raw.pointer("/truncation/truncated")
                        .and_then(Value::as_bool),
                    Some(true)
                );
                let raw_text = serde_json::to_string(&raw).expect("raw output json");
                assert!(!raw_text.contains("UNIFIED_PATCH_BODY_SHOULD_NOT_APPEAR"));
                assert!(!raw_text.contains("new body"));
            }
            SessionUpdate::AgentMessageChunk(_) => panic!("diff must not use assistant text"),
            other => panic!("unexpected completed update: {other:?}"),
        }
    }

    #[test]
    fn reasoning_blocks_emit_incremental_thought_chunks() {
        let mut projection = AcpLiveProjection::new(false);
        let mut block = sample_transcript_block(
            "reasoning-1",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Running,
            Some("Thinking"),
            Some("first"),
            None,
        );
        let entry = sample_transcript_entry(vec![block.clone()]);
        let session_id = SessionId::new("session-test");

        let updates = transcript_entry_session_updates(&entry, &session_id, &mut projection, true);
        assert_eq!(updates.len(), 1);
        assert_eq!(thought_text(&updates[0]), Some("first"));

        block.body = Some("first second".to_string());
        let entry = sample_transcript_entry(vec![block.clone()]);
        let updates = transcript_entry_session_updates(&entry, &session_id, &mut projection, true);
        assert_eq!(updates.len(), 1);
        assert_eq!(thought_text(&updates[0]), Some(" second"));

        let updates = transcript_entry_session_updates(&entry, &session_id, &mut projection, true);
        assert!(updates.is_empty(), "{updates:?}");
        let updates = transcript_entry_session_updates(&entry, &session_id, &mut projection, false);
        assert!(updates.is_empty(), "{updates:?}");
    }

    #[test]
    fn exec_command_update_shows_command_title_content_and_raw_input() {
        let mut projection = AcpLiveProjection::new(false);
        let block = sample_transcript_block(
            "tool:call_exec",
            TranscriptBlockKind::Shell,
            TranscriptBlockStatus::Running,
            Some("exec_command cargo test"),
            Some("running tests\n"),
            Some(json!({
                "tool_name": "exec_command",
                "tool_call_id": "call_exec",
                "args": {"cmd": "cargo test\n--workspace"}
            })),
        );

        let update =
            transcript_block_session_update(&block, &mut projection, true).expect("tool update");
        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("unexpected update: {update:?}");
        };
        assert_eq!(
            update.fields.title.as_deref(),
            Some("exec_command cargo test")
        );
        assert_eq!(update.fields.kind, Some(ToolKind::Execute));
        assert_eq!(update.fields.status, Some(ToolCallStatus::InProgress));
        assert_eq!(
            update
                .fields
                .raw_input
                .as_ref()
                .and_then(|value| value.pointer("/args/cmd"))
                .and_then(Value::as_str),
            Some("cargo test\n--workspace")
        );
        let content = update.fields.content.expect("tool content");
        assert_eq!(
            tool_content_text(&content[0]),
            Some("$ cargo test\n--workspace\n\nrunning tests\n")
        );
    }

    #[test]
    fn terminal_output_opt_in_uses_terminal_content_and_meta() {
        let mut projection = AcpLiveProjection::new(true);
        let mut block = sample_transcript_block(
            "tool:call_exec",
            TranscriptBlockKind::Shell,
            TranscriptBlockStatus::Running,
            Some("exec_command python fetch.py"),
            Some("first\n"),
            Some(json!({
                "tool_name": "exec_command",
                "tool_call_id": "call_exec",
                "args": {"cmd": "python fetch.py"}
            })),
        );

        let update =
            transcript_block_session_update(&block, &mut projection, true).expect("tool update");
        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("unexpected update: {update:?}");
        };
        let content = update.fields.content.expect("terminal content");
        assert_eq!(tool_content_text(&content[0]), Some("$ python fetch.py"));
        let meta = update.meta.expect("terminal meta");
        assert_eq!(meta["terminal_info"]["terminal_id"], "call_exec");
        assert_eq!(
            meta["terminal_output"]["data"],
            "$ python fetch.py\nfirst\n"
        );

        block.body = Some("first\nsecond\n".to_string());
        let update =
            transcript_block_session_update(&block, &mut projection, true).expect("tool update");
        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("unexpected update: {update:?}");
        };
        let meta = update.meta.expect("terminal meta");
        assert_eq!(meta["terminal_output"]["data"], "second\n");

        block.status = TranscriptBlockStatus::Completed;
        block.body = Some("first\nsecond\nthird\n".to_string());
        block.metadata = Some(json!({
            "tool_name": "exec_command",
            "tool_call_id": "call_exec",
            "args": {"cmd": "python fetch.py"},
            "result": {"exit_code": 0}
        }));
        let update =
            transcript_block_session_update(&block, &mut projection, true).expect("tool update");
        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("unexpected update: {update:?}");
        };
        let meta = update.meta.expect("terminal meta");
        assert_eq!(meta["terminal_output"]["data"], "third\n");
        assert_eq!(meta["terminal_exit"]["terminal_id"], "call_exec");
        assert_eq!(meta["terminal_exit"]["exit_code"].as_i64(), Some(0));
    }

    fn thought_text(update: &SessionUpdate) -> Option<&str> {
        let SessionUpdate::AgentThoughtChunk(chunk) = update else {
            return None;
        };
        match &chunk.content {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        }
    }

    fn tool_content_text(content: &ToolCallContent) -> Option<&str> {
        let ToolCallContent::Content(content) = content else {
            return None;
        };
        match &content.content {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        }
    }

    fn sample_transcript_entry(blocks: Vec<TranscriptBlock>) -> TranscriptEntry {
        TranscriptEntry {
            id: "entry-1".to_string(),
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            message_seq: None,
            role: TranscriptEntryRole::Assistant,
            status: TranscriptBlockStatus::Running,
            source: "live".to_string(),
            blocks,
            metadata: None,
            usage: None,
            accounting: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    fn sample_transcript_block(
        id: &str,
        kind: TranscriptBlockKind,
        status: TranscriptBlockStatus,
        title: Option<&str>,
        body: Option<&str>,
        metadata: Option<Value>,
    ) -> TranscriptBlock {
        TranscriptBlock {
            id: id.to_string(),
            kind,
            status,
            order: 0,
            source: "live".to_string(),
            title: title.map(ToString::to_string),
            body: body.map(ToString::to_string),
            preview: None,
            detail: None,
            artifact_ids: Vec::new(),
            metadata,
            result: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    fn sample_workspace_diff() -> WorkspaceDiff {
        WorkspaceDiff {
            is_git_repo: true,
            files: vec![WorkspaceDiffFile {
                path: "src/lib.rs".to_string(),
                status: WorkspaceDiffFileStatus::Modified,
                old_text: Some("old body\n".to_string()),
                new_text: Some("new body\n".to_string()),
                binary: false,
                unreadable: false,
                placeholder: None,
            }],
            unified_diff:
                "diff --git a/src/lib.rs b/src/lib.rs\n+UNIFIED_PATCH_BODY_SHOULD_NOT_APPEAR\n"
                    .to_string(),
            truncation: WorkspaceDiffTruncation {
                truncated: true,
                max_bytes: 256,
                max_lines: 3000,
                omitted_bytes: 64,
                omitted_lines: 2,
            },
        }
    }
}
