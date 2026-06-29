use std::path::Path;

use serde_json::{Map, Value, json};

use super::types::{HookStateRecord, HookStateStore};
use crate::config::{
    CONFIG_FILE_NAME, load_toml_config_file, resolve_psychevo_home, write_toml_config_file,
};
use crate::error::{Error, Result};
use crate::types::RunOptions;

pub(crate) fn load_hook_state_from_config(path: &Path) -> Result<HookStateStore> {
    let value = load_toml_config_file(path, false)?;
    let state = value
        .get("hooks")
        .and_then(|hooks| hooks.get("state"))
        .and_then(Value::as_object)
        .map(|state| {
            state
                .iter()
                .map(|(key, value)| {
                    let enabled = value
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(true);
                    let trusted_hash = value
                        .get("trusted_hash")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    (
                        key.clone(),
                        HookStateRecord {
                            enabled,
                            trusted_hash,
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(HookStateStore { state })
}

pub(crate) fn set_hook_state_in_profile(
    options: &RunOptions,
    hook_key: &str,
    enabled: Option<bool>,
    trusted_hash: Option<String>,
) -> Result<()> {
    let env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| std::env::vars().collect());
    let home = resolve_psychevo_home(&env_map)?;
    let path = home.join(CONFIG_FILE_NAME);
    let mut value = load_toml_config_file(&path, false)?;
    let root = value
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let hooks = root
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| Error::Config("hooks must be an object".to_string()))?;
    let state = hooks
        .entry("state".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| Error::Config("hooks.state must be an object".to_string()))?;
    let entry = state
        .entry(hook_key.to_string())
        .or_insert_with(|| json!({"enabled": true}))
        .as_object_mut()
        .ok_or_else(|| Error::Config(format!("hooks.state.{hook_key} must be an object")))?;
    if let Some(enabled) = enabled {
        entry.insert("enabled".to_string(), Value::Bool(enabled));
    }
    if let Some(hash) = trusted_hash {
        entry.insert("trusted_hash".to_string(), Value::String(hash));
    }
    write_toml_config_file(&path, &value)
}
