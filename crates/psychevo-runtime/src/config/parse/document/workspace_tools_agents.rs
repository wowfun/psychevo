pub(crate) fn parse_workspaces_config(value: &Value) -> Result<WorkspacesConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("workspaces must be an object".to_string()))?;
    let mut config = WorkspacesConfig::default();
    if let Some(root) = object.get("root") {
        let root = root
            .as_str()
            .map(str::trim)
            .filter(|root| !root.is_empty())
            .ok_or_else(|| Error::Config("workspaces.root must not be empty".to_string()))?;
        config.root = root.to_string();
    }
    Ok(config)
}

pub(crate) fn parse_sandbox_config(value: &Value) -> Result<SandboxConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("sandbox must be an object".to_string()))?;
    let mut config = SandboxConfig::default();
    if let Some(enabled) = optional_bool_field(object, "enabled")? {
        config.enabled = enabled;
    }
    if let Some(mode) = optional_string_field(object, "mode")? {
        config.mode = SandboxMode::parse(&mode)?;
    }
    config.writable_roots = string_array_field(object, "writable_roots", "sandbox.writable_roots")?;
    if let Some(include_tmp) = optional_bool_field(object, "include_tmp")? {
        config.include_tmp = include_tmp;
    }
    if let Some(include_common_caches) = optional_bool_field(object, "include_common_caches")? {
        config.include_common_caches = include_common_caches;
    }
    Ok(config)
}

pub(crate) fn parse_project_context_config(value: &Value) -> Result<ProjectContextConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("project_context must be an object".to_string()))?;
    let mut config = ProjectContextConfig::default();
    if let Some(instructions) = optional_string_field(object, "instructions")? {
        config.instructions =
            ProjectContextInstructionMode::parse(&instructions).ok_or_else(|| {
                Error::Config(
                    "project_context.instructions must be git-root, cwd, or off".to_string(),
                )
            })?;
    }
    Ok(config)
}

pub(crate) fn parse_tool_selection_config(value: &Value) -> Result<ToolSelectionConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("tools must be an object".to_string()))?;
    let mut config = ToolSelectionConfig::default();
    if let Some(tool_search) = object.get("tool_search") {
        config.tool_search = parse_tool_search_config(tool_search)?;
    }
    if let Some(modes) = object.get("modes") {
        let modes = modes
            .as_object()
            .ok_or_else(|| Error::Config("tools.modes must be an object".to_string()))?;
        for (mode, value) in modes {
            if !matches!(mode.as_str(), "plan" | "default") {
                return Err(Error::Config(format!(
                    "tools.modes.{mode} must be plan or default"
                )));
            }
            config
                .modes
                .insert(mode.clone(), parse_tool_mode_config(mode, value)?);
        }
    }
    Ok(config)
}

fn parse_tool_search_config(value: &Value) -> Result<ToolSearchConfig> {
    match value {
        Value::Bool(enabled) => Ok(ToolSearchConfig { enabled: *enabled }),
        Value::Object(object) => Ok(ToolSearchConfig {
            enabled: optional_bool_field(object, "enabled")?
                .unwrap_or_else(|| ToolSearchConfig::default().enabled),
        }),
        _ => Err(Error::Config(
            "tools.tool_search must be a boolean or object".to_string(),
        )),
    }
}

pub(crate) fn parse_tool_mode_config(mode: &str, value: &Value) -> Result<ToolModeConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("tools.modes.{mode} must be an object")))?;
    Ok(ToolModeConfig {
        enabled_toolsets: object
            .get("enabled_toolsets")
            .map(|_| string_array_field(object, "enabled_toolsets", "enabled_toolsets"))
            .transpose()?,
        disabled_toolsets: string_array_field(object, "disabled_toolsets", "disabled_toolsets")?,
    })
}

pub(crate) fn parse_custom_toolsets(
    value: &Value,
) -> Result<BTreeMap<String, CustomToolsetConfig>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("toolsets must be an object".to_string()))?;
    let mut out = BTreeMap::new();
    for (name, value) in object {
        validate_toolset_name(name)?;
        let toolset = value
            .as_object()
            .ok_or_else(|| Error::Config(format!("toolsets.{name} must be an object")))?;
        out.insert(
            name.clone(),
            CustomToolsetConfig {
                description: optional_string_field(toolset, "description")?,
                tools: string_array_field(toolset, "tools", &format!("toolsets.{name}.tools"))?,
                includes: string_array_field(
                    toolset,
                    "includes",
                    &format!("toolsets.{name}.includes"),
                )?,
            },
        );
    }
    Ok(out)
}

pub(crate) fn validate_toolset_name(name: &str) -> Result<()> {
    let valid = !name.trim().is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'));
    if valid {
        Ok(())
    } else {
        Err(Error::Config(format!("invalid toolset name: {name}")))
    }
}

