#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Debug, Clone)]
pub struct ChannelSetupInput {
    pub config_dir: PathBuf,
    pub id: String,
    pub channel: String,
    pub label: Option<String>,
    pub credential_env: Option<String>,
    pub credential: Option<String>,
    pub account_env: Option<String>,
    pub account_id: Option<String>,
    pub base_url_env: Option<String>,
    pub base_url: Option<String>,
    pub allow_users: Vec<String>,
    pub allow_groups: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ChannelUpdateInput {
    pub config_dir: PathBuf,
    pub id: String,
    pub label: Option<String>,
    pub enabled: Option<bool>,
    pub cwd: Option<String>,
    pub runtime_ref: Option<String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub require_mention: Option<bool>,
    pub credential_env: Option<String>,
    pub account_env: Option<String>,
    pub base_url_env: Option<String>,
    pub app_id_env: Option<String>,
    pub allow_users: Option<Vec<String>>,
    pub allow_groups: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ChannelRuntimeConnection {
    pub id: String,
    pub channel: String,
    pub domain: Option<String>,
    pub enabled: bool,
    pub label: String,
    pub transport: String,
    pub cwd: Option<String>,
    pub runtime_ref: Option<String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub require_mention: bool,
    pub credential: Option<String>,
    pub app_id: Option<String>,
    pub app_secret: Option<String>,
    pub account_id: Option<String>,
    pub base_url: Option<String>,
    pub allow_users: Vec<String>,
    pub allow_groups: Vec<String>,
    pub config_status: String,
}

struct LoadedChannelsConfig {
    channels: ChannelsConfig,
    env: BTreeMap<String, String>,
}

pub fn channel_list_value(options: &RunOptions) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_channels_config(options, &cwd)?;
    Ok(json!({
        "channels": channel_rows(&loaded),
    }))
}

pub fn channel_show_value(options: &RunOptions, id: &str) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_channels_config(options, &cwd)?;
    let row = loaded
        .channels
        .connections
        .iter()
        .find(|connection| connection.id == id)
        .ok_or_else(|| Error::Config(format!("unknown channel connection `{id}`")))?;
    Ok(json!({
        "channel": channel_row(row, &loaded.env),
    }))
}

pub fn channel_doctor_value(options: &RunOptions, id: Option<&str>, live: bool) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_channels_config(options, &cwd)?;
    let connections = loaded
        .channels
        .connections
        .iter()
        .filter(|connection| id.is_none_or(|id| connection.id == id))
        .collect::<Vec<_>>();
    if connections.is_empty()
        && let Some(id) = id
    {
        return Err(Error::Config(format!("unknown channel connection `{id}`")));
    }
    let rows = connections
        .iter()
        .map(|connection| {
            let credential_present = channel_credential_present(connection, &loaded.env);
            let allowlist_present = channel_allowlist_present(connection);
            let mut checks = Vec::new();
            checks.push(json!({
                "name": "credential",
                "status": if credential_present { "ok" } else { "fail" },
                "message": if credential_present {
                    "credential env is present"
                } else if matches!(connection.platform, ChannelPlatform::Wechat) {
                    "WeChat QR login is required to write WECHAT_BOT_TOKEN"
                } else {
                    "credential env is missing"
                },
            }));
            if matches!(connection.platform, ChannelPlatform::Wechat) {
                let account_present = channel_account_present(connection, &loaded.env);
                checks.push(json!({
                    "name": "account",
                    "status": if account_present { "ok" } else { "fail" },
                    "message": if account_present {
                        "WeChat account env is present"
                    } else {
                        "WeChat account env is missing; rerun QR setup or pass --account-id"
                    },
                }));
            }
            checks.push(json!({
                "name": "allowlist",
                "status": if allowlist_present { "ok" } else { "fail" },
                "message": if allowlist_present && matches!(connection.platform, ChannelPlatform::Wechat) && connection.allow_users.is_empty() && !connection.allow_groups.is_empty() {
                    "WeChat group allowlist is configured, but iLink group delivery may be unavailable"
                } else if allowlist_present {
                    "allowlist is configured"
                } else if matches!(connection.platform, ChannelPlatform::Wechat) {
                    "send one direct message to the iLink bot and add that sender id"
                } else {
                    "allowlist is required before receiving messages"
                },
            }));
            if matches!(connection.platform, ChannelPlatform::Wechat)
                && !connection.allow_groups.is_empty()
            {
                checks.push(json!({
                    "name": "group_limit",
                    "status": "warn",
                    "message": "ordinary WeChat group events may not reach iLink bot identities",
                }));
            }
            checks.push(json!({
                "name": "live",
                "status": if live { "fail" } else { "skipped" },
                "message": if live {
                    "live channel API checks are not wired into doctor yet"
                } else {
                    "use live validation to opt in to real channel API checks"
                },
            }));
            json!({
                "id": connection.id,
                "channel": connection.platform.as_str(),
                "enabled": connection.enabled,
                "runtime_status": channel_runtime_status(connection, &loaded.env),
                "checks": checks,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "live": live,
        "channels": rows,
    }))
}

