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
    pub(crate) use super::*;

    #[test]
    fn converts_acp_mcp_servers_to_runtime_inputs() {
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

    #[test]
    fn converts_prompt_text_and_http_images() {
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
        );
        assert_eq!(text, "hello");
        assert_eq!(
            images,
            vec![ImageInput::ImageUrl(
                "https://example.com/a.png".to_string()
            )]
        );
    }

    #[test]
    fn synthesizes_usage_from_runtime_accounting() {
        let mut usage = AcpUsageAccumulator::default();
        usage.record_stream_event(&RunStreamEvent::value(json!({
            "type": "message_end",
            "accounting": {
                "billable_input_tokens": 8,
                "billable_output_tokens": 5,
                "cache_read_tokens": 2,
                "reasoning_tokens": 1,
                "reported_total_tokens": 16,
            },
        })));

        assert_eq!(usage.context_tokens_for_usage_update(), Some(16));
        let metrics = usage.to_usage().expect("usage");
        assert_eq!(metrics.input_tokens, 10);
        assert_eq!(metrics.output_tokens, 6);
        assert_eq!(metrics.cached_read_tokens, Some(2));
        assert_eq!(metrics.thought_tokens, Some(1));
        assert_eq!(metrics.total_tokens, 16);
    }

    #[test]
    fn tool_call_pending_raw_input_preserves_partial_arguments() {
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

    #[test]
    fn runtime_tool_execution_start_includes_timing_meta() {
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
                "source": "psychevo_runtime",
                "startedAtMs": 1_234,
            })
        );
    }

    #[test]
    fn runtime_tool_execution_end_includes_timing_meta() {
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
                "source": "psychevo_runtime",
                "elapsedMs": 321,
            })
        );
    }

    #[test]
    fn advertises_tools_slash_command() {
        let commands = available_command_lines_from(available_commands_from(
            psychevo_runtime::command_registry::available_slash_commands_for_surface(
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

    #[test]
    fn parses_slash_prompt_command_and_args() {
        use psychevo_runtime::command_registry::{
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

    #[test]
    fn handles_status_slash_command_locally() {
        let agent = PsychevoAcpAgent::new(AcpOptions {
            home: std::env::temp_dir().join("psychevo-acp-test-home"),
            db_path: PathBuf::from(":memory:"),
            config_path: None,
            inherited_env: BTreeMap::new(),
        })
        .expect("agent");
        let session_id = SessionId::new("acp-test");
        let session = AcpSession::new(
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            None,
            Vec::new(),
        );
        let text = agent.status_command_text(&session_id, &session);
        assert!(text.contains("ACP session: acp-test"), "{text}");
        assert!(text.contains("commands: "), "{text}");
    }
}
