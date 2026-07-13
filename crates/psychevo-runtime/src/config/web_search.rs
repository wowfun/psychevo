#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn parse_web_config(value: &Value) -> Result<WebConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("web must be an object".into()))?;
    reject_unknown(object, &["search"], "web")?;
    let mut web = WebConfig::default();
    if let Some(search) = object.get("search") {
        web.search = parse_web_search_config(search)?;
    }
    Ok(web)
}

fn parse_web_search_config(value: &Value) -> Result<WebSearchConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("web.search must be an object".into()))?;
    reject_unknown(
        object,
        &[
            "execution",
            "backend",
            "external_access",
            "context_size",
            "return_token_budget",
            "content_types",
            "allowed_domains",
            "blocked_domains",
            "background_storage_acknowledged",
            "location",
            "image",
        ],
        "web.search",
    )?;
    let mut out = WebSearchConfig::default();
    if let Some(value) = optional_string(object, "execution", "web.search.execution")? {
        out.execution = match value.as_str() {
            "auto" => WebSearchExecution::Auto,
            "local" => WebSearchExecution::Local,
            "hosted" => WebSearchExecution::Hosted,
            _ => {
                return Err(Error::Config(
                    "web.search.execution must be auto, local, or hosted".into(),
                ));
            }
        };
    }
    if let Some(value) = optional_string(object, "backend", "web.search.backend")? {
        out.backend = match value.as_str() {
            "auto" => WebSearchBackend::Auto,
            "searxng" => WebSearchBackend::Searxng,
            "brave" => WebSearchBackend::Brave,
            "exa" => WebSearchBackend::Exa,
            "parallel" => WebSearchBackend::Parallel,
            _ => {
                return Err(Error::Config(
                    "web.search.backend must be auto, searxng, brave, exa, or parallel".into(),
                ));
            }
        };
    }
    if let Some(value) = optional_string(object, "external_access", "web.search.external_access")? {
        out.external_access = match value.as_str() {
            "live" => WebSearchExternalAccess::Live,
            "cached" => WebSearchExternalAccess::Cached,
            _ => {
                return Err(Error::Config(
                    "web.search.external_access must be live or cached".into(),
                ));
            }
        };
    }
    if let Some(value) = optional_string(object, "context_size", "web.search.context_size")? {
        out.context_size = match value.as_str() {
            "low" => WebSearchContextSize::Low,
            "medium" => WebSearchContextSize::Medium,
            "high" => WebSearchContextSize::High,
            _ => {
                return Err(Error::Config(
                    "web.search.context_size must be low, medium, or high".into(),
                ));
            }
        };
    }
    if let Some(value) = optional_string(
        object,
        "return_token_budget",
        "web.search.return_token_budget",
    )? {
        out.return_token_budget = match value.as_str() {
            "default" => WebSearchTokenBudget::Default,
            "unlimited" => WebSearchTokenBudget::Unlimited,
            _ => {
                return Err(Error::Config(
                    "web.search.return_token_budget must be default or unlimited".into(),
                ));
            }
        };
    }
    if let Some(value) = object.get("content_types") {
        let values = value
            .as_array()
            .ok_or_else(|| Error::Config("web.search.content_types must be an array".into()))?;
        if values.is_empty() {
            return Err(Error::Config(
                "web.search.content_types must not be empty".into(),
            ));
        }
        out.content_types = values
            .iter()
            .map(|value| match value.as_str() {
                Some("text") => Ok(WebSearchContentType::Text),
                Some("image") => Ok(WebSearchContentType::Image),
                _ => Err(Error::Config(
                    "web.search.content_types entries must be text or image".into(),
                )),
            })
            .collect::<Result<Vec<_>>>()?;
        out.content_types.sort_by_key(|value| match value {
            WebSearchContentType::Text => 0,
            WebSearchContentType::Image => 1,
        });
        out.content_types.dedup();
    }
    out.allowed_domains = string_array(object, "allowed_domains", "web.search.allowed_domains")?;
    out.blocked_domains = string_array(object, "blocked_domains", "web.search.blocked_domains")?;
    if out
        .allowed_domains
        .iter()
        .any(|domain| out.blocked_domains.contains(domain))
    {
        return Err(Error::Config(
            "web.search domain cannot be both allowed and blocked".into(),
        ));
    }
    if let Some(value) = object.get("background_storage_acknowledged") {
        out.background_storage_acknowledged = value.as_bool().ok_or_else(|| {
            Error::Config("web.search.background_storage_acknowledged must be a boolean".into())
        })?;
    }
    if let Some(value) = object.get("location") {
        out.location = parse_location(value)?;
    }
    if let Some(value) = object.get("image") {
        out.image = parse_image(value)?;
    }
    if out.return_token_budget == WebSearchTokenBudget::Unlimited
        && !out.background_storage_acknowledged
    {
        return Err(Error::Config("web.search.return_token_budget=unlimited requires background_storage_acknowledged=true".into()));
    }
    Ok(out)
}