pub fn channel_summary_value(options: &RunOptions) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_channels_config(options, &cwd)?;
    Ok(channel_summary(&loaded))
}

pub fn channel_runtime_connections(
    options: &RunOptions,
    cwd: &Path,
) -> Result<Vec<ChannelRuntimeConnection>> {
    let cwd = canonical_cwd(cwd)?;
    let loaded = load_channels_config(options, &cwd)?;
    Ok(loaded
        .channels
        .connections
        .iter()
        .map(|connection| ChannelRuntimeConnection {
            id: connection.id.clone(),
            channel: connection.platform.as_str().to_string(),
            domain: connection.domain.clone(),
            enabled: connection.enabled,
            label: connection.label.clone(),
            transport: connection.transport.as_str().to_string(),
            cwd: connection.cwd.clone(),
            runtime_ref: connection.runtime_ref.clone(),
            model: connection.model.clone(),
            permission_mode: connection.permission_mode.clone(),
            require_mention: connection.require_mention,
            credential: connection
                .credential_env
                .as_deref()
                .and_then(|key| env_value(&loaded.env, key)),
            app_id: connection
                .app_id_env
                .as_deref()
                .and_then(|key| env_value(&loaded.env, key)),
            app_secret: connection
                .app_secret_env
                .as_deref()
                .and_then(|key| env_value(&loaded.env, key)),
            account_id: connection
                .account_env
                .as_deref()
                .and_then(|key| env_value(&loaded.env, key)),
            base_url: connection
                .base_url_env
                .as_deref()
                .and_then(|key| env_value(&loaded.env, key)),
            allow_users: connection.allow_users.clone(),
            allow_groups: connection.allow_groups.clone(),
            config_status: channel_runtime_status(connection, &loaded.env).to_string(),
        })
        .collect())
}

pub fn setup_channel_connection(input: ChannelSetupInput) -> Result<Value> {
    setup_channel_connection_inner(input, false)
}

pub fn upsert_channel_connection(input: ChannelSetupInput) -> Result<Value> {
    setup_channel_connection_inner(input, true)
}

