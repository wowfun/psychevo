fn configured_model_option_view(
    model: &psychevo_runtime::ConfiguredModel,
) -> wire::ModelOptionView {
    let reasoning_supported = model.metadata.capabilities.reasoning;
    wire::ModelOptionView {
        value: format!("{}/{}", model.provider, model.model),
        provider: model.provider.clone(),
        id: model.model.clone(),
        label: None,
        provider_label: Some(model.provider_label.clone()),
        free: configured_model_is_free(model),
        context_limit: model.context_limit,
        reasoning_supported,
        reasoning_efforts: reasoning_efforts_for_model(reasoning_supported),
    }
}

fn model_options_with_cached_catalog(
    state: &WebState,
    options: &RunOptions,
    configured: &[psychevo_runtime::ConfiguredModel],
) -> Vec<wire::ModelOptionView> {
    let mut seen = std::collections::BTreeSet::new();
    let mut views = Vec::new();
    for model in configured {
        let option = configured_model_option_view(model);
        if seen.insert(option.value.clone()) {
            views.push(option);
        }
    }
    for provider in model_catalog_providers(options).unwrap_or_default() {
        let Some(models) = read_cached_model_catalog(&state.inner.home, &provider) else {
            continue;
        };
        for model in models {
            let option = catalog_model_option_view(&provider, model);
            if seen.insert(option.value.clone()) {
                views.push(option);
            }
        }
    }
    views.sort_by(|left, right| left.value.cmp(&right.value));
    views
}

fn reasoning_efforts_for_model(reasoning_supported: Option<bool>) -> Vec<String> {
    if reasoning_supported == Some(false) {
        return vec!["none".to_string()];
    }
    REASONING_EFFORT_VALUES
        .iter()
        .map(|value| (*value).to_string())
        .collect()
}

fn model_settings_value(state: &WebState, cwd: &Path) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(model_settings_result(
        state, cwd,
    )?)?)
}

