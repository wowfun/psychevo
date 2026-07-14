#[allow(unused_imports)]
use super::*;
pub(crate) fn model_selection_from_raw(
    raw: &str,
    configured_keys: &HashSet<String>,
    provider_override: Option<String>,
    reasoning_effort: Option<String>,
) -> ModelSelection {
    let raw = raw.trim();
    let mut selection = ModelSelection {
        id: (!raw.is_empty()).then_some(raw.to_string()),
        provider: provider_override,
        reasoning_effort,
    };
    if selection.provider.is_none()
        && let Some((provider, model)) = raw.split_once('/')
    {
        let normalized = normalize_provider_id(provider);
        if configured_keys.contains(&normalized) || built_in_provider(&normalized).is_some() {
            selection.provider = Some(normalized);
            selection.id = (!model.trim().is_empty()).then_some(model.trim().to_string());
        }
    }
    selection
}

pub(crate) fn parse_model_override(raw: Option<&String>) -> Result<ModelSelection> {
    let Some(raw) = raw else {
        return Ok(ModelSelection::default());
    };
    let raw = raw.trim();
    let Some((provider, model)) = raw.split_once('/') else {
        return Err(Error::Config(
            "model override must use provider/model form".to_string(),
        ));
    };
    let provider = provider.trim();
    let model = model.trim();
    if provider.is_empty() || model.is_empty() {
        return Err(Error::Config(
            "model override must use provider/model form".to_string(),
        ));
    }
    Ok(ModelSelection {
        id: Some(model.to_string()),
        provider: Some(normalize_provider_id(provider)),
        reasoning_effort: None,
    })
}

pub(crate) fn resolve_run_provider(
    options: &RunOptions,
    loaded: &LoadedRunConfig,
) -> Result<ResolvedRunProvider> {
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
            let (model, reasoning_effort) = model_for_provider(
                candidate,
                &cli_model,
                &loaded.config.model,
                &env_model,
                loaded.config.provider.get(*candidate),
            );
            if let Ok(resolved) =
                resolve_one_provider(candidate, model, reasoning_effort, options, loaded, true)
            {
                return Ok(resolved);
            }
        }
        return Err(Error::Config(
            "auto provider could not find usable credentials and model".to_string(),
        ));
    }

    let (model, reasoning_effort) = model_for_provider(
        &provider,
        &cli_model,
        &loaded.config.model,
        &env_model,
        loaded.config.provider.get(&provider),
    );
    resolve_one_provider(&provider, model, reasoning_effort, options, loaded, false)
}

pub(crate) fn resolve_compression_config(
    options: &RunOptions,
    loaded: &LoadedRunConfig,
    current: &ResolvedRunProvider,
) -> Result<ResolvedCompressionConfig> {
    if let Some(provider) = resolve_auxiliary_task_provider(
        &loaded.config.auxiliary.compression,
        options,
        loaded,
        current,
    )? {
        return Ok(ResolvedCompressionConfig {
            model_configured: true,
            provider,
        });
    }

    let compression = &loaded.config.compression;
    let provider = if compression.model_configured {
        let inferred_provider = compression
            .model
            .id
            .as_deref()
            .and_then(|model| infer_provider_for_model(&loaded.config, model));
        let provider = compression
            .model
            .provider
            .clone()
            .or(inferred_provider)
            .unwrap_or_else(|| current.provider.clone());
        let model = compression
            .model
            .id
            .clone()
            .unwrap_or_else(|| current.model.clone());
        let reasoning_effort = compression
            .reasoning_effort
            .clone()
            .or_else(|| compression.model.reasoning_effort.clone());
        resolve_one_provider(
            &provider,
            Some(model),
            reasoning_effort,
            options,
            loaded,
            false,
        )?
    } else {
        let mut provider = current.clone();
        if let Some(reasoning_effort) = &compression.reasoning_effort {
            provider.reasoning_effort = enabled_reasoning_effort(Some(reasoning_effort.clone()))?;
        }
        provider
    };
    Ok(ResolvedCompressionConfig {
        model_configured: compression.model_configured,
        provider,
    })
}

pub(crate) fn resolve_title_generation_provider(
    options: &RunOptions,
    loaded: &LoadedRunConfig,
    current: &ResolvedRunProvider,
) -> Result<ResolvedRunProvider> {
    resolve_auxiliary_task_provider(
        &loaded.config.auxiliary.title_generation,
        options,
        loaded,
        current,
    )
    .map(|provider| provider.unwrap_or_else(|| current.clone()))
}

