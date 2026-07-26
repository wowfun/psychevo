#[allow(unused_imports)]
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PROVIDER_MODELS_CACHE_FILE: &str = "provider_models_cache.json";
pub const PROVIDER_MODELS_CACHE_VERSION: u32 = 1;

fn configured_model_name(
    model_config: Option<&ConfigModelEntry>,
    metadata: &ModelMetadata,
) -> Option<String> {
    model_config
        .and_then(|entry| entry.name.as_deref())
        .or_else(|| {
            metadata
                .raw
                .as_ref()
                .and_then(|raw| raw.get("name"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ProviderModelsCacheFile {
    version: u32,
    #[serde(default)]
    providers: BTreeMap<String, ProviderModelsCacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderModelsCacheEntry {
    fingerprint: String,
    base_url: String,
    fetched_at_ms: i64,
    models: Vec<ModelCatalogEntry>,
}

pub fn configured_models(options: &RunOptions) -> Result<Vec<ConfiguredModel>> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let cli_model = parse_model_override(options.model.as_ref())?;
    let env_model = loaded
        .env
        .get("PSYCHEVO_INFERENCE_MODEL")
        .map(|value| {
            parse_model_selection(
                &Value::String(value.clone()),
                &loaded.config.provider.keys().cloned().collect(),
            )
        })
        .transpose()?
        .unwrap_or_default();

    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    let mut push_model = |provider: &str,
                          model: &str,
                          reasoning_effort: Option<String>,
                          rows: &mut Vec<ConfiguredModel>| {
        let provider = normalize_provider_id(provider);
        let model = model.trim().to_string();
        if provider.is_empty() || model.is_empty() || !seen.insert(format!("{provider}/{model}")) {
            return;
        }
        let config_entry = loaded.config.provider.get(&provider);
        let model_config = config_model_entry(config_entry, &model);
        let base_url = provider_base_url(&provider, config_entry, &loaded.env);
        let metadata = resolve_model_metadata_cache_first(
            &provider,
            &model,
            base_url.as_deref(),
            model_config,
            &loaded.env,
        );
        let reasoning_effort = if metadata.capabilities.reasoning == Some(false) {
            None
        } else {
            reasoning_effort
        };
        rows.push(ConfiguredModel {
            provider: provider.clone(),
            provider_label: provider_label(&provider, loaded.config.provider.get(&provider)),
            model,
            model_name: configured_model_name(model_config, &metadata),
            reasoning_effort,
            context_limit: metadata.context_limit(),
            metadata,
        });
    };

    for (provider, entry) in &loaded.config.provider {
        for (model, config) in &entry.models {
            push_model(provider, model, config.reasoning_effort.clone(), &mut rows);
        }
    }

    for selection in [&cli_model, &loaded.config.model, &env_model] {
        if let (Some(provider), Some(model)) = (&selection.provider, &selection.id) {
            let reasoning_effort = loaded
                .config
                .provider
                .get(provider)
                .and_then(|entry| config_model_entry(Some(entry), model))
                .and_then(|entry| entry.reasoning_effort.clone())
                .or_else(|| selection.reasoning_effort.clone());
            push_model(provider, model, reasoning_effort, &mut rows);
        }
    }

    rows.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.model.cmp(&right.model))
    });
    Ok(rows)
}

pub fn model_catalog_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if let Some(prefix) = trimmed.strip_suffix("/chat/completions") {
        format!("{prefix}/models")
    } else {
        format!("{trimmed}/models")
    }
}

pub fn model_catalog_providers(options: &RunOptions) -> Result<Vec<ModelCatalogProvider>> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let cli_model = parse_model_override(options.model.as_ref())?;
    let env_model = loaded
        .env
        .get("PSYCHEVO_INFERENCE_MODEL")
        .map(|value| {
            parse_model_selection(
                &Value::String(value.clone()),
                &loaded.config.provider.keys().cloned().collect(),
            )
        })
        .transpose()?
        .unwrap_or_default();

    let mut providers = BTreeSet::new();
    providers.extend(loaded.config.provider.keys().cloned());
    if let Some(provider) = cli_model.provider {
        providers.insert(provider);
    }
    if let Some(provider) = loaded.config.model.provider.clone().or_else(|| {
        loaded
            .config
            .model
            .id
            .as_deref()
            .and_then(|model| infer_provider_for_model(&loaded.config, model))
    }) {
        providers.insert(provider);
    }
    if let Some(provider) = loaded
        .env
        .get("PSYCHEVO_INFERENCE_PROVIDER")
        .map(|value| normalize_provider_id(value))
    {
        providers.insert(provider);
    }
    if let Some(provider) = env_model.provider.or_else(|| {
        env_model
            .id
            .as_deref()
            .and_then(|model| infer_provider_for_model(&loaded.config, model))
    }) {
        providers.insert(provider);
    }

    let mut rows = providers
        .into_iter()
        .filter_map(|provider| catalog_provider_for(&provider, &loaded))
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        left.display_label
            .cmp(&right.display_label)
            .then_with(|| left.provider.cmp(&right.provider))
    });
    Ok(rows)
}