fn model_settings_result(
    state: &WebState,
    cwd: &Path,
) -> psychevo_runtime::Result<wire::ModelSettingsResult> {
    let options = model_settings_global_options(state, cwd);
    let selected_model = selected_configured_model(&options)?;
    let default_reasoning_effort = selected_model
        .as_ref()
        .and_then(|model| model.reasoning_effort.clone());
    let default_model = selected_model.map(|model| format!("{}/{}", model.provider, model.model));
    let configured = configured_models(&options).unwrap_or_default();
    let effective_config = config_show_value(&options, ConfigScope::Effective)?;
    let effective = effective_config.get("value").unwrap_or(&Value::Null);
    let configured_provider_ids = effective
        .get("provider")
        .and_then(Value::as_object)
        .map(|providers| {
            providers
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    let auth = auth_status_value(&options, None)?;
    let mut providers = auth
        .get("providers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| model_provider_view(row, &configured_provider_ids))
        .collect::<Vec<_>>();
    if !providers.iter().any(|provider| provider.id == "custom") {
        providers.push(wire::ModelProviderView {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            built_in: false,
            configured: false,
            base_url: None,
            api_key_env: None,
            credential_status: wire::ModelCredentialStatus::Missing,
            no_auth: false,
            can_fetch_models: false,
            unavailable_reason: Some("requires provider setup".to_string()),
        });
    }
    providers.sort_by(|left, right| {
        provider_sort_key(&left.id)
            .cmp(&provider_sort_key(&right.id))
            .then_with(|| left.label.cmp(&right.label))
    });
    let model_options = model_options_with_cached_catalog(state, &options, &configured);
    Ok(wire::ModelSettingsResult {
        scope: wire::ModelSettingsScope::Global,
        cwd: cwd.display().to_string(),
        default_model,
        default_reasoning_effort,
        providers,
        auxiliary: vec![
            auxiliary_model_assignment_view(effective, "title_generation", "Title generation"),
            auxiliary_model_assignment_view(effective, "compression", "Context compression"),
        ],
        model_options,
    })
}

fn model_settings_global_options(state: &WebState, cwd: &Path) -> RunOptions {
    let mut options = state.run_options(cwd.to_path_buf(), None);
    options.config_path = Some(state.inner.home.join("config.toml"));
    options
}

fn model_provider_view(
    row: &Value,
    configured_provider_ids: &std::collections::BTreeSet<String>,
) -> Option<wire::ModelProviderView> {
    let id = row.get("provider").and_then(Value::as_str)?.to_string();
    let base_url = row
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::to_string);
    let status = match row
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("missing")
    {
        "present" => wire::ModelCredentialStatus::Present,
        "not_required" => wire::ModelCredentialStatus::NotRequired,
        _ => wire::ModelCredentialStatus::Missing,
    };
    let can_fetch_models = base_url.is_some() && status != wire::ModelCredentialStatus::Missing;
    let unavailable_reason = (!can_fetch_models).then(|| {
        row.get("api_key_env")
            .and_then(Value::as_str)
            .map(|key| format!("missing {key}"))
            .unwrap_or_else(|| "requires provider setup".to_string())
    });
    Some(wire::ModelProviderView {
        id: id.clone(),
        label: row
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or(id.as_str())
            .to_string(),
        built_in: is_known_builtin_provider(&id),
        configured: configured_provider_ids.contains(&id),
        base_url,
        api_key_env: row
            .get("api_key_env")
            .and_then(Value::as_str)
            .map(str::to_string),
        credential_status: status,
        no_auth: row.get("no_auth").and_then(Value::as_bool).unwrap_or(false),
        can_fetch_models,
        unavailable_reason,
    })
}

fn auxiliary_model_assignment_view(
    effective: &Value,
    task: &str,
    label: &str,
) -> wire::AuxiliaryModelAssignmentView {
    let task_value = effective
        .get("auxiliary")
        .and_then(|auxiliary| auxiliary.get(task));
    let provider = task_value
        .and_then(|value| value.get("provider"))
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .to_string();
    let model = task_value
        .and_then(|value| value.get("model"))
        .and_then(config_model_value_string)
        .unwrap_or_default();
    let effective_model = if !model.trim().is_empty() && provider != "auto" {
        Some(format!("{provider}/{model}"))
    } else if task == "compression" {
        effective
            .get("compression")
            .and_then(|value| value.get("model"))
            .and_then(config_model_value_string)
    } else {
        None
    };
    let reasoning_effort = task_value
        .and_then(|value| value.get("model"))
        .and_then(config_model_reasoning_effort);
    wire::AuxiliaryModelAssignmentView {
        task: task.to_string(),
        label: label.to_string(),
        provider,
        model,
        reasoning_effort,
        effective_model,
    }
}

fn config_model_value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        Value::Object(object) => object
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

fn config_model_reasoning_effort(value: &Value) -> Option<String> {
    value
        .as_object()
        .and_then(|object| object.get("reasoning_effort"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
        .map(str::to_string)
}

async fn model_provider_catalog_value(
    state: &WebState,
    cwd: &Path,
    params: wire::ModelProviderCatalogParams,
) -> psychevo_runtime::Result<Value> {
    let options = model_settings_global_options(state, cwd);
    let provider_id = normalize_provider_id(&params.provider_id);
    let provider = model_catalog_provider(&options, &provider_id)?
        .ok_or_else(|| Error::Config(format!("unknown provider: {provider_id}")))?;
    let models = fetch_and_cache_model_catalog(&state.inner.home, &provider).await?;
    let models = models
        .into_iter()
        .map(|model| catalog_model_option_view(&provider, model))
        .collect();
    Ok(serde_json::to_value(wire::ModelProviderCatalogResult {
        provider_id: provider.provider,
        models,
    })?)
}

fn catalog_model_option_view(
    provider: &ModelCatalogProvider,
    model: ModelCatalogEntry,
) -> wire::ModelOptionView {
    let reasoning_supported = model.metadata.capabilities.reasoning;
    wire::ModelOptionView {
        provider: provider.provider.clone(),
        value: format!("{}/{}", provider.provider, model.id),
        free: model_catalog_entry_is_free(&provider.provider, &model),
        context_limit: model.context_limit,
        id: model.id,
        label: None,
        provider_label: Some(provider.display_label.clone()),
        reasoning_supported,
        reasoning_efforts: reasoning_efforts_for_model(reasoning_supported),
    }
}

fn model_state_read_value(
    state: &WebState,
    cwd: &Path,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(model_state_result(
        state, cwd, thread_id,
    )?)?)
}

fn model_state_set_value(
    state: &WebState,
    cwd: &Path,
    thread_id: Option<&str>,
    params: wire::ModelStateSetParams,
) -> psychevo_runtime::Result<Value> {
    let (model_spec, provider, model_id) = normalize_provider_qualified_model(&params.model)?;
    let reasoning_effort = normalize_model_state_reasoning_effort(params.reasoning_effort)?;
    let path = ModelState::path_for_home(&state.inner.home);
    let mut model_state = ModelState::load(&path)?;
    let cwd_key = cwd.to_string_lossy().to_string();
    model_state.set_model(&cwd_key, model_spec.clone(), reasoning_effort.clone());
    model_state.save(&path)?;
    if let Some(thread_id) = thread_id {
        let store = state.inner.state.store();
        store.set_session_model(thread_id, &provider, &model_id)?;
        let mut metadata = serde_json::Map::new();
        metadata.insert("model".to_string(), Value::String(model_spec));
        if let Some(reasoning_effort) = reasoning_effort {
            metadata.insert(
                "reasoningEffort".to_string(),
                Value::String(reasoning_effort),
            );
        }
        store.set_session_metadata_field(
            thread_id,
            SESSION_COMPOSER_MODEL_METADATA_KEY,
            Some(Value::Object(metadata)),
        )?;
    }
    Ok(serde_json::to_value(model_state_result(
        state, cwd, thread_id,
    )?)?)
}

fn model_state_result(
    state: &WebState,
    cwd: &Path,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<wire::ModelStateResult> {
    let model_state = ModelState::load(&ModelState::path_for_home(&state.inner.home))?;
    let cwd_key = cwd.to_string_lossy().to_string();
    let session_selection = thread_id
        .map(|thread_id| session_model_state_selection(state, thread_id))
        .transpose()?
        .flatten();
    Ok(wire::ModelStateResult {
        cwd: cwd.display().to_string(),
        thread_id: thread_id.map(str::to_string),
        model: session_selection
            .as_ref()
            .and_then(|selection| selection.model.clone())
            .or_else(|| model_state.model_for(&cwd_key)),
        reasoning_effort: session_selection
            .as_ref()
            .and_then(|selection| selection.reasoning_effort.clone())
            .or_else(|| model_state.reasoning_effort_for(&cwd_key)),
        recent_models: model_state.recent_model_values(),
    })
}

fn normalize_provider_qualified_model(
    value: &str,
) -> psychevo_runtime::Result<(String, String, String)> {
    let value = value.trim();
    let Some((provider, model)) = value.split_once('/') else {
        return Err(Error::Config(
            "model must use provider/model format".to_string(),
        ));
    };
    let provider = normalize_provider_id(provider);
    validate_model_provider_id(&provider)?;
    let model = model.trim();
    if model.is_empty() {
        return Err(Error::Config("model id is required".to_string()));
    }
    Ok((format!("{provider}/{model}"), provider, model.to_string()))
}

fn normalize_model_state_reasoning_effort(
    value: Option<String>,
) -> psychevo_runtime::Result<Option<String>> {
    let reasoning_effort = normalize_reasoning_effort(value);
    if let Some(reasoning_effort) = reasoning_effort.as_deref()
        && !REASONING_EFFORT_VALUES.contains(&reasoning_effort)
    {
        return Err(Error::Config(format!(
            "reasoning_effort must be one of {}",
            REASONING_EFFORT_VALUES.join(", ")
        )));
    }
    Ok(reasoning_effort)
}

fn model_provider_save_value(
    state: &WebState,
    cwd: &Path,
    params: wire::ModelProviderSaveParams,
) -> psychevo_runtime::Result<Value> {
    let provider_id = normalize_provider_id(&params.provider_id);
    validate_model_provider_id(&provider_id)?;
    let label = params.label.trim();
    if label.is_empty() {
        return Err(Error::Config("provider label is required".to_string()));
    }
    let base_url = validate_model_base_url(&params.base_url)?;
    let config_dir = state.inner.home.clone();
    set_config_value(
        config_dir.clone(),
        &format!("provider.{provider_id}.label"),
        json!(label),
    )?;
    set_config_value(
        config_dir.clone(),
        &format!("provider.{provider_id}.options.base_url"),
        json!(base_url),
    )?;
    if params.no_auth {
        if params
            .api_key
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Err(Error::Config(
                "no_auth provider save must not include an API key".to_string(),
            ));
        }
        remove_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.options.api_key_env"),
        )?;
        set_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.options.no_auth"),
            json!(true),
        )?;
    } else {
        let api_key_env = params
            .api_key_env
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| custom_provider_api_key_env(&provider_id));
        validate_model_api_key_env(&api_key_env)?;
        set_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.options.api_key_env"),
            json!(api_key_env),
        )?;
        remove_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.options.no_auth"),
        )?;
        if let Some(api_key) = params
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let options = state.run_options(cwd.to_path_buf(), None);
            set_provider_api_key(&options, config_dir, &provider_id, api_key)?;
        }
    }
    model_settings_value(state, cwd)
}

