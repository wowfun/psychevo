pub(crate) fn parse_run_config(value: Value) -> Result<RunConfig> {
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
    if let Some(compression) = object.get("compression") {
        config.compression = parse_compression_config(compression, &configured_keys)?;
    }
    if let Some(auxiliary) = object.get("auxiliary") {
        config.auxiliary = parse_auxiliary_config(auxiliary, &configured_keys)?;
    }
    if let Some(lsp) = object.get("lsp") {
        config.lsp = parse_lsp_config(lsp)?;
    }
    if let Some(project_context) = object.get("project_context") {
        config.project_context = parse_project_context_config(project_context)?;
    }
    if let Some(workspaces) = object.get("workspaces") {
        config.workspaces = parse_workspaces_config(workspaces)?;
    }
    config.permissions = parse_permission_config(object)?;
    if let Some(sandbox) = object.get("sandbox") {
        config.sandbox = parse_sandbox_config(sandbox)?;
    }
    if let Some(tools) = object.get("tools") {
        config.tools = parse_tool_selection_config(tools)?;
    }
    if let Some(web) = object.get("web") {
        config.web = parse_web_config(web)?;
    }
    if let Some(toolsets) = object.get("toolsets") {
        config.toolsets = parse_custom_toolsets(toolsets)?;
    }
    if let Some(mcp_servers) = object
        .get("mcp_servers")
        .or_else(|| object.get("mcpServers"))
    {
        config.mcp_servers = parse_profile_mcp_servers(mcp_servers)?;
    }
    if let Some(agents) = object.get("agents") {
        config.agent_backends = parse_agent_backend_configs(agents)?;
    }
    if let Some(runtime_profiles) = object
        .get("runtime_profiles")
        .or_else(|| object.get("runtimeProfiles"))
    {
        config.runtime_profiles = parse_runtime_profile_configs(runtime_profiles)?;
    }
    if let Some(channels) = object.get("channels") {
        config.channels = parse_channels_config(channels)?;
    }
    if let Some(voice) = object.get("voice") {
        config.voice = parse_voice_config(voice)?;
    }
    if let Some(image_generation) = object
        .get("image_generation")
        .or_else(|| object.get("imageGeneration"))
    {
        config.image_generation = parse_image_generation_config(image_generation)?;
    }
    if let Some(codex_plugins) = object
        .get("codex_plugins")
        .or_else(|| object.get("codexPlugins"))
    {
        config.codex_plugins = parse_codex_plugins_config(codex_plugins)?;
    }
    if let Some(plugins) = object.get("plugins") {
        config.plugins = parse_plugin_policy_config(plugins)?;
    }
    if let Some(plugins) = object
        .get("builtin_plugins")
        .or_else(|| object.get("builtinPlugins"))
    {
        config.builtin_plugins = parse_builtin_plugin_policy_config(plugins)?;
    }
    Ok(config)
}

#[path = "document/exec_policy.rs"]
mod exec_policy;
#[path = "document/models_lsp.rs"]
mod models_lsp;
#[path = "document/permissions.rs"]
mod permissions;
#[path = "document/plugins_channels.rs"]
mod plugins_channels;
#[path = "document/workspace_tools_agents.rs"]
mod workspace_tools_agents;

use std::collections::HashSet;

use serde_json::Value;

use super::validation::{parse_config_provider_entry, parse_model_selection};
use crate::config::{
    Error, Result, RunConfig, normalize_provider_id, parse_image_generation_config,
    parse_voice_config, parse_web_config,
};

pub(crate) use models_lsp::{parse_auxiliary_config, parse_compression_config, parse_lsp_config};
pub(crate) use permissions::parse_permission_config;
pub(crate) use plugins_channels::{
    parse_builtin_plugin_policy_config, parse_channels_config, parse_codex_plugins_config,
    parse_plugin_policy_config, validate_channel_id,
};
pub(crate) use workspace_tools_agents::{
    parse_agent_backend_configs, parse_custom_toolsets, parse_profile_mcp_servers,
    parse_project_context_config, parse_runtime_profile_configs, parse_sandbox_config,
    parse_tool_selection_config, parse_workspaces_config,
};
