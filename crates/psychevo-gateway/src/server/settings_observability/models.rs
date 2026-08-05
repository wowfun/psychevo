use std::path::Path;

use psychevo::config::{ConfigScope, ModelCatalogEntry, ModelCatalogProvider};
use psychevo::config::{
    REASONING_EFFORT_VALUES, model_catalog_entry_is_free, normalize_provider_id,
    remove_config_value, set_auxiliary_model_with_reasoning, set_config_value,
    set_default_model_with_reasoning, set_provider_model_config,
};
use psychevo::model_state::{ModelState, normalize_reasoning_effort};
use psychevo::{Configuration, ConfigurationQuery, Error, ThreadModelSelection};
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};

use super::WebState;
use super::workbench::session_model_state_selection;

fn configured_model_option_view(
    model: &psychevo::config::ConfiguredModel,
) -> wire::settings_workspace_context::ModelOptionView {
    let reasoning_supported = model.metadata.capabilities.reasoning;
    wire::settings_workspace_context::ModelOptionView {
        value: format!("{}/{}", model.provider, model.model),
        provider: model.provider.clone(),
        id: model.model.clone(),
        name: model.model_name.clone(),
        provider_name: Some(model.provider_label.clone()),
        free: configured_model_is_free(model),
        limit: wire::settings_workspace_context::ModelLimitView {
            context: model.context_limit,
            output: model.metadata.limits.output,
        },
        reasoning_supported,
        reasoning_efforts: reasoning_efforts_for_model(reasoning_supported),
    }
}

