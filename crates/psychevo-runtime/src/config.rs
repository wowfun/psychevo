use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::paths::canonical_workdir;
use crate::types::{ConfiguredModel, RunOptions};

#[derive(Debug, Clone, Default)]
pub(crate) struct RunConfig {
    model: ModelSelection,
    provider: BTreeMap<String, ConfigProviderEntry>,
}

#[derive(Debug, Clone, Default)]
struct ModelSelection {
    id: Option<String>,
    provider: Option<String>,
    reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ConfigProviderEntry {
    options: ConfigProviderOptions,
    models: BTreeMap<String, ConfigModelEntry>,
}

#[derive(Debug, Clone, Default)]
struct ConfigProviderOptions {
    base_url: Option<String>,
    api_key_env: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ConfigModelEntry {
    reasoning_effort: Option<String>,
    context_limit: Option<u64>,
}

#[derive(Debug, Clone)]
struct BuiltInProvider {
    id: &'static str,
    label: &'static str,
    base_url: Option<&'static str>,
    api_key_envs: &'static [&'static str],
    base_url_env: Option<&'static str>,
    allow_no_auth: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedRunProvider {
    pub(crate) provider: String,
    pub(crate) display_label: String,
    pub(crate) model: String,
    pub(crate) base_url: String,
    pub(crate) api_key_env: Option<String>,
    pub(crate) api_key: String,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) context_limit: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedRunConfig {
    pub(crate) config: RunConfig,
    pub(crate) env: BTreeMap<String, String>,
}

const AUTO_PROVIDER_ORDER: &[&str] = &[
    "openrouter",
    "openai",
    "xai",
    "zai",
    "deepseek",
    "dashscope",
    "xiaomi",
    "lmstudio",
    "custom",
];

const REASONING_EFFORT_VALUES: &[&str] =
    &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

const BUILT_IN_PROVIDERS: &[BuiltInProvider] = &[
    BuiltInProvider {
        id: "openrouter",
        label: "OpenRouter",
        base_url: Some("https://openrouter.ai/api/v1"),
        api_key_envs: &["OPENROUTER_API_KEY", "OPENAI_API_KEY"],
        base_url_env: Some("OPENROUTER_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "openai",
        label: "OpenAI",
        base_url: Some("https://api.openai.com/v1"),
        api_key_envs: &["OPENAI_API_KEY"],
        base_url_env: Some("OPENAI_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "xai",
        label: "xAI",
        base_url: Some("https://api.x.ai/v1"),
        api_key_envs: &["XAI_API_KEY"],
        base_url_env: Some("XAI_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "zai",
        label: "Z.AI / GLM",
        base_url: Some("https://api.z.ai/api/paas/v4"),
        api_key_envs: &["GLM_API_KEY", "ZAI_API_KEY", "Z_AI_API_KEY"],
        base_url_env: Some("GLM_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "deepseek",
        label: "DeepSeek",
        base_url: Some("https://api.deepseek.com/v1"),
        api_key_envs: &["DEEPSEEK_API_KEY"],
        base_url_env: Some("DEEPSEEK_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "dashscope",
        label: "Alibaba Cloud DashScope",
        base_url: Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        api_key_envs: &["DASHSCOPE_API_KEY"],
        base_url_env: Some("DASHSCOPE_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "xiaomi",
        label: "Xiaomi MiMo",
        base_url: Some("https://api.xiaomimimo.com/v1"),
        api_key_envs: &["XIAOMI_API_KEY"],
        base_url_env: Some("XIAOMI_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "lmstudio",
        label: "LM Studio",
        base_url: Some("http://127.0.0.1:1234/v1"),
        api_key_envs: &["LM_API_KEY"],
        base_url_env: Some("LM_BASE_URL"),
        allow_no_auth: true,
    },
    BuiltInProvider {
        id: "custom",
        label: "Custom",
        base_url: None,
        api_key_envs: &[],
        base_url_env: None,
        allow_no_auth: false,
    },
];

pub(crate) fn load_run_config(options: &RunOptions, workdir: &Path) -> Result<LoadedRunConfig> {
    let mut env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    let project_dir = workdir.join(".psychevo");
    let mut value = json!({});

    if let Some(config_path) = resolve_config_path(options, &env_map)? {
        let loaded = load_jsonc_config_file(&config_path, true)?;
        deep_merge(&mut value, loaded);
        if let Some(parent) = config_path.parent() {
            load_dotenv_file(&parent.join(".env"), &mut env_map)?;
        }
    } else {
        let home = resolve_psychevo_home(&env_map)?;
        let home_config = home.join("config.jsonc");
        if !home_config.exists() {
            return Err(Error::Config(format!(
                "Psychevo home is not initialized; run `pevo init` to create {}",
                home_config.display()
            )));
        }
        let loaded = load_jsonc_config_file(&home_config, true)?;
        deep_merge(&mut value, loaded);
        load_dotenv_file(&home.join(".env"), &mut env_map)?;
        let loaded = load_jsonc_config_file(&project_dir.join("config.jsonc"), false)?;
        deep_merge(&mut value, loaded);
    }

    load_dotenv_file(&project_dir.join(".env"), &mut env_map)?;
    Ok(LoadedRunConfig {
        config: parse_run_config(value)?,
        env: env_map,
    })
}

fn resolve_config_path(
    options: &RunOptions,
    env_map: &BTreeMap<String, String>,
) -> Result<Option<PathBuf>> {
    if let Some(path) = &options.config_path {
        return Ok(Some(resolve_explicit_path(path, env_map)?));
    }
    env_map
        .get("PSYCHEVO_CONFIG")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| resolve_explicit_path(Path::new(value), env_map))
        .transpose()
}

fn resolve_psychevo_home(env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    if let Some(value) = env_map
        .get("PSYCHEVO_HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        resolve_explicit_path(Path::new(value), env_map)
    } else {
        resolve_explicit_path(Path::new("~/.psychevo"), env_map)
    }
}

fn resolve_explicit_path(path: &Path, env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    let expanded = expand_tilde(path, env_map)?;
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(env::current_dir()?.join(expanded))
    }
}

fn expand_tilde(path: &Path, env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    let raw = path.to_string_lossy();
    if raw == "~" {
        return home_path(env_map);
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return Ok(home_path(env_map)?.join(rest));
    }
    Ok(path.to_path_buf())
}

fn home_path(env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    env_map
        .get("HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::Config("HOME is required to expand ~".to_string()))
}

fn load_jsonc_config_file(path: &Path, required: bool) -> Result<Value> {
    if !path.exists() {
        if required {
            return Err(Error::Config(format!(
                "config file not found: {}",
                path.display()
            )));
        }
        return Ok(json!({}));
    }
    let text = fs::read_to_string(path)?;
    let parsed: Option<Value> = jsonc_parser::parse_to_serde_value(&text, &Default::default())
        .map_err(|err| Error::Config(format!("{}: {err}", path.display())))?;
    let value = parsed.unwrap_or_else(|| json!({}));
    if !value.is_object() {
        return Err(Error::Config(format!(
            "{} must contain a JSON object",
            path.display()
        )));
    }
    Ok(value)
}

fn deep_merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                if let Some(existing) = base.get_mut(&key) {
                    deep_merge(existing, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn load_dotenv_file(path: &Path, env_map: &mut BTreeMap<String, String>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(path)?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if !valid_env_name(name) {
            continue;
        }
        env_map.insert(name.to_string(), strip_env_quotes(value.trim()).to_string());
    }
    Ok(())
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|ch| matches!(ch, '_' | 'A'..='Z' | 'a'..='z' | '0'..='9'))
}

fn strip_env_quotes(value: &str) -> &str {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

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
    let mut entry = ConfigProviderEntry::default();
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
        display_label: built_in
            .map(|provider| provider.label.to_string())
            .unwrap_or_else(|| provider.clone()),
        model,
        base_url,
        api_key_env,
        api_key,
        reasoning_effort,
        context_limit,
    })
}

fn built_in_context_limit(provider: &str, model: &str) -> Option<u64> {
    let model = model.to_lowercase();
    match normalize_provider_id(provider).as_str() {
        "deepseek" if model.contains("deepseek") => Some(64_000),
        "openai" if model.contains("gpt-4.1") || model.contains("gpt-4o") => Some(128_000),
        _ => None,
    }
}

fn built_in_provider(provider: &str) -> Option<&'static BuiltInProvider> {
    BUILT_IN_PROVIDERS
        .iter()
        .find(|entry| entry.id == normalize_provider_id(provider))
}

fn normalize_provider_id(provider: &str) -> String {
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

fn first_string(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn infer_provider_for_model(config: &RunConfig, model: &str) -> Option<String> {
    let matches = config
        .provider
        .iter()
        .filter_map(|(provider, entry)| {
            entry.models.contains_key(model).then_some(provider.clone())
        })
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches[0].clone())
}

fn unique_config_model(entry: Option<&ConfigProviderEntry>) -> Option<String> {
    let entry = entry?;
    (entry.models.len() == 1).then(|| entry.models.keys().next().expect("one model").clone())
}

fn config_model_entry<'a>(
    entry: Option<&'a ConfigProviderEntry>,
    model: &str,
) -> Option<&'a ConfigModelEntry> {
    entry.and_then(|entry| entry.models.get(model))
}

pub fn configured_models(options: &RunOptions) -> Result<Vec<ConfiguredModel>> {
    let workdir = canonical_workdir(&options.workdir)?;
    let loaded = load_run_config(options, &workdir)?;
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

    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    let mut push_model = |provider: &str,
                          model: &str,
                          reasoning_effort: Option<String>,
                          context_limit: Option<u64>,
                          rows: &mut Vec<ConfiguredModel>| {
        let provider = normalize_provider_id(provider);
        let model = model.trim().to_string();
        if provider.is_empty() || model.is_empty() || !seen.insert(format!("{provider}/{model}")) {
            return;
        }
        let context_limit = context_limit.or_else(|| built_in_context_limit(&provider, &model));
        rows.push(ConfiguredModel {
            provider: provider.clone(),
            provider_label: provider_label(&provider),
            model,
            reasoning_effort,
            context_limit,
        });
    };

    for (provider, entry) in &loaded.config.provider {
        for (model, config) in &entry.models {
            push_model(
                provider,
                model,
                config.reasoning_effort.clone(),
                config.context_limit,
                &mut rows,
            );
        }
    }

    for selection in [&cli_model, &loaded.config.model, &env_model] {
        if let (Some(provider), Some(model)) = (&selection.provider, &selection.id) {
            let reasoning_effort = loaded
                .config
                .provider
                .get(provider)
                .and_then(|entry| config_model_entry(Some(entry), model))
                .and_then(|entry| entry.reasoning_effort.clone())
                .or_else(|| selection.reasoning_effort.clone());
            let context_limit = loaded
                .config
                .provider
                .get(provider)
                .and_then(|entry| config_model_entry(Some(entry), model))
                .and_then(|entry| entry.context_limit);
            push_model(provider, model, reasoning_effort, context_limit, &mut rows);
        }
    }

    rows.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.model.cmp(&right.model))
    });
    Ok(rows)
}

pub fn selected_configured_model(options: &RunOptions) -> Result<Option<ConfiguredModel>> {
    let workdir = canonical_workdir(&options.workdir)?;
    let loaded = load_run_config(options, &workdir)?;
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
            if let Some(model) = selected_configured_model_for_provider(
                candidate, &cli_model, &env_model, options, &loaded,
            )? {
                return Ok(Some(model));
            }
        }
        return Ok(None);
    }

    selected_configured_model_for_provider(&provider, &cli_model, &env_model, options, &loaded)
}

