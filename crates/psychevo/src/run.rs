use psychevo_ai::{Anthropic, DeploymentConfig, OpenAi, Provider, SecretValue, TimeoutPolicy};

use crate::error::{Error, Result};

#[path = "run/entrypoints.rs"]
mod entrypoints;
pub(crate) use entrypoints::reload_session_context;
pub(crate) use entrypoints::run_live_streaming_controlled;
pub(crate) use entrypoints::{
    SESSION_TITLE_MAX_CHARS, run_live_streaming_controlled_with_provider, start_agent_task,
};
#[path = "run/execution.rs"]
mod execution;
#[cfg(test)]
pub(crate) use execution::{
    materialize_first_use_empty_session, run_live_internal, should_title_visible_first_turn,
};
#[path = "run/titles.rs"]
mod titles;
#[cfg(test)]
pub(crate) use titles::{
    ensure_new_visible_session_title, fallback_session_title, session_title_request,
};
pub(crate) use titles::{
    normalize_session_title, visible_session_source_allows_auto_title, warning_event,
};

pub(crate) fn generation_provider(
    base_url: impl Into<String>,
    api_key: impl Into<String>,
    provider: impl Into<String>,
    inference_idle_timeout_secs: u64,
) -> Result<Provider> {
    let base_url = base_url.into();
    let api_key = api_key.into();
    let provider = provider.into();
    let provider_family = crate::config::normalize_provider_id(&provider);
    let protocol = language_protocol_for_provider(&provider_family);
    let config = DeploymentConfig::new(provider_family.clone(), provider_family.clone(), base_url)
        .with_default_language_protocol(protocol)
        .with_timeout_policy(TimeoutPolicy {
            progress_idle_timeout_secs: inference_idle_timeout_secs,
            ..TimeoutPolicy::default()
        });
    let result = if provider_family == "anthropic" {
        let builder = Anthropic::builder(config);
        if api_key.trim().is_empty() {
            builder.build()
        } else {
            builder.with_api_key(SecretValue::new(api_key)).build()
        }
        .and_then(|facade| facade.provider())
    } else {
        let builder = OpenAi::builder(config);
        if api_key.trim().is_empty() {
            builder.build()
        } else {
            builder.with_api_key(SecretValue::new(api_key)).build()
        }
        .and_then(|facade| facade.provider())
    };
    result.map_err(|error| Error::Config(error.to_string()))
}

pub(crate) fn language_protocol_for_provider(provider: &str) -> &'static str {
    match crate::config::normalize_provider_id(provider).as_str() {
        "openai" => "openai_responses",
        "anthropic" => "anthropic_messages",
        _ => "openai_chat",
    }
}
