#[allow(unused_imports)]
pub(crate) use super::*;

pub fn resolve_image_generation_config(
    options: &RunOptions,
    provider: Option<&str>,
    model: Option<&str>,
    size: Option<&str>,
    format: Option<ImageGenerationFormat>,
) -> Result<ResolvedImageGenerationConfig> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    resolve_image_generation_config_from_loaded(&loaded, provider, model, size, format)
}

pub(crate) fn resolve_image_generation_config_from_loaded(
    loaded: &LoadedRunConfig,
    provider: Option<&str>,
    model: Option<&str>,
    size: Option<&str>,
    format: Option<ImageGenerationFormat>,
) -> Result<ResolvedImageGenerationConfig> {
    let configured = &loaded.config.image_generation;
    let provider = provider
        .map(normalize_provider_id)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| configured.provider.clone());
    let model = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| configured.model.clone());
    let size = size
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| configured.size.clone());
    validate_image_generation_size(&size)?;
    let format = format.unwrap_or(configured.format);
    let provider_config = resolve_image_generation_provider(loaded, &provider)?;
    Ok(ResolvedImageGenerationConfig {
        provider,
        display_label: provider_config.display_label,
        model,
        base_url: provider_config.base_url,
        api_key_env: provider_config.api_key_env,
        api_key: provider_config.api_key,
        size,
        format,
    })
}

pub fn image_generation_config_value(options: &RunOptions) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let resolved = resolve_image_generation_config_from_loaded(&loaded, None, None, None, None)?;
    Ok(image_generation_value(
        &resolved,
        &loaded.config.image_generation,
    ))
}

pub(crate) fn parse_image_generation_config(value: &Value) -> Result<ImageGenerationConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("image_generation must be an object".to_string()))?;
    reject_raw_image_generation_keys("image_generation", object)?;
    let mut config = ImageGenerationConfig::default();
    if let Some(provider) = optional_string_field(object, "provider")? {
        let provider = normalize_provider_id(&provider);
        if !provider.is_empty() {
            config.provider = provider;
        }
    }
    if let Some(model) = optional_string_field(object, "model")?
        && !model.trim().is_empty()
    {
        config.model = model.trim().to_string();
    }
    if let Some(size) = optional_string_field(object, "size")?
        && !size.trim().is_empty()
    {
        let size = size.trim().to_string();
        validate_image_generation_size(&size)?;
        config.size = size;
    }
    if let Some(format) = optional_string_field(object, "format")? {
        config.format = parse_image_generation_format_config(&format).ok_or_else(|| {
            Error::Config("image_generation.format must be png, jpeg, jpg, or webp".to_string())
        })?;
    }
    Ok(config)
}

fn image_generation_value(
    resolved: &ResolvedImageGenerationConfig,
    configured: &ImageGenerationConfig,
) -> Value {
    let default = ImageGenerationConfig::default();
    json!({
        "provider": resolved.provider,
        "label": resolved.display_label,
        "model": resolved.model,
        "baseUrl": resolved.base_url,
        "apiKeyEnv": resolved.api_key_env,
        "credentialStatus": if resolved.provider == "fake" {
            "notRequired"
        } else if resolved.api_key.as_deref().is_some_and(|value| !value.trim().is_empty()) {
            "present"
        } else {
            "missing"
        },
        "size": resolved.size,
        "format": resolved.format.as_str(),
        "mimeType": resolved.format.mime_type(),
        "configured": configured.provider != default.provider
            || configured.model != default.model
            || configured.size != default.size
            || configured.format != default.format,
    })
}

fn parse_image_generation_format_config(value: &str) -> Option<ImageGenerationFormat> {
    match value.trim().to_ascii_lowercase().as_str() {
        "png" => Some(ImageGenerationFormat::Png),
        "jpg" | "jpeg" => Some(ImageGenerationFormat::Jpeg),
        "webp" => Some(ImageGenerationFormat::Webp),
        _ => None,
    }
}

fn validate_image_generation_size(value: &str) -> Result<()> {
    match value.trim() {
        "auto" | "1024x1024" | "1024x1536" | "1536x1024" => Ok(()),
        value => Err(Error::Config(format!(
            "image_generation.size must be auto, 1024x1024, 1024x1536, or 1536x1024, got {value}"
        ))),
    }
}

fn reject_raw_image_generation_keys(
    path: &str,
    object: &serde_json::Map<String, Value>,
) -> Result<()> {
    if object.contains_key("api_key") || object.contains_key("apiKey") {
        return Err(Error::Config(format!(
            "{path} must not contain raw API keys"
        )));
    }
    Ok(())
}

struct ResolvedImageGenerationProviderConfig {
    display_label: String,
    base_url: String,
    api_key_env: Option<String>,
    api_key: Option<String>,
}

fn resolve_image_generation_provider(
    loaded: &LoadedRunConfig,
    provider: &str,
) -> Result<ResolvedImageGenerationProviderConfig> {
    if provider == "fake" {
        return Ok(ResolvedImageGenerationProviderConfig {
            display_label: "Fake".to_string(),
            base_url: "fake://image-generation".to_string(),
            api_key_env: None,
            api_key: None,
        });
    }
    if provider == "openai" {
        let provider_entry = loaded.config.provider.get(provider);
        let api_key_env = provider_entry
            .and_then(|entry| entry.api_key_env.clone())
            .or_else(|| Some("OPENAI_API_KEY".to_string()));
        let api_key = api_key_env
            .as_deref()
            .and_then(|key| loaded.env.get(key))
            .cloned();
        return Ok(ResolvedImageGenerationProviderConfig {
            display_label: provider_entry
                .and_then(|entry| entry.name.clone())
                .unwrap_or_else(|| "OpenAI".to_string()),
            base_url: provider_entry
                .and_then(|entry| entry.api.clone())
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            api_key_env,
            api_key,
        });
    }
    let Some(provider_entry) = loaded.config.provider.get(provider) else {
        return Err(Error::Config(format!(
            "unknown image generation provider: {provider}"
        )));
    };
    let api_key_env = provider_entry
        .api_key_env
        .clone()
        .or_else(|| Some(custom_provider_api_key_env(provider)));
    let api_key = api_key_env
        .as_deref()
        .and_then(|key| loaded.env.get(key))
        .cloned();
    Ok(ResolvedImageGenerationProviderConfig {
        display_label: provider_entry
            .name
            .clone()
            .unwrap_or_else(|| provider.to_string()),
        base_url: provider_entry
            .api
            .clone()
            .ok_or_else(|| Error::Config(format!("provider.{provider}.api is required")))?,
        api_key_env,
        api_key,
    })
}
