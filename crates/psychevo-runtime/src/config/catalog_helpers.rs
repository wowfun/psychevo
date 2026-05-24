#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) fn built_in_provider(provider: &str) -> Option<&'static BuiltInProvider> {
    BUILT_IN_PROVIDERS
        .iter()
        .find(|entry| entry.id == normalize_provider_id(provider))
}

pub(crate) fn catalog_provider_for(
    provider: &str,
    loaded: &LoadedRunConfig,
) -> Option<ModelCatalogProvider> {
    let provider = normalize_provider_id(provider);
    let config_entry = loaded.config.provider.get(&provider);
    let built_in = built_in_provider(&provider);
    if built_in.is_none() && config_entry.is_none() {
        return None;
    }
    let display_label = provider_label(&provider, config_entry);
    let base_url = provider_base_url(&provider, config_entry, &loaded.env);
    let api_key_env = first_string([
        config_entry.and_then(|entry| entry.options.api_key_env.clone()),
        built_in.and_then(|provider| {
            provider
                .api_key_envs
                .iter()
                .find(|key| env_value(&loaded.env, key).is_some())
                .or_else(|| provider.api_key_envs.first())
                .map(|key| (*key).to_string())
        }),
    ]);
    let Some(base_url) = base_url else {
        return Some(ModelCatalogProvider {
            provider,
            display_label,
            base_url: String::new(),
            api_key_env,
            missing_credentials: None,
            unavailable_reason: Some("requires base_url".to_string()),
            no_auth: false,
            api_key: None,
        });
    };
    let api_key = api_key_env
        .as_deref()
        .and_then(|key| env_value(&loaded.env, key));
    let no_auth_allowed =
        built_in.is_some_and(|provider| provider.allow_no_auth) || is_loopback_base_url(&base_url);
    let no_auth = api_key.is_none() && no_auth_allowed;
    let missing_credentials = (api_key.is_none() && !no_auth_allowed).then(|| {
        api_key_env
            .clone()
            .unwrap_or_else(|| "credentials".to_string())
    });
    Some(ModelCatalogProvider {
        provider,
        display_label,
        base_url,
        api_key_env,
        missing_credentials,
        unavailable_reason: None,
        no_auth,
        api_key,
    })
}

pub(crate) fn parse_model_catalog_response(
    provider: &str,
    value: &Value,
) -> Result<Vec<ModelCatalogEntry>> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Message("model catalog response missing data".to_string()))?;
    let mut seen = BTreeSet::new();
    let mut models = data
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id").and_then(Value::as_str)?.trim();
            (!id.is_empty()).then_some((id, entry))
        })
        .filter(|(id, _)| seen.insert((*id).to_string()))
        .map(|(id, entry)| {
            let mut metadata = built_in_model_metadata(provider, id).unwrap_or_default();
            let endpoint_metadata =
                metadata_from_models_dev_model(provider.to_string(), entry.clone(), "provider");
            metadata = merge_model_metadata(metadata, endpoint_metadata);
            ModelCatalogEntry {
                id: id.to_string(),
                context_limit: metadata.context_limit(),
                metadata,
            }
        })
        .collect::<Vec<_>>();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(models)
}

pub(crate) fn provider_base_url(
    provider: &str,
    config_entry: Option<&ConfigProviderEntry>,
    env_map: &BTreeMap<String, String>,
) -> Option<String> {
    let built_in = built_in_provider(provider);
    first_string([
        config_entry.and_then(|entry| entry.options.base_url.clone()),
        built_in
            .and_then(|provider| provider.base_url_env)
            .and_then(|key| env_map.get(key).cloned())
            .filter(|value| !value.trim().is_empty()),
        built_in.and_then(|provider| provider.base_url.map(str::to_string)),
    ])
}

pub(crate) fn truncate_error(value: &str) -> String {
    let trimmed = value.trim().replace(['\r', '\n', '\t'], " ");
    if trimmed.chars().count() <= 160 {
        trimmed
    } else {
        let mut out = trimmed.chars().take(157).collect::<String>();
        out.push_str("...");
        out
    }
}

pub(crate) fn normalize_provider_id(provider: &str) -> String {
    let key = provider.trim().to_lowercase();
    match key.as_str() {
        "z.ai" | "z-ai" | "glm" => "zai".to_string(),
        "alibaba" | "qwen" => "dashscope".to_string(),
        "mimo" => "xiaomi".to_string(),
        "x-ai" | "x.ai" | "grok" => "xai".to_string(),
        "lm-studio" | "lm_studio" => "lmstudio".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn first_string(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

pub(crate) fn infer_provider_for_model(config: &RunConfig, model: &str) -> Option<String> {
    let matches = config
        .provider
        .iter()
        .filter_map(|(provider, entry)| {
            entry.models.contains_key(model).then_some(provider.clone())
        })
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches[0].clone())
}

pub(crate) fn unique_config_model(entry: Option<&ConfigProviderEntry>) -> Option<String> {
    let entry = entry?;
    (entry.models.len() == 1).then(|| entry.models.keys().next().expect("one model").clone())
}

pub(crate) fn config_model_entry<'a>(
    entry: Option<&'a ConfigProviderEntry>,
    model: &str,
) -> Option<&'a ConfigModelEntry> {
    entry.and_then(|entry| entry.models.get(model))
}
