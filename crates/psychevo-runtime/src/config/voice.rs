#[allow(unused_imports)]
use super::*;

pub fn resolve_voice_asr_config(
    options: &RunOptions,
    provider: Option<&str>,
    model: Option<&str>,
    language: Option<&str>,
) -> Result<ResolvedVoiceAsrConfig> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let configured = &loaded.config.voice.asr;
    let provider = provider
        .map(normalize_provider_id)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| configured.provider.clone());
    let model = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| configured.model.clone());
    let language = language
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| configured.language.clone());
    let provider_config = resolve_voice_provider(&loaded, &provider)?;
    Ok(ResolvedVoiceAsrConfig {
        provider,
        display_label: provider_config.display_label,
        model,
        base_url: provider_config.base_url,
        api_key_env: provider_config.api_key_env,
        api_key: provider_config.api_key,
        language,
    })
}

pub fn resolve_voice_tts_config(
    options: &RunOptions,
    provider: Option<&str>,
    model: Option<&str>,
    voice: Option<&str>,
    format: Option<VoiceAudioFormat>,
) -> Result<ResolvedVoiceTtsConfig> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let configured = &loaded.config.voice.tts;
    let provider = provider
        .map(normalize_provider_id)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| configured.provider.clone());
    let model = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| configured.model.clone());
    let voice = voice
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| configured.voice.clone());
    let format = format.unwrap_or(configured.format);
    if !format.supports_tts_output() {
        return Err(Error::Config(format!(
            "voice.tts.format must be wav or pcm16, got {}",
            format.as_str()
        )));
    }
    let provider_config = resolve_voice_provider(&loaded, &provider)?;
    Ok(ResolvedVoiceTtsConfig {
        provider,
        display_label: provider_config.display_label,
        model,
        base_url: provider_config.base_url,
        api_key_env: provider_config.api_key_env,
        api_key: provider_config.api_key,
        voice,
        format,
    })
}

pub fn resolve_voice_realtime_config(
    options: &RunOptions,
    provider: Option<&str>,
    model: Option<&str>,
    transport: Option<VoiceRealtimeTransport>,
    voice: Option<&str>,
) -> Result<Option<ResolvedVoiceRealtimeConfig>> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let configured = loaded.config.voice.realtime.as_ref();
    let Some(provider) = provider
        .map(normalize_provider_id)
        .filter(|value| !value.is_empty())
        .or_else(|| configured.map(|config| config.provider.clone()))
    else {
        return Ok(None);
    };
    let model = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| configured.map(|config| config.model.clone()))
        .ok_or_else(|| Error::Config("voice.realtime.model is required".to_string()))?;
    let transport = transport
        .or_else(|| configured.map(|config| config.transport))
        .unwrap_or(VoiceRealtimeTransport::Webrtc);
    let voice = voice
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| configured.and_then(|config| config.voice.clone()));
    let provider_config = resolve_voice_provider(&loaded, &provider)?;
    Ok(Some(ResolvedVoiceRealtimeConfig {
        provider,
        display_label: provider_config.display_label,
        model,
        base_url: provider_config.base_url,
        api_key_env: provider_config.api_key_env,
        api_key: provider_config.api_key,
        transport,
        voice,
    }))
}

pub fn voice_config_value(options: &RunOptions) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let asr = resolve_voice_asr_config(options, None, None, None)?;
    let tts = resolve_voice_tts_config(options, None, None, None, None)?;
    let realtime = resolve_voice_realtime_config(options, None, None, None, None)?;
    Ok(json!({
        "asr": voice_asr_value(&asr),
        "tts": voice_tts_value(&tts),
        "realtime": realtime.as_ref().map(voice_realtime_value),
        "configured": {
            "asr": loaded.config.voice.asr.provider != VoiceAsrConfig::default().provider
                || loaded.config.voice.asr.model != VoiceAsrConfig::default().model,
            "tts": loaded.config.voice.tts.provider != VoiceTtsConfig::default().provider
                || loaded.config.voice.tts.model != VoiceTtsConfig::default().model,
            "realtime": loaded.config.voice.realtime.is_some(),
        }
    }))
}

pub(crate) fn parse_voice_config(value: &Value) -> Result<VoiceConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("voice must be an object".to_string()))?;
    reject_raw_voice_keys("voice", object)?;
    let mut config = VoiceConfig::default();
    if let Some(asr) = object.get("asr") {
        config.asr = parse_voice_asr_config(asr)?;
    }
    if let Some(tts) = object.get("tts") {
        config.tts = parse_voice_tts_config(tts)?;
    }
    if let Some(realtime) = object.get("realtime") {
        config.realtime = Some(parse_voice_realtime_config(realtime)?);
    }
    Ok(config)
}

fn parse_voice_asr_config(value: &Value) -> Result<VoiceAsrConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("voice.asr must be an object".to_string()))?;
    reject_raw_voice_keys("voice.asr", object)?;
    let mut config = VoiceAsrConfig::default();
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
    if let Some(language) = optional_string_field(object, "language")? {
        config.language = (!language.trim().is_empty()).then(|| language.trim().to_string());
    }
    Ok(config)
}