pub fn update_channel_connection(input: ChannelUpdateInput) -> Result<Value> {
    let id = input.id.trim().to_string();
    validate_channel_id(&id)?;
    let config_path = input.config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, true)?;
    let channels = channels_config_from_value(&parsed)?;
    let platform = channels
        .connections
        .iter()
        .find(|connection| connection.id == id)
        .map(|connection| connection.platform)
        .ok_or_else(|| Error::Config(format!("unknown channel connection `{id}`")))?;
    let connections = parsed
        .get_mut("channels")
        .and_then(Value::as_object_mut)
        .and_then(|channels| channels.get_mut("connections"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Error::Config("channels.connections is not configured".to_string()))?;
    let connection = connections
        .iter_mut()
        .find(|connection| connection.get("id").and_then(Value::as_str) == Some(id.as_str()))
        .ok_or_else(|| Error::Config(format!("unknown channel connection `{id}`")))?;
    let connection = connection
        .as_object_mut()
        .ok_or_else(|| Error::Config(format!("channel connection `{id}` must be an object")))?;

    if let Some(label) = input.label {
        let label = normalize_optional_single_line("channel label", Some(label))?
            .unwrap_or_else(|| platform.default_label().to_string());
        connection.insert("label".to_string(), json!(label));
    }
    if let Some(enabled) = input.enabled {
        connection.insert("enabled".to_string(), json!(enabled));
    }
    if let Some(value) = input.cwd {
        set_optional_string_field(connection, "cwd", "channel cwd", value, false)?;
    }
    if let Some(value) = input.runtime_ref {
        set_optional_string_field(
            connection,
            "runtime_ref",
            "channel runtime profile",
            value,
            false,
        )?;
    }
    if let Some(value) = input.model {
        set_optional_string_field(connection, "model", "channel model", value, false)?;
    }
    if let Some(value) = input.permission_mode {
        let permission_mode = normalize_permission_mode(value)?;
        set_object_optional_string(connection, "permission_mode", permission_mode);
    }
    if let Some(require_mention) = input.require_mention {
        connection.insert("require_mention".to_string(), json!(require_mention));
    }
    if let Some(value) = input.credential_env {
        set_env_field(
            connection,
            "credential_env",
            value,
            Some(platform.default_credential_env()),
        )?;
    }
    if let Some(value) = input.account_env {
        set_env_field(
            connection,
            "account_env",
            value,
            platform.default_account_env(),
        )?;
    }
    if let Some(value) = input.base_url_env {
        set_env_field(
            connection,
            "base_url_env",
            value,
            platform.default_base_url_env(),
        )?;
    }
    if let Some(value) = input.app_id_env {
        set_env_field(
            connection,
            "app_id_env",
            value,
            platform.default_app_id_env(),
        )?;
    }
    if let Some(users) = input.allow_users {
        connection.insert(
            "allow_users".to_string(),
            json!(normalize_channel_list(users)),
        );
    }
    if let Some(groups) = input.allow_groups {
        connection.insert(
            "allow_groups".to_string(),
            json!(normalize_channel_list(groups)),
        );
    }

    channels_config_from_value(&parsed)?;
    write_toml_config_file(&config_path, &parsed)?;
    Ok(json!({
        "id": id,
        "channel": platform.as_str(),
        "path": config_path,
    }))
}

pub fn delete_channel_connection(config_dir: PathBuf, id: &str) -> Result<Value> {
    let id = id.trim().to_string();
    validate_channel_id(&id)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, true)?;
    let connections = parsed
        .get_mut("channels")
        .and_then(Value::as_object_mut)
        .and_then(|channels| channels.get_mut("connections"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Error::Config("channels.connections is not configured".to_string()))?;
    let before = connections.len();
    connections
        .retain(|connection| connection.get("id").and_then(Value::as_str) != Some(id.as_str()));
    if connections.len() == before {
        return Err(Error::Config(format!("unknown channel connection `{id}`")));
    }
    channels_config_from_value(&parsed)?;
    write_toml_config_file(&config_path, &parsed)?;
    Ok(json!({
        "id": id,
        "path": config_path,
    }))
}