pub fn model_catalog_provider(
    options: &RunOptions,
    provider: &str,
) -> Result<Option<ModelCatalogProvider>> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    Ok(catalog_provider_for(provider, &loaded))
}

pub async fn fetch_model_catalog(
    provider: &ModelCatalogProvider,
) -> Result<Vec<ModelCatalogEntry>> {
    let client = reqwest::Client::new();
    fetch_model_catalog_with_client(provider, &client, MODEL_CATALOG_TIMEOUT).await
}

pub async fn fetch_and_cache_model_catalog(
    home: &Path,
    provider: &ModelCatalogProvider,
) -> Result<Vec<ModelCatalogEntry>> {
    let models = fetch_model_catalog(provider).await?;
    if !models.is_empty() {
        write_cached_model_catalog(home, provider, &models)?;
    }
    Ok(models)
}

pub async fn fetch_model_catalog_with_client(
    provider: &ModelCatalogProvider,
    client: &reqwest::Client,
    timeout: Duration,
) -> Result<Vec<ModelCatalogEntry>> {
    if let Some(reason) = &provider.unavailable_reason {
        return Err(Error::Config(reason.clone()));
    }
    if let Some(missing) = &provider.missing_credentials {
        return Err(Error::Config(format!("missing {missing}")));
    }
    let endpoint = model_catalog_endpoint(&provider.base_url);
    let request = client
        .get(endpoint)
        .header("accept", "application/json")
        .header(
            "user-agent",
            format!("psychevo/{}", env!("CARGO_PKG_VERSION")),
        );
    let request = if let Some(api_key) = &provider.api_key {
        request.bearer_auth(api_key)
    } else {
        request
    };
    let value = tokio::time::timeout(timeout, async move {
        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|err| format!("<failed to read error body: {err}>"));
            return Err(Error::Message(format!(
                "HTTP {status}: {}",
                truncate_error(&body)
            )));
        }
        let value = response.json::<Value>().await?;
        Ok(value)
    })
    .await
    .map_err(|_| Error::Message("timeout".to_string()))??;
    parse_model_catalog_response(&provider.provider, &value)
}

pub fn model_catalog_entry_is_free(provider: &str, entry: &ModelCatalogEntry) -> bool {
    if model_cost_is_free(entry.metadata.cost.as_ref()) {
        return true;
    }
    let provider = normalize_provider_id(provider);
    if provider == "opencode-zen" {
        let id = entry.id.trim().to_lowercase();
        return id.ends_with("-free") || id == "big-pickle";
    }
    false
}

pub fn read_cached_model_catalog(
    home: &Path,
    provider: &ModelCatalogProvider,
) -> Option<Vec<ModelCatalogEntry>> {
    let cache = read_provider_models_cache(home)?;
    let entry = cache
        .providers
        .get(&normalize_provider_id(&provider.provider))?;
    (entry.fingerprint == provider_models_cache_fingerprint(provider)).then(|| {
        entry
            .models
            .iter()
            .cloned()
            .map(sanitize_model_catalog_entry_for_cache)
            .collect()
    })
}