pub(super) fn model_options_with_cached_catalog(
    configuration: &Configuration,
    configured: &[psychevo::config::ConfiguredModel],
) -> Vec<wire::settings_workspace_context::ModelOptionView> {
    let mut seen = std::collections::BTreeSet::new();
    let mut views = Vec::new();
    for model in configured {
        let option = configured_model_option_view(model);
        if seen.insert(option.value.clone()) {
            views.push(option);
        }
    }
    for provider in configuration.model_catalog_providers().unwrap_or_default() {
        let Some(models) = configuration.cached_model_catalog(&provider) else {
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

pub(in super::super) fn model_settings_value(
    state: &WebState,
    cwd: &Path,
) -> psychevo::Result<Value> {
    let mut query = ConfigurationQuery::profile(cwd);
    query.inherited_env = Some(state.inner.inherited_env.clone());
    let configuration = state.inner.framework.configuration(query)?;
    Ok(serde_json::to_value(model_settings_result(
        cwd,
        &configuration,
    )?)?)
}

fn model_settings_result(
    cwd: &Path,
    configuration: &Configuration,
) -> psychevo::Result<wire::settings_workspace_context::ModelSettingsResult> {
    let selected_model = configuration.selected_model()?;
    let default_reasoning_effort = selected_model
        .as_ref()
        .and_then(|model| model.reasoning_effort.clone());
    let default_model = selected_model.map(|model| format!("{}/{}", model.provider, model.model));
    let configured = configuration.configured_models().unwrap_or_default();
    let effective_config = configuration.config_value(ConfigScope::Effective)?;
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
    let auth = configuration.auth_status(None)?;
    let mut providers = auth
        .get("providers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| model_provider_view(row, &configured_provider_ids))
        .collect::<Vec<_>>();
    if !providers.iter().any(|provider| provider.id == "custom") {
        providers.push(wire::settings_workspace_context::ModelProviderView {
            id: "custom".to_string(),
            name: "Custom".to_string(),
            built_in: false,
            configured: false,
            api: None,
            api_key_env: None,
            credential_status: wire::settings_workspace_context::ModelCredentialStatus::Missing,
            no_auth: false,
            can_fetch_models: false,
            unavailable_reason: Some("requires provider setup".to_string()),
        });
    }
    providers.sort_by(|left, right| {
        provider_sort_key(&left.id)
            .cmp(&provider_sort_key(&right.id))
            .then_with(|| left.name.cmp(&right.name))
    });
    let model_options = model_options_with_cached_catalog(configuration, &configured);
    Ok(wire::settings_workspace_context::ModelSettingsResult {
        scope: wire::settings_workspace_context::ModelSettingsScope::Global,
        cwd: cwd.display().to_string(),
        default_model,
        default_reasoning_effort,
        providers,
        auxiliary: vec![
            auxiliary_model_assignment_view(effective, "title_generation", "Title generation"),
            auxiliary_model_assignment_view(effective, "compression", "Context compression"),
        ],
        model_options,
        voice: configuration.voice_settings().ok(),
        image_generation: configuration.image_generation_settings().ok(),
    })
}

fn model_provider_view(
    row: &Value,
    configured_provider_ids: &std::collections::BTreeSet<String>,
) -> Option<wire::settings_workspace_context::ModelProviderView> {
    let id = row.get("provider").and_then(Value::as_str)?.to_string();
    let api = row.get("api").and_then(Value::as_str).map(str::to_string);
    let status = match row
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("missing")
    {
        "present" => wire::settings_workspace_context::ModelCredentialStatus::Present,
        "not_required" => wire::settings_workspace_context::ModelCredentialStatus::NotRequired,
        _ => wire::settings_workspace_context::ModelCredentialStatus::Missing,
    };
    let can_fetch_models =
        api.is_some() && status != wire::settings_workspace_context::ModelCredentialStatus::Missing;
    let unavailable_reason = (!can_fetch_models).then(|| {
        row.get("api_key_env")
            .and_then(Value::as_str)
            .map(|key| format!("missing {key}"))
            .unwrap_or_else(|| "requires provider setup".to_string())
    });
    Some(wire::settings_workspace_context::ModelProviderView {
        id: id.clone(),
        name: row
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(id.as_str())
            .to_string(),
        built_in: is_known_builtin_provider(&id),
        configured: configured_provider_ids.contains(&id),
        api,
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
) -> wire::settings_workspace_context::AuxiliaryModelAssignmentView {
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
    wire::settings_workspace_context::AuxiliaryModelAssignmentView {
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

pub(in super::super) async fn model_provider_catalog_value(
    state: &WebState,
    cwd: &Path,
    params: wire::settings_workspace_context::ModelProviderCatalogParams,
) -> psychevo::Result<Value> {
    let mut query = ConfigurationQuery::profile(cwd);
    query.inherited_env = Some(state.inner.inherited_env.clone());
    let configuration = state.inner.framework.configuration(query)?;
    let provider_id = normalize_provider_id(&params.provider_id);
    let provider = configuration
        .model_catalog_provider(&provider_id)?
        .ok_or_else(|| Error::Config(format!("unknown provider: {provider_id}")))?;
    let models = configuration
        .fetch_and_cache_model_catalog(&provider)
        .await?;
    let models = models
        .into_iter()
        .map(|model| catalog_model_option_view(&provider, model))
        .collect();
    Ok(serde_json::to_value(
        wire::settings_workspace_context::ModelProviderCatalogResult {
            provider_id: provider.provider,
            models,
        },
    )?)
}

fn catalog_model_option_view(
    provider: &ModelCatalogProvider,
    model: ModelCatalogEntry,
) -> wire::settings_workspace_context::ModelOptionView {
    let reasoning_supported = model.metadata.capabilities.reasoning;
    wire::settings_workspace_context::ModelOptionView {
        provider: provider.provider.clone(),
        value: format!("{}/{}", provider.provider, model.id),
        free: model_catalog_entry_is_free(&provider.provider, &model),
        limit: wire::settings_workspace_context::ModelLimitView {
            context: model.context_limit,
            output: model.metadata.limits.output,
        },
        id: model.id,
        name: None,
        provider_name: Some(provider.display_label.clone()),
        reasoning_supported,
        reasoning_efforts: reasoning_efforts_for_model(reasoning_supported),
    }
}

pub(in super::super) async fn model_state_read_value(
    state: &WebState,
    cwd: &Path,
    thread_id: Option<&str>,
) -> psychevo::Result<Value> {
    Ok(serde_json::to_value(
        model_state_result(state, cwd, thread_id).await?,
    )?)
}

pub(in super::super) async fn model_state_set_value(
    state: &WebState,
    cwd: &Path,
    thread_id: Option<&str>,
    params: wire::settings_workspace_context::ModelStateSetParams,
) -> psychevo::Result<Value> {
    let (model_spec, provider, model_id) = normalize_provider_qualified_model(&params.model)?;
    let reasoning_effort = normalize_model_state_reasoning_effort(params.reasoning_effort)?;
    let path = ModelState::path_for_home(&state.inner.home);
    let mut model_state = ModelState::load(&path)?;
    let cwd_key = cwd.to_string_lossy().to_string();
    model_state.set_model(&cwd_key, model_spec.clone(), reasoning_effort.clone());
    model_state.save(&path)?;
    if let Some(thread_id) = thread_id {
        state
            .inner
            .framework
            .resume_thread(thread_id)
            .await?
            .set_model_selection(ThreadModelSelection {
                provider,
                model: model_id,
                reasoning_effort,
            })
            .await?;
    }
    Ok(serde_json::to_value(
        model_state_result(state, cwd, thread_id).await?,
    )?)
}

async fn model_state_result(
    state: &WebState,
    cwd: &Path,
    thread_id: Option<&str>,
) -> psychevo::Result<wire::settings_workspace_context::ModelStateResult> {
    let model_state = ModelState::load(&ModelState::path_for_home(&state.inner.home))?;
    let cwd_key = cwd.to_string_lossy().to_string();
    let session_selection = match thread_id {
        Some(thread_id) => session_model_state_selection(state, thread_id).await?,
        None => None,
    };
    Ok(wire::settings_workspace_context::ModelStateResult {
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

fn normalize_provider_qualified_model(value: &str) -> psychevo::Result<(String, String, String)> {
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
) -> psychevo::Result<Option<String>> {
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

pub(in super::super) fn model_provider_save_value(
    state: &WebState,
    cwd: &Path,
    params: wire::settings_workspace_context::ModelProviderSaveParams,
) -> psychevo::Result<Value> {
    let mut query = ConfigurationQuery::profile(cwd);
    query.inherited_env = Some(state.inner.inherited_env.clone());
    let configuration = state.inner.framework.configuration(query)?;
    let provider_id = normalize_provider_id(&params.provider_id);
    validate_model_provider_id(&provider_id)?;
    let name = params
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api = validate_model_api(&params.api)?;
    let config_dir = state.inner.home.clone();
    remove_config_value(config_dir.clone(), &format!("provider.{provider_id}.label"))?;
    remove_config_value(
        config_dir.clone(),
        &format!("provider.{provider_id}.options"),
    )?;
    if let Some(name) = name {
        set_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.name"),
            json!(name),
        )?;
    } else {
        remove_config_value(config_dir.clone(), &format!("provider.{provider_id}.name"))?;
    }
    set_config_value(
        config_dir.clone(),
        &format!("provider.{provider_id}.api"),
        json!(api),
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
        set_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.no_auth"),
            json!(true),
        )?;
    } else {
        remove_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.no_auth"),
        )?;
        if let Some(api_key) = params
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            configuration.set_provider_api_key(ConfigScope::Global, &provider_id, api_key)?;
        }
    }
    if let Some(model) = params.model {
        let model_id = validate_model_id(&model.id)?;
        let model_value = model_config_value(&model)?;
        set_provider_model_config(config_dir.clone(), &provider_id, &model_id, model_value)?;
    }
    Ok(serde_json::to_value(model_settings_result(
        cwd,
        &configuration,
    )?)?)
}

pub(in super::super) fn model_assignment_set_value(
    state: &WebState,
    cwd: &Path,
    params: wire::settings_workspace_context::ModelAssignmentSetParams,
) -> psychevo::Result<Value> {
    let provider = normalize_provider_id(&params.provider);
    validate_model_provider_id(&provider)?;
    let reasoning_effort = assignment_reasoning_effort(params.reasoning_effort.as_deref());
    match params.target {
        wire::settings_workspace_context::ModelAssignmentTarget::Default => {
            let model_spec = format!("{provider}/{}", params.model.trim());
            set_default_model_with_reasoning(
                &state.inner.home,
                cwd,
                true,
                &model_spec,
                reasoning_effort,
            )?;
            Ok(serde_json::to_value(
                wire::settings_workspace_context::ModelAssignmentSetResult {
                    ok: true,
                    target: wire::settings_workspace_context::ModelAssignmentTarget::Default,
                    task: None,
                    provider,
                    model: params.model.trim().to_string(),
                    reasoning_effort: reasoning_effort.map(str::to_string),
                },
            )?)
        }
        wire::settings_workspace_context::ModelAssignmentTarget::Auxiliary => {
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
            Ok(serde_json::to_value(
                wire::settings_workspace_context::ModelAssignmentSetResult {
                    ok: true,
                    target: wire::settings_workspace_context::ModelAssignmentTarget::Auxiliary,
                    task: Some(task.to_string()),
                    provider,
                    model: params.model.trim().to_string(),
                    reasoning_effort: reasoning_effort.map(str::to_string),
                },
            )?)
        }
    }
}

fn assignment_reasoning_effort(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
}

fn validate_model_provider_id(provider_id: &str) -> psychevo::Result<()> {
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

fn validate_model_api(value: &str) -> psychevo::Result<String> {
    let value = value.trim().trim_end_matches('/').to_string();
    if value.starts_with("http://") || value.starts_with("https://") {
        Ok(value)
    } else {
        Err(Error::Config(
            "provider api must start with http:// or https://".to_string(),
        ))
    }
}

fn validate_model_id(value: &str) -> psychevo::Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::Config("model id is required".to_string()));
    }
    if value.contains('\n') || value.contains('\r') {
        return Err(Error::Config("model id must be a single line".to_string()));
    }
    Ok(value.to_string())
}

fn model_config_value(
    model: &wire::settings_workspace_context::ModelProviderSaveModelParams,
) -> psychevo::Result<Value> {
    let mut object = advanced_model_metadata_object(
        model.advanced_format.as_deref(),
        model.advanced.as_deref(),
    )?;
    if let Some(name) = model
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("name".to_string(), json!(name));
    }
    let mut limit = serde_json::Map::new();
    if let Some(context) = model.limit.context {
        if context == 0 {
            return Err(Error::Config("limit.context must be positive".to_string()));
        }
        limit.insert("context".to_string(), json!(context));
    }
    if let Some(output) = model.limit.output {
        if output == 0 {
            return Err(Error::Config("limit.output must be positive".to_string()));
        }
        limit.insert("output".to_string(), json!(output));
    }
    if !limit.is_empty() {
        object.insert("limit".to_string(), Value::Object(limit));
    }
    Ok(Value::Object(object))
}

fn advanced_model_metadata_object(
    format: Option<&str>,
    raw: Option<&str>,
) -> psychevo::Result<serde_json::Map<String, Value>> {
    let raw = raw.map(str::trim).filter(|value| !value.is_empty());
    let Some(raw) = raw else {
        return Ok(serde_json::Map::new());
    };
    let value = match format
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("toml") => {
            let value = toml::from_str::<toml::Value>(raw).map_err(|err| {
                Error::Config(format!("advanced metadata TOML is invalid: {err}"))
            })?;
            serde_json::to_value(value)
                .map_err(|err| Error::Config(format!("advanced metadata TOML is invalid: {err}")))?
        }
        _ => serde_json::from_str::<Value>(raw)
            .map_err(|err| Error::Config(format!("advanced metadata JSON is invalid: {err}")))?,
    };
    value
        .as_object()
        .cloned()
        .ok_or_else(|| Error::Config("advanced metadata must be an object".to_string()))
}

fn configured_model_is_free(model: &psychevo::config::ConfiguredModel) -> bool {
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
