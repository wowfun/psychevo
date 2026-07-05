#[allow(unused_imports)]
pub(crate) use super::*;

use sha2::{Digest, Sha256};

pub const MCP_OAUTH_KEYRING_SERVICE: &str = "psychevo-mcp-oauth";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerConfigInput {
    pub name: String,
    pub transport: String,
    pub enabled: Option<bool>,
    pub required: Option<bool>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub bearer_token_env_var: Option<String>,
    pub scopes: Vec<String>,
    pub oauth_resource: Option<String>,
    pub oauth_client_id: Option<String>,
    pub enabled_tools: Option<Vec<String>>,
    pub disabled_tools: Vec<String>,
    pub supports_parallel_tool_calls: Option<bool>,
    pub startup_timeout_secs: Option<u64>,
    pub tool_timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolPolicyInput {
    pub enabled_tools: Option<Vec<String>>,
    pub disabled_tools: Vec<String>,
}

pub fn mcp_servers_value(options: &RunOptions, scope: ConfigScope) -> Result<Value> {
    let document = config_show_value(options, scope)?;
    let value = document.get("value").cloned().unwrap_or_else(|| json!({}));
    let config = parse_run_config(value)?;
    let env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    let home = resolve_psychevo_home(&env_map).unwrap_or_else(|_| {
        options
            .cwd
            .join(".psychevo")
            .canonicalize()
            .unwrap_or_else(|_| options.cwd.join(".psychevo"))
    });
    let servers = config
        .mcp_servers
        .iter()
        .map(|server| mcp_server_view(&home, server))
        .collect::<Vec<_>>();
    Ok(json!({
        "scope": document.get("scope").cloned().unwrap_or(Value::String("effective".to_string())),
        "path": document.get("path").cloned().unwrap_or(Value::Null),
        "sources": document.get("sources").cloned().unwrap_or(Value::Array(Vec::new())),
        "servers": servers,
        "count": servers.len(),
    }))
}

pub fn mcp_server_value(options: &RunOptions, name: &str) -> Result<Value> {
    let document = mcp_servers_value(options, ConfigScope::Effective)?;
    let servers = document
        .get("servers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(server) = servers.into_iter().find(|server| {
        server
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|value| value == name)
    }) else {
        return Err(Error::Config(format!("unknown MCP server: {name}")));
    };
    Ok(json!({"server": server}))
}

pub fn upsert_mcp_server(config_dir: PathBuf, input: McpServerConfigInput) -> Result<Value> {
    let name = normalize_mcp_server_name(&input.name)?;
    let server_value = mcp_server_document_value(&name, input)?;
    fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut document = load_toml_config_file(&config_path, false)?;
    ensure_json_object(&mut document);
    let root = document
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let mcp_servers = root
        .entry("mcp_servers".to_string())
        .or_insert_with(|| json!({}));
    ensure_json_object(mcp_servers);
    let mcp_servers = mcp_servers
        .as_object_mut()
        .ok_or_else(|| Error::Config("mcp_servers must be an object".to_string()))?;
    let changed = mcp_servers.get(&name) != Some(&server_value);
    mcp_servers.insert(name.clone(), server_value.clone());
    if changed {
        write_toml_config_file(&config_path, &document)?;
    }
    Ok(json!({
        "success": true,
        "changed": changed,
        "name": name,
        "path": config_path,
        "server": redacted_mcp_document_value(&name, &server_value),
    }))
}

pub fn remove_mcp_server(config_dir: PathBuf, name: &str) -> Result<Value> {
    let name = normalize_mcp_server_name(name)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut document = load_toml_config_file(&config_path, false)?;
    let changed = document
        .get_mut("mcp_servers")
        .and_then(Value::as_object_mut)
        .and_then(|servers| servers.remove(&name))
        .is_some();
    if changed {
        write_toml_config_file(&config_path, &document)?;
    }
    Ok(json!({
        "success": true,
        "changed": changed,
        "name": name,
        "path": config_path,
    }))
}

pub fn set_mcp_server_enabled(config_dir: PathBuf, name: &str, enabled: bool) -> Result<Value> {
    let name = normalize_mcp_server_name(name)?;
    update_mcp_server_document(config_dir, &name, |server| {
        server.insert("enabled".to_string(), Value::Bool(enabled));
        Ok(())
    })
}

pub fn set_mcp_server_tool_policy(
    config_dir: PathBuf,
    name: &str,
    policy: McpToolPolicyInput,
) -> Result<Value> {
    let name = normalize_mcp_server_name(name)?;
    update_mcp_server_document(config_dir, &name, |server| {
        match policy.enabled_tools {
            Some(enabled_tools) => {
                server.insert(
                    "enabled_tools".to_string(),
                    string_array_value(enabled_tools),
                );
            }
            None => {
                server.remove("enabled_tools");
            }
        }
        if policy.disabled_tools.is_empty() {
            server.remove("disabled_tools");
        } else {
            server.insert(
                "disabled_tools".to_string(),
                string_array_value(policy.disabled_tools),
            );
        }
        Ok(())
    })
}

