#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn send_runtime_event_update(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    value: Value,
) {
    let Some(event_type) = value.get("type").and_then(Value::as_str) else {
        return;
    };
    match event_type {
        "tool_execution_start" => {
            let call_id = value
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let tool_name = value
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let args = value.get("args").cloned();
            send_session_update(
                cx,
                session_id.clone(),
                SessionUpdate::ToolCall(
                    ToolCall::new(call_id.to_string(), tool_title(tool_name))
                        .kind(tool_kind(tool_name))
                        .status(ToolCallStatus::InProgress)
                        .raw_input(args),
                ),
            );
        }
        "tool_execution_end" => {
            let call_id = value
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let tool_name = value
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let result = value.get("result").cloned();
            let failed = value
                .get("outcome")
                .and_then(Value::as_str)
                .is_some_and(|outcome| outcome != "normal");
            let content = result
                .as_ref()
                .map(compact_tool_result_text)
                .filter(|text| !text.is_empty())
                .map(|text| vec![ToolCallContent::from(text)])
                .unwrap_or_default();
            send_session_update(
                cx,
                session_id.clone(),
                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    call_id.to_string(),
                    ToolCallUpdateFields::new()
                        .title(tool_title(tool_name))
                        .status(if failed {
                            ToolCallStatus::Failed
                        } else {
                            ToolCallStatus::Completed
                        })
                        .content(content)
                        .raw_output(result),
                )),
            );
        }
        _ => {}
    }
}

pub(crate) fn prompt_parts(prompt: Vec<ContentBlock>) -> (String, Vec<ImageInput>) {
    let mut text = Vec::new();
    let mut images = Vec::new();
    for block in prompt {
        match block {
            ContentBlock::Text(content) => text.push(content.text),
            ContentBlock::Image(content) => {
                if let Some(uri) = content.uri {
                    if uri.starts_with("http://") || uri.starts_with("https://") {
                        images.push(ImageInput::ImageUrl(uri));
                    } else {
                        text.push(format!("[image: {uri}]"));
                    }
                } else {
                    text.push("[embedded image omitted]".to_string());
                }
            }
            other => {
                if let Ok(serialized) = serde_json::to_string(&other) {
                    text.push(serialized);
                }
            }
        }
    }
    (text.join("\n\n"), images)
}

pub(crate) fn acp_mcp_servers(servers: Vec<McpServer>) -> Vec<McpServerInput> {
    servers
        .into_iter()
        .map(|server| match server {
            McpServer::Http(McpServerHttp {
                name, url, headers, ..
            }) => McpServerInput {
                name,
                transport: McpTransportInput::StreamableHttp {
                    url,
                    headers: headers
                        .into_iter()
                        .map(|header| (header.name, header.value))
                        .collect(),
                },
            },
            McpServer::Stdio(McpServerStdio {
                name,
                command,
                args,
                env,
                ..
            }) => McpServerInput {
                name,
                transport: McpTransportInput::Stdio {
                    command,
                    args,
                    env: env_variable_map(env),
                },
            },
            McpServer::Sse(server) => McpServerInput {
                name: server.name,
                transport: McpTransportInput::Unsupported {
                    kind: "sse".to_string(),
                },
            },
            McpServer::Acp(server) => McpServerInput {
                name: server.name,
                transport: McpTransportInput::Unsupported {
                    kind: "acp".to_string(),
                },
            },
            _ => McpServerInput {
                name: "unknown".to_string(),
                transport: McpTransportInput::Unsupported {
                    kind: "unknown".to_string(),
                },
            },
        })
        .collect()
}

pub(crate) fn env_variable_map(vars: Vec<EnvVariable>) -> BTreeMap<String, String> {
    vars.into_iter().map(|var| (var.name, var.value)).collect()
}

pub(crate) fn mode_state(mode: RunMode) -> SessionModeState {
    SessionModeState::new(
        mode.as_str(),
        vec![
            SessionMode::new("default", "Default").description("Run tools and edit code"),
            SessionMode::new("plan", "Plan").description("Discuss and inspect without edits"),
        ],
    )
}

pub(crate) fn session_config_options(
    mode: RunMode,
) -> Vec<agent_client_protocol::schema::SessionConfigOption> {
    vec![agent_client_protocol::schema::SessionConfigOption::select(
        "mode",
        "Mode",
        mode.as_str(),
        vec![
            SessionConfigSelectOption::new("default", "Default"),
            SessionConfigSelectOption::new("plan", "Plan"),
        ],
    )]
}

pub(crate) fn tool_title(tool_name: &str) -> String {
    if let Some(rest) = tool_name.strip_prefix("mcp__")
        && let Some((server, tool)) = rest.split_once("__")
    {
        return format!("Tool: {server}/{tool}");
    }
    format!("Tool: {tool_name}")
}

pub(crate) fn tool_kind(tool_name: &str) -> ToolKind {
    match tool_name {
        "read" => ToolKind::Read,
        "write" | "edit" => ToolKind::Edit,
        "exec_command" | "write_stdin" => ToolKind::Execute,
        "web_fetch" => ToolKind::Fetch,
        _ => ToolKind::Other,
    }
}

pub(crate) fn compact_tool_result_text(value: &Value) -> String {
    value
        .get("model_content")
        .and_then(Value::as_str)
        .or_else(|| value.get("error").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default())
}

pub(crate) fn stop_reason(outcome: psychevo_ai::Outcome) -> StopReason {
    match outcome {
        psychevo_ai::Outcome::Normal => StopReason::EndTurn,
        psychevo_ai::Outcome::Aborted => StopReason::Cancelled,
        psychevo_ai::Outcome::Stopped => StopReason::EndTurn,
        psychevo_ai::Outcome::Failed => StopReason::Refusal,
    }
}

pub(crate) fn acp_internal_error(err: impl std::fmt::Display) -> Error {
    Error::internal_error().data(err.to_string())
}

pub(crate) fn env_path_or_default(
    env: &BTreeMap<String, String>,
    name: &str,
    default: &str,
    cwd: &Path,
) -> PathBuf {
    env.get(name)
        .filter(|value| !value.trim().is_empty())
        .map(String::as_str)
        .unwrap_or(default)
        .pipe(|value| resolve_path(value, env, cwd))
}

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
        let (text, images) = prompt_parts(vec![
            ContentBlock::Text(agent_client_protocol::schema::TextContent::new("hello")),
            ContentBlock::Image(
                agent_client_protocol::schema::ImageContent::new("", "image/png")
                    .uri("https://example.com/a.png"),
            ),
        ]);
        assert_eq!(text, "hello");
        assert_eq!(
            images,
            vec![ImageInput::ImageUrl(
                "https://example.com/a.png".to_string()
            )]
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
