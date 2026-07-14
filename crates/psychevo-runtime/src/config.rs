pub(crate) use std::collections::{BTreeMap, BTreeSet, HashSet};
pub(crate) use std::env;
pub(crate) use std::fs;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::time::Duration;

pub use psychevo_ai::{ImageGenerationFormat, VoiceAudioFormat, VoiceRealtimeTransport};
pub(crate) use serde_json::{Value, json};

pub(crate) use crate::agents::{
    AgentBackendConfig, AgentBackendKind, AgentEntrypoint, default_peer_agent_entrypoints,
    default_peer_client_capabilities, valid_agent_name,
};
pub(crate) use crate::error::{Error, Result};
pub(crate) use crate::paths::canonical_cwd;
pub(crate) use crate::sandbox::{SandboxConfig, SandboxMode};
pub(crate) use crate::types::{
    ApprovalPolicy, ApprovalsReviewer, AutoReviewConfig, ConfigScope, ConfiguredModel,
    CustomProviderInput, CustomProviderResult, ExecPolicyConfig, ExecPolicyDecision,
    ExecPolicyExample, ExecPolicyHostExecutable, ExecPolicyPatternToken, ExecPolicyRule,
    GranularApprovalConfig, McpServerInput, McpServerPolicy, McpTransportInput, ModelCapabilities,
    ModelCatalogEntry, ModelCatalogProvider, ModelCost, ModelCostTier, ModelLimits, ModelMetadata,
    ModelMetadataCacheTarget, PermissionAccess, PermissionConfig, PermissionProfileConfig,
    ProjectContextInstructionMode, RunMode, RunOptions, ScopedCustomProviderInput,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct RunConfig {
    pub(crate) model: ModelSelection,
    pub(crate) provider: BTreeMap<String, ConfigProviderEntry>,
    pub(crate) compression: CompressionConfig,
    pub(crate) auxiliary: AuxiliaryConfig,
    pub(crate) permissions: PermissionConfig,
    pub(crate) sandbox: SandboxConfig,
    pub(crate) lsp: LspConfig,
    pub(crate) project_context: ProjectContextConfig,
    pub(crate) workspaces: WorkspacesConfig,
    pub(crate) tools: ToolSelectionConfig,
    pub(crate) web: WebConfig,
    pub(crate) toolsets: BTreeMap<String, CustomToolsetConfig>,
    pub(crate) mcp_servers: Vec<McpServerInput>,
    pub(crate) agent_backends: BTreeMap<String, AgentBackendConfig>,
    pub(crate) runtime_profiles: BTreeMap<String, RuntimeProfileConfig>,
    pub(crate) channels: ChannelsConfig,
    pub(crate) voice: VoiceConfig,
    pub(crate) image_generation: ImageGenerationConfig,
    pub(crate) plugins: PluginPolicyConfig,
    pub(crate) builtin_plugins: BuiltinPluginPolicyConfig,
}

// Configuration internals are split by loading, parsing, resolution, and catalog concerns.
#[path = "config/types.rs"]
pub(crate) mod config_types;
#[allow(unused_imports)]
pub(crate) use config_types::{
    AUTO_PROVIDER_ORDER, AuxiliaryConfig, AuxiliaryTaskConfig, BUILT_IN_PROVIDERS, BuiltInProvider,
    BuiltinPluginPolicyConfig, ChannelConnectionConfig, ChannelPlatform, ChannelTransport,
    ChannelsConfig, CompressionConfig, ConfigModelEntry, ConfigProviderEntry, CustomToolsetConfig,
    ImageGenerationConfig, LoadedConfigValue, LoadedRunConfig, LspConfig, MODEL_CATALOG_TIMEOUT,
    ModelSelection, PluginPolicyConfig, PluginPolicyEntry, ProjectContextConfig,
    ResolvedCompressionConfig, ResolvedRunProvider, ToolModeConfig, ToolSearchConfig,
    ToolSelectionConfig, ToolsetContribution, VoiceAsrConfig, VoiceConfig, VoiceRealtimeConfig,
    VoiceTtsConfig, WebConfig, WebSearchBackend, WebSearchConfig, WebSearchContentType,
    WebSearchContextSize, WebSearchExecution, WebSearchExternalAccess, WebSearchImageConfig,
    WebSearchLocation, WebSearchTokenBudget, WorkspacesConfig, load_config_value,
    load_plugin_policy_config_lenient, load_project_context_instruction_mode, load_run_config,
};
pub use config_types::{
    DEFAULT_WORKSPACE_NAME, DEFAULT_WORKSPACE_ROOT, REASONING_EFFORT_VALUES,
    ResolvedImageGenerationConfig, ResolvedVoiceAsrConfig, ResolvedVoiceRealtimeConfig,
    ResolvedVoiceTtsConfig, RuntimeProfileConfig, RuntimeProfileKind, load_agent_backend_configs,
    load_runtime_profile_configs, resolve_default_workspace_cwd, resolve_workspace_root,
    validate_runtime_profile_backend_ref,
};
#[path = "config/file_env.rs"]
pub(crate) mod config_file_env;
#[allow(unused_imports)]
pub(crate) use config_file_env::{
    CONFIG_FILE_NAME, deep_merge, expand_tilde, home_path, json_to_toml_value, load_dotenv_file,
    load_toml_config_file, resolve_config_path, resolve_explicit_path, resolve_psychevo_home,
    strip_env_quotes, toml_config_string, valid_env_name, write_toml_config_file,
};
#[path = "config/parse.rs"]
pub(crate) mod config_parse;
#[allow(unused_imports)]
pub(crate) use config_parse::{
    enabled_reasoning_effort, exec_policy_example, exec_policy_examples_field,
    exec_policy_prefix_field, first_string_array_field, non_empty_string, optional_bool_field,
    optional_f64_field, optional_string_alias_field, optional_string_field, optional_u64_field,
    parse_access_map, parse_agent_backend_client_capabilities, parse_agent_backend_config,
    parse_agent_backend_configs, parse_agent_backend_entrypoints, parse_approval_config,
    parse_auto_review_config, parse_auxiliary_config, parse_auxiliary_task_config,
    parse_builtin_plugin_policy_config, parse_channel_connection, parse_channels_config,
    parse_compression_config, parse_config_model_entry, parse_config_model_metadata,
    parse_config_provider_entry, parse_custom_toolsets, parse_exec_policy_config,
    parse_host_executables, parse_lsp_config, parse_mcp_server_policy, parse_model_cost,
    parse_model_cost_tier, parse_model_limits, parse_model_selection, parse_network_domains,
    parse_permission_config, parse_permission_profile, parse_plugin_policy_config,
    parse_profile_mcp_servers, parse_project_context_config, parse_run_config,
    parse_runtime_profile_config, parse_runtime_profile_configs, parse_sandbox_config,
    parse_string_array_value, parse_tool_grants, parse_tool_mode_config,
    parse_tool_selection_config, parse_web_search_queries, parse_workspaces_config,
    reject_legacy_permission_keys, required_bool_field, string_array_field, string_map_field,
    validate_channel_id, validate_exec_policy_rule_examples, validate_permission_profile_name,
    validate_plugin_policy_name, validate_reasoning_effort, validate_toolset_name,
};
#[path = "config/model_metadata.rs"]
pub(crate) mod config_model_metadata;
pub use config_model_metadata::refresh_model_metadata_cache;
#[allow(unused_imports)]
pub(crate) use config_model_metadata::{
    MODELS_DEV_CACHE_FILE, MODELS_DEV_FETCH_TIMEOUT_SECS, MODELS_DEV_URL, MODELS_DEV_URL_ENV,
    bool_from_keys, built_in_limits_metadata, built_in_model_metadata, f64_from_keys,
    fetch_models_dev_registry, merge_capabilities, merge_model_metadata,
    metadata_from_models_dev_model, models_dev_cache_path, models_dev_cache_path_for_home,
    models_dev_metadata, models_dev_model_entry, models_dev_provider_candidates,
    models_dev_provider_key, models_dev_url, normalize_base_url, parse_metadata_capabilities,
    parse_metadata_cost, parse_metadata_cost_tier, parse_metadata_limits, pricing_value,
    provider_without_models, prune_models_dev_registry, read_json_file, read_models_dev_cache,
    resolve_model_metadata_cache_first, string_from_keys, string_vec_from_value, u64_from_keys,
};
#[path = "config/resolution.rs"]
pub(crate) mod config_resolution;
#[allow(unused_imports)]
pub(crate) use config_resolution::{
    model_for_provider, model_selection_from_raw, parse_model_override,
    resolve_auxiliary_task_provider, resolve_compression_config, resolve_one_provider,
    resolve_run_provider, resolve_title_generation_provider,
};
#[path = "config/catalog_helpers.rs"]
pub(crate) mod config_catalog_helpers;
pub use config_catalog_helpers::normalize_provider_id;
#[allow(unused_imports)]
pub(crate) use config_catalog_helpers::{
    built_in_provider, catalog_provider_for, config_model_entry, first_string,
    infer_provider_for_model, parse_model_catalog_response, provider_api_key_env,
    provider_base_url, truncate_error, unique_config_model,
};
#[path = "config/models.rs"]
pub(crate) mod config_models;
#[allow(unused_imports)]
pub use config_models::{
    PROVIDER_MODELS_CACHE_FILE, PROVIDER_MODELS_CACHE_VERSION, configured_models,
    fetch_and_cache_model_catalog, fetch_model_catalog, fetch_model_catalog_with_client,
    model_catalog_endpoint, model_catalog_entry_is_free, model_catalog_provider,
    model_catalog_providers, provider_models_cache_fingerprint,
    provider_models_cache_path_for_home, read_cached_model_catalog, selected_configured_model,
    write_cached_model_catalog,
};
#[allow(unused_imports)]
pub(crate) use config_models::{
    env_value, is_loopback_base_url, model_cost_is_free, provider_label,
    selected_configured_model_for_provider,
};
#[path = "config/custom_provider.rs"]
pub(crate) mod config_custom_provider;
#[allow(unused_imports)]
pub(crate) use config_custom_provider::{
    append_dotenv_value, ensure_json_object, set_dotenv_value, valid_provider_id,
    validate_custom_provider_id, write_provider_config,
};
pub use config_custom_provider::{
    create_global_custom_provider, create_scoped_custom_provider, custom_provider_api_key_env,
    set_provider_api_key, set_provider_model_config,
};
#[path = "config/default_model.rs"]
pub(crate) mod config_default_model;
#[allow(unused_imports)]
pub(crate) use config_default_model::{
    parse_provider_model_spec, validate_auxiliary_model_task, validate_default_model_provider,
};
pub use config_default_model::{
    set_auxiliary_model, set_auxiliary_model_with_reasoning, set_default_model,
    set_default_model_with_reasoning,
};
#[path = "config/cli_views.rs"]
pub(crate) mod config_cli_views;
#[allow(unused_imports)]
pub use config_cli_views::{
    ConfigRemoveResult, ConfigSetResult, auth_status_value, config_provider_list_value,
    config_show_value, remove_config_value, set_config_value,
};
#[allow(unused_imports)]
pub(crate) use config_cli_views::{
    config_document_value, redact_sensitive_config, remove_config_path_value, set_config_path_value,
};
#[path = "config/permissions.rs"]
pub(crate) mod config_permissions;
#[allow(unused_imports)]
pub(crate) use config_permissions::{
    LegacyPermissionMutation, access_for_legacy_kind, access_map_value,
    append_local_web_search_grant_with_extends, ensure_local_profile, exec_prefix_from_command,
    exec_prefix_strings_from_value, exec_prefix_value, legacy_rule_parts, local_profile_object_mut,
    mutate_local_config, nested_object_mut, normalize_permission_rule, object_entry_mut,
    parse_legacy_rule_for_mutation, permission_config_value, permission_decision_for_legacy_kind,
    remove_exec_policy_rule, remove_profile_access, root_object_mut, set_string_entry,
    validate_permission_rule_kind, web_fetch_host,
};
#[allow(unused_imports)]
pub use config_permissions::{
    PermissionRuleMutationResult, append_local_exec_policy_rule, append_local_filesystem_grant,
    append_local_filesystem_grant_with_extends, append_local_network_grant,
    append_local_network_grant_with_extends, append_local_permission_allow_rule,
    append_local_permission_rule, append_local_skill_grant, append_local_skill_grant_with_extends,
    permission_rules_value, remove_local_permission_rule,
};
#[path = "config/toolsets.rs"]
pub(crate) mod config_toolsets;
pub use config_toolsets::{
    ToolsetMutationResult, create_local_toolset, remove_local_toolset, set_local_toolset_enabled,
    toolsets_value,
};
#[allow(unused_imports)]
pub(crate) use config_toolsets::{
    mode_toolset_array_mut, normalize_toolset_name, push_unique_string, remove_mode_toolset_entry,
    validate_toolset_entries,
};
#[path = "config/mcp_management.rs"]
pub(crate) mod config_mcp_management;
#[allow(unused_imports)]
pub use config_mcp_management::{
    MCP_OAUTH_KEYRING_SERVICE, McpServerConfigInput, McpToolPolicyInput,
    clear_mcp_oauth_access_token, load_mcp_oauth_access_token, mcp_oauth_keyring_account,
    mcp_server_value, mcp_servers_value, remove_mcp_server, save_mcp_oauth_access_token,
    set_mcp_server_enabled, set_mcp_server_tool_policy, upsert_mcp_server,
};
#[path = "config/channels.rs"]
pub(crate) mod config_channels;
pub use config_channels::{
    ChannelRuntimeConnection, ChannelSetupInput, ChannelUpdateInput, channel_doctor_value,
    channel_list_value, channel_runtime_connections, channel_show_value, channel_summary_value,
    delete_channel_connection, set_channel_enabled, setup_channel_connection,
    update_channel_connection, upsert_channel_connection,
};
#[path = "config/voice.rs"]
pub(crate) mod config_voice;
pub(crate) use config_voice::parse_voice_config;
pub use config_voice::{
    resolve_voice_asr_config, resolve_voice_realtime_config, resolve_voice_tts_config,
    voice_config_value,
};
#[path = "config/image_generation.rs"]
pub(crate) mod config_image_generation;
pub use config_image_generation::{image_generation_config_value, resolve_image_generation_config};
pub(crate) use config_image_generation::{
    parse_image_generation_config, resolve_image_generation_config_from_loaded,
};
#[path = "config/web_search.rs"]
pub(crate) mod config_web_search;
pub(crate) use config_web_search::{
    hosted_web_search_value, parse_web_config, resolve_web_search_execution,
};
pub use config_web_search::{update_global_web_search_settings, web_search_settings_value};