fn model_assignment_set_value(
    state: &WebState,
    cwd: &Path,
    params: wire::ModelAssignmentSetParams,
) -> psychevo_runtime::Result<Value> {
    let provider = normalize_provider_id(&params.provider);
    validate_model_provider_id(&provider)?;
    let reasoning_effort = assignment_reasoning_effort(params.reasoning_effort.as_deref());
    match params.target {
        wire::ModelAssignmentTarget::Default => {
            let model_spec = format!("{provider}/{}", params.model.trim());
            set_default_model_with_reasoning(
                &state.inner.home,
                cwd,
                true,
                &model_spec,
                reasoning_effort,
            )?;
            Ok(serde_json::to_value(wire::ModelAssignmentSetResult {
                ok: true,
                target: wire::ModelAssignmentTarget::Default,
                task: None,
                provider,
                model: params.model.trim().to_string(),
                reasoning_effort: reasoning_effort.map(str::to_string),
            })?)
        }
        wire::ModelAssignmentTarget::Auxiliary => {
            let task = params
                .task
                .as_deref()
                .ok_or_else(|| Error::Config("auxiliary assignment requires task".to_string()))?;
            set_auxiliary_model_with_reasoning(
                &state.inner.home,
                cwd,
                true,
                task,
                &provider,
                params.model.trim(),
                reasoning_effort,
            )?;
            Ok(serde_json::to_value(wire::ModelAssignmentSetResult {
                ok: true,
                target: wire::ModelAssignmentTarget::Auxiliary,
                task: Some(task.to_string()),
                provider,
                model: params.model.trim().to_string(),
                reasoning_effort: reasoning_effort.map(str::to_string),
            })?)
        }
    }
}