fn setup_channel_connection_inner(input: ChannelSetupInput, upsert: bool) -> Result<Value> {
    let platform = ChannelPlatform::parse(input.channel.trim()).ok_or_else(|| {
        Error::Config("channel must be wechat, telegram, feishu, or lark".to_string())
    })?;
    let id = input.id.trim().to_string();
    validate_channel_id(&id)?;
    let credential_env = input
        .credential_env
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| platform.default_credential_env().to_string());
    if !valid_env_name(&credential_env) {
        return Err(Error::Config(
            "credential env must be a valid environment variable name".to_string(),
        ));
    }
    if let Some(credential) = &input.credential
        && (credential.trim().is_empty() || credential.contains('\n') || credential.contains('\r'))
    {
        return Err(Error::Config(
            "channel credential must be non-empty and single-line".to_string(),
        ));
    }
    let account_env = input
        .account_env
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| platform.default_account_env().map(str::to_string));
    let base_url_env = input
        .base_url_env
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| platform.default_base_url_env().map(str::to_string));
    for (label, env) in [
        ("account env", account_env.as_deref()),
        ("base URL env", base_url_env.as_deref()),
    ] {
        if let Some(env) = env
            && !valid_env_name(env)
        {
            return Err(Error::Config(format!(
                "{label} must be a valid environment variable name"
            )));
        }
    }
    for (label, value) in [
        ("channel account id", input.account_id.as_deref()),
        ("channel base URL", input.base_url.as_deref()),
    ] {
        if let Some(value) = value
            && (value.trim().is_empty() || value.contains('\n') || value.contains('\r'))
        {
            return Err(Error::Config(format!(
                "{label} must be non-empty and single-line"
            )));
        }
    }

    let config_path = input.config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, false)?;
    let existing = channels_config_from_value(&parsed)?;
    let existing_connection = existing
        .connections
        .iter()
        .find(|connection| connection.id == id);
    if let Some(connection) = existing_connection
        && connection.platform != platform
    {
        return Err(Error::Config(format!(
            "channel connection `{id}` already exists with channel `{}`",
            connection.platform.as_str()
        )));
    }
    if existing_connection.is_some() && !upsert {
        return Err(Error::Config(format!(
            "channel connection `{id}` already exists"
        )));
    }
    ensure_json_object(&mut parsed);
    let root = parsed
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let channels = root
        .entry("channels".to_string())
        .or_insert_with(|| json!({}));
    ensure_json_object(channels);
    let channels = channels
        .as_object_mut()
        .ok_or_else(|| Error::Config("channels must be an object".to_string()))?;
    let connections = channels
        .entry("connections".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let connections = connections
        .as_array_mut()
        .ok_or_else(|| Error::Config("channels.connections must be an array".to_string()))?;
    let mut next_connection = json!({
        "id": id.clone(),
        "channel": platform.as_str(),
        "domain": platform.default_domain(),
        "enabled": false,
        "label": input.label.clone().unwrap_or_else(|| platform.default_label().to_string()),
        "transport": platform.default_transport().as_str(),
        "require_mention": true,
        "credential_env": credential_env.clone(),
        "allow_users": input.allow_users.clone(),
        "allow_groups": input.allow_groups.clone(),
    });
    if let Some(app_id_env) = platform.default_app_id_env() {
        next_connection["app_id_env"] = json!(app_id_env);
    }
    if let Some(account_env) = &account_env {
        next_connection["account_env"] = json!(account_env);
    }
    if let Some(base_url_env) = &base_url_env {
        next_connection["base_url_env"] = json!(base_url_env);
    }
    if let Some(existing_connection) = connections
        .iter_mut()
        .find(|connection| connection.get("id").and_then(Value::as_str) == Some(id.as_str()))
    {
        let label = input
            .label
            .clone()
            .or_else(|| {
                existing_connection
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| platform.default_label().to_string());
        existing_connection["channel"] = json!(platform.as_str());
        existing_connection["domain"] = json!(platform.default_domain());
        existing_connection["label"] = json!(label);
        existing_connection["transport"] = json!(platform.default_transport().as_str());
        existing_connection["credential_env"] = json!(credential_env.clone());
        existing_connection["require_mention"] = existing_connection
            .get("require_mention")
            .cloned()
            .unwrap_or(Value::Bool(true));
        existing_connection["enabled"] = existing_connection
            .get("enabled")
            .cloned()
            .unwrap_or(Value::Bool(false));
        if let Some(app_id_env) = platform.default_app_id_env() {
            existing_connection["app_id_env"] = json!(app_id_env);
        }
        if let Some(account_env) = &account_env {
            existing_connection["account_env"] = json!(account_env);
        }
        if let Some(base_url_env) = &base_url_env {
            existing_connection["base_url_env"] = json!(base_url_env);
        }
        if !input.allow_users.is_empty() {
            existing_connection["allow_users"] = json!(input.allow_users.clone());
        } else if existing_connection.get("allow_users").is_none() {
            existing_connection["allow_users"] = json!([]);
        }
        if !input.allow_groups.is_empty() {
            existing_connection["allow_groups"] = json!(input.allow_groups.clone());
        } else if existing_connection.get("allow_groups").is_none() {
            existing_connection["allow_groups"] = json!([]);
        }
    } else {
        connections.push(next_connection);
    }
    write_toml_config_file(&config_path, &parsed)?;

    let env_path = input.config_dir.join(".env");
    let wrote_credential = if let Some(credential) = input.credential {
        set_dotenv_value(&env_path, &credential_env, credential.trim())?;
        true
    } else {
        false
    };
    let wrote_account =
        if let (Some(account_env), Some(account_id)) = (&account_env, input.account_id) {
            set_dotenv_value(&env_path, account_env, account_id.trim())?;
            true
        } else {
            false
        };
    let wrote_base_url =
        if let (Some(base_url_env), Some(base_url)) = (&base_url_env, input.base_url) {
            set_dotenv_value(&env_path, base_url_env, base_url.trim())?;
            true
        } else {
            false
        };
    Ok(json!({
        "id": id,
        "channel": platform.as_str(),
        "path": config_path,
        "credential_env": credential_env,
        "account_env": account_env,
        "base_url_env": base_url_env,
        "wrote_credential": wrote_credential,
        "wrote_account": wrote_account,
        "wrote_base_url": wrote_base_url,
        "wrote_env": wrote_credential || wrote_account || wrote_base_url,
    }))
}

pub fn set_channel_enabled(config_dir: PathBuf, id: &str, enabled: bool) -> Result<Value> {
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, true)?;
    let mut found = false;
    let connections = parsed
        .get_mut("channels")
        .and_then(Value::as_object_mut)
        .and_then(|channels| channels.get_mut("connections"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Error::Config("channels.connections is not configured".to_string()))?;
    for connection in connections {
        if connection.get("id").and_then(Value::as_str) == Some(id) {
            connection["enabled"] = json!(enabled);
            found = true;
            break;
        }
    }
    if !found {
        return Err(Error::Config(format!("unknown channel connection `{id}`")));
    }
    let channels = channels_config_from_value(&parsed)?;
    write_toml_config_file(&config_path, &parsed)?;
    let connection = channels
        .connections
        .iter()
        .find(|connection| connection.id == id)
        .ok_or_else(|| Error::Config(format!("unknown channel connection `{id}`")))?;
    Ok(json!({
        "id": id,
        "enabled": enabled,
        "channel": connection.platform.as_str(),
        "path": config_path,
    }))
}

fn normalize_optional_single_line(label: &str, value: Option<String>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_string();
    if value.contains('\n') || value.contains('\r') {
        return Err(Error::Config(format!("{label} must be single-line")));
    }
    Ok((!value.is_empty()).then_some(value))
}

fn set_optional_string_field(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
    label: &str,
    value: String,
    keep_default: bool,
) -> Result<()> {
    let value = normalize_optional_single_line(label, Some(value))?;
    if keep_default {
        object.insert(field.to_string(), json!(value.unwrap_or_default()));
    } else {
        set_object_optional_string(object, field, value);
    }
    Ok(())
}

fn set_object_optional_string(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        object.insert(field.to_string(), json!(value));
    } else {
        object.remove(field);
    }
}

