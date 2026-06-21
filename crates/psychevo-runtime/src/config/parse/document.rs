#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) fn parse_run_config(value: Value) -> Result<RunConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let mut config = RunConfig::default();
    let configured_keys = object
        .get("provider")
        .and_then(Value::as_object)
        .map(|providers| {
            providers
                .keys()
                .map(|key| normalize_provider_id(key))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    if let Some(model) = object.get("model") {
        config.model = parse_model_selection(model, &configured_keys)?;
    }
    if let Some(providers) = object.get("provider") {
        let providers = providers
            .as_object()
            .ok_or_else(|| Error::Config("provider must be an object".to_string()))?;
        for (key, entry) in providers {
            let provider_id = normalize_provider_id(key);
            config
                .provider
                .insert(provider_id, parse_config_provider_entry(key, entry)?);
        }
    }
    if let Some(compression) = object.get("compression") {
        config.compression = parse_compression_config(compression, &configured_keys)?;
    }
    if let Some(lsp) = object.get("lsp") {
        config.lsp = parse_lsp_config(lsp)?;
    }
    if let Some(project_context) = object.get("project_context") {
        config.project_context = parse_project_context_config(project_context)?;
    }
    if let Some(workspaces) = object.get("workspaces") {
        config.workspaces = parse_workspaces_config(workspaces)?;
    }
    config.permissions = parse_permission_config(object)?;
    if let Some(sandbox) = object.get("sandbox") {
        config.sandbox = parse_sandbox_config(sandbox)?;
    }
    if let Some(tools) = object.get("tools") {
        config.tools = parse_tool_selection_config(tools)?;
    }
    if let Some(toolsets) = object.get("toolsets") {
        config.toolsets = parse_custom_toolsets(toolsets)?;
    }
    if let Some(agents) = object.get("agents") {
        config.agent_backends = parse_agent_backend_configs(agents)?;
    }
    if let Some(channels) = object.get("channels") {
        config.channels = parse_channels_config(channels)?;
    }
    Ok(config)
}

pub(crate) fn parse_channels_config(value: &Value) -> Result<ChannelsConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("channels must be an object".to_string()))?;
    let Some(connections) = object.get("connections") else {
        return Ok(ChannelsConfig::default());
    };
    let connections = connections
        .as_array()
        .ok_or_else(|| Error::Config("channels.connections must be an array".to_string()))?;
    let mut out = Vec::new();
    let mut ids = BTreeSet::new();
    for (index, value) in connections.iter().enumerate() {
        let path = format!("channels.connections[{index}]");
        let connection = parse_channel_connection(&path, value)?;
        if !ids.insert(connection.id.clone()) {
            return Err(Error::Config(format!(
                "duplicate channel connection id `{}`",
                connection.id
            )));
        }
        out.push(connection);
    }
    Ok(ChannelsConfig { connections: out })
}

pub(crate) fn parse_channel_connection(
    path: &str,
    value: &Value,
) -> Result<ChannelConnectionConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("{path} must be an object")))?;
    let id = optional_string_field(object, "id")?
        .ok_or_else(|| Error::Config(format!("{path}.id is required")))?;
    validate_channel_id(&id)?;
    if object.contains_key("platform") {
        return Err(Error::Config(format!(
            "{path}.platform is not supported; use {path}.channel"
        )));
    }
    let platform_raw = optional_string_field(object, "channel")?
        .ok_or_else(|| Error::Config(format!("{path}.channel is required")))?;
    let platform = ChannelPlatform::parse(&platform_raw).ok_or_else(|| {
        Error::Config(format!(
            "{path}.channel must be wechat, telegram, feishu, or lark"
        ))
    })?;
    let transport = optional_string_field(object, "transport")?
        .map(|value| {
            ChannelTransport::parse(&value).ok_or_else(|| {
                Error::Config(format!(
                    "{path}.transport must be polling, webhook, or long_connection"
                ))
            })
        })
        .transpose()?
        .unwrap_or_else(|| platform.default_transport());
    validate_channel_transport(platform, transport, path)?;
    let domain = optional_string_field(object, "domain")?.or_else(|| {
        matches!(platform, ChannelPlatform::Feishu | ChannelPlatform::Lark)
            .then(|| platform.default_domain().to_string())
    });
    let enabled = optional_bool_field(object, "enabled")?.unwrap_or(false);
    let label = optional_string_field(object, "label")?
        .unwrap_or_else(|| platform.default_label().to_string());
    let credential_env = optional_string_field(object, "credential_env")?
        .or_else(|| Some(platform.default_credential_env().to_string()));
    let app_id_env =
        optional_string_field(object, "app_id_env")?.or_else(|| platform.default_app_id_env().map(str::to_string));
    let app_secret_env = optional_string_field(object, "app_secret_env")?;
    let account_env =
        optional_string_field(object, "account_env")?.or_else(|| platform.default_account_env().map(str::to_string));
    let base_url_env =
        optional_string_field(object, "base_url_env")?.or_else(|| platform.default_base_url_env().map(str::to_string));
    for (field, env) in [
        ("credential_env", credential_env.as_deref()),
        ("app_id_env", app_id_env.as_deref()),
        ("app_secret_env", app_secret_env.as_deref()),
        ("account_env", account_env.as_deref()),
        ("base_url_env", base_url_env.as_deref()),
    ] {
        if let Some(env) = env
            && !valid_env_name(env)
        {
            return Err(Error::Config(format!(
                "{path}.{field} must be a valid environment variable name"
            )));
        }
    }
    Ok(ChannelConnectionConfig {
        id,
        platform,
        domain,
        enabled,
        label,
        transport,
        workdir: optional_string_field(object, "workdir")?,
        model: optional_string_field(object, "model")?,
        permission_mode: optional_string_field(object, "permission_mode")?,
        require_mention: optional_bool_field(object, "require_mention")?.unwrap_or(true),
        credential_env,
        app_id_env,
        app_secret_env,
        account_env,
        base_url_env,
        allow_users: string_array_field(object, "allow_users", &format!("{path}.allow_users"))?,
        allow_groups: string_array_field(object, "allow_groups", &format!("{path}.allow_groups"))?,
    })
}