pub(crate) fn parse_profile_mcp_servers(value: &Value) -> Result<Vec<McpServerInput>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("mcp_servers must be an object".to_string()))?;
    let mut out = Vec::new();
    for (name, value) in object {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Err(Error::Config(
                "mcp_servers keys must not be empty".to_string(),
            ));
        }
        let server = value
            .as_object()
            .ok_or_else(|| Error::Config(format!("mcp_servers.{name} must be an object")))?;
        let transport = optional_string_field(server, "transport")?;
        let command = optional_string_field(server, "command")?;
        let url = optional_string_field(server, "url")?;
        let transport_input = match transport.as_deref() {
            Some("stdio") => {
                let command = command.ok_or_else(|| {
                    Error::Config(format!("mcp_servers.{name}.command is required"))
                })?;
                McpTransportInput::Stdio {
                    command: PathBuf::from(command),
                    args: string_array_field(server, "args", &format!("mcp_servers.{name}.args"))?,
                    env: string_map_field(server, "env", &format!("mcp_servers.{name}.env"))?,
                    cwd: optional_string_field(server, "cwd")?.map(PathBuf::from),
                }
            }
            Some("streamable_http" | "http") => {
                let url = url
                    .ok_or_else(|| Error::Config(format!("mcp_servers.{name}.url is required")))?;
                McpTransportInput::StreamableHttp {
                    url,
                    headers: string_map_field(
                        server,
                        "headers",
                        &format!("mcp_servers.{name}.headers"),
                    )?,
                }
            }
            Some(kind) => McpTransportInput::Unsupported {
                kind: kind.to_string(),
            },
            None if command.is_some() => McpTransportInput::Stdio {
                command: PathBuf::from(command.expect("checked is_some")),
                args: string_array_field(server, "args", &format!("mcp_servers.{name}.args"))?,
                env: string_map_field(server, "env", &format!("mcp_servers.{name}.env"))?,
                cwd: optional_string_field(server, "cwd")?.map(PathBuf::from),
            },
            None if url.is_some() => McpTransportInput::StreamableHttp {
                url: url.expect("checked is_some"),
                headers: string_map_field(
                    server,
                    "headers",
                    &format!("mcp_servers.{name}.headers"),
                )?,
            },
            None => {
                return Err(Error::Config(format!(
                    "mcp_servers.{name} must declare command or url"
                )));
            }
        };
        out.push(McpServerInput::with_source(
            trimmed_name.to_string(),
            transport_input,
            format!("profile:mcp:{trimmed_name}"),
            "profile",
        ));
    }
    Ok(out)
}

pub(crate) fn parse_agent_backend_configs(
    value: &Value,
) -> Result<BTreeMap<String, AgentBackendConfig>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("agents must be an object".to_string()))?;
    let Some(backends) = object.get("backends") else {
        return Ok(BTreeMap::new());
    };
    let backends = backends
        .as_object()
        .ok_or_else(|| Error::Config("agents.backends must be an object".to_string()))?;
    let mut out = BTreeMap::new();
    for (id, value) in backends {
        if !valid_agent_name(id) {
            return Err(Error::Config(format!(
                "agents.backends.{id} must be a valid agent/backend id"
            )));
        }
        out.insert(id.clone(), parse_agent_backend_config(id, value)?);
    }
    Ok(out)
}

pub(crate) fn parse_agent_backend_config(id: &str, value: &Value) -> Result<AgentBackendConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("agents.backends.{id} must be an object")))?;
    let kind_raw = optional_string_field(object, "kind")?
        .ok_or_else(|| Error::Config(format!("agents.backends.{id}.kind is required")))?;
    let kind = AgentBackendKind::parse(&kind_raw).ok_or_else(|| {
        Error::Config(format!(
            "agents.backends.{id}.kind `{kind_raw}` must be acp"
        ))
    })?;
    let enabled = optional_bool_field(object, "enabled")?.unwrap_or(true);
    let label = optional_string_field(object, "label")?.unwrap_or_else(|| id.to_string());
    let description = optional_string_field(object, "description")?;
    let command = optional_string_field(object, "command")?;
    let args = string_array_field(object, "args", &format!("agents.backends.{id}.args"))?;
    let env = string_map_field(object, "env", &format!("agents.backends.{id}.env"))?;
    let cwd = optional_string_field(object, "cwd")?.unwrap_or_else(|| "invocation".to_string());
    let entrypoints = object
        .get("entrypoints")
        .map(|value| parse_agent_backend_entrypoints(id, value))
        .transpose()?
        .unwrap_or_else(default_peer_agent_entrypoints);
    let client_capabilities = object
        .get("client_capabilities")
        .or_else(|| object.get("clientCapabilities"))
        .map(|value| parse_agent_backend_client_capabilities(id, value))
        .transpose()?
        .unwrap_or_else(default_peer_client_capabilities);
    let mcp_servers = object
        .get("mcp_servers")
        .or_else(|| object.get("mcpServers"))
        .map(parse_string_array_value)
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .collect();
    Ok(AgentBackendConfig {
        id: id.to_string(),
        kind,
        enabled,
        label,
        description,
        command,
        args,
        env,
        cwd,
        entrypoints,
        client_capabilities,
        mcp_servers,
    })
}

pub(crate) fn parse_agent_backend_entrypoints(
    id: &str,
    value: &Value,
) -> Result<BTreeSet<AgentEntrypoint>> {
    let values = parse_string_array_value(value)?;
    if values.is_empty() {
        return Err(Error::Config(format!(
            "agents.backends.{id}.entrypoints must include at least one value"
        )));
    }
    let mut entrypoints = BTreeSet::new();
    for value in values {
        let entrypoint = AgentEntrypoint::parse(&value).ok_or_else(|| {
            Error::Config(format!(
                "agents.backends.{id}.entrypoints contains `{value}`; expected peer or subagent"
            ))
        })?;
        entrypoints.insert(entrypoint);
    }
    Ok(entrypoints)
}

pub(crate) fn parse_agent_backend_client_capabilities(
    id: &str,
    value: &Value,
) -> Result<BTreeSet<String>> {
    let values = parse_string_array_value(value)?;
    let mut capabilities = BTreeSet::new();
    for value in values {
        if !matches!(value.as_str(), "fs.read" | "fs.write" | "terminal") {
            return Err(Error::Config(format!(
                "agents.backends.{id}.client_capabilities contains `{value}`; expected fs.read, fs.write, or terminal"
            )));
        }
        capabilities.insert(value);
    }
    Ok(capabilities)
}
