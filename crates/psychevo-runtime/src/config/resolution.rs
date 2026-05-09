fn model_selection_from_raw(
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

fn parse_model_override(raw: Option<&String>) -> Result<ModelSelection> {
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

fn model_for_provider(
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

fn resolve_one_provider(
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
    let reasoning_effort = enabled_reasoning_effort(first_string([
        options.reasoning_effort.clone(),
        explicit_reasoning_effort,
        config_model_entry(config_entry, &model).and_then(|entry| entry.reasoning_effort.clone()),
    ]))?;
    let context_limit = config_model_entry(config_entry, &model)
        .and_then(|entry| entry.context_limit)
        .or_else(|| built_in_context_limit(&provider, &model));
    let base_url = first_string([
        config_entry.and_then(|entry| entry.options.base_url.clone()),
        built_in
            .and_then(|provider| provider.base_url_env)
            .and_then(|key| loaded.env.get(key).cloned())
            .filter(|value| !value.trim().is_empty()),
        built_in.and_then(|provider| provider.base_url.map(str::to_string)),
    ])
    .ok_or_else(|| Error::Config(format!("provider {provider} requires a base_url")))?;

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
    let api_key = api_key_env
        .as_deref()
        .and_then(|key| env_value(&loaded.env, key))
        .or_else(|| {
            let allow_no_auth = built_in.is_some_and(|provider| provider.allow_no_auth)
                || is_loopback_base_url(&base_url);
            allow_no_auth.then(|| "not-needed".to_string())
        });
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
        context_limit,
    })
}
