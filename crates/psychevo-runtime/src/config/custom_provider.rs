use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstRootNode};
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

pub fn create_global_custom_provider(
    input: CustomProviderInput,
) -> Result<CustomProviderResult> {
    let provider_id = input.provider_id.trim().to_string();
    let label = input.label.trim().to_string();
    let base_url = input.base_url.trim().trim_end_matches('/').to_string();
    let api_key = input
        .api_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    validate_custom_provider_id(&provider_id)?;
    if label.is_empty() {
        return Err(Error::Config("provider label is required".to_string()));
    }
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err(Error::Config(
            "provider base_url must start with http:// or https://".to_string(),
        ));
    }
    if let Some(api_key) = &api_key
        && (api_key.contains('\n') || api_key.contains('\r'))
    {
        return Err(Error::Config(
            "provider API key must not contain newlines".to_string(),
        ));
    }

    let home = input.home;
    fs::create_dir_all(&home)?;
    let config_path = home.join("config.jsonc");
    let env_path = home.join(".env");
    let config_text = if config_path.exists() {
        fs::read_to_string(&config_path)?
    } else {
        "{}\n".to_string()
    };
    let parsed = load_jsonc_config_file(&config_path, false)?;
    let existing = parse_run_config(parsed)?;
    if existing.provider.contains_key(&provider_id) {
        return Err(Error::Config(format!(
            "provider {provider_id} already exists"
        )));
    }

    let api_key_env = custom_provider_api_key_env(&provider_id);
    let mut env_values = BTreeMap::new();
    load_dotenv_file(&env_path, &mut env_values)?;
    let reused_existing_api_key = env_value(&env_values, &api_key_env).is_some();
    if !reused_existing_api_key && api_key.is_none() {
        return Err(Error::Config(format!(
            "provider requires API key for {api_key_env}"
        )));
    }

    write_provider_config(
        &config_path,
        &config_text,
        &provider_id,
        &label,
        &base_url,
        &api_key_env,
    )?;
    let mut wrote_api_key = false;
    if !reused_existing_api_key
        && let Some(api_key) = api_key
    {
        append_dotenv_value(&env_path, &api_key_env, &api_key)?;
        wrote_api_key = true;
    }

    Ok(CustomProviderResult {
        provider_id,
        label,
        base_url,
        api_key_env,
        wrote_api_key,
        reused_existing_api_key,
    })
}

fn validate_custom_provider_id(provider_id: &str) -> Result<()> {
    if !valid_provider_id(provider_id) {
        return Err(Error::Config(
            "provider id must use lowercase letters, numbers, hyphens, or underscores"
                .to_string(),
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

fn valid_provider_id(provider_id: &str) -> bool {
    let mut chars = provider_id.chars();
    matches!(chars.next(), Some('a'..='z' | '0'..='9'))
        && chars.all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '-' | '_'))
}

fn write_provider_config(
    path: &Path,
    text: &str,
    provider_id: &str,
    label: &str,
    base_url: &str,
    api_key_env: &str,
) -> Result<()> {
    let text = if text.trim().is_empty() { "{}\n" } else { text };
    let root = CstRootNode::parse(text, &ParseOptions::default())
        .map_err(|err| Error::Config(format!("{}: {err}", path.display())))?;
    let root_object = root.object_value_or_set();
    let providers = root_object.object_value_or_set("provider");
    providers.append(
        provider_id,
        CstInputValue::Object(vec![
            ("label".to_string(), CstInputValue::String(label.to_string())),
            (
                "options".to_string(),
                CstInputValue::Object(vec![
                    (
                        "base_url".to_string(),
                        CstInputValue::String(base_url.to_string()),
                    ),
                    (
                        "api_key_env".to_string(),
                        CstInputValue::String(api_key_env.to_string()),
                    ),
                ]),
            ),
            ("models".to_string(), CstInputValue::Object(Vec::new())),
        ]),
    );
    fs::write(path, ensure_trailing_newline(root.to_string()))?;
    Ok(())
}

fn append_dotenv_value(path: &Path, key: &str, value: &str) -> Result<()> {
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

fn ensure_trailing_newline(mut text: String) -> String {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}