fn parse_location(value: &Value) -> Result<WebSearchLocation> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("web.search.location must be an object".into()))?;
    reject_unknown(
        object,
        &["country", "region", "city", "timezone"],
        "web.search.location",
    )?;
    Ok(WebSearchLocation {
        country: optional_string(object, "country", "web.search.location.country")?
            .unwrap_or_default(),
        region: optional_string(object, "region", "web.search.location.region")?
            .unwrap_or_default(),
        city: optional_string(object, "city", "web.search.location.city")?.unwrap_or_default(),
        timezone: optional_string(object, "timezone", "web.search.location.timezone")?
            .unwrap_or_default(),
    })
}

fn parse_image(value: &Value) -> Result<WebSearchImageConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("web.search.image must be an object".into()))?;
    reject_unknown(object, &["max_results", "caption"], "web.search.image")?;
    let mut out = WebSearchImageConfig::default();
    if let Some(value) = object.get("max_results") {
        out.max_results = value
            .as_u64()
            .filter(|value| (1..=20).contains(value))
            .ok_or_else(|| {
                Error::Config("web.search.image.max_results must be an integer from 1 to 20".into())
            })? as usize;
    }
    if let Some(value) = object.get("caption") {
        out.caption = value
            .as_bool()
            .ok_or_else(|| Error::Config("web.search.image.caption must be a boolean".into()))?;
    }
    Ok(out)
}

fn optional_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<String>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| Error::Config(format!("{path} must be a string")))
        })
        .transpose()
}

fn string_array(
    object: &serde_json::Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Vec<String>> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| Error::Config(format!("{path} must be an array")))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| Error::Config(format!("{path} entries must be non-empty strings")))
        })
        .collect()
}

fn reject_unknown(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
    path: &str,
) -> Result<()> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(Error::Config(format!("unknown {path} key `{key}`")));
    }
    Ok(())
}

pub(crate) fn resolve_web_search_execution(
    config: &WebSearchConfig,
    provider: &str,
    capabilities: &ModelCapabilities,
    permissions: &PermissionConfig,
) -> Result<WebSearchExecution> {
    let hosted_supported =
        normalize_provider_id(provider) == "openai" && capabilities.web_search == Some(true);
    let permission_allows = web_search_is_unconditionally_allowed(permissions);
    match config.execution {
        WebSearchExecution::Local => Ok(WebSearchExecution::Local),
        WebSearchExecution::Auto if hosted_supported && permission_allows => Ok(WebSearchExecution::Hosted),
        WebSearchExecution::Auto => Ok(WebSearchExecution::Local),
        WebSearchExecution::Hosted if !hosted_supported => Err(Error::Config(format!(
            "web.search.execution=hosted requires built-in openai and explicit web_search capability; provider `{provider}` is not eligible"
        ))),
        WebSearchExecution::Hosted if !permission_allows => Err(Error::Config(
            "web.search.execution=hosted requires permissions that unconditionally allow every WebSearch query".into(),
        )),
        WebSearchExecution::Hosted => Ok(WebSearchExecution::Hosted),
    }
}

