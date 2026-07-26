#[cfg(test)]
mod tests {
    use super::*;
    use psychevo::command_registry::{
        SlashCommandEffect, SlashCommandParse, SlashCommandSurface,
        available_slash_commands_for_surface, parse_slash_command_line, slash_invocation_effect,
    };
    use psychevo::{session_export::SessionExportInclude, workspace_diff::WorkspaceDiffFileStatus, workspace_diff::WorkspaceDiffTruncation};

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
    fn acp_does_not_advertise_side_conversation_commands() {
        let available = available_slash_commands_for_surface(
            acp_command_capabilities(),
            false,
            &[],
            ACP_COMMAND_ADVERTISEMENT_LIMIT,
        );
        assert!(
            !available
                .commands
                .iter()
                .any(|command| command.name == "btw")
        );

        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/btw explain") else {
            panic!("expected /btw to parse");
        };
        let effect = slash_invocation_effect(
            &invocation,
            acp_command_capabilities(),
            SlashCommandSurface::Acp,
            false,
        )
        .expect("unsupported command guidance");
        assert!(matches!(
            effect,
            SlashCommandEffect::Unsupported(message) if message.contains("side chat")
        ));
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
            SessionUpdate::ToolCallUpdate(call) => {
                assert_eq!(
                    call.title.value().map(String::as_str),
                    Some("Workspace diff")
                );
                assert_eq!(call.kind.value(), Some(&ToolKind::Read));
                assert_eq!(call.status.value(), Some(&ToolCallStatus::InProgress));
                assert_eq!(
                    call.raw_input
                        .value()
                        .and_then(|value| value.get("command"))
                        .and_then(Value::as_str),
                    Some("/diff")
                );
                assert!(call.content.value().is_none());
            }
            SessionUpdate::AgentMessageChunk(_) => panic!("diff must not use assistant text"),
            other => panic!("unexpected start update: {other:?}"),
        }

        match completed {
            SessionUpdate::ToolCallUpdate(update) => {
                assert_eq!(
                    update.title.value().map(String::as_str),
                    Some("Workspace diff")
                );
                assert_eq!(update.kind.value(), Some(&ToolKind::Read));
                assert_eq!(update.status.value(), Some(&ToolCallStatus::Completed));
                let content = update.content.take().expect("diff content");
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ToolCallContent::Diff(diff) => {
                        assert_eq!(diff.changes.len(), 1);
                        let DiffChangeOperation::Modify(change) = &diff.changes[0].operation else {
                            panic!("expected modified-file change");
                        };
                        assert_eq!(change.path, PathBuf::from("src/lib.rs"));
                        assert!(
                            diff.patch
                                .as_ref()
                                .is_some_and(|patch| patch.diff.contains("UNIFIED_PATCH_BODY"))
                        );
                    }
                    other => panic!("unexpected content: {other:?}"),
                }

                let raw = update.raw_output.take().expect("raw output");
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
