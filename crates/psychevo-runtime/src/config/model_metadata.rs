#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) const MODELS_DEV_URL: &str = "https://models.dev/api.json";
pub(crate) const MODELS_DEV_CACHE_FILE: &str = "models_dev_cache.json";
pub(crate) const MODELS_DEV_FETCH_TIMEOUT_SECS: u64 = 15;
pub(crate) const MODELS_DEV_URL_ENV: &str = "PSYCHEVO_MODELS_DEV_URL";

pub(crate) fn resolve_model_metadata_cache_first(
    provider: &str,
    model: &str,
    base_url: Option<&str>,
    config_entry: Option<&ConfigModelEntry>,
    env_map: &BTreeMap<String, String>,
) -> ModelMetadata {
    let mut metadata = built_in_model_metadata(provider, model).unwrap_or_default();
    if let Some(registry) = read_models_dev_cache(env_map)
        && let Some(models_dev) = models_dev_metadata(&registry, provider, model, base_url)
    {
        metadata = merge_model_metadata(metadata, models_dev);
    }
    if let Some(config_entry) = config_entry {
        metadata = merge_model_metadata(metadata, config_entry.metadata.clone());
    }
    metadata
}

pub(crate) fn merge_model_metadata(
    mut base: ModelMetadata,
    overlay: ModelMetadata,
) -> ModelMetadata {
    if overlay.limits.context.is_some() {
        base.limits.context = overlay.limits.context;
    }
    if overlay.limits.input.is_some() {
        base.limits.input = overlay.limits.input;
    }
    if overlay.limits.output.is_some() {
        base.limits.output = overlay.limits.output;
    }
    if overlay.cost.is_some() {
        base.cost = overlay.cost;
    }
    merge_capabilities(&mut base.capabilities, overlay.capabilities);
    if overlay.raw.is_some() {
        base.raw = overlay.raw;
    }
    if overlay.source.is_some() {
        base.source = overlay.source;
    }
    base
}

pub(crate) fn merge_capabilities(base: &mut ModelCapabilities, overlay: ModelCapabilities) {
    if overlay.reasoning.is_some() {
        base.reasoning = overlay.reasoning;
    }
    if overlay.tool_call.is_some() {
        base.tool_call = overlay.tool_call;
    }
    if overlay.developer_role.is_some() {
        base.developer_role = overlay.developer_role;
    }
    if overlay.temperature.is_some() {
        base.temperature = overlay.temperature;
    }
    if overlay.attachment.is_some() {
        base.attachment = overlay.attachment;
    }
    if overlay.structured_output.is_some() {
        base.structured_output = overlay.structured_output;
    }
    if overlay.interleaved.is_some() {
        base.interleaved = overlay.interleaved;
    }
    if !overlay.input_modalities.is_empty() {
        base.input_modalities = overlay.input_modalities;
    }
    if !overlay.output_modalities.is_empty() {
        base.output_modalities = overlay.output_modalities;
    }
}

pub async fn refresh_model_metadata_cache(
    home: PathBuf,
    env_map: BTreeMap<String, String>,
    targets: Vec<ModelMetadataCacheTarget>,
) -> Result<()> {
    if targets.is_empty() {
        return Err(Error::Message("no model metadata targets".to_string()));
    }
    let value = fetch_models_dev_registry(&models_dev_url(&env_map)).await?;
    let value = prune_models_dev_registry(&value, &targets)
        .ok_or_else(|| Error::Message("no matching models.dev metadata".to_string()))?;
    let path = models_dev_cache_path_for_home(&home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_file_name(format!("{MODELS_DEV_CACHE_FILE}.tmp"));
    fs::write(&temp_path, serde_json::to_vec(&value)?)?;
    fs::rename(temp_path, path)?;
    Ok(())
}

pub(crate) async fn fetch_models_dev_registry(url: &str) -> Result<Value> {
    let client = reqwest::Client::new();
    tokio::time::timeout(Duration::from_secs(MODELS_DEV_FETCH_TIMEOUT_SECS), async {
        let response = client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(Error::Message(format!(
                "models.dev returned {}",
                response.status()
            )));
        }
        response.json::<Value>().await.map_err(Error::from)
    })
    .await
    .map_err(|_| {
        Error::Message(format!(
            "models.dev refresh timed out after {MODELS_DEV_FETCH_TIMEOUT_SECS}s"
        ))
    })?
}

