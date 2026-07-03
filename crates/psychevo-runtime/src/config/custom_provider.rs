#[allow(unused_imports)]
pub(crate) use super::*;
use std::io::Write;

pub fn custom_provider_api_key_env(provider_id: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in provider_id.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    format!("{out}_API_KEY")
}

pub fn create_global_custom_provider(input: CustomProviderInput) -> Result<CustomProviderResult> {
    create_scoped_custom_provider(ScopedCustomProviderInput {
        config_dir: input.home,
        provider_id: input.provider_id,
        label: input.label,
        base_url: input.base_url,
        api_key_env: None,
        api_key: input.api_key,
        require_api_key: true,
        no_auth: input.no_auth,
    })
}

pub fn create_scoped_custom_provider(
    input: ScopedCustomProviderInput,
) -> Result<CustomProviderResult> {
    let provider_id = input.provider_id.trim().to_string();
    let name = input.label.trim().to_string();
    let base_url = input.base_url.trim().trim_end_matches('/').to_string();
    let requested_api_key_env = input
        .api_key_env
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(api_key_env) = &requested_api_key_env
        && !valid_env_name(api_key_env)
    {
        return Err(Error::Config(
            "api_key_env must be a valid environment variable name".to_string(),
        ));
    }
    let api_key_env = if input.no_auth {
        None
    } else {
        Some(
            requested_api_key_env
                .clone()
                .unwrap_or_else(|| custom_provider_api_key_env(&provider_id)),
        )
    };
    let api_key = input
        .api_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    validate_custom_provider_id(&provider_id)?;
    if name.is_empty() {
        return Err(Error::Config("provider name is required".to_string()));
    }
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err(Error::Config(
            "provider api must start with http:// or https://".to_string(),
        ));
    }
    if let Some(api_key) = &api_key
        && (api_key.contains('\n') || api_key.contains('\r'))
    {
        return Err(Error::Config(
            "provider API key must not contain newlines".to_string(),
        ));
    }

    let config_dir = input.config_dir;
    fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let env_path = config_dir.join(".env");
    let mut parsed = load_toml_config_file(&config_path, false)?;
    let existing = parse_run_config(parsed.clone())?;
    if existing.provider.contains_key(&provider_id) {
        return Err(Error::Config(format!(
            "provider {provider_id} already exists"
        )));
    }

    let mut env_values = BTreeMap::new();
    load_dotenv_file(&env_path, &mut env_values)?;
    let reused_existing_api_key = api_key_env
        .as_deref()
        .is_some_and(|api_key_env| env_value(&env_values, api_key_env).is_some());
    if input.require_api_key && !input.no_auth && !reused_existing_api_key && api_key.is_none() {
        return Err(Error::Config(format!(
            "provider requires API key for {}",
            api_key_env.as_deref().unwrap_or("credentials")
        )));
    }

    write_provider_config(
        &config_path,
        &mut parsed,
        &provider_id,
        &name,
        &base_url,
        requested_api_key_env
            .is_some()
            .then_some(api_key_env.as_deref())
            .flatten(),
        input.no_auth,
    )?;
    let mut wrote_api_key = false;
    if !reused_existing_api_key
        && let Some(api_key_env) = &api_key_env
        && let Some(api_key) = api_key
    {
        append_dotenv_value(&env_path, api_key_env, &api_key)?;
        wrote_api_key = true;
    }

    Ok(CustomProviderResult {
        provider_id,
        label: name,
        base_url,
        api_key_env: api_key_env.unwrap_or_default(),
        wrote_api_key,
        reused_existing_api_key,
    })
}

pub fn set_provider_api_key(
    options: &RunOptions,
    config_dir: PathBuf,
    provider_id: &str,
    api_key: &str,
) -> Result<Value> {
    let provider_id = normalize_provider_id(provider_id);
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err(Error::Config("provider API key is required".to_string()));
    }
    if api_key.contains('\n') || api_key.contains('\r') {
        return Err(Error::Config(
            "provider API key must not contain newlines".to_string(),
        ));
    }
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let config_entry = loaded.config.provider.get(&provider_id);
    let built_in = built_in_provider(&provider_id);
    if built_in.is_none() && config_entry.is_none() {
        return Err(Error::Config(format!("unknown provider: {provider_id}")));
    }
    if config_entry.is_some_and(|entry| entry.no_auth) {
        return Err(Error::Config(format!(
            "provider {provider_id} is configured with no_auth"
        )));
    }
    let api_key_env = provider_api_key_env(&provider_id, config_entry)
        .ok_or_else(|| Error::Config(format!("provider {provider_id} does not use API keys")))?;
    fs::create_dir_all(&config_dir)?;
    let env_path = config_dir.join(".env");
    let replaced_existing = set_dotenv_value(&env_path, &api_key_env, api_key)?;
    Ok(json!({
        "provider": provider_id,
        "api_key_env": api_key_env,
        "env_path": env_path,
        "wrote_api_key": true,
        "replaced_existing": replaced_existing,
    }))
}

