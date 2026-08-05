use std::collections::BTreeMap;
use std::path::Path;

use psychevo::command_registry::{
    parse_key_chord_display, parse_key_sequence_display, parse_shared_slash_config,
    slash_command_spec, split_key_sequence_list, split_slash_command_token,
    validate_configured_alias, validate_configured_slash_target, validate_shared_slash_config,
};
use psychevo::config::ConfigScope;
use psychevo::config::set_config_value;
use psychevo::{ConfigurationQuery, Error};
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};

use super::super::agents::active_profile_config_dir;
use super::super::binding::WebState;
use super::super::scope_session::ResolvedScope;
use super::{GatewaySlashAlias, GatewaySlashConfig, GatewaySlashKeybind};

pub(in super::super) fn slash_settings_read_value(
    state: &WebState,
    scope: &ResolvedScope,
    cwd: &Path,
) -> psychevo::Result<Value> {
    Ok(serde_json::to_value(slash_settings_result(
        state,
        scope,
        cwd,
        profile_slash_config(state, scope)?,
        Vec::new(),
    )?)?)
}

pub(in super::super) fn slash_settings_update_value(
    state: &WebState,
    scope: &ResolvedScope,
    cwd: &Path,
    params: wire::thread_command_turn::SlashSettingsUpdateParams,
) -> psychevo::Result<Value> {
    if params.scope != wire::settings_workspace_context::ModelSettingsScope::Global {
        return Err(Error::Config(
            "slash settings writes support only global scope".to_string(),
        ));
    }
    let config = slash_config_from_update(params)?;
    let config_dir = active_profile_config_dir(state, scope);
    let aliases = slash_aliases_config_value(&config.aliases);
    let keybinds = slash_keybinds_config_value(&config.keybinds);
    set_config_value(
        config_dir.clone(),
        "tui.leader_key",
        json!(config.leader_key),
    )?;
    set_config_value(
        config_dir.clone(),
        "tui.leader_timeout_ms",
        json!(config.leader_timeout_ms),
    )?;
    set_config_value(config_dir.clone(), "tui.slash_aliases", aliases)?;
    set_config_value(config_dir, "tui.slash_keybinds", keybinds)?;
    Ok(serde_json::to_value(slash_settings_result(
        state,
        scope,
        cwd,
        profile_slash_config(state, scope)?,
        Vec::new(),
    )?)?)
}

fn slash_settings_result(
    _state: &WebState,
    _scope: &ResolvedScope,
    cwd: &Path,
    config: GatewaySlashConfig,
    diagnostics: Vec<String>,
) -> psychevo::Result<wire::thread_command_turn::SlashSettingsResult> {
    Ok(wire::thread_command_turn::SlashSettingsResult {
        scope: wire::settings_workspace_context::ModelSettingsScope::Global,
        cwd: cwd.display().to_string(),
        leader_key: config.leader_key,
        leader_timeout_ms: config.leader_timeout_ms,
        aliases: config
            .aliases
            .into_iter()
            .map(|entry| wire::thread_command_turn::SlashAliasSetting {
                target_summary: slash_target_summary(&entry.target),
                alias: entry.alias,
                target: entry.target,
            })
            .collect(),
        keybinds: config
            .keybinds
            .into_iter()
            .map(|entry| wire::thread_command_turn::SlashKeybindSetting {
                target_summary: slash_target_summary(&entry.target),
                shortcut: entry.shortcut,
                target: entry.target,
            })
            .collect(),
        diagnostics,
    })
}

pub(super) fn effective_slash_config(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo::Result<GatewaySlashConfig> {
    let mut query = ConfigurationQuery::new(&scope.cwd);
    query.inherited_env = Some(state.inner.inherited_env.clone());
    let configuration = state.inner.framework.configuration(query)?;
    let document = match configuration.config_value(ConfigScope::Effective) {
        Ok(document) => document,
        Err(Error::Config(message)) if message.contains("home is not initialized") => {
            return Ok(default_gateway_slash_config());
        }
        Err(err) => return Err(err),
    };
    parse_gateway_slash_config(document.get("value").unwrap_or(&Value::Null))
}

fn profile_slash_config(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo::Result<GatewaySlashConfig> {
    let path = active_profile_config_dir(state, scope).join("config.toml");
    let value = read_toml_config_value(&path)?;
    parse_gateway_slash_config(&value)
}

fn read_toml_config_value(path: &Path) -> psychevo::Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = std::fs::read_to_string(path)?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let parsed: toml::Value =
        toml::from_str(&text).map_err(|err| Error::Config(format!("{}: {err}", path.display())))?;
    Ok(serde_json::to_value(parsed)?)
}

fn parse_gateway_slash_config(root: &Value) -> psychevo::Result<GatewaySlashConfig> {
    parse_shared_slash_config(root)
}

fn default_gateway_slash_config() -> GatewaySlashConfig {
    GatewaySlashConfig::default()
}

fn slash_config_from_update(
    params: wire::thread_command_turn::SlashSettingsUpdateParams,
) -> psychevo::Result<GatewaySlashConfig> {
    let leader_key = match params.leader_key {
        Some(value) => parse_key_chord_display(&value, "leaderKey")?,
        None => default_gateway_slash_config().leader_key,
    };
    let leader_timeout_ms = params
        .leader_timeout_ms
        .unwrap_or_else(|| default_gateway_slash_config().leader_timeout_ms);
    if leader_timeout_ms == 0 {
        return Err(Error::Config(
            "leaderTimeoutMs must be a positive integer".to_string(),
        ));
    }
    let aliases = params
        .aliases
        .into_iter()
        .map(|entry| {
            Ok(GatewaySlashAlias {
                alias: validate_configured_alias(&entry.alias, "aliases[].alias")?,
                target: validate_configured_slash_target(&entry.target, "aliases[].target")?,
            })
        })
        .collect::<psychevo::Result<Vec<_>>>()?;
    let keybinds = params
        .keybinds
        .into_iter()
        .flat_map(|entry| {
            split_key_sequence_list(&entry.shortcut)
                .into_iter()
                .map(move |shortcut| (shortcut, entry.target.clone()))
        })
        .filter(|(shortcut, _)| !shortcut.eq_ignore_ascii_case("none"))
        .map(|(shortcut, target)| {
            Ok(GatewaySlashKeybind {
                shortcut: parse_key_sequence_display(&shortcut, "keybinds[].shortcut")?,
                target: validate_configured_slash_target(&target, "keybinds[].target")?,
            })
        })
        .collect::<psychevo::Result<Vec<_>>>()?;
    let config = GatewaySlashConfig {
        leader_key,
        leader_timeout_ms,
        aliases,
        keybinds,
    };
    validate_shared_slash_config(&config)?;
    Ok(config)
}

fn slash_aliases_config_value(aliases: &[GatewaySlashAlias]) -> Value {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for alias in aliases {
        grouped
            .entry(alias.target.clone())
            .or_default()
            .push(alias.alias.clone());
    }
    json!(grouped)
}

fn slash_keybinds_config_value(keybinds: &[GatewaySlashKeybind]) -> Value {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for keybind in keybinds {
        grouped
            .entry(keybind.target.clone())
            .or_default()
            .push(keybind.shortcut.clone());
    }
    json!(grouped)
}

fn slash_target_summary(target: &str) -> Option<String> {
    let (command, _) = split_slash_command_token(target);
    slash_command_spec(command).map(|spec| spec.summary.to_string())
}
