use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::config_file_env::{
    CONFIG_FILE_NAME, deep_merge, load_dotenv_file, load_toml_config_file, resolve_config_path,
    resolve_config_path_from, resolve_explicit_path, resolve_psychevo_home, write_toml_config_file,
};
use super::config_parse::{
    parse_agent_backend_configs, parse_codex_plugins_config, parse_plugin_policy_config,
    parse_project_context_config, parse_run_config, parse_runtime_profile_configs,
    parse_workspaces_config,
};
use super::config_types::{
    CodexPluginsConfig, DEFAULT_WORKSPACE_NAME, LoadedConfigValue, LoadedRunConfig,
    PluginPolicyConfig, RuntimeProfileConfig,
};
use crate::agents::AgentBackendConfig;
use crate::types::{ProjectContextInstructionMode, RunOptions};
use crate::{Error, Result};

pub(crate) fn load_run_config(options: &RunOptions, cwd: &Path) -> Result<LoadedRunConfig> {
    let inherited_env = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    let mut loaded = load_run_config_from(options.config_path.as_deref(), &inherited_env, cwd)?;
    if let Some(mode) = options.project_context_override {
        loaded.config.project_context.instructions = mode;
    }
    if let Some(sandbox) = &options.sandbox_override {
        loaded.config.sandbox = crate::sandbox::SandboxConfig {
            enabled: sandbox.enabled,
            mode: match sandbox.mode {
                crate::types::RunSandboxMode::WorkspaceWrite => {
                    crate::sandbox::SandboxMode::WorkspaceWrite
                }
                crate::types::RunSandboxMode::ReadOnly => crate::sandbox::SandboxMode::ReadOnly,
            },
            writable_roots: sandbox.writable_roots.clone(),
            include_tmp: sandbox.include_tmp,
            include_common_caches: sandbox.include_common_caches,
        };
    }
    Ok(loaded)
}

pub(crate) fn load_run_config_from(
    config_path: Option<&Path>,
    inherited_env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<LoadedRunConfig> {
    let loaded = load_config_value_from(config_path, inherited_env, cwd)?;
    Ok(LoadedRunConfig {
        config: parse_run_config(loaded.value)?,
        env: loaded.env,
        sources: loaded.sources,
    })
}

pub fn load_codex_plugins_profile_config(home: &Path) -> Result<CodexPluginsConfig> {
    let value = load_toml_config_file(&home.join(CONFIG_FILE_NAME), false)?;
    value
        .get("codex_plugins")
        .or_else(|| value.get("codexPlugins"))
        .map(parse_codex_plugins_config)
        .transpose()
        .map(Option::unwrap_or_default)
}

pub fn write_codex_plugins_profile_config(
    home: &Path,
    enabled: bool,
    binary: Option<&str>,
) -> Result<Value> {
    fs::create_dir_all(home)?;
    let path = home.join(CONFIG_FILE_NAME);
    let mut value = load_toml_config_file(&path, false)?;
    if !value.is_object() {
        value = json!({});
    }
    let object = value
        .as_object_mut()
        .expect("Codex profile config root initialized as object");
    let mut authority = serde_json::Map::new();
    authority.insert("enabled".to_string(), Value::Bool(enabled));
    if let Some(binary) = binary.map(str::trim).filter(|value| !value.is_empty()) {
        authority.insert("binary".to_string(), Value::String(binary.to_string()));
    }
    object.insert("codex_plugins".to_string(), Value::Object(authority));
    object.remove("codexPlugins");
    write_toml_config_file(&path, &value)?;
    Ok(json!({
        "success": true,
        "enabled": enabled,
        "binary": binary.map(str::trim).filter(|value| !value.is_empty()),
        "path": path,
    }))
}

fn validate_project_codex_plugins(value: &Value) -> Result<()> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    if object.contains_key("codex_plugins") || object.contains_key("codexPlugins") {
        return Err(Error::Config(
            "codex_plugins is profile-only and cannot appear in project config".to_string(),
        ));
    }
    let Some(plugins) = object.get("plugins").and_then(Value::as_object) else {
        return Ok(());
    };
    for (selector, entry) in plugins {
        if selector.starts_with("codex:")
            && entry
                .as_object()
                .and_then(|entry| entry.get("enabled"))
                .and_then(Value::as_bool)
                == Some(true)
        {
            return Err(Error::Config(format!(
                "project policy cannot enable Codex plugin `{selector}`; enable it in the profile or remove the project override"
            )));
        }
    }
    Ok(())
}

