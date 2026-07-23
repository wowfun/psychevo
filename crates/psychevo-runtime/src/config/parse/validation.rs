pub(crate) fn parse_host_executables(value: &Value) -> Result<Vec<ExecPolicyHostExecutable>> {
    let values = value.as_array().ok_or_else(|| {
        Error::Config("exec_policy.host_executables must be an array".to_string())
    })?;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let object = value.as_object().ok_or_else(|| {
                Error::Config(format!(
                    "exec_policy.host_executables[{index}] must be an object"
                ))
            })?;
            let name = optional_string_field(object, "name")?.ok_or_else(|| {
                Error::Config(format!(
                    "exec_policy.host_executables[{index}].name is required"
                ))
            })?;
            if name.contains('/') || name.contains('\\') {
                return Err(Error::Config(format!(
                    "exec_policy.host_executables[{index}].name must be a basename"
                )));
            }
            let paths = string_array_field(
                object,
                "paths",
                &format!("exec_policy.host_executables[{index}].paths"),
            )?;
            if paths.is_empty() {
                return Err(Error::Config(format!(
                    "exec_policy.host_executables[{index}].paths must not be empty"
                )));
            }
            if paths.iter().any(|path| !Path::new(path).is_absolute()) {
                return Err(Error::Config(format!(
                    "exec_policy.host_executables[{index}].paths entries must be absolute paths"
                )));
            }
            Ok(ExecPolicyHostExecutable { name, paths })
        })
        .collect()
}

pub(crate) fn non_empty_string(raw: &str, path: &str) -> Result<String> {
    raw.trim()
        .to_string()
        .is_empty()
        .then(|| Error::Config(format!("{path} must be a non-empty string")))
        .map_or_else(|| Ok(raw.trim().to_string()), Err)
}

pub(crate) fn validate_permission_profile_name(value: &str) -> Result<()> {
    if value.starts_with(':') {
        match value {
            ":read-only" | ":workspace" | ":danger-full-access" => return Ok(()),
            _ => {
                return Err(Error::Config(format!(
                    "unknown built-in permission profile `{value}`"
                )));
            }
        }
    }
    if value.trim().is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        Err(Error::Config(format!(
            "invalid permission profile name: {value}"
        )))
    } else {
        Ok(())
    }
}

pub(crate) fn parse_model_selection(
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

pub(crate) fn parse_config_provider_entry(
    name: &str,
    value: &Value,
) -> Result<ConfigProviderEntry> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("provider.{name} must be an object")))?;
    let api_key_env = optional_string_field(object, "api_key_env")?;
    if let Some(api_key_env) = &api_key_env
        && !valid_env_name(api_key_env)
    {
        return Err(Error::Config(format!(
            "provider.{name}.api_key_env must be a valid environment variable name"
        )));
    }
    let mut entry = ConfigProviderEntry {
        name: optional_string_field(object, "name")?,
        api: optional_string_field(object, "api")?,
        api_key_env,
        no_auth: optional_bool_field(object, "no_auth")?.unwrap_or(false),
        inference_idle_timeout_secs: optional_nonnegative_u64_field(
            object,
            "inference_idle_timeout_secs",
        )?,
        ..Default::default()
    };
    if object.contains_key("api_key") || object.contains_key("apiKey") {
        return Err(Error::Config(format!(
            "provider.{name} must not contain raw API keys"
        )));
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

pub(crate) fn parse_config_model_entry(
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
        name: optional_string_field(object, "name")?,
        reasoning_effort: validate_reasoning_effort(optional_string_field(
            object,
            "reasoning_effort",
        )?)?,
        metadata: parse_config_model_metadata(object)?,
    })
}

pub(crate) fn parse_config_model_metadata(
    object: &serde_json::Map<String, Value>,
) -> Result<ModelMetadata> {
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
    metadata.capabilities.web_search = optional_bool_field(object, "web_search")?;
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
        || metadata.capabilities.web_search.is_some()
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

pub(crate) fn parse_model_limits(value: &Value) -> Result<ModelLimits> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("limit must be an object".to_string()))?;
    Ok(ModelLimits {
        context: optional_u64_field(object, "context")?,
        input: optional_u64_field(object, "input")?,
        output: optional_u64_field(object, "output")?,
    })
}

pub(crate) fn parse_model_cost(value: &Value) -> Result<ModelCost> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("cost must be an object".to_string()))?;
    Ok(ModelCost {
        input: optional_f64_field(object, "input")?,
        output: optional_f64_field(object, "output")?,
        cache_read: optional_f64_field(object, "cache_read")?,
        cache_write: optional_f64_field(object, "cache_write")?,
        request: optional_f64_field(object, "request")?,
        context_over_200k: object
            .get("context_over_200k")
            .map(parse_model_cost_tier)
            .transpose()?,
        source: Some("config".to_string()),
        version: None,
    })
}

pub(crate) fn parse_model_cost_tier(value: &Value) -> Result<ModelCostTier> {
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

pub(crate) fn optional_string_field(
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

#[allow(dead_code)]
pub(crate) fn optional_string_alias_field(
    object: &serde_json::Map<String, Value>,
    primary: &str,
    alias: &str,
) -> Result<Option<String>> {
    match optional_string_field(object, primary)? {
        Some(value) => Ok(Some(value)),
        None => optional_string_field(object, alias),
    }
}

pub(crate) fn optional_u64_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<u64>> {
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

pub(crate) fn optional_nonnegative_u64_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<u64>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| Error::Config(format!("{key} must be a non-negative integer")))
        })
        .transpose()
}

pub(crate) fn optional_f64_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<f64>> {
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

pub(crate) fn optional_bool_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<bool>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| Error::Config(format!("{key} must be a boolean")))
        })
        .transpose()
}

pub(crate) fn required_bool_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<bool> {
    optional_bool_field(object, key)?.ok_or_else(|| Error::Config(format!("{path} is required")))
}

pub(crate) fn string_array_field(
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

pub(crate) fn parse_string_array_value(value: &Value) -> Result<Vec<String>> {
    let values = value
        .as_array()
        .ok_or_else(|| Error::Config("value must be an array".to_string()))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| Error::Config("array entries must be strings".to_string()))
        })
        .collect()
}

pub(crate) fn string_map_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<BTreeMap<String, String>> {
    let Some(value) = object.get(key) else {
        return Ok(BTreeMap::new());
    };
    let values = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("{path} must be an object")))?;
    values
        .iter()
        .map(|(key, value)| {
            let value = value
                .as_str()
                .map(str::trim)
                .ok_or_else(|| Error::Config(format!("{path}.{key} must be a string")))?;
            Ok((key.clone(), value.to_string()))
        })
        .collect()
}

pub(crate) fn validate_reasoning_effort(value: Option<String>) -> Result<Option<String>> {
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

pub(crate) fn enabled_reasoning_effort(value: Option<String>) -> Result<Option<String>> {
    match validate_reasoning_effort(value)? {
        Some(value) if value == "none" => Ok(None),
        value => Ok(value),
    }
}
