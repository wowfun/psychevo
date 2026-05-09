fn parse_run_config(value: Value) -> Result<RunConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let mut config = RunConfig::default();
    let configured_keys = object
        .get("provider")
        .and_then(Value::as_object)
        .map(|providers| {
            providers
                .keys()
                .map(|key| normalize_provider_id(key))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    if let Some(model) = object.get("model") {
        config.model = parse_model_selection(model, &configured_keys)?;
    }
    if let Some(providers) = object.get("provider") {
        let providers = providers
            .as_object()
            .ok_or_else(|| Error::Config("provider must be an object".to_string()))?;
        for (key, entry) in providers {
            let provider_id = normalize_provider_id(key);
            config
                .provider
                .insert(provider_id, parse_config_provider_entry(key, entry)?);
        }
    }
    Ok(config)
}

fn parse_model_selection(
    value: &Value,
    configured_keys: &HashSet<String>,
) -> Result<ModelSelection> {
    match value {
        Value::String(raw) => Ok(model_selection_from_raw(raw, configured_keys, None, None)),
        Value::Object(object) => {
            let id = optional_string_field(object, "id")?;
            let provider = optional_string_field(object, "provider")?
                .map(|provider| normalize_provider_id(&provider));
            let reasoning_effort =
                validate_reasoning_effort(optional_string_field(object, "reasoning_effort")?)?;
            if let Some(id) = id {
                Ok(model_selection_from_raw(
                    &id,
                    configured_keys,
                    provider,
                    reasoning_effort,
                ))
            } else {
                Err(Error::Config("model object requires id".to_string()))
            }
        }
        _ => Err(Error::Config(
            "model must be a string or object".to_string(),
        )),
    }
}

fn parse_config_provider_entry(name: &str, value: &Value) -> Result<ConfigProviderEntry> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("provider.{name} must be an object")))?;
    let mut entry = ConfigProviderEntry {
        label: optional_string_field(object, "label")?,
        ..Default::default()
    };
    if let Some(options) = object.get("options") {
        let options = options
            .as_object()
            .ok_or_else(|| Error::Config(format!("provider.{name}.options must be an object")))?;
        if options.contains_key("api_key") || options.contains_key("apiKey") {
            return Err(Error::Config(format!(
                "provider.{name}.options must not contain raw API keys"
            )));
        }
        entry.options.base_url = optional_string_field(options, "base_url")?;
        entry.options.api_key_env = optional_string_field(options, "api_key_env")?;
    }
    if let Some(models) = object.get("models") {
        let models = models
            .as_object()
            .ok_or_else(|| Error::Config(format!("provider.{name}.models must be an object")))?;
        for (model_id, model_value) in models {
            entry.models.insert(
                model_id.clone(),
                parse_config_model_entry(name, model_id, model_value)?,
            );
        }
    }
    Ok(entry)
}

fn parse_config_model_entry(
    provider_name: &str,
    model_id: &str,
    value: &Value,
) -> Result<ConfigModelEntry> {
    if value.is_null() {
        return Ok(ConfigModelEntry::default());
    }
    let object = value.as_object().ok_or_else(|| {
        Error::Config(format!(
            "provider.{provider_name}.models.{model_id} must be an object"
        ))
    })?;
    Ok(ConfigModelEntry {
        reasoning_effort: validate_reasoning_effort(optional_string_field(
            object,
            "reasoning_effort",
        )?)?,
        context_limit: optional_u64_field(object, "context_limit")?,
    })
}

fn optional_string_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_str()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| Error::Config(format!("{key} must be a non-empty string")))
        })
        .transpose()
}

fn optional_u64_field(object: &serde_json::Map<String, Value>, key: &str) -> Result<Option<u64>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(|| Error::Config(format!("{key} must be a positive integer")))
        })
        .transpose()
}

fn validate_reasoning_effort(value: Option<String>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if REASONING_EFFORT_VALUES.contains(&value.as_str()) {
        Ok(Some(value))
    } else {
        Err(Error::Config(format!(
            "reasoning_effort must be one of {}",
            REASONING_EFFORT_VALUES.join(", ")
        )))
    }
}

fn enabled_reasoning_effort(value: Option<String>) -> Result<Option<String>> {
    match validate_reasoning_effort(value)? {
        Some(value) if value == "none" => Ok(None),
        value => Ok(value),
    }
}
