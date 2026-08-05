use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) fn resolve_path(value: &str, env: &BTreeMap<String, String>, cwd: &Path) -> PathBuf {
    let path = if value == "~" {
        env.get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.to_path_buf())
    } else if let Some(rest) = value.strip_prefix("~/") {
        env.get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.to_path_buf())
            .join(rest)
    } else {
        PathBuf::from(value)
    };
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

pub(crate) trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use agent_client_protocol::ErrorCode;
    use agent_client_protocol::schema::v2::{
        AudioContent, ContentBlock, EmbeddedResource, EmbeddedResourceResource, EnvVariable,
        McpServer, McpServerStdio, ResourceLink, SessionId, SessionUpdate, TextContent,
        TextResourceContents,
    };
    use psychevo::mcp::McpTransportInput;
    use psychevo::{ImageInput, StartThreadRequest};
    use serde_json::json;

    use crate::commands::{
        ACP_COMMAND_ADVERTISEMENT_LIMIT, acp_command_capabilities, available_command_lines_from,
        available_commands_from,
    };
    use crate::protocol::{
        ACP_TEXT_RESOURCE_MAX_BYTES, AcpUsageAccumulator, acp_mcp_servers, prompt_parts,
        runtime_event_session_update, tool_call_pending_raw_input,
    };
    use crate::stdio::{AcpOptions, AcpSession, PsychevoAcpAgent};

    #[tokio::test]
    async fn converts_acp_mcp_servers_to_runtime_inputs() {
        let servers = vec![McpServer::Stdio(
            McpServerStdio::new("repo tools", "server")
                .args(vec!["--stdio".to_string()])
                .env(vec![EnvVariable::new("A", "B")]),
        )];
        let converted = acp_mcp_servers(servers);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].name, "repo tools");
        match &converted[0].transport {
            McpTransportInput::Stdio { args, env, .. } => {
                assert_eq!(args, &vec!["--stdio".to_string()]);
                assert_eq!(env.get("A").map(String::as_str), Some("B"));
            }
            other => panic!("unexpected transport: {other:?}"),
        }
    }

    #[tokio::test]
    async fn converts_prompt_text_and_http_images() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let (text, images) = prompt_parts(
            vec![
                ContentBlock::Text(TextContent::new("hello")),
                ContentBlock::Image(
                    agent_client_protocol::schema::v2::ImageContent::new("", "image/png")
                        .uri("https://example.com/a.png"),
                ),
            ],
            &cwd,
        )
        .expect("prompt conversion");
        assert_eq!(text, "hello");
        assert_eq!(
            images,
            vec![ImageInput::ImageUrl(
                "https://example.com/a.png".to_string()
            )]
        );
    }

    #[test]
    fn prompt_conversion_rejects_audio_remote_links_and_oversized_embedded_text() {
        let cwd = std::env::current_dir().expect("cwd");
        let cases = [
            (
                ContentBlock::Audio(AudioContent::new("YXVkaW8=", "audio/mpeg")),
                "audio content is not supported",
            ),
            (
                ContentBlock::ResourceLink(ResourceLink::new(
                    "remote",
                    "https://example.com/context.txt",
                )),
                "remote ResourceLink is not supported",
            ),
            (
                ContentBlock::Resource(EmbeddedResource::new(
                    EmbeddedResourceResource::TextResourceContents(TextResourceContents::new(
                        "x".repeat(ACP_TEXT_RESOURCE_MAX_BYTES + 1),
                        "memory://oversized",
                    )),
                )),
                "text resource exceeds the 524288-byte limit",
            ),
        ];

        for (block, expected) in cases {
            let error = prompt_parts(vec![block], &cwd).expect_err("prompt must be rejected");
            assert_eq!(error.code, ErrorCode::InvalidParams);
            assert_eq!(
                error.data.as_ref().and_then(serde_json::Value::as_str),
                Some(expected)
            );
        }
    }

    #[test]
    fn local_resource_links_are_canonical_cwd_contained_and_byte_bounded() {
        let root =
            std::env::temp_dir().join(format!("psychevo-acp-resource-{}", uuid::Uuid::now_v7()));
        let cwd = root.join("workspace");
        std::fs::create_dir_all(&cwd).expect("workspace");
        let inside = cwd.join("inside.txt");
        let outside = root.join("outside.txt");
        let oversized = cwd.join("oversized.txt");
        std::fs::write(&inside, "inside").expect("inside resource");
        std::fs::write(&outside, "outside").expect("outside resource");
        std::fs::write(&oversized, vec![b'x'; ACP_TEXT_RESOURCE_MAX_BYTES + 1])
            .expect("oversized resource");

        for uri in ["inside.txt".to_string(), inside.display().to_string()] {
            let (text, images) = prompt_parts(
                vec![ContentBlock::ResourceLink(
                    ResourceLink::new("inside", uri).mime_type("text/plain".to_string()),
                )],
                &cwd,
            )
            .expect("contained resource");
            assert!(text.ends_with("inside"), "{text}");
            assert!(images.is_empty());
        }

        for uri in [outside.display().to_string(), "../outside.txt".to_string()] {
            let error = prompt_parts(
                vec![ContentBlock::ResourceLink(
                    ResourceLink::new("outside", uri).mime_type("text/plain".to_string()),
                )],
                &cwd,
            )
            .expect_err("cwd escape must be rejected");
            assert_eq!(error.code, ErrorCode::InvalidParams);
            assert_eq!(
                error.data.as_ref().and_then(serde_json::Value::as_str),
                Some("local ResourceLink must resolve to a file inside the session cwd")
            );
        }

        #[cfg(unix)]
        {
            let escape = cwd.join("escape.txt");
            std::os::unix::fs::symlink(&outside, &escape).expect("escape symlink");
            let error = prompt_parts(
                vec![ContentBlock::ResourceLink(
                    ResourceLink::new("escape", "escape.txt").mime_type("text/plain".to_string()),
                )],
                &cwd,
            )
            .expect_err("symlink escape must be rejected");
            assert_eq!(error.code, ErrorCode::InvalidParams);
        }

        let error = prompt_parts(
            vec![ContentBlock::ResourceLink(
                ResourceLink::new("oversized", "oversized.txt").mime_type("text/plain".to_string()),
            )],
            &cwd,
        )
        .expect_err("oversized linked text must be rejected");
        assert_eq!(error.code, ErrorCode::InvalidParams);
        assert_eq!(
            error.data.as_ref().and_then(serde_json::Value::as_str),
            Some("text resource exceeds the 524288-byte limit")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn synthesizes_usage_from_runtime_accounting() {
        let mut usage = AcpUsageAccumulator::default();
        usage.record_runtime_value(&json!({
            "type": "message_end",
            "accounting": {
                "billable_input_tokens": 8,
                "billable_output_tokens": 5,
                "cache_read_tokens": 2,
                "reasoning_tokens": 1,
                "reported_total_tokens": 16,
            },
        }));

        assert_eq!(usage.context_tokens_for_usage_update(), Some(16));
        let metrics = usage.to_usage().expect("usage");
        assert_eq!(metrics.input_tokens, 10);
        assert_eq!(metrics.output_tokens, 6);
        assert_eq!(metrics.cached_read_tokens, Some(2));
        assert_eq!(metrics.thought_tokens, Some(1));
        assert_eq!(metrics.total_tokens, 16);
    }

    #[tokio::test]
    async fn tool_call_pending_raw_input_preserves_partial_arguments() {
        assert_eq!(
            tool_call_pending_raw_input(&json!({
                "arguments_json": "{\"path\":\"add.py\"",
            })),
            json!({
                "arguments_json": "{\"path\":\"add.py\"",
                "partial": true,
            })
        );
        assert_eq!(
            tool_call_pending_raw_input(&json!({
                "arguments_json": "{\"path\":\"add.py\"}",
            })),
            json!({ "path": "add.py" })
        );
    }

    #[tokio::test]
    async fn runtime_tool_execution_start_includes_timing_meta() {
        let update = runtime_event_session_update(&json!({
            "type": "tool_execution_start",
            "tool_call_id": "call-1",
            "tool_name": "edit",
            "args": { "path": "add.py" },
            "started_at_ms": 1_234,
        }))
        .expect("session update");

        let SessionUpdate::ToolCallUpdate(tool_call) = update else {
            panic!("expected tool call update");
        };
        assert_eq!(
            tool_call.meta.value().expect("meta")["psychevo"]["toolTiming"],
            json!({
                "source": "psychevo",
                "startedAtMs": 1_234,
            })
        );
    }

    #[tokio::test]
    async fn runtime_tool_execution_end_includes_timing_meta() {
        let update = runtime_event_session_update(&json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-1",
            "tool_name": "edit",
            "result": { "success": true },
            "outcome": "normal",
            "elapsed_ms": 321,
        }))
        .expect("session update");

        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("expected tool call update");
        };
        assert_eq!(
            update.meta.value().expect("meta")["psychevo"]["toolTiming"],
            json!({
                "source": "psychevo",
                "elapsedMs": 321,
            })
        );
    }

    #[tokio::test]
    async fn advertises_tools_slash_command() {
        let commands = available_command_lines_from(available_commands_from(
            psychevo::command_registry::available_slash_commands_for_surface(
                acp_command_capabilities(),
                false,
                &[],
                ACP_COMMAND_ADVERTISEMENT_LIMIT,
            ),
        ))
        .join("\n");
        assert!(
            commands.contains("/tools [list|enable|disable <toolset>] - toolsets"),
            "{commands}"
        );
    }

    #[tokio::test]
    async fn parses_slash_prompt_command_and_args() {
        use psychevo::command_registry::{
            SlashCommandAction, SlashCommandParse, parse_slash_command_line,
        };

        let SlashCommandParse::Known(invocation) = parse_slash_command_line(" /tools ") else {
            panic!("expected known command");
        };
        assert_eq!(invocation.spec.action, SlashCommandAction::Tools);
        assert!(invocation.args.is_empty());

        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/mode plan") else {
            panic!("expected known command");
        };
        assert_eq!(invocation.spec.action, SlashCommandAction::ModeSet);
        assert_eq!(invocation.args, "plan");

        assert!(matches!(
            parse_slash_command_line("hello /tools"),
            SlashCommandParse::NotSlash
        ));
    }

    #[tokio::test]
    async fn handles_status_slash_command_locally() {
        let agent = PsychevoAcpAgent::new(AcpOptions {
            home: std::env::temp_dir().join("psychevo-acp-test-home"),
            db_path: PathBuf::from(":memory:"),
            config_path: None,
            inherited_env: BTreeMap::new(),
        })
        .await
        .expect("agent");
        let session_id = SessionId::new("acp-test");
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let thread = agent
            .framework
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .expect("thread");
        let session = AcpSession::loaded(cwd, thread, Vec::new());
        let text = agent.status_command_text(&session_id, &session).await;
        assert!(text.contains("ACP session: acp-test"), "{text}");
        assert!(text.contains("commands: "), "{text}");
    }
}