pub(crate) fn prune_models_dev_registry(
    registry: &Value,
    targets: &[ModelMetadataCacheTarget],
) -> Option<Value> {
    let providers = registry.as_object()?;
    let mut pruned = serde_json::Map::new();
    for target in targets {
        let Some(provider_key) =
            models_dev_provider_key(providers, &target.provider, target.base_url.as_deref())
        else {
            continue;
        };
        let Some(provider_value) = providers.get(&provider_key) else {
            continue;
        };
        let Some((model_id, model_value)) = models_dev_model_entry(provider_value, &target.model)
        else {
            continue;
        };
        let provider_entry = pruned
            .entry(provider_key)
            .or_insert_with(|| provider_without_models(provider_value));
        let Some(provider_object) = provider_entry.as_object_mut() else {
            continue;
        };
        let models = provider_object
            .entry("models".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Some(models_object) = models.as_object_mut() {
            models_object.insert(model_id, model_value);
        }
    }
    (!pruned.is_empty()).then_some(Value::Object(pruned))
}

pub(crate) fn provider_without_models(provider_value: &Value) -> Value {
    let mut provider = provider_value
        .as_object()
        .cloned()
        .unwrap_or_else(serde_json::Map::new);
    provider.insert("models".to_string(), Value::Object(serde_json::Map::new()));
    Value::Object(provider)
}

pub(crate) fn read_models_dev_cache(env_map: &BTreeMap<String, String>) -> Option<Value> {
    models_dev_cache_path(env_map).and_then(|path| read_json_file(&path))
}

pub(crate) fn models_dev_cache_path(env_map: &BTreeMap<String, String>) -> Option<PathBuf> {
    resolve_psychevo_home(env_map)
        .ok()
        .map(|home| models_dev_cache_path_for_home(&home))
}

pub(crate) fn models_dev_cache_path_for_home(home: &Path) -> PathBuf {
    home.join(MODELS_DEV_CACHE_FILE)
}

pub(crate) fn models_dev_url(env_map: &BTreeMap<String, String>) -> String {
    env_map
        .get(MODELS_DEV_URL_ENV)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(MODELS_DEV_URL)
        .to_string()
}

pub(crate) fn read_json_file(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub(crate) fn models_dev_metadata(
    registry: &Value,
    provider: &str,
    model: &str,
    base_url: Option<&str>,
) -> Option<ModelMetadata> {
    let providers = registry.as_object()?;
    let provider_key = models_dev_provider_key(providers, provider, base_url)?;
    let provider_value = providers.get(&provider_key)?;
    let (_, model_value) = models_dev_model_entry(provider_value, model)?;
    Some(metadata_from_models_dev_model(
        provider_key,
        model_value,
        "models.dev",
    ))
}

pub(crate) fn models_dev_model_entry(
    provider_value: &Value,
    model: &str,
) -> Option<(String, Value)> {
    provider_value
        .get("models")
        .and_then(Value::as_object)
        .and_then(|models| {
            models
                .get(model)
                .map(|value| (model.to_string(), value.clone()))
                .or_else(|| {
                    models
                        .iter()
                        .find(|(id, _)| id.eq_ignore_ascii_case(model))
                        .map(|(id, value)| (id.clone(), value.clone()))
                })
        })
}

pub(crate) fn models_dev_provider_key(
    providers: &serde_json::Map<String, Value>,
    provider: &str,
    base_url: Option<&str>,
) -> Option<String> {
    for candidate in models_dev_provider_candidates(provider) {
        if providers.contains_key(&candidate) {
            return Some(candidate);
        }
    }
    let normalized_base_url = base_url.map(normalize_base_url)?;
    providers.iter().find_map(|(key, value)| {
        value
            .get("api")
            .and_then(Value::as_str)
            .map(normalize_base_url)
            .filter(|api| api == &normalized_base_url)
            .map(|_| key.clone())
    })
}

pub(crate) fn models_dev_provider_candidates(provider: &str) -> Vec<String> {
    let provider = normalize_provider_id(provider);
    let mut candidates = vec![provider.clone()];
    match provider.as_str() {
        "xiaomi-token-plan" | "xiaomi-token-plan-cn" => {
            candidates.push("xiaomi-token-plan-cn".to_string());
        }
        "xiaomi" => candidates.push("xiaomi".to_string()),
        "deepseek" => candidates.push("deepseek".to_string()),
        _ => {}
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

pub(crate) fn normalize_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_ascii_lowercase()
}

pub(crate) fn metadata_from_models_dev_model(
    provider_id: String,
    model_value: Value,
    source: &str,
) -> ModelMetadata {
    let mut metadata = ModelMetadata {
        source: Some(source.to_string()),
        raw: Some(model_value.clone()),
        ..Default::default()
    };
    metadata.limits = parse_metadata_limits(&model_value);
    metadata.cost = parse_metadata_cost(&model_value).map(|mut cost| {
        cost.source = Some(source.to_string());
        cost
    });
    metadata.capabilities = parse_metadata_capabilities(&model_value);
    if metadata.source.as_deref() == Some("models.dev") {
        metadata.source = Some(format!("models.dev:{provider_id}"));
        if let Some(cost) = &mut metadata.cost {
            cost.source = metadata.source.clone();
        }
    }
    metadata
}

pub(crate) fn parse_metadata_limits(value: &Value) -> ModelLimits {
    let limit = value
        .get("limit")
        .or_else(|| value.get("limits"))
        .unwrap_or(value);
    ModelLimits {
        context: u64_from_keys(limit, &["context", "context_window", "context_length"]).or_else(
            || {
                u64_from_keys(
                    value,
                    &[
                        "context_limit",
                        "context_window",
                        "context_length",
                        "max_context_tokens",
                    ],
                )
            },
        ),
        input: u64_from_keys(limit, &["input", "max_input_tokens"])
            .or_else(|| u64_from_keys(value, &["input_limit", "max_input_tokens"])),
        output: u64_from_keys(limit, &["output", "max_output_tokens"])
            .or_else(|| u64_from_keys(value, &["output_limit", "max_output_tokens"])),
    }
}

pub(crate) fn parse_metadata_cost(value: &Value) -> Option<ModelCost> {
    let cost = value.get("cost")?.as_object()?;
    Some(ModelCost {
        input: f64_from_keys(cost, &["input"]),
        output: f64_from_keys(cost, &["output"]),
        cache_read: f64_from_keys(cost, &["cache_read"]),
        cache_write: f64_from_keys(cost, &["cache_write"]),
        context_over_200k: cost
            .get("context_over_200k")
            .and_then(parse_metadata_cost_tier),
        source: None,
    })
}

pub(crate) fn parse_metadata_cost_tier(value: &Value) -> Option<ModelCostTier> {
    let object = value.as_object()?;
    Some(ModelCostTier {
        input: f64_from_keys(object, &["input"]),
        output: f64_from_keys(object, &["output"]),
        cache_read: f64_from_keys(object, &["cache_read"]),
        cache_write: f64_from_keys(object, &["cache_write"]),
    })
}

pub(crate) fn parse_metadata_capabilities(value: &Value) -> ModelCapabilities {
    let mut capabilities = ModelCapabilities {
        reasoning: bool_from_keys(value, &["reasoning"]),
        tool_call: bool_from_keys(value, &["tool_call", "toolcall", "tools"]),
        developer_role: bool_from_keys(value, &["developer_role", "developer"]),
        temperature: bool_from_keys(value, &["temperature"]),
        attachment: bool_from_keys(value, &["attachment", "attachments"]),
        structured_output: bool_from_keys(value, &["structured_output"]),
        interleaved: value.get("interleaved").cloned(),
        ..Default::default()
    };
    if let Some(modalities) = value.get("modalities").and_then(Value::as_object) {
        capabilities.input_modalities = string_vec_from_value(modalities.get("input"));
        capabilities.output_modalities = string_vec_from_value(modalities.get("output"));
    }
    capabilities
}

pub(crate) fn built_in_model_metadata(provider: &str, model: &str) -> Option<ModelMetadata> {
    let provider = normalize_provider_id(provider);
    let lower = model.to_lowercase();

    let context = match provider.as_str() {
        "deepseek"
            if lower.contains("deepseek-v4")
                || lower.contains("deepseek-chat")
                || lower.contains("deepseek-reasoner") =>
        {
            1_000_000
        }
        "openai" if lower.contains("gpt-4.1") || lower.contains("gpt-4o") => 128_000,
        _ => return None,
    };
    Some(built_in_limits_metadata(context, None))
}

pub(crate) fn built_in_limits_metadata(context: u64, output: Option<u64>) -> ModelMetadata {
    ModelMetadata {
        limits: ModelLimits {
            context: Some(context),
            output,
            ..Default::default()
        },
        source: Some("built-in".to_string()),
        ..Default::default()
    }
}

pub(crate) fn u64_from_keys(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
}

pub(crate) fn f64_from_keys(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value >= 0.0)
    })
}

pub(crate) fn bool_from_keys(value: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_bool))
}

pub(crate) fn string_vec_from_value(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
