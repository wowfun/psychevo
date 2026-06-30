pub(crate) fn parse_plugin_policy_config(value: &Value) -> Result<PluginPolicyConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("plugins must be an object".to_string()))?;
    let mut out = BTreeMap::new();
    for (name, value) in object {
        validate_plugin_policy_name(name)?;
        let entry = value
            .as_object()
            .ok_or_else(|| Error::Config(format!("plugins.{name} must be an object")))?;
        let enabled = optional_bool_field(entry, "enabled")?;
        if entry.contains_key("capabilities") {
            return Err(Error::Config(format!(
                "plugins.{name}.capabilities is no longer supported; enable or disable the plugin package and configure fine-grained behavior on the owning runtime surface"
            )));
        }
        out.insert(name.clone(), PluginPolicyEntry { enabled });
    }
    Ok(PluginPolicyConfig { plugins: out })
}

pub(crate) fn validate_plugin_policy_name(name: &str) -> Result<()> {
    let valid = !name.trim().is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '@'));
    if valid {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "plugins.{name} must be a valid plugin policy name"
        )))
    }
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
        cwd: optional_string_field(object, "cwd")?,
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