fn parse_voice_tts_config(value: &Value) -> Result<VoiceTtsConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("voice.tts must be an object".to_string()))?;
    reject_raw_voice_keys("voice.tts", object)?;
    let mut config = VoiceTtsConfig::default();
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
    if let Some(voice) = optional_string_field(object, "voice")?
        && !voice.trim().is_empty()
    {
        config.voice = voice.trim().to_string();
    }
    if let Some(format) = optional_string_field(object, "format")? {
        let format = parse_voice_audio_format_config(&format)
            .ok_or_else(|| Error::Config("voice.tts.format must be wav or pcm16".to_string()))?;
        if !format.supports_tts_output() {
            return Err(Error::Config(
                "voice.tts.format must be wav or pcm16".to_string(),
            ));
        }
        config.format = format;
    }
    Ok(config)
}

fn parse_voice_realtime_config(value: &Value) -> Result<VoiceRealtimeConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("voice.realtime must be an object".to_string()))?;
    reject_raw_voice_keys("voice.realtime", object)?;
    let provider = optional_string_field(object, "provider")?
        .map(|provider| normalize_provider_id(&provider))
        .filter(|provider| !provider.is_empty())
        .ok_or_else(|| Error::Config("voice.realtime.provider is required".to_string()))?;
    let model = optional_string_field(object, "model")?
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .ok_or_else(|| Error::Config("voice.realtime.model is required".to_string()))?;
    let transport = optional_string_field(object, "transport")?
        .as_deref()
        .map(parse_voice_realtime_transport_config)
        .transpose()?
        .unwrap_or(VoiceRealtimeTransport::Webrtc);
    let voice = optional_string_field(object, "voice")?
        .map(|voice| voice.trim().to_string())
        .filter(|voice| !voice.is_empty());
    Ok(VoiceRealtimeConfig {
        provider,
        model,
        transport,
        voice,
    })
}

fn parse_voice_audio_format_config(value: &str) -> Option<VoiceAudioFormat> {
    match value.trim().to_ascii_lowercase().as_str() {
        "wav" | "wave" => Some(VoiceAudioFormat::Wav),
        "mp3" | "mpeg" => Some(VoiceAudioFormat::Mp3),
        "pcm16" | "pcm_s16le" | "s16le" => Some(VoiceAudioFormat::Pcm16),
        _ => None,
    }
}

fn parse_voice_realtime_transport_config(value: &str) -> Result<VoiceRealtimeTransport> {
    match value.trim().to_ascii_lowercase().as_str() {
        "webrtc" => Ok(VoiceRealtimeTransport::Webrtc),
        "websocket" | "ws" => Ok(VoiceRealtimeTransport::Websocket),
        _ => Err(Error::Config(
            "voice.realtime.transport must be webrtc or websocket".to_string(),
        )),
    }
}

fn reject_raw_voice_keys(path: &str, object: &serde_json::Map<String, Value>) -> Result<()> {
    if object.contains_key("api_key") || object.contains_key("apiKey") {
        return Err(Error::Config(format!(
            "{path} must not contain raw API keys"
        )));
    }
    Ok(())
}

struct ResolvedVoiceProviderConfig {
    display_label: String,
    base_url: String,
    api_key_env: Option<String>,
    api_key: Option<String>,
}

fn resolve_voice_provider(
    loaded: &LoadedRunConfig,
    provider: &str,
) -> Result<ResolvedVoiceProviderConfig> {
    if provider == "fake" {
        return Ok(ResolvedVoiceProviderConfig {
            display_label: "Fake Voice".to_string(),
            base_url: "fake://voice".to_string(),
            api_key_env: None,
            api_key: None,
        });
    }
    let config_entry = loaded.config.provider.get(provider);
    if built_in_provider(provider).is_none() && config_entry.is_none() {
        return Err(Error::Config(format!("unknown voice provider: {provider}")));
    }
    let base_url = provider_base_url(provider, config_entry, &loaded.env)
        .ok_or_else(|| Error::Config(format!("voice provider {provider} requires base_url")))?;
    let api_key_env = provider_api_key_env(provider, config_entry);
    let api_key = api_key_env
        .as_deref()
        .and_then(|key| env_value(&loaded.env, key));
    Ok(ResolvedVoiceProviderConfig {
        display_label: provider_label(provider, config_entry),
        base_url,
        api_key_env,
        api_key,
    })
}

fn voice_asr_value(config: &ResolvedVoiceAsrConfig) -> Value {
    json!({
        "provider": config.provider,
        "providerLabel": config.display_label,
        "model": config.model,
        "language": config.language,
        "baseUrl": config.base_url,
        "apiKeyEnv": config.api_key_env,
        "credentialStatus": credential_status(config.api_key.as_deref()),
    })
}

fn voice_tts_value(config: &ResolvedVoiceTtsConfig) -> Value {
    json!({
        "provider": config.provider,
        "providerLabel": config.display_label,
        "model": config.model,
        "voice": config.voice,
        "format": config.format.as_str(),
        "baseUrl": config.base_url,
        "apiKeyEnv": config.api_key_env,
        "credentialStatus": credential_status(config.api_key.as_deref()),
    })
}

fn voice_realtime_value(config: &ResolvedVoiceRealtimeConfig) -> Value {
    json!({
        "provider": config.provider,
        "providerLabel": config.display_label,
        "model": config.model,
        "voice": config.voice,
        "transport": match config.transport {
            VoiceRealtimeTransport::Webrtc => "webrtc",
            VoiceRealtimeTransport::Websocket => "websocket",
        },
        "baseUrl": config.base_url,
        "apiKeyEnv": config.api_key_env,
        "credentialStatus": credential_status(config.api_key.as_deref()),
    })
}

fn credential_status(api_key: Option<&str>) -> &'static str {
    if api_key.is_some_and(|value| !value.trim().is_empty()) {
        "present"
    } else {
        "missing"
    }
}