pub fn web_search_settings_value(options: &RunOptions, cwd: &Path) -> Result<Value> {
    let loaded = load_run_config(options, cwd)?;
    let mut value = serde_json::to_value(&loaded.config.web.search)?;
    value["credentials"] = json!({
        "exa": credential_status(&loaded.env, "EXA_API_KEY"),
        "parallel": credential_status(&loaded.env, "PARALLEL_API_KEY"),
        "brave": credential_status(&loaded.env, "BRAVE_SEARCH_API_KEY"),
        "searxng": credential_status(&loaded.env, "SEARXNG_URL"),
    });
    Ok(value)
}

pub fn update_global_web_search_settings(
    home: &Path,
    search: Value,
    credentials: BTreeMap<String, String>,
) -> Result<Value> {
    let parsed = parse_web_config(&json!({"search": search}))?;
    let config_path = home.join(CONFIG_FILE_NAME);
    let mut document = load_toml_config_file(&config_path, false)?;
    let root = document
        .as_object_mut()
        .ok_or_else(|| Error::Config("global config must be an object".into()))?;
    root.insert("web".into(), json!({"search": parsed.search.clone()}));
    write_toml_config_file(&config_path, &document)?;
    if !credentials.is_empty() {
        update_search_dotenv(&home.join(".env"), credentials)?;
    }
    Ok(serde_json::to_value(parsed.search)?)
}

fn credential_status(env: &BTreeMap<String, String>, key: &str) -> &'static str {
    if env.get(key).is_some_and(|value| !value.trim().is_empty()) {
        "present"
    } else {
        "missing"
    }
}