fn selected_configured_model_for_provider(
    provider: &str,
    cli_model: &ModelSelection,
    env_model: &ModelSelection,
    options: &RunOptions,
    loaded: &LoadedRunConfig,
) -> Result<Option<ConfiguredModel>> {
    let provider = normalize_provider_id(provider);
    let config_entry = loaded.config.provider.get(&provider);
    if built_in_provider(&provider).is_none() && config_entry.is_none() {
        return Ok(None);
    }
    let (model, explicit_reasoning_effort) = model_for_provider(
        &provider,
        cli_model,
        &loaded.config.model,
        env_model,
        config_entry,
    );
    let Some(model) = model
        .or_else(|| unique_config_model(config_entry))
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    let reasoning_effort = validate_reasoning_effort(first_string([
        options.reasoning_effort.clone(),
        explicit_reasoning_effort,
        config_model_entry(config_entry, &model).and_then(|entry| entry.reasoning_effort.clone()),
    ]))?;
    let context_limit = config_model_entry(config_entry, &model)
        .and_then(|entry| entry.context_limit)
        .or_else(|| built_in_context_limit(&provider, &model));
    Ok(Some(ConfiguredModel {
        provider: provider.clone(),
        provider_label: provider_label(&provider),
        model,
        reasoning_effort,
        context_limit,
    }))
}

fn provider_label(provider: &str) -> String {
    built_in_provider(provider)
        .map(|entry| entry.label.to_string())
        .unwrap_or_else(|| provider.to_string())
}

fn env_value(env_map: &BTreeMap<String, String>, key: &str) -> Option<String> {
    env_map
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn is_loopback_base_url(base_url: &str) -> bool {
    let value = base_url.to_lowercase();
    value.contains("://localhost")
        || value.contains("://127.0.0.1")
        || value.contains("://0.0.0.0")
        || value.contains("://[::1]")
}