pub(crate) fn validate_channel_id(id: &str) -> Result<()> {
    let mut chars = id.chars();
    let valid = matches!(chars.next(), Some('a'..='z' | '0'..='9'))
        && chars.all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '-' | '_'));
    if valid {
        Ok(())
    } else {
        Err(Error::Config(
            "channel id must use lowercase letters, numbers, hyphens, or underscores".to_string(),
        ))
    }
}

fn validate_channel_transport(
    platform: ChannelPlatform,
    transport: ChannelTransport,
    path: &str,
) -> Result<()> {
    let valid = match platform {
        ChannelPlatform::Wechat => matches!(transport, ChannelTransport::Polling),
        ChannelPlatform::Telegram => {
            matches!(transport, ChannelTransport::Polling | ChannelTransport::Webhook)
        }
        ChannelPlatform::Feishu | ChannelPlatform::Lark => {
            matches!(transport, ChannelTransport::LongConnection)
        }
    };
    if valid {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "{path}.transport `{}` is not supported for {}",
            transport.as_str(),
            platform.as_str()
        )))
    }
}

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

pub(crate) fn parse_compression_config(
    value: &Value,
    configured_keys: &HashSet<String>,
) -> Result<CompressionConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("compression must be an object".to_string()))?;
    let mut config = CompressionConfig::default();
    if let Some(enabled) = optional_bool_field(object, "enabled")? {
        config.enabled = enabled;
    }
    if let Some(auto) = optional_bool_field(object, "auto")? {
        config.auto = auto;
    }
    if let Some(threshold) = optional_f64_field(object, "threshold_percent")? {
        if !(0.0..=100.0).contains(&threshold) || threshold == 0.0 {
            return Err(Error::Config(
                "compression.threshold_percent must be greater than 0 and at most 100".to_string(),
            ));
        }
        config.threshold_percent = threshold;
    }
    if let Some(reserve) = optional_u64_field(object, "reserve_tokens")? {
        config.reserve_tokens = reserve;
    }
    if let Some(keep_recent) = optional_u64_field(object, "keep_recent_tokens")? {
        if keep_recent == 0 {
            return Err(Error::Config(
                "compression.keep_recent_tokens must be greater than 0".to_string(),
            ));
        }
        config.keep_recent_tokens = keep_recent;
    }
    if let Some(model) = object.get("model") {
        config.model = parse_model_selection(model, configured_keys)?;
        config.model_configured = true;
    }
    config.reasoning_effort =
        validate_reasoning_effort(optional_string_field(object, "reasoning_effort")?)?;
    Ok(config)
}

