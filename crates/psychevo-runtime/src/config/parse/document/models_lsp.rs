pub(crate) fn parse_compression_config(
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

pub(crate) fn parse_auxiliary_config(
    value: &Value,
    configured_keys: &HashSet<String>,
) -> Result<AuxiliaryConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("auxiliary must be an object".to_string()))?;
    let mut config = AuxiliaryConfig::default();
    if let Some(value) = object.get("title_generation") {
        config.title_generation =
            parse_auxiliary_task_config("auxiliary.title_generation", value, configured_keys)?;
    }
    if let Some(value) = object.get("compression") {
        config.compression =
            parse_auxiliary_task_config("auxiliary.compression", value, configured_keys)?;
    }
    Ok(config)
}

pub(crate) fn parse_auxiliary_task_config(
    path: &str,
    value: &Value,
    configured_keys: &HashSet<String>,
) -> Result<AuxiliaryTaskConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("{path} must be an object")))?;
    let provider = optional_string_field(object, "provider")?
        .map(|provider| normalize_provider_id(&provider))
        .filter(|provider| !provider.is_empty() && provider != "auto");
    let mut config = AuxiliaryTaskConfig {
        provider,
        ..Default::default()
    };
    if let Some(model) = object.get("model")
        && !model.as_str().is_some_and(|value| value.trim().is_empty())
    {
        config.model = parse_model_selection(model, configured_keys)?;
        if config.model.provider.is_none() {
            config.model.provider = config.provider.clone();
        }
        config.model_configured = true;
    }
    Ok(config)
}

pub(crate) fn parse_lsp_config(value: &Value) -> Result<LspConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("lsp must be an object".to_string()))?;
    let mut config = LspConfig::default();
    if let Some(enabled) = optional_bool_field(object, "enabled")? {
        config.enabled = enabled;
    }
    if let Some(wait_mode) = optional_string_field(object, "wait_mode")? {
        if wait_mode != "document" && wait_mode != "full" {
            return Err(Error::Config(
                "lsp.wait_mode must be document or full".to_string(),
            ));
        }
        config.wait_mode = wait_mode;
    }
    if let Some(wait_timeout) = optional_f64_field(object, "wait_timeout")? {
        if wait_timeout <= 0.0 {
            return Err(Error::Config(
                "lsp.wait_timeout must be greater than 0".to_string(),
            ));
        }
        config.wait_timeout_secs = wait_timeout;
    }
    if let Some(install_strategy) = optional_string_field(object, "install_strategy")? {
        if !matches!(install_strategy.as_str(), "auto" | "manual" | "off") {
            return Err(Error::Config(
                "lsp.install_strategy must be auto, manual, or off".to_string(),
            ));
        }
        config.install_strategy = install_strategy;
    }
    Ok(config)
}