fn assignment_reasoning_effort(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
}

fn validate_model_provider_id(provider_id: &str) -> psychevo_runtime::Result<()> {
    if provider_id == "custom" {
        return Err(Error::Config(
            "custom provider save requires a unique provider id".to_string(),
        ));
    }
    let mut chars = provider_id.chars();
    if matches!(chars.next(), Some('a'..='z' | '0'..='9'))
        && chars.all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '-' | '_'))
    {
        Ok(())
    } else {
        Err(Error::Config(
            "provider id must use lowercase letters, numbers, hyphens, or underscores".to_string(),
        ))
    }
}

fn validate_model_base_url(value: &str) -> psychevo_runtime::Result<String> {
    let value = value.trim().trim_end_matches('/').to_string();
    if value.starts_with("http://") || value.starts_with("https://") {
        Ok(value)
    } else {
        Err(Error::Config(
            "provider base_url must start with http:// or https://".to_string(),
        ))
    }
}

fn validate_model_api_key_env(value: &str) -> psychevo_runtime::Result<()> {
    let mut chars = value.chars();
    if matches!(chars.next(), Some('A'..='Z' | '_'))
        && chars.all(|ch| matches!(ch, 'A'..='Z' | '0'..='9' | '_'))
    {
        Ok(())
    } else {
        Err(Error::Config(
            "provider api_key_env must be a valid environment variable name".to_string(),
        ))
    }
}

fn configured_model_is_free(model: &psychevo_runtime::ConfiguredModel) -> bool {
    let Some(cost) = &model.metadata.cost else {
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

fn is_known_builtin_provider(provider_id: &str) -> bool {
    matches!(
        provider_id,
        "openrouter"
            | "openai"
            | "opencode-zen"
            | "xai"
            | "zai"
            | "deepseek"
            | "dashscope"
            | "xiaomi"
            | "xiaomi-token-plan"
            | "lmstudio"
    )
}

fn provider_sort_key(provider_id: &str) -> (u8, &str) {
    let index = match provider_id {
        "openrouter" => 0,
        "openai" => 1,
        "opencode-zen" => 2,
        "xai" => 3,
        "zai" => 4,
        "deepseek" => 5,
        "dashscope" => 6,
        "xiaomi" => 7,
        "xiaomi-token-plan" => 8,
        "lmstudio" => 9,
        _ => 100,
    };
    (index, provider_id)
}