pub(crate) fn parse_lsp_config(value: &Value) -> Result<LspConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("lsp must be an object".to_string()))?;
    let mut config = LspConfig::default();
    if let Some(enabled) = optional_bool_field(object, "enabled")? {
        config.enabled = enabled;
    }
    if let Some(wait_mode) = optional_string_field(object, "wait_mode")? {
        if wait_mode != "document" && wait_mode != "full" {
            return Err(Error::Config(
                "lsp.wait_mode must be document or full".to_string(),
            ));
        }
        config.wait_mode = wait_mode;
    }
    if let Some(wait_timeout) = optional_f64_field(object, "wait_timeout")? {
        if wait_timeout <= 0.0 {
            return Err(Error::Config(
                "lsp.wait_timeout must be greater than 0".to_string(),
            ));
        }
        config.wait_timeout_secs = wait_timeout;
    }
    if let Some(install_strategy) = optional_string_field(object, "install_strategy")? {
        if !matches!(install_strategy.as_str(), "auto" | "manual" | "off") {
            return Err(Error::Config(
                "lsp.install_strategy must be auto, manual, or off".to_string(),
            ));
        }
        config.install_strategy = install_strategy;
    }
    Ok(config)
}

pub(crate) fn parse_permission_config(
    root: &serde_json::Map<String, Value>,
) -> Result<PermissionConfig> {
    reject_legacy_permission_keys(root)?;
    let mut config = PermissionConfig::default();
    if let Some(value) = optional_string_field(root, "approval_policy")? {
        if value == "on-failure" || value == "on_failure" {
            return Err(Error::Config(
                "approval_policy = \"on-failure\" is not supported; use on-request, untrusted, never, or granular"
                    .to_string(),
            ));
        }
        config.approval_policy = ApprovalPolicy::parse(&value).ok_or_else(|| {
            Error::Config(
                "approval_policy must be on-request, untrusted, never, or granular".to_string(),
            )
        })?;
    }
    if let Some(value) = optional_string_field(root, "approvals_reviewer")? {
        config.approvals_reviewer = ApprovalsReviewer::parse(&value)
            .ok_or_else(|| Error::Config("approvals_reviewer must be user or smart".to_string()))?;
    }
    if let Some(value) = optional_string_field(root, "default_permissions")? {
        validate_permission_profile_name(&value)?;
        config.default_permissions = value;
    }
    if let Some(auto_review) = root.get("auto_review") {
        config.auto_review = parse_auto_review_config(auto_review)?;
    }
    if let Some(approval) = root.get("approval") {
        config.granular = parse_approval_config(approval)?;
    }
    if matches!(config.approval_policy, ApprovalPolicy::Granular) && config.granular.is_none() {
        return Err(Error::Config(
            "approval_policy = \"granular\" requires [approval.granular] with filesystem, network, exec, mcp, skill, and request_permissions"
                .to_string(),
        ));
    }
    if let Some(permissions) = root.get("permissions") {
        let permissions = permissions
            .as_object()
            .ok_or_else(|| Error::Config("permissions must be an object".to_string()))?;
        config.allow_login_shell =
            optional_bool_field(permissions, "allow_login_shell")?.unwrap_or(false);
        for (name, value) in permissions {
            if name == "allow_login_shell" {
                continue;
            }
            validate_permission_profile_name(name)?;
            config
                .profiles
                .insert(name.clone(), parse_permission_profile(name, value)?);
        }
    }
    if let Some(exec_policy) = root.get("exec_policy") {
        config.exec_policy = parse_exec_policy_config(exec_policy)?;
    }
    Ok(config)
}

pub(crate) fn reject_legacy_permission_keys(root: &serde_json::Map<String, Value>) -> Result<()> {
    for key in [
        "permission_mode",
        "permissionMode",
        "approval_mode",
        "approvalMode",
    ] {
        if root.contains_key(key) {
            return Err(Error::Config(format!(
                "{key} is deprecated; use approval_policy, approvals_reviewer, default_permissions, and [permissions.<profile>]"
            )));
        }
    }
    let Some(permissions) = root.get("permissions").and_then(Value::as_object) else {
        return Ok(());
    };
    for key in [
        "permission_mode",
        "permissionMode",
        "approval_mode",
        "approvalMode",
        "smart_model",
        "smartModel",
        "allow",
        "ask",
        "deny",
    ] {
        if permissions.contains_key(key) {
            return Err(Error::Config(format!(
                "permissions.{key} is deprecated; use approval_policy, approvals_reviewer, [permissions.<profile>], and [[exec_policy.rules]]"
            )));
        }
    }
    Ok(())
}

pub(crate) fn parse_auto_review_config(value: &Value) -> Result<AutoReviewConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("auto_review must be an object".to_string()))?;
    Ok(AutoReviewConfig {
        model: optional_string_field(object, "model")?,
        timeout_secs: optional_u64_field(object, "timeout_secs")?.unwrap_or(90),
        policy: optional_string_field(object, "policy")?,
    })
}

