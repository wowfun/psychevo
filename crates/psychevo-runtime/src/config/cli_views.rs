#[allow(unused_imports)]
pub(crate) use super::*;
pub fn config_show_value(options: &RunOptions, scope: ConfigScope) -> Result<Value> {
    let env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    match scope {
        ConfigScope::Global => {
            let home = resolve_psychevo_home(&env_map)?;
            let path = home.join(CONFIG_FILE_NAME);
            Ok(config_document_value(
                "global",
                Some(path.clone()),
                Vec::new(),
                path.exists(),
                load_toml_config_file(&path, false)?,
            ))
        }
        ConfigScope::Local => {
            let cwd = canonical_cwd(&options.cwd)?;
            let path = cwd.join(".psychevo").join(CONFIG_FILE_NAME);
            Ok(config_document_value(
                "local",
                Some(path.clone()),
                Vec::new(),
                path.exists(),
                load_toml_config_file(&path, false)?,
            ))
        }
        ConfigScope::Effective => {
            let cwd = canonical_cwd(&options.cwd)?;
            let loaded = load_config_value(options, &cwd)?;
            Ok(config_document_value(
                "effective",
                None,
                loaded.sources,
                true,
                loaded.value,
            ))
        }
    }
}

pub fn config_provider_list_value(options: &RunOptions, scope: ConfigScope) -> Result<Value> {
    let document = config_show_value(options, scope)?;
    let value = document.get("value").cloned().unwrap_or_else(|| json!({}));
    let config = parse_run_config(value)?;
    let providers = config
        .provider
        .iter()
        .map(|(provider, entry)| {
            json!({
                "provider": provider,
                "label": provider_label(provider, Some(entry)),
                "base_url": entry.options.base_url,
                "api_key_env": entry.options.api_key_env,
                "no_auth": entry.options.no_auth,
                "models": entry.models.keys().cloned().collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "scope": document.get("scope").cloned().unwrap_or(Value::String("effective".to_string())),
        "path": document.get("path").cloned().unwrap_or(Value::Null),
        "sources": document.get("sources").cloned().unwrap_or(Value::Array(Vec::new())),
        "providers": providers,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSetResult {
    pub path: PathBuf,
    pub key: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRemoveResult {
    pub path: PathBuf,
    pub key: String,
    pub changed: bool,
}

pub fn set_config_value(config_dir: PathBuf, key: &str, value: Value) -> Result<ConfigSetResult> {
    let path_parts = key
        .split('.')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if path_parts.is_empty() {
        return Err(Error::Config("config key must not be empty".to_string()));
    }
    fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, false)?;
    if !parsed.is_object() {
        parsed = json!({});
    }
    let before = parsed.clone();
    set_config_path_value(&mut parsed, &path_parts, value)?;
    let changed = parsed != before;
    if changed {
        write_toml_config_file(&config_path, &parsed)?;
    }
    Ok(ConfigSetResult {
        path: config_path,
        key: key.to_string(),
        changed,
    })
}

pub fn remove_config_value(config_dir: PathBuf, key: &str) -> Result<ConfigRemoveResult> {
    let path_parts = key
        .split('.')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if path_parts.is_empty() {
        return Err(Error::Config("config key must not be empty".to_string()));
    }
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, false)?;
    if !parsed.is_object() {
        parsed = json!({});
    }
    let before = parsed.clone();
    remove_config_path_value(&mut parsed, &path_parts)?;
    let changed = parsed != before;
    if changed {
        fs::create_dir_all(&config_dir)?;
        write_toml_config_file(&config_path, &parsed)?;
    }
    Ok(ConfigRemoveResult {
        path: config_path,
        key: key.to_string(),
        changed,
    })
}

pub(crate) fn set_config_path_value(root: &mut Value, path: &[&str], value: Value) -> Result<()> {
    let mut current = root
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    for part in &path[..path.len().saturating_sub(1)] {
        let entry = current
            .entry((*part).to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        current = entry
            .as_object_mut()
            .ok_or_else(|| Error::Config(format!("config path `{part}` must be an object")))?;
    }
    let last = path
        .last()
        .ok_or_else(|| Error::Config("config key must not be empty".to_string()))?;
    current.insert((*last).to_string(), value);
    Ok(())
}

pub(crate) fn remove_config_path_value(root: &mut Value, path: &[&str]) -> Result<()> {
    let mut current = root
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    for part in &path[..path.len().saturating_sub(1)] {
        let Some(next) = current.get_mut(*part) else {
            return Ok(());
        };
        current = next
            .as_object_mut()
            .ok_or_else(|| Error::Config(format!("config path `{part}` must be an object")))?;
    }
    if let Some(last) = path.last() {
        current.remove(*last);
    }
    Ok(())
}

pub fn auth_status_value(options: &RunOptions, provider: Option<&str>) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let mut providers = BTreeSet::new();
    if let Some(provider) = provider {
        providers.insert(normalize_provider_id(provider));
    } else {
        providers.extend(
            BUILT_IN_PROVIDERS
                .iter()
                .filter(|provider| provider.id != "custom")
                .map(|provider| provider.id.to_string()),
        );
        providers.extend(loaded.config.provider.keys().cloned());
    }

    let mut rows = Vec::new();
    for provider in providers {
        let config_entry = loaded.config.provider.get(&provider);
        let built_in = built_in_provider(&provider);
        if built_in.is_none() && config_entry.is_none() {
            return Err(Error::Config(format!("unknown provider: {provider}")));
        }
        let base_url = provider_base_url(&provider, config_entry, &loaded.env);
        let explicit_no_auth = config_entry.is_some_and(|entry| entry.options.no_auth);
        let api_key_env = (!explicit_no_auth)
            .then(|| {
                first_string([
                    config_entry.and_then(|entry| entry.options.api_key_env.clone()),
                    built_in.and_then(|provider| {
                        provider
                            .api_key_envs
                            .iter()
                            .find(|key| env_value(&loaded.env, key).is_some())
                            .or_else(|| provider.api_key_envs.first())
                            .map(|key| (*key).to_string())
                    }),
                ])
            })
            .flatten();
        let credential_present = api_key_env
            .as_deref()
            .is_some_and(|key| env_value(&loaded.env, key).is_some());
        let no_auth = explicit_no_auth
            || base_url.as_deref().is_some_and(is_loopback_base_url)
            || built_in.is_some_and(|provider| provider.allow_no_auth);
        let status = if credential_present {
            "present"
        } else if no_auth {
            "not_required"
        } else {
            "missing"
        };
        rows.push(json!({
            "provider": provider,
            "label": provider_label(&provider, config_entry),
            "base_url": base_url,
            "api_key_env": api_key_env,
            "credential_present": credential_present,
            "no_auth": no_auth,
            "status": status,
        }));
    }
    rows.sort_by(|left, right| {
        left.get("provider")
            .and_then(Value::as_str)
            .cmp(&right.get("provider").and_then(Value::as_str))
    });
    Ok(json!({ "providers": rows }))
}

pub(crate) fn config_document_value(
    scope: &str,
    path: Option<PathBuf>,
    sources: Vec<PathBuf>,
    exists: bool,
    mut value: Value,
) -> Value {
    redact_sensitive_config(&mut value);
    json!({
        "scope": scope,
        "path": path,
        "sources": sources,
        "exists": exists,
        "value": value,
    })
}

pub(crate) fn redact_sensitive_config(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if key == "api_key" || key == "apiKey" {
                    *value = Value::String("<redacted>".to_string());
                } else {
                    redact_sensitive_config(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_sensitive_config(value);
            }
        }
        _ => {}
    }
}