pub fn set_provider_model_config(
    config_dir: PathBuf,
    provider_id: &str,
    model_id: &str,
    model: Value,
) -> Result<()> {
    let provider_id = normalize_provider_id(provider_id);
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return Err(Error::Config("model id is required".to_string()));
    }
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, false)?;
    ensure_json_object(&mut parsed);
    let root = parsed
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let providers = root
        .entry("provider".to_string())
        .or_insert_with(|| json!({}));
    ensure_json_object(providers);
    let providers = providers
        .as_object_mut()
        .ok_or_else(|| Error::Config("provider must be an object".to_string()))?;
    let provider = providers.entry(provider_id).or_insert_with(|| json!({}));
    ensure_json_object(provider);
    let provider = provider
        .as_object_mut()
        .ok_or_else(|| Error::Config("provider entry must be an object".to_string()))?;
    let models = provider
        .entry("models".to_string())
        .or_insert_with(|| json!({}));
    ensure_json_object(models);
    let models = models
        .as_object_mut()
        .ok_or_else(|| Error::Config("provider models must be an object".to_string()))?;
    models.insert(model_id.to_string(), model);
    fs::create_dir_all(&config_dir)?;
    write_toml_config_file(&config_path, &parsed)
}

pub(crate) fn validate_custom_provider_id(provider_id: &str) -> Result<()> {
    if !valid_provider_id(provider_id) {
        return Err(Error::Config(
            "provider id must use lowercase letters, numbers, hyphens, or underscores".to_string(),
        ));
    }
    let normalized = normalize_provider_id(provider_id);
    if normalized != provider_id || built_in_provider(provider_id).is_some() {
        return Err(Error::Config(format!(
            "provider id {provider_id} collides with a built-in provider or alias"
        )));
    }
    Ok(())
}

pub(crate) fn valid_provider_id(provider_id: &str) -> bool {
    let mut chars = provider_id.chars();
    matches!(chars.next(), Some('a'..='z' | '0'..='9'))
        && chars.all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '-' | '_'))
}

pub(crate) fn write_provider_config(
    path: &Path,
    value: &mut Value,
    provider_id: &str,
    name: &str,
    api: &str,
    api_key_env: Option<&str>,
    no_auth: bool,
) -> Result<()> {
    ensure_json_object(value);
    let root = value
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let providers = root
        .entry("provider".to_string())
        .or_insert_with(|| json!({}));
    ensure_json_object(providers);
    let providers = providers
        .as_object_mut()
        .ok_or_else(|| Error::Config("provider must be an object".to_string()))?;
    let mut provider = json!({
        "name": name,
        "api": api,
        "models": {},
    });
    if let Some(api_key_env) = api_key_env {
        provider["api_key_env"] = json!(api_key_env);
    }
    if no_auth {
        provider["no_auth"] = json!(true);
    }
    providers.insert(provider_id.to_string(), provider);
    write_toml_config_file(path, value)
}

pub(crate) fn ensure_json_object(value: &mut Value) {
    if !value.is_object() {
        *value = json!({});
    }
}

pub(crate) fn append_dotenv_value(path: &Path, key: &str, value: &str) -> Result<()> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let mut out = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(key);
    out.push('=');
    out.push_str(value);
    out.push('\n');
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(out.as_bytes())?;
    Ok(())
}

pub(crate) fn set_dotenv_value(path: &Path, key: &str, value: &str) -> Result<bool> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let mut replaced = false;
    let mut lines = Vec::new();
    for line in existing.lines() {
        let trimmed = line.trim_start();
        let is_match = !trimmed.starts_with('#')
            && trimmed
                .split_once('=')
                .is_some_and(|(name, _)| name.trim() == key);
        if is_match {
            if !replaced {
                lines.push(format!("{key}={value}"));
            }
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        lines.push(format!("{key}={value}"));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    fs::write(path, out)?;
    Ok(replaced)
}