pub(crate) fn parse_approval_config(value: &Value) -> Result<Option<GranularApprovalConfig>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("approval must be an object".to_string()))?;
    let Some(granular) = object.get("granular") else {
        return Ok(None);
    };
    let granular = granular
        .as_object()
        .ok_or_else(|| Error::Config("approval.granular must be an object".to_string()))?;
    Ok(Some(GranularApprovalConfig {
        filesystem: required_bool_field(granular, "filesystem", "approval.granular.filesystem")?,
        network: required_bool_field(granular, "network", "approval.granular.network")?,
        exec: required_bool_field(granular, "exec", "approval.granular.exec")?,
        mcp: required_bool_field(granular, "mcp", "approval.granular.mcp")?,
        skill: required_bool_field(granular, "skill", "approval.granular.skill")?,
        request_permissions: required_bool_field(
            granular,
            "request_permissions",
            "approval.granular.request_permissions",
        )?,
    }))
}

pub(crate) fn parse_permission_profile(
    name: &str,
    value: &Value,
) -> Result<PermissionProfileConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("permissions.{name} must be an object")))?;
    Ok(PermissionProfileConfig {
        extends: optional_string_field(object, "extends")?,
        filesystem: object
            .get("filesystem")
            .map(|value| parse_access_map(value, &format!("permissions.{name}.filesystem")))
            .transpose()?
            .unwrap_or_default(),
        network_domains: object
            .get("network")
            .map(|value| parse_network_domains(value, name))
            .transpose()?
            .unwrap_or_default(),
        skill_tools: object
            .get("tools")
            .map(|value| parse_tool_grants(value, name))
            .transpose()?
            .unwrap_or_default(),
    })
}

pub(crate) fn parse_network_domains(
    value: &Value,
    profile: &str,
) -> Result<BTreeMap<String, PermissionAccess>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("permissions.{profile}.network must be an object")))?;
    object
        .get("domains")
        .map(|value| parse_access_map(value, &format!("permissions.{profile}.network.domains")))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn parse_tool_grants(
    value: &Value,
    profile: &str,
) -> Result<BTreeMap<String, PermissionAccess>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("permissions.{profile}.tools must be an object")))?;
    object
        .get("skills")
        .map(|value| parse_access_map(value, &format!("permissions.{profile}.tools.skills")))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn parse_access_map(
    value: &Value,
    path: &str,
) -> Result<BTreeMap<String, PermissionAccess>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("{path} must be an object")))?;
    let mut out = BTreeMap::new();
    for (key, value) in object {
        let raw = value
            .as_str()
            .map(str::trim)
            .ok_or_else(|| Error::Config(format!("{path}.{key} must be a string")))?;
        let access = PermissionAccess::parse(raw).ok_or_else(|| {
            Error::Config(format!(
                "{path}.{key} must be deny, read, write, allow, or prompt"
            ))
        })?;
        out.insert(key.clone(), access);
    }
    Ok(out)
}

pub(crate) fn parse_exec_policy_config(value: &Value) -> Result<ExecPolicyConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("exec_policy must be an object".to_string()))?;
    let host_executables = object
        .get("host_executables")
        .map(parse_host_executables)
        .transpose()?
        .unwrap_or_default();
    let rules = object
        .get("rules")
        .map(|value| {
            value
                .as_array()
                .ok_or_else(|| Error::Config("exec_policy.rules must be an array".to_string()))
        })
        .transpose()?
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (index, value) in rules.iter().enumerate() {
        let object = value.as_object().ok_or_else(|| {
            Error::Config(format!("exec_policy.rules[{index}] must be an object"))
        })?;
        let prefix = exec_policy_prefix_field(
            object.get("prefix"),
            &format!("exec_policy.rules[{index}].prefix"),
        )?;
        if prefix.is_empty() {
            return Err(Error::Config(format!(
                "exec_policy.rules[{index}].prefix must not be empty"
            )));
        }
        let decision = optional_string_field(object, "decision")?
            .and_then(|value| ExecPolicyDecision::parse(&value))
            .ok_or_else(|| {
                Error::Config(format!(
                    "exec_policy.rules[{index}].decision must be allow, prompt, or deny"
                ))
            })?;
        let justification = optional_string_field(object, "justification")?;
        let match_examples = exec_policy_examples_field(
            object.get("match"),
            &format!("exec_policy.rules[{index}].match"),
        )?;
        let not_match_examples = exec_policy_examples_field(
            object.get("not_match"),
            &format!("exec_policy.rules[{index}].not_match"),
        )?;
        let rule = ExecPolicyRule {
            prefix,
            decision,
            justification,
            match_examples,
            not_match_examples,
        };
        validate_exec_policy_rule_examples(&rule, index, &host_executables)?;
        out.push(rule);
    }
    Ok(ExecPolicyConfig {
        rules: out,
        host_executables,
    })
}