pub fn write_cached_model_catalog(
    home: &Path,
    provider: &ModelCatalogProvider,
    models: &[ModelCatalogEntry],
) -> Result<()> {
    if models.is_empty() {
        return Ok(());
    }
    let mut cache = read_provider_models_cache(home).unwrap_or_default();
    cache.version = PROVIDER_MODELS_CACHE_VERSION;
    cache.providers.insert(
        normalize_provider_id(&provider.provider),
        ProviderModelsCacheEntry {
            fingerprint: provider_models_cache_fingerprint(provider),
            base_url: normalize_provider_base_url_for_cache(&provider.base_url),
            fetched_at_ms: now_ms(),
            models: models
                .iter()
                .cloned()
                .map(sanitize_model_catalog_entry_for_cache)
                .collect(),
        },
    );
    let path = provider_models_cache_path_for_home(home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_file_name(format!("{PROVIDER_MODELS_CACHE_FILE}.tmp"));
    fs::write(&temp_path, serde_json::to_vec_pretty(&cache)?)?;
    fs::rename(temp_path, path)?;
    Ok(())
}

pub fn provider_models_cache_path_for_home(home: &Path) -> PathBuf {
    home.join("cache").join(PROVIDER_MODELS_CACHE_FILE)
}

pub fn provider_models_cache_fingerprint(provider: &ModelCatalogProvider) -> String {
    let auth_marker = if provider.no_auth {
        "no-auth".to_string()
    } else if let Some(api_key) = provider.api_key.as_deref() {
        format!("api-key:{api_key}")
    } else {
        "missing-auth".to_string()
    };
    let mut hasher = Sha256::new();
    for part in [
        normalize_provider_id(&provider.provider),
        normalize_provider_base_url_for_cache(&provider.base_url),
        provider.api_key_env.clone().unwrap_or_default(),
        auth_marker,
    ] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn read_provider_models_cache(home: &Path) -> Option<ProviderModelsCacheFile> {
    let path = provider_models_cache_path_for_home(home);
    let text = fs::read_to_string(path).ok()?;
    let cache: ProviderModelsCacheFile = serde_json::from_str(&text).ok()?;
    (cache.version == PROVIDER_MODELS_CACHE_VERSION).then_some(cache)
}

fn normalize_provider_base_url_for_cache(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/chat/completions")
        .unwrap_or(trimmed)
        .to_string()
}

fn sanitize_model_catalog_entry_for_cache(mut entry: ModelCatalogEntry) -> ModelCatalogEntry {
    entry.metadata.raw = None;
    entry.context_limit = entry
        .context_limit
        .or_else(|| entry.metadata.context_limit());
    entry
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn model_cost_is_free(cost: Option<&ModelCost>) -> bool {
    let Some(cost) = cost else {
        return false;
    };
    let values = [
        cost.input,
        cost.output,
        cost.cache_read,
        cost.cache_write,
        cost.request,
    ];
    values.iter().flatten().any(|value| *value == 0.0)
        && values.iter().flatten().all(|value| *value == 0.0)
}

pub fn selected_configured_model(options: &RunOptions) -> Result<Option<ConfiguredModel>> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let cli_model = parse_model_override(options.model.as_ref())?;
    let env_model = loaded
        .env
        .get("PSYCHEVO_INFERENCE_MODEL")
        .map(|value| {
            parse_model_selection(
                &Value::String(value.clone()),
                &loaded.config.provider.keys().cloned().collect(),
            )
        })
        .transpose()?
        .unwrap_or_default();

    let inferred_config_provider = loaded
        .config
        .model
        .id
        .as_deref()
        .and_then(|model| infer_provider_for_model(&loaded.config, model));
    let inferred_env_provider = env_model
        .id
        .as_deref()
        .and_then(|model| infer_provider_for_model(&loaded.config, model));
    let provider = first_string([
        cli_model.provider.clone(),
        loaded.config.model.provider.clone(),
        inferred_config_provider,
        loaded
            .env
            .get("PSYCHEVO_INFERENCE_PROVIDER")
            .map(|value| normalize_provider_id(value)),
        env_model.provider.clone(),
        inferred_env_provider,
    ])
    .unwrap_or_else(|| "auto".to_string());

    if provider == "auto" {
        for candidate in AUTO_PROVIDER_ORDER {
            if let Some(model) = selected_configured_model_for_provider(
                candidate, &cli_model, &env_model, options, &loaded,
            )? {
                return Ok(Some(model));
            }
        }
        return Ok(None);
    }

    selected_configured_model_for_provider(&provider, &cli_model, &env_model, options, &loaded)
}

pub(crate) fn selected_configured_model_for_provider(
    provider: &str,
    cli_model: &ModelSelection,
    env_model: &ModelSelection,
    options: &RunOptions,
    loaded: &LoadedRunConfig,
) -> Result<Option<ConfiguredModel>> {
    let provider = normalize_provider_id(provider);
    let config_entry = loaded.config.provider.get(&provider);
    if built_in_provider(&provider).is_none() && config_entry.is_none() {
        return Ok(None);
    }
    let (model, explicit_reasoning_effort) = model_for_provider(
        &provider,
        cli_model,
        &loaded.config.model,
        env_model,
        config_entry,
    );
    let Some(model) = model
        .or_else(|| unique_config_model(config_entry))
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    let reasoning_effort = validate_reasoning_effort(first_string([
        options.reasoning_effort.clone(),
        explicit_reasoning_effort,
        config_model_entry(config_entry, &model).and_then(|entry| entry.reasoning_effort.clone()),
    ]))?;
    let model_config = config_model_entry(config_entry, &model);
    let base_url = provider_base_url(&provider, config_entry, &loaded.env);
    let metadata = resolve_model_metadata_cache_first(
        &provider,
        &model,
        base_url.as_deref(),
        model_config,
        &loaded.env,
    );
    let reasoning_effort = if metadata.capabilities.reasoning == Some(false) {
        None
    } else {
        reasoning_effort
    };
    Ok(Some(ConfiguredModel {
        provider: provider.clone(),
        provider_label: provider_label(&provider, config_entry),
        model,
        model_name: configured_model_name(model_config, &metadata),
        reasoning_effort,
        context_limit: metadata.context_limit(),
        metadata,
    }))
}

pub(crate) fn provider_label(provider: &str, config_entry: Option<&ConfigProviderEntry>) -> String {
    if let Some(name) = config_entry.and_then(|entry| entry.name.clone()) {
        return name;
    }
    built_in_provider(provider)
        .map(|entry| entry.name.to_string())
        .unwrap_or_else(|| provider.to_string())
}

pub(crate) fn env_value(env_map: &BTreeMap<String, String>, key: &str) -> Option<String> {
    env_map
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn is_loopback_base_url(base_url: &str) -> bool {
    let value = base_url.to_lowercase();
    value.contains("://localhost")
        || value.contains("://127.0.0.1")
        || value.contains("://0.0.0.0")
        || value.contains("://[::1]")
}
