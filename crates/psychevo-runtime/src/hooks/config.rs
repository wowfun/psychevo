use std::path::Path;

use serde_json::{Map, Value, json};

use super::runtime::HookRuntime;
use super::state::{load_hook_state_from_config, set_hook_state_in_profile};
use super::types::{HookRuntimeConfig, HookSourceDescriptor};
use crate::config::{
    CONFIG_FILE_NAME, load_config_value, load_toml_config_file, resolve_psychevo_home,
};
use crate::error::{Error, Result};
use crate::types::RunOptions;

pub fn hook_runtime_config_from_options(
    options: &RunOptions,
    cwd: &Path,
) -> Result<HookRuntimeConfig> {
    let loaded = load_config_value(options, cwd)?;
    let home = resolve_psychevo_home(&loaded.env)?;
    let profile_config = home.join(CONFIG_FILE_NAME);
    let state = load_hook_state_from_config(&profile_config)?;
    let bypass_trust = loaded
        .env
        .get("PSYCHEVO_BYPASS_HOOK_TRUST")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);
    let mut sources = Vec::new();
    for source_path in loaded.sources {
        let source_kind = if source_path == profile_config {
            "profile"
        } else if source_path.starts_with(cwd.join(".psychevo")) {
            "project"
        } else {
            "profile"
        };
        sources.extend(config_hook_sources_for_path(&source_path, source_kind)?);
    }
    Ok(HookRuntimeConfig {
        sources,
        state,
        bypass_trust,
    })
}

pub fn hook_runtime_config_with_plugin_sources_from_options(
    options: &RunOptions,
    cwd: &Path,
) -> Result<HookRuntimeConfig> {
    let mut config = hook_runtime_config_from_options(options, cwd)?;
    let loaded = crate::config::load_run_config(options, cwd)?;
    let home = resolve_psychevo_home(&loaded.env)?;
    config
        .sources
        .extend(crate::plugins::load_enabled_plugin_hook_sources(
            &home,
            cwd,
            &loaded.env,
            &loaded.config.plugins,
        ));
    Ok(config)
}

pub fn hook_metadata_value(options: &RunOptions, cwd: &Path) -> Result<Value> {
    let config = hook_runtime_config_for_review(options, cwd)?;
    let runtime = HookRuntime::new(cwd.to_path_buf(), config);
    Ok(json!({
        "hooks": runtime.metadata(),
    }))
}

pub fn trust_hook_in_profile(options: &RunOptions, cwd: &Path, hook_key: &str) -> Result<Value> {
    let config = hook_runtime_config_for_review(options, cwd)?;
    let runtime = HookRuntime::new(cwd.to_path_buf(), config);
    let metadata = runtime
        .metadata()
        .into_iter()
        .find(|metadata| metadata.key == hook_key)
        .ok_or_else(|| Error::Message(format!("hook `{hook_key}` was not found")))?;
    set_hook_state_in_profile(
        options,
        hook_key,
        Some(true),
        Some(metadata.current_hash.clone()),
    )?;
    Ok(json!({
        "success": true,
        "hook": hook_key,
        "trusted_hash": metadata.current_hash,
    }))
}

fn hook_runtime_config_for_review(options: &RunOptions, cwd: &Path) -> Result<HookRuntimeConfig> {
    hook_runtime_config_with_plugin_sources_from_options(options, cwd)
}

pub fn set_hook_enabled_in_profile(
    options: &RunOptions,
    hook_key: &str,
    enabled: bool,
) -> Result<Value> {
    set_hook_state_in_profile(options, hook_key, Some(enabled), None)?;
    Ok(json!({
        "success": true,
        "hook": hook_key,
        "enabled": enabled,
    }))
}

pub(crate) fn config_hook_sources_for_path(
    path: &Path,
    source_kind: &str,
) -> Result<Vec<HookSourceDescriptor>> {
    let mut sources = Vec::new();
    let config_value = load_toml_config_file(path, false)?;
    if let Some(hooks) = hook_declaration_object(config_value.get("hooks")) {
        sources.push(HookSourceDescriptor::new(
            format!("{source_kind}:{}#inline", path.display()),
            source_kind,
            Some(format!("{source_kind} config")),
            Some(path.to_path_buf()),
            hooks,
        ));
    }
    if let Some(parent) = path.parent() {
        let hooks_json = parent.join("hooks.json");
        if hooks_json.exists() {
            let value: Value = serde_json::from_slice(&std::fs::read(&hooks_json)?)?;
            if let Some(hooks) = hook_declaration_object(Some(&value)) {
                sources.push(HookSourceDescriptor::new(
                    format!("{source_kind}:{}#hooks.json", hooks_json.display()),
                    source_kind,
                    Some(format!("{source_kind} hooks.json")),
                    Some(hooks_json),
                    hooks,
                ));
            }
        }
    }
    Ok(sources)
}

fn hook_declaration_object(value: Option<&Value>) -> Option<Value> {
    let value = value?;
    let object = value.as_object()?;
    if let Some(nested) = object.get("hooks").and_then(Value::as_object) {
        return Some(Value::Object(
            nested
                .iter()
                .filter(|(key, _)| key.as_str() != "state")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ));
    }
    let declarations = object
        .iter()
        .filter(|(key, _)| key.as_str() != "state")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Map<_, _>>();
    (!declarations.is_empty()).then_some(Value::Object(declarations))
}