pub(crate) fn exec_policy_prefix_field(
    value: Option<&Value>,
    path: &str,
) -> Result<Vec<ExecPolicyPatternToken>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| Error::Config(format!("{path} must be an array")))?;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| match value {
            Value::String(raw) => non_empty_string(raw, &format!("{path}[{index}]"))
                .map(ExecPolicyPatternToken::Single),
            Value::Array(alternatives) => {
                let alternatives = alternatives
                    .iter()
                    .enumerate()
                    .map(|(alt_index, value)| {
                        value
                            .as_str()
                            .ok_or_else(|| {
                                Error::Config(format!(
                                    "{path}[{index}][{alt_index}] must be a string"
                                ))
                            })
                            .and_then(|raw| {
                                non_empty_string(raw, &format!("{path}[{index}][{alt_index}]"))
                            })
                    })
                    .collect::<Result<Vec<_>>>()?;
                if alternatives.is_empty() {
                    return Err(Error::Config(format!(
                        "{path}[{index}] alternatives must not be empty"
                    )));
                }
                Ok(ExecPolicyPatternToken::Alternatives(alternatives))
            }
            _ => Err(Error::Config(format!(
                "{path}[{index}] must be a string or array of strings"
            ))),
        })
        .collect()
}

pub(crate) fn exec_policy_examples_field(
    value: Option<&Value>,
    path: &str,
) -> Result<Vec<ExecPolicyExample>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        Value::String(_) => Ok(vec![exec_policy_example(value, path)?]),
        Value::Array(values) => {
            if values.iter().all(Value::is_string)
                && (values.len() == 1
                    || values
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|value| value.chars().any(char::is_whitespace)))
            {
                return values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| exec_policy_example(value, &format!("{path}[{index}]")))
                    .collect();
            }
            if values.iter().all(Value::is_string) {
                return Ok(vec![exec_policy_example(value, path)?]);
            }
            values
                .iter()
                .enumerate()
                .map(|(index, value)| exec_policy_example(value, &format!("{path}[{index}]")))
                .collect()
        }
        _ => Err(Error::Config(format!(
            "{path} must be a string, token array, or array of examples"
        ))),
    }
}

pub(crate) fn exec_policy_example(value: &Value, path: &str) -> Result<ExecPolicyExample> {
    match value {
        Value::String(raw) => {
            let raw = non_empty_string(raw, path)?;
            let tokens = crate::permissions::shell_command_tokens(&raw).ok_or_else(|| {
                Error::Config(format!("{path} must be a parseable single shell command"))
            })?;
            Ok(ExecPolicyExample { raw, tokens })
        }
        Value::Array(values) => {
            let tokens = values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    value
                        .as_str()
                        .ok_or_else(|| {
                            Error::Config(format!("{path}[{index}] entries must be strings"))
                        })
                        .and_then(|raw| non_empty_string(raw, &format!("{path}[{index}]")))
                })
                .collect::<Result<Vec<_>>>()?;
            if tokens.is_empty() {
                return Err(Error::Config(format!("{path} must not be empty")));
            }
            Ok(ExecPolicyExample {
                raw: tokens.join(" "),
                tokens,
            })
        }
        _ => Err(Error::Config(format!(
            "{path} must be a string or token array"
        ))),
    }
}

pub(crate) fn validate_exec_policy_rule_examples(
    rule: &ExecPolicyRule,
    index: usize,
    host_executables: &[ExecPolicyHostExecutable],
) -> Result<()> {
    for example in &rule.match_examples {
        if !crate::permissions::exec_prefix_matches(
            &rule.prefix,
            &example.tokens,
            Some(host_executables),
        ) {
            return Err(Error::Config(format!(
                "exec_policy.rules[{index}].match example `{}` does not match prefix",
                example.raw
            )));
        }
    }
    for example in &rule.not_match_examples {
        if crate::permissions::exec_prefix_matches(
            &rule.prefix,
            &example.tokens,
            Some(host_executables),
        ) {
            return Err(Error::Config(format!(
                "exec_policy.rules[{index}].not_match example `{}` matches prefix",
                example.raw
            )));
        }
    }
    Ok(())
}