fn update_search_dotenv(path: &Path, credentials: BTreeMap<String, String>) -> Result<()> {
    let allowed = BTreeSet::from([
        "EXA_API_KEY",
        "PARALLEL_API_KEY",
        "BRAVE_SEARCH_API_KEY",
        "SEARXNG_URL",
    ]);
    if let Some(key) = credentials
        .keys()
        .find(|key| !allowed.contains(key.as_str()))
    {
        return Err(Error::Config(format!(
            "unsupported web search credential `{key}`"
        )));
    }
    let existing = fs::read_to_string(path).unwrap_or_default();
    let mut lines = existing.lines().map(str::to_owned).collect::<Vec<_>>();
    for (key, value) in credentials {
        if value.contains(['\n', '\r']) {
            return Err(Error::Config(format!("{key} must be a single line")));
        }
        let replacement = format!("{key}={}", dotenv_quote(&value));
        if let Some(line) = lines
            .iter_mut()
            .find(|line| line.trim_start().starts_with(&format!("{key}=")))
        {
            *line = replacement;
        } else if !value.is_empty() {
            lines.push(replacement);
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        },
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn dotenv_quote(value: &str) -> String {
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-._~:/".contains(&byte))
    {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

pub(crate) fn hosted_web_search_value(config: &WebSearchConfig) -> Value {
    let mut value = json!({
        "search_context_size": match config.context_size { WebSearchContextSize::Low => "low", WebSearchContextSize::Medium => "medium", WebSearchContextSize::High => "high" },
        "external_web_access": config.external_access == WebSearchExternalAccess::Live,
        "return_token_budget": match config.return_token_budget { WebSearchTokenBudget::Default => "default", WebSearchTokenBudget::Unlimited => "unlimited" },
        "content_types": config.content_types.iter().map(|value| match value { WebSearchContentType::Text => "text", WebSearchContentType::Image => "image" }).collect::<Vec<_>>(),
        "image": { "max_results": config.image.max_results, "caption": config.image.caption },
    });
    if !config.allowed_domains.is_empty() || !config.blocked_domains.is_empty() {
        value["filters"] = json!({
            "allowed_domains": config.allowed_domains,
            "blocked_domains": config.blocked_domains,
        });
    }
    let location = &config.location;
    if [
        &location.country,
        &location.region,
        &location.city,
        &location.timezone,
    ]
    .iter()
    .any(|value| !value.is_empty())
    {
        value["user_location"] = json!({
            "type": "approximate", "country": location.country, "region": location.region,
            "city": location.city, "timezone": location.timezone,
        });
    }
    value
}

pub(crate) fn web_search_is_unconditionally_allowed(config: &PermissionConfig) -> bool {
    fn profile(name: &str, config: &PermissionConfig, seen: &mut BTreeSet<String>) -> bool {
        match name {
            ":workspace" | ":danger-full-access" => true,
            ":read-only" => false,
            _ if !seen.insert(name.to_string()) => false,
            _ => {
                let Some(rule_profile) = config.profiles.get(name) else {
                    return false;
                };
                if !rule_profile.web_search_queries.is_empty() {
                    return rule_profile.web_search_queries.len() == 1
                        && rule_profile.web_search_queries.get("*")
                            == Some(&PermissionAccess::Allow);
                }
                rule_profile
                    .extends
                    .as_deref()
                    .is_some_and(|parent| profile(parent, config, seen))
            }
        }
    }
    profile(&config.default_permissions, config, &mut BTreeSet::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults_and_full_search_config() {
        assert_eq!(parse_web_config(&json!({})).unwrap(), WebConfig::default());
        let web = parse_web_config(&json!({"search": {
            "execution": "local", "backend": "exa", "external_access": "cached",
            "context_size": "high", "return_token_budget": "unlimited",
            "content_types": ["image", "text"], "allowed_domains": ["example.com"],
            "background_storage_acknowledged": true,
            "location": {"country": "US"}, "image": {"max_results": 5, "caption": false}
        }}))
        .unwrap();
        assert_eq!(web.search.execution, WebSearchExecution::Local);
        assert_eq!(web.search.backend, WebSearchBackend::Exa);
        assert_eq!(web.search.image.max_results, 5);
    }

    #[test]
    fn rejects_unacknowledged_unlimited_and_unknown_keys() {
        assert!(
            parse_web_config(&json!({"search": {"return_token_budget": "unlimited"}}))
                .unwrap_err()
                .to_string()
                .contains("acknowledged")
        );
        assert!(
            parse_web_config(&json!({"search": {"vendor_option": true}}))
                .unwrap_err()
                .to_string()
                .contains("vendor_option")
        );
    }

    #[test]
    fn resolves_hosted_only_with_capability_and_static_permission() {
        let config = WebSearchConfig::default();
        let capabilities = ModelCapabilities {
            web_search: Some(true),
            ..Default::default()
        };
        assert_eq!(
            resolve_web_search_execution(
                &config,
                "openai",
                &capabilities,
                &PermissionConfig::default()
            )
            .unwrap(),
            WebSearchExecution::Hosted
        );
        assert_eq!(
            resolve_web_search_execution(
                &config,
                "custom",
                &capabilities,
                &PermissionConfig::default()
            )
            .unwrap(),
            WebSearchExecution::Local
        );
        let permissions = PermissionConfig {
            default_permissions: ":read-only".into(),
            ..Default::default()
        };
        assert_eq!(
            resolve_web_search_execution(&config, "openai", &capabilities, &permissions).unwrap(),
            WebSearchExecution::Local
        );
    }

    #[test]
    fn global_settings_store_credentials_only_in_dotenv() {
        let temp = tempfile::tempdir().unwrap();
        let search = serde_json::to_value(WebSearchConfig::default()).unwrap();
        let returned = update_global_web_search_settings(
            temp.path(),
            search,
            BTreeMap::from([("BRAVE_SEARCH_API_KEY".into(), "secret value".into())]),
        )
        .unwrap();
        assert!(returned.get("credentials").is_none());
        let config = fs::read_to_string(temp.path().join(CONFIG_FILE_NAME)).unwrap();
        assert!(!config.contains("secret value"));
        let dotenv = fs::read_to_string(temp.path().join(".env")).unwrap();
        assert!(dotenv.contains("BRAVE_SEARCH_API_KEY="));
    }
}