fn normalize_permission_mode(value: String) -> Result<Option<String>> {
    let Some(value) = normalize_optional_single_line("permission mode", Some(value))? else {
        return Ok(None);
    };
    if value == "default" {
        return Ok(None);
    }
    let Some(mode) = crate::types::PermissionMode::parse(&value) else {
        return Err(Error::Config(
            "permission mode must be default, acceptEdits, dontAsk, or bypassPermissions"
                .to_string(),
        ));
    };
    Ok(Some(mode.as_str().to_string()))
}

fn set_env_field(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
    value: String,
    default: Option<&'static str>,
) -> Result<()> {
    let normalized = normalize_optional_single_line(field, Some(value))?;
    let next = normalized.or_else(|| default.map(str::to_string));
    if let Some(env) = next {
        if !valid_env_name(&env) {
            return Err(Error::Config(format!(
                "{field} must be a valid environment variable name"
            )));
        }
        object.insert(field.to_string(), json!(env));
    } else {
        object.remove(field);
    }
    Ok(())
}

fn normalize_channel_list(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || !seen.insert(value.to_string()) {
            continue;
        }
        out.push(value.to_string());
    }
    out
}

fn load_channels_config(options: &RunOptions, cwd: &Path) -> Result<LoadedChannelsConfig> {
    let loaded = load_config_value(options, cwd)?;
    Ok(LoadedChannelsConfig {
        channels: channels_config_from_value(&loaded.value)?,
        env: loaded.env,
    })
}

fn channels_config_from_value(value: &Value) -> Result<ChannelsConfig> {
    value
        .get("channels")
        .map(parse_channels_config)
        .transpose()
        .map(|channels| channels.unwrap_or_default())
}

fn channel_rows(loaded: &LoadedChannelsConfig) -> Vec<Value> {
    loaded
        .channels
        .connections
        .iter()
        .map(|connection| channel_row(connection, &loaded.env))
        .collect()
}