pub(crate) fn load_plugin_policy_config_lenient(
    options: &RunOptions,
    cwd: &Path,
) -> Result<(PluginPolicyConfig, BTreeMap<String, String>, PathBuf)> {
    let mut env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    let home = resolve_psychevo_home(&env_map)?;
    let project_dir = cwd.join(".psychevo");
    let mut value = json!({});

    if let Some(config_path) = resolve_config_path(options, &env_map)? {
        deep_merge(&mut value, load_toml_config_file(&config_path, true)?);
        if let Some(parent) = config_path.parent() {
            load_dotenv_file(&parent.join(".env"), &mut env_map)?;
        }
    } else {
        deep_merge(
            &mut value,
            load_toml_config_file(&home.join(CONFIG_FILE_NAME), false)?,
        );
        load_dotenv_file(&home.join(".env"), &mut env_map)?;
        deep_merge(
            &mut value,
            load_toml_config_file(&project_dir.join(CONFIG_FILE_NAME), false)?,
        );
    }

    load_dotenv_file(&project_dir.join(".env"), &mut env_map)?;
    let plugins = value
        .get("plugins")
        .map(parse_plugin_policy_config)
        .transpose()?
        .unwrap_or_default();
    Ok((plugins, env_map, home))
}

pub fn resolve_workspace_root(options: &RunOptions, _cwd: &Path) -> Result<PathBuf> {
    let mut env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    let value = if let Some(config_path) = resolve_config_path(options, &env_map)? {
        let value = load_toml_config_file(&config_path, true)?;
        if let Some(parent) = config_path.parent() {
            load_dotenv_file(&parent.join(".env"), &mut env_map)?;
        }
        value
    } else {
        let home = resolve_psychevo_home(&env_map)?;
        let home_config = home.join(CONFIG_FILE_NAME);
        if !home_config.exists() {
            return Err(Error::Config(format!(
                "Psychevo home is not initialized; run `pevo init` to create {}",
                home_config.display()
            )));
        }
        let value = load_toml_config_file(&home_config, true)?;
        load_dotenv_file(&home.join(".env"), &mut env_map)?;
        value
    };
    let root = value
        .get("workspaces")
        .map(parse_workspaces_config)
        .transpose()?
        .unwrap_or_default()
        .root;
    resolve_explicit_path(Path::new(&root), &env_map)
}

pub fn resolve_default_workspace_cwd(options: &RunOptions, cwd: &Path) -> Result<PathBuf> {
    Ok(resolve_workspace_root(options, cwd)?.join(DEFAULT_WORKSPACE_NAME))
}

pub(crate) fn load_project_context_instruction_mode(
    options: &RunOptions,
    cwd: &Path,
) -> Result<ProjectContextInstructionMode> {
    if let Some(mode) = options.project_context_override {
        return Ok(mode);
    }
    let env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    let mut value = json!({});

    if let Some(config_path) = resolve_config_path(options, &env_map)? {
        deep_merge(&mut value, load_toml_config_file(&config_path, true)?);
    } else {
        if let Ok(home) = resolve_psychevo_home(&env_map) {
            deep_merge(
                &mut value,
                load_toml_config_file(&home.join(CONFIG_FILE_NAME), false)?,
            );
        }
        deep_merge(
            &mut value,
            load_toml_config_file(&cwd.join(".psychevo").join(CONFIG_FILE_NAME), false)?,
        );
    }

    value
        .get("project_context")
        .map(parse_project_context_config)
        .transpose()
        .map(|config| config.unwrap_or_default().instructions)
}