pub(crate) fn resolve_auxiliary_task_provider(
    task: &AuxiliaryTaskConfig,
    options: &RunOptions,
    loaded: &LoadedRunConfig,
    current: &ResolvedRunProvider,
) -> Result<Option<ResolvedRunProvider>> {
    if !task.model_configured {
        return Ok(None);
    }
    let inferred_provider = task
        .model
        .id
        .as_deref()
        .and_then(|model| infer_provider_for_model(&loaded.config, model));
    let provider = task
        .model
        .provider
        .clone()
        .or_else(|| task.provider.clone())
        .or(inferred_provider)
        .unwrap_or_else(|| current.provider.clone());
    let model = task
        .model
        .id
        .clone()
        .unwrap_or_else(|| current.model.clone());
    let reasoning_effort = task.model.reasoning_effort.clone();
    resolve_one_provider(
        &provider,
        Some(model),
        reasoning_effort,
        options,
        loaded,
        false,
    )
    .map(Some)
}

pub(crate) fn model_for_provider(
    provider: &str,
    cli_model: &ModelSelection,
    config_model: &ModelSelection,
    env_model: &ModelSelection,
    config_entry: Option<&ConfigProviderEntry>,
) -> (Option<String>, Option<String>) {
    for selection in [cli_model, config_model, env_model] {
        if let Some(id) = &selection.id
            && selection
                .provider
                .as_deref()
                .is_none_or(|selected_provider| selected_provider == provider)
        {
            let reasoning_effort = selection.reasoning_effort.clone().or_else(|| {
                config_model_entry(config_entry, id)
                    .and_then(|entry| entry.reasoning_effort.clone())
            });
            return (Some(id.clone()), reasoning_effort);
        }
    }
    let model = unique_config_model(config_entry);
    let reasoning_effort = model
        .as_deref()
        .and_then(|model| config_model_entry(config_entry, model))
        .and_then(|entry| entry.reasoning_effort.clone());
    (model, reasoning_effort)
}

pub(crate) fn resolve_one_provider(
    provider: &str,
    explicit_model: Option<String>,
    explicit_reasoning_effort: Option<String>,
    options: &RunOptions,
    loaded: &LoadedRunConfig,
    skip_missing: bool,
) -> Result<ResolvedRunProvider> {
    let provider = normalize_provider_id(provider);
    let config_entry = loaded.config.provider.get(&provider);
    let built_in = built_in_provider(&provider);
    if built_in.is_none() && config_entry.is_none() {
        return Err(Error::Config(format!("unknown provider: {provider}")));
    }
    let model = explicit_model
        .or_else(|| unique_config_model(config_entry))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::Config(format!("provider {provider} requires a model")))?;
    let model_config = config_model_entry(config_entry, &model);
    let base_url = provider_base_url(&provider, config_entry, &loaded.env)
        .ok_or_else(|| Error::Config(format!("provider {provider} requires a base_url")))?;
    let metadata = resolve_model_metadata_cache_first(
        &provider,
        &model,
        Some(&base_url),
        model_config,
        &loaded.env,
    );
    let reasoning_effort = enabled_reasoning_effort(first_string([
        options.reasoning_effort.clone(),
        explicit_reasoning_effort,
        model_config.and_then(|entry| entry.reasoning_effort.clone()),
    ]))?;
    let reasoning_effort = if metadata.capabilities.reasoning == Some(false) {
        None
    } else {
        reasoning_effort
    };

    let explicit_no_auth = config_entry.is_some_and(|entry| entry.no_auth);
    let api_key_env = provider_api_key_env(&provider, config_entry);
    let allow_no_auth = explicit_no_auth
        || built_in.is_some_and(|provider| provider.allow_no_auth)
        || is_loopback_base_url(&base_url);
    let api_key = api_key_env
        .as_deref()
        .and_then(|key| env_value(&loaded.env, key))
        .or_else(|| allow_no_auth.then(String::new));
    let Some(api_key) = api_key else {
        if skip_missing {
            return Err(Error::Config("missing credentials".to_string()));
        }
        return Err(Error::Config(format!(
            "provider {provider} requires credentials{}",
            api_key_env
                .as_ref()
                .map(|key| format!(" in {key}"))
                .unwrap_or_default()
        )));
    };

    Ok(ResolvedRunProvider {
        provider: provider.clone(),
        display_label: provider_label(&provider, config_entry),
        model,
        base_url,
        api_key_env,
        api_key,
        reasoning_effort,
        context_limit: metadata.context_limit(),
        metadata,
    })
}