fn channel_row(connection: &ChannelConnectionConfig, env: &BTreeMap<String, String>) -> Value {
    json!({
        "id": connection.id,
        "channel": connection.platform.as_str(),
        "domain": connection.domain,
        "enabled": connection.enabled,
        "label": connection.label,
        "transport": connection.transport.as_str(),
        "cwd": connection.cwd,
        "runtime_ref": connection.runtime_ref,
        "model": connection.model,
        "permission_mode": connection.permission_mode,
        "require_mention": connection.require_mention,
        "credential": {
            "env": connection.credential_env,
            "status": if channel_credential_present(connection, env) { "present" } else { "missing" },
        },
        "app_id": {
            "env": connection.app_id_env,
            "status": channel_app_id_status(connection, env),
        },
        "account": {
            "env": connection.account_env,
            "status": channel_account_status(connection, env),
        },
        "base_url": {
            "env": connection.base_url_env,
            "status": channel_base_url_status(connection, env),
        },
        "allowlist": {
            "users": connection.allow_users,
            "groups": connection.allow_groups,
            "status": if channel_allowlist_present(connection) { "present" } else { "missing" },
        },
        "runtime_status": channel_runtime_status(connection, env),
    })
}

fn channel_summary(loaded: &LoadedChannelsConfig) -> Value {
    let configured = loaded.channels.connections.len();
    let enabled = loaded
        .channels
        .connections
        .iter()
        .filter(|connection| connection.enabled)
        .count();
    let ready = loaded
        .channels
        .connections
        .iter()
        .filter(|connection| channel_runtime_status(connection, &loaded.env) == "ready")
        .count();
    let blocked = loaded
        .channels
        .connections
        .iter()
        .filter(|connection| {
            channel_runtime_blocked(channel_runtime_status(connection, &loaded.env))
        })
        .count();
    json!({
        "configured": configured,
        "enabled": enabled,
        "ready": ready,
        "blocked": blocked,
        "setup_needed": configured == 0 || blocked > 0,
    })
}

fn channel_credential_present(
    connection: &ChannelConnectionConfig,
    env: &BTreeMap<String, String>,
) -> bool {
    let main = connection
        .credential_env
        .as_deref()
        .is_some_and(|key| env_value(env, key).is_some());
    let app_id = connection
        .app_id_env
        .as_deref()
        .is_none_or(|key| env_value(env, key).is_some());
    main && app_id
}

fn channel_account_present(
    connection: &ChannelConnectionConfig,
    env: &BTreeMap<String, String>,
) -> bool {
    connection
        .account_env
        .as_deref()
        .is_none_or(|key| env_value(env, key).is_some())
}

fn channel_account_status(
    connection: &ChannelConnectionConfig,
    env: &BTreeMap<String, String>,
) -> &'static str {
    match connection.account_env.as_deref() {
        Some(key) if env_value(env, key).is_some() => "present",
        Some(_) => "missing",
        None => "not_required",
    }
}

fn channel_app_id_status(
    connection: &ChannelConnectionConfig,
    env: &BTreeMap<String, String>,
) -> &'static str {
    match connection.app_id_env.as_deref() {
        Some(key) if env_value(env, key).is_some() => "present",
        Some(_) => "missing",
        None => "not_required",
    }
}

fn channel_base_url_status(
    connection: &ChannelConnectionConfig,
    env: &BTreeMap<String, String>,
) -> &'static str {
    match connection.base_url_env.as_deref() {
        Some(key) if env_value(env, key).is_some() => "present",
        Some(_) => "default",
        None => "not_required",
    }
}

fn channel_allowlist_present(connection: &ChannelConnectionConfig) -> bool {
    !connection.allow_users.is_empty() || !connection.allow_groups.is_empty()
}

fn channel_runtime_status(
    connection: &ChannelConnectionConfig,
    env: &BTreeMap<String, String>,
) -> &'static str {
    if !connection.enabled {
        "disabled"
    } else if matches!(connection.platform, ChannelPlatform::Wechat)
        && !channel_credential_present(connection, env)
    {
        "needs_qr_login"
    } else if matches!(connection.platform, ChannelPlatform::Wechat)
        && !channel_account_present(connection, env)
    {
        "needs_account"
    } else if !channel_allowlist_present(connection) {
        if matches!(connection.platform, ChannelPlatform::Wechat) {
            "needs_allow_user"
        } else {
            "blocked"
        }
    } else if matches!(connection.platform, ChannelPlatform::Wechat)
        && connection.allow_users.is_empty()
        && !connection.allow_groups.is_empty()
    {
        "group_limited"
    } else if channel_credential_present(connection, env) {
        "ready"
    } else {
        "blocked"
    }
}

fn channel_runtime_blocked(status: &str) -> bool {
    matches!(
        status,
        "blocked" | "needs_qr_login" | "needs_account" | "needs_allow_user" | "group_limited"
    )
}