pub fn load_agent_backend_configs(
    home: &Path,
    cwd: &Path,
    env_map: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, AgentBackendConfig>> {
    let mut value = json!({});
    if let Some(config_path) = env_map
        .get("PSYCHEVO_CONFIG")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| resolve_explicit_path(Path::new(value), env_map))
        .transpose()?
    {
        deep_merge(&mut value, load_toml_config_file(&config_path, true)?);
    } else {
        deep_merge(
            &mut value,
            load_toml_config_file(&home.join(CONFIG_FILE_NAME), false)?,
        );
    }
    deep_merge(
        &mut value,
        load_toml_config_file(&cwd.join(".psychevo").join(CONFIG_FILE_NAME), false)?,
    );
    value
        .get("agents")
        .map(parse_agent_backend_configs)
        .transpose()
        .map(Option::unwrap_or_default)
}

pub fn load_runtime_profile_configs(
    home: &Path,
    cwd: &Path,
    env_map: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, RuntimeProfileConfig>> {
    let mut value = json!({});
    if let Some(config_path) = env_map
        .get("PSYCHEVO_CONFIG")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| resolve_explicit_path(Path::new(value), env_map))
        .transpose()?
    {
        deep_merge(&mut value, load_toml_config_file(&config_path, true)?);
    } else {
        deep_merge(
            &mut value,
            load_toml_config_file(&home.join(CONFIG_FILE_NAME), false)?,
        );
    }
    deep_merge(
        &mut value,
        load_toml_config_file(&cwd.join(".psychevo").join(CONFIG_FILE_NAME), false)?,
    );
    value
        .get("runtime_profiles")
        .or_else(|| value.get("runtimeProfiles"))
        .map(parse_runtime_profile_configs)
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn load_config_value(options: &RunOptions, cwd: &Path) -> Result<LoadedConfigValue> {
    let env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    load_config_value_from(options.config_path.as_deref(), &env_map, cwd)
}

fn load_config_value_from(
    config_path: Option<&Path>,
    inherited_env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<LoadedConfigValue> {
    let mut env_map = inherited_env.clone();
    let project_dir = cwd.join(".psychevo");
    let mut value = json!({});
    let mut sources = Vec::new();

    if let Some(config_path) = resolve_config_path_from(config_path, &env_map)? {
        let loaded = load_toml_config_file(&config_path, true)?;
        if config_path == project_dir.join(CONFIG_FILE_NAME) {
            validate_project_codex_plugins(&loaded)?;
        }
        deep_merge(&mut value, loaded);
        sources.push(config_path.clone());
        if let Some(parent) = config_path.parent() {
            load_dotenv_file(&parent.join(".env"), &mut env_map)?;
        }
    } else {
        let home = resolve_psychevo_home(&env_map)?;
        let home_config = home.join(CONFIG_FILE_NAME);
        if !home_config.exists() {
            return Err(Error::Config(format!(
                "Psychevo home is not initialized; run `pevo init` to create {}",
                home_config.display()
            )));
        }
        let loaded = load_toml_config_file(&home_config, true)?;
        deep_merge(&mut value, loaded);
        sources.push(home_config);
        load_dotenv_file(&home.join(".env"), &mut env_map)?;
        let project_config = project_dir.join(CONFIG_FILE_NAME);
        let loaded = load_toml_config_file(&project_config, false)?;
        validate_project_codex_plugins(&loaded)?;
        if project_config.exists() {
            sources.push(project_config);
        }
        deep_merge(&mut value, loaded);
    }

    load_dotenv_file(&project_dir.join(".env"), &mut env_map)?;
    Ok(LoadedConfigValue {
        value,
        env: env_map,
        sources,
    })
}
