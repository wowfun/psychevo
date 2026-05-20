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
    if let Some(compression) = object.get("compression") {
        config.compression = parse_compression_config(compression, &configured_keys)?;
    }
    if let Some(permissions) = object.get("permissions") {
        config.permissions = parse_permission_config(permissions)?;
    }
    Ok(config)
}

fn parse_compression_config(
    value: &Value,
    configured_keys: &HashSet<String>,
) -> Result<CompressionConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("compression must be an object".to_string()))?;
    let mut config = CompressionConfig::default();
    if let Some(enabled) = optional_bool_field(object, "enabled")? {
        config.enabled = enabled;
    }
    if let Some(auto) = optional_bool_field(object, "auto")? {
        config.auto = auto;
    }
    if let Some(threshold) = optional_f64_field(object, "threshold_percent")? {
        if !(0.0..=100.0).contains(&threshold) || threshold == 0.0 {
            return Err(Error::Config(
                "compression.threshold_percent must be greater than 0 and at most 100".to_string(),
            ));
        }
        config.threshold_percent = threshold;
    }
    if let Some(reserve) = optional_u64_field(object, "reserve_tokens")? {
        config.reserve_tokens = reserve;
    }
    if let Some(keep_recent) = optional_u64_field(object, "keep_recent_tokens")? {
        if keep_recent == 0 {
            return Err(Error::Config(
                "compression.keep_recent_tokens must be greater than 0".to_string(),
            ));
        }
        config.keep_recent_tokens = keep_recent;
    }
    if let Some(model) = object.get("model") {
        config.model = parse_model_selection(model, configured_keys)?;
        config.model_configured = true;
    }
    config.reasoning_effort =
        validate_reasoning_effort(optional_string_field(object, "reasoning_effort")?)?;
    Ok(config)
}

fn parse_permission_config(value: &Value) -> Result<PermissionConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("permissions must be an object".to_string()))?;
    let approval_mode = optional_string_alias_field(object, "approval_mode", "approvalMode")?
        .map(|value| {
            ApprovalMode::parse(&value).ok_or_else(|| {
                Error::Config("permissions.approval_mode must be manual or smart".to_string())
            })
        })
        .transpose()?;
    let permission_mode = optional_string_alias_field(object, "permission_mode", "permissionMode")?
        .map(|value| {
            PermissionMode::parse(&value).ok_or_else(|| {
                Error::Config(
                    "permissions.permission_mode must be default, acceptEdits, dontAsk, or bypassPermissions"
                        .to_string(),
                )
            })
        })
        .transpose()?;
    Ok(PermissionConfig {
        approval_mode,
        permission_mode,
        smart_model: optional_string_alias_field(object, "smart_model", "smartModel")?,
        allow: string_array_field(object, "allow", "permissions.allow")?,
        ask: string_array_field(object, "ask", "permissions.ask")?,
        deny: string_array_field(object, "deny", "permissions.deny")?,
    })
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
        metadata: parse_config_model_metadata(object)?,
    })
}

fn parse_config_model_metadata(object: &serde_json::Map<String, Value>) -> Result<ModelMetadata> {
    if object.contains_key("context_limit") {
        return Err(Error::Config(
            "context_limit is no longer supported; use limit.context".to_string(),
        ));
    }
    let mut metadata = ModelMetadata::default();
    if let Some(limit) = object.get("limit") {
        metadata.limits = parse_model_limits(limit)?;
    }
    if let Some(cost) = object.get("cost") {
        metadata.cost = Some(parse_model_cost(cost)?);
    }
    metadata.capabilities.reasoning = optional_bool_field(object, "reasoning")?;
    metadata.capabilities.tool_call = optional_bool_field(object, "tool_call")?;
    metadata.capabilities.developer_role = optional_bool_field(object, "developer_role")?;
    metadata.capabilities.temperature = optional_bool_field(object, "temperature")?;
    metadata.capabilities.attachment = optional_bool_field(object, "attachment")?;
    metadata.capabilities.structured_output = optional_bool_field(object, "structured_output")?;
    metadata.capabilities.interleaved = object.get("interleaved").cloned();
    if let Some(modalities) = object.get("modalities") {
        let modalities = modalities
            .as_object()
            .ok_or_else(|| Error::Config("modalities must be an object".to_string()))?;
        metadata.capabilities.input_modalities =
            string_array_field(modalities, "input", "modalities.input")?;
        metadata.capabilities.output_modalities =
            string_array_field(modalities, "output", "modalities.output")?;
    }
    if metadata.limits.context.is_some()
        || metadata.limits.input.is_some()
        || metadata.limits.output.is_some()
        || metadata.cost.is_some()
        || metadata.capabilities.reasoning.is_some()
        || metadata.capabilities.tool_call.is_some()
        || metadata.capabilities.developer_role.is_some()
        || metadata.capabilities.temperature.is_some()
        || metadata.capabilities.attachment.is_some()
        || metadata.capabilities.structured_output.is_some()
        || metadata.capabilities.interleaved.is_some()
        || !metadata.capabilities.input_modalities.is_empty()
        || !metadata.capabilities.output_modalities.is_empty()
    {
        metadata.source = Some("config".to_string());
    }
    Ok(metadata)
}

fn parse_model_limits(value: &Value) -> Result<ModelLimits> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("limit must be an object".to_string()))?;
    Ok(ModelLimits {
        context: optional_u64_field(object, "context")?,
        input: optional_u64_field(object, "input")?,
        output: optional_u64_field(object, "output")?,
    })
}

fn parse_model_cost(value: &Value) -> Result<ModelCost> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("cost must be an object".to_string()))?;
    Ok(ModelCost {
        input: optional_f64_field(object, "input")?,
        output: optional_f64_field(object, "output")?,
        cache_read: optional_f64_field(object, "cache_read")?,
        cache_write: optional_f64_field(object, "cache_write")?,
        context_over_200k: object
            .get("context_over_200k")
            .map(parse_model_cost_tier)
            .transpose()?,
        source: Some("config".to_string()),
    })
}

fn parse_model_cost_tier(value: &Value) -> Result<ModelCostTier> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("cost.context_over_200k must be an object".to_string()))?;
    Ok(ModelCostTier {
        input: optional_f64_field(object, "input")?,
        output: optional_f64_field(object, "output")?,
        cache_read: optional_f64_field(object, "cache_read")?,
        cache_write: optional_f64_field(object, "cache_write")?,
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

fn optional_string_alias_field(
    object: &serde_json::Map<String, Value>,
    primary: &str,
    alias: &str,
) -> Result<Option<String>> {
    match optional_string_field(object, primary)? {
        Some(value) => Ok(Some(value)),
        None => optional_string_field(object, alias),
    }
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

fn optional_f64_field(object: &serde_json::Map<String, Value>, key: &str) -> Result<Option<f64>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .ok_or_else(|| Error::Config(format!("{key} must be a non-negative number")))
        })
        .transpose()
}

fn optional_bool_field(object: &serde_json::Map<String, Value>, key: &str) -> Result<Option<bool>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| Error::Config(format!("{key} must be a boolean")))
        })
        .transpose()
}

fn string_array_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Vec<String>> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| Error::Config(format!("{path} must be an array")))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| Error::Config(format!("{path} entries must be strings")))
        })
        .collect()
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