pub fn mcp_oauth_keyring_account(profile_home: &Path, server_name: &str, url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(profile_home.display().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(server_name.as_bytes());
    hasher.update(b"\0");
    hasher.update(url.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn load_mcp_oauth_access_token(
    profile_home: &Path,
    server_name: &str,
    url: &str,
) -> Result<Option<String>> {
    let account = mcp_oauth_keyring_account(profile_home, server_name, url);
    let entry = keyring::Entry::new(MCP_OAUTH_KEYRING_SERVICE, &account)
        .map_err(|err| Error::Config(format!("keyring entry failed: {err}")))?;
    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(Error::Config(format!("keyring read failed: {err}"))),
    }
}

pub fn save_mcp_oauth_access_token(
    profile_home: &Path,
    server_name: &str,
    url: &str,
    access_token: &str,
) -> Result<()> {
    let account = mcp_oauth_keyring_account(profile_home, server_name, url);
    let entry = keyring::Entry::new(MCP_OAUTH_KEYRING_SERVICE, &account)
        .map_err(|err| Error::Config(format!("keyring entry failed: {err}")))?;
    entry
        .set_password(access_token)
        .map_err(|err| Error::Config(format!("keyring write failed: {err}")))
}

pub fn clear_mcp_oauth_access_token(
    profile_home: &Path,
    server_name: &str,
    url: &str,
) -> Result<bool> {
    let account = mcp_oauth_keyring_account(profile_home, server_name, url);
    let entry = keyring::Entry::new(MCP_OAUTH_KEYRING_SERVICE, &account)
        .map_err(|err| Error::Config(format!("keyring entry failed: {err}")))?;
    match entry.delete_credential() {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(err) => Err(Error::Config(format!("keyring delete failed: {err}"))),
    }
}

fn update_mcp_server_document<F>(config_dir: PathBuf, name: &str, mutate: F) -> Result<Value>
where
    F: FnOnce(&mut serde_json::Map<String, Value>) -> Result<()>,
{
    fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut document = load_toml_config_file(&config_path, false)?;
    ensure_json_object(&mut document);
    let root = document
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let servers = root
        .entry("mcp_servers".to_string())
        .or_insert_with(|| json!({}));
    ensure_json_object(servers);
    let servers = servers
        .as_object_mut()
        .ok_or_else(|| Error::Config("mcp_servers must be an object".to_string()))?;
    let server = servers
        .get_mut(name)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| Error::Config(format!("unknown MCP server: {name}")))?;
    let before = server.clone();
    mutate(server)?;
    let changed = *server != before;
    let server_value = servers.get(name).cloned().unwrap_or_else(|| json!({}));
    if changed {
        write_toml_config_file(&config_path, &document)?;
    }
    Ok(json!({
        "success": true,
        "changed": changed,
        "name": name,
        "path": config_path,
        "server": redacted_mcp_document_value(name, &server_value),
    }))
}

fn mcp_server_document_value(name: &str, input: McpServerConfigInput) -> Result<Value> {
    let mut server = serde_json::Map::new();
    match input.transport.trim() {
        "stdio" => {
            reject_stdio_auth_input(&input)?;
            server.insert("transport".to_string(), Value::String("stdio".to_string()));
            let command = input
                .command
                .ok_or_else(|| Error::Config(format!("mcp_servers.{name}.command is required")))?;
            server.insert("command".to_string(), Value::String(command));
            if !input.args.is_empty() {
                server.insert("args".to_string(), string_array_value(input.args));
            }
            if !input.env.is_empty() {
                server.insert("env".to_string(), json!(input.env));
            }
            if let Some(cwd) = input.cwd {
                server.insert("cwd".to_string(), Value::String(cwd));
            }
        }
        "streamable_http" | "http" => {
            if input
                .headers
                .keys()
                .any(|key| key.eq_ignore_ascii_case("authorization"))
            {
                return Err(Error::Config(format!(
                    "mcp_servers.{name}.headers.Authorization is not supported; use bearer_token_env_var"
                )));
            }
            server.insert(
                "transport".to_string(),
                Value::String("streamable_http".to_string()),
            );
            let url = input
                .url
                .ok_or_else(|| Error::Config(format!("mcp_servers.{name}.url is required")))?;
            server.insert("url".to_string(), Value::String(url));
            if !input.headers.is_empty() {
                server.insert("headers".to_string(), json!(input.headers));
            }
            if let Some(env_var) = input.bearer_token_env_var {
                server.insert("bearer_token_env_var".to_string(), Value::String(env_var));
            }
            if !input.scopes.is_empty() {
                server.insert("scopes".to_string(), string_array_value(input.scopes));
            }
            if let Some(resource) = input.oauth_resource {
                server.insert("oauth_resource".to_string(), Value::String(resource));
            }
            if let Some(client_id) = input.oauth_client_id {
                server.insert("oauth".to_string(), json!({ "client_id": client_id }));
            }
        }
        other => {
            return Err(Error::Config(format!(
                "unsupported MCP server transport: {other}"
            )));
        }
    }
    if let Some(enabled) = input.enabled {
        server.insert("enabled".to_string(), Value::Bool(enabled));
    }
    if let Some(required) = input.required {
        server.insert("required".to_string(), Value::Bool(required));
    }
    if let Some(enabled_tools) = input.enabled_tools {
        server.insert(
            "enabled_tools".to_string(),
            string_array_value(enabled_tools),
        );
    }
    if !input.disabled_tools.is_empty() {
        server.insert(
            "disabled_tools".to_string(),
            string_array_value(input.disabled_tools),
        );
    }
    if let Some(value) = input.supports_parallel_tool_calls {
        server.insert(
            "supports_parallel_tool_calls".to_string(),
            Value::Bool(value),
        );
    }
    if let Some(value) = input.startup_timeout_secs {
        server.insert("startup_timeout_secs".to_string(), json!(value));
    }
    if let Some(value) = input.tool_timeout_secs {
        server.insert("tool_timeout_secs".to_string(), json!(value));
    }
    Ok(Value::Object(server))
}

fn reject_stdio_auth_input(input: &McpServerConfigInput) -> Result<()> {
    if input.bearer_token_env_var.is_some()
        || !input.scopes.is_empty()
        || input.oauth_resource.is_some()
        || input.oauth_client_id.is_some()
    {
        return Err(Error::Config(
            "OAuth and bearer auth fields are only valid for streamable HTTP MCP servers"
                .to_string(),
        ));
    }
    Ok(())
}

fn normalize_mcp_server_name(name: &str) -> Result<String> {
    let name = name.trim();
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'));
    if valid {
        Ok(name.to_string())
    } else {
        Err(Error::Config(format!("invalid MCP server name: {name}")))
    }
}

fn string_array_value(values: Vec<String>) -> Value {
    Value::Array(values.into_iter().map(Value::String).collect())
}

fn mcp_server_view(profile_home: &Path, server: &McpServerInput) -> Value {
    let transport = match &server.transport {
        McpTransportInput::Stdio {
            command,
            args,
            env,
            cwd,
        } => json!({
            "kind": "stdio",
            "command": command,
            "args": args,
            "envKeys": env.keys().cloned().collect::<Vec<_>>(),
            "cwd": cwd,
        }),
        McpTransportInput::StreamableHttp {
            url,
            headers,
            bearer_token_env_var,
            scopes,
            oauth_resource,
            oauth_client_id,
        } => {
            let stored_oauth_token = load_mcp_oauth_access_token(profile_home, &server.name, url)
                .ok()
                .flatten()
                .is_some();
            json!({
                "kind": "streamable_http",
                "url": url,
                "headers": headers,
                "auth": {
                    "bearerTokenEnvVar": bearer_token_env_var,
                    "scopes": scopes,
                    "oauthResource": oauth_resource,
                    "oauthClientId": oauth_client_id,
                    "oauthConfigured": oauth_client_id.is_some() || oauth_resource.is_some(),
                    "storedOAuthToken": stored_oauth_token,
                }
            })
        }
        McpTransportInput::Unsupported { kind } => json!({
            "kind": "unsupported",
            "unsupportedKind": kind,
        }),
    };
    json!({
        "name": server.name,
        "sourceId": server.source_id,
        "sourceKind": server.source_kind,
        "enabled": server.policy.enabled,
        "required": server.policy.required,
        "transport": transport,
        "policy": {
            "enabledTools": server.policy.enabled_tools,
            "disabledTools": server.policy.disabled_tools,
            "supportsParallelToolCalls": server.policy.supports_parallel_tool_calls,
            "startupTimeoutSecs": server.policy.startup_timeout_secs,
            "toolTimeoutSecs": server.policy.tool_timeout_secs,
        }
    })
}

fn redacted_mcp_document_value(name: &str, value: &Value) -> Value {
    let mut object = value.as_object().cloned().unwrap_or_default();
    if let Some(env) = object.get("env").and_then(Value::as_object) {
        object.insert(
            "envKeys".to_string(),
            Value::Array(env.keys().cloned().map(Value::String).collect()),
        );
        object.remove("env");
    }
    object.remove("bearer_token");
    json!({
        "name": name,
        "config": Value::Object(object),
    })
}
