use super::*;

pub(super) fn requested_peer_mcp_server_names(
    peer: &ResolvedPeerTurn,
) -> psychevo_runtime::Result<std::collections::BTreeSet<String>> {
    for name in &peer.agent.tool_policy.mcp_servers {
        if !peer.backend.mcp_servers.contains(name) {
            return Err(crate::agent_session_error(
                "acp_mcp_backend_policy_rejected",
                crate::AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                format!(
                    "Agent `{}` requests MCP server `{name}`, but backend `{}` does not allow it.",
                    peer.agent.name, peer.backend.id
                ),
                Some(format!("acp-mcp:{}:{name}", peer.backend.id)),
            ));
        }
    }
    Ok(peer.agent.tool_policy.mcp_servers.clone())
}

pub(super) fn acp_mcp_server_declarations(
    peer: &ResolvedPeerTurn,
    resolved_servers: &[psychevo_runtime::ResolvedMcpServerInput],
    capabilities: &AgentCapabilities,
) -> psychevo_runtime::Result<Vec<McpServer>> {
    resolved_servers
        .iter()
        .map(|resolved| {
            let server = &resolved.server;
            validate_portable_acp_mcp_policy(server)?;
            match &server.transport {
                McpTransportInput::Stdio {
                    command,
                    args,
                    env,
                    cwd,
                } => {
                    if cwd.is_some() {
                        return Err(Error::Message(format!(
                            "ACP MCP server `{}` declares a stdio cwd, which stable ACP v1 cannot represent",
                            server.name
                        )));
                    }
                    Ok(McpServer::Stdio(
                        McpServerStdio::new(server.name.clone(), command.clone())
                            .args(args.clone())
                            .env(
                                env.iter()
                                    .map(|(name, value)| {
                                        EnvVariable::new(name.clone(), value.clone())
                                    })
                                    .collect(),
                            ),
                    ))
                }
                McpTransportInput::StreamableHttp { url, headers, .. } => {
                    if !capabilities.mcp_capabilities.http {
                        return Err(Error::Message(format!(
                            "ACP peer `{}` does not advertise HTTP MCP capability required by server `{}`",
                            peer.backend.id, server.name
                        )));
                    }
                    let parsed = reqwest::Url::parse(url).map_err(|error| {
                        Error::Message(format!(
                            "ACP MCP server `{}` has an invalid URL: {error}",
                            server.name
                        ))
                    })?;
                    if !matches!(parsed.scheme(), "http" | "https") {
                        return Err(Error::Message(format!(
                            "ACP MCP server `{}` URL must use http or https",
                            server.name
                        )));
                    }
                    let mut wire_headers = headers
                        .iter()
                        .map(|(name, value)| acp_mcp_http_header(&server.name, name, value))
                        .collect::<psychevo_runtime::Result<Vec<_>>>()?;
                    if let Some(token) = resolved.bearer_token.as_deref() {
                        if token.contains(['\r', '\n']) {
                            return Err(Error::Message(format!(
                                "ACP MCP server `{}` bearer token contains a line break",
                                server.name
                            )));
                        }
                        wire_headers.retain(|header| {
                            !header.name.eq_ignore_ascii_case("authorization")
                        });
                        wire_headers.push(HttpHeader::new(
                            "Authorization",
                            format!("Bearer {token}"),
                        ));
                    }
                    Ok(McpServer::Http(
                        McpServerHttp::new(server.name.clone(), url.clone())
                            .headers(wire_headers),
                    ))
                }
                McpTransportInput::Unsupported { kind } => Err(Error::Message(format!(
                    "ACP MCP server `{}` uses unsupported transport `{kind}`",
                    server.name
                ))),
            }
        })
        .collect()
}

fn validate_portable_acp_mcp_policy(
    server: &psychevo_runtime::McpServerInput,
) -> psychevo_runtime::Result<()> {
    let policy = &server.policy;
    if policy.required
        || policy.enabled_tools.is_some()
        || !policy.disabled_tools.is_empty()
        || policy.supports_parallel_tool_calls
        || policy.startup_timeout_secs.is_some()
        || policy.tool_timeout_secs.is_some()
    {
        return Err(Error::Message(format!(
            "ACP MCP server `{}` uses a per-server policy that stable ACP v1 cannot represent",
            server.name
        )));
    }
    Ok(())
}

fn acp_mcp_http_header(
    server_name: &str,
    name: &str,
    value: &str,
) -> psychevo_runtime::Result<HttpHeader> {
    let valid_name = !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        });
    if !valid_name || value.contains(['\r', '\n']) {
        return Err(Error::Message(format!(
            "ACP MCP server `{server_name}` has an invalid HTTP header `{name}`"
        )));
    }
    Ok(HttpHeader::new(name.to_string(), value.to_string()))
}
