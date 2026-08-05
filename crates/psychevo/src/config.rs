use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageGenerationFormat {
    Png,
    Jpeg,
    Webp,
}

impl ImageGenerationFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceAudioFormat {
    Wav,
    Mp3,
    Pcm16,
}

impl VoiceAudioFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Pcm16 => "pcm16",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::Mp3 => "audio/mpeg",
            Self::Pcm16 => "audio/pcm",
        }
    }

    pub fn supports_asr_input(self) -> bool {
        matches!(self, Self::Wav | Self::Mp3)
    }

    pub fn supports_tts_output(self) -> bool {
        matches!(self, Self::Wav | Self::Pcm16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceRealtimeTransport {
    Webrtc,
    Websocket,
}

pub(crate) use crate::agents::{
    AgentBackendConfig, AgentBackendKind, AgentEntrypoint, default_peer_agent_entrypoints,
    default_peer_client_capabilities, valid_agent_name,
};
pub(crate) use crate::error::{Error, Result};
pub(crate) use crate::sandbox::{SandboxConfig, SandboxMode};
pub(crate) use crate::types::{
    ApprovalPolicy, ApprovalsReviewer, AutoReviewConfig, ExecPolicyConfig, ExecPolicyDecision,
    ExecPolicyExample, ExecPolicyHostExecutable, ExecPolicyPatternToken, ExecPolicyRule,
    GranularApprovalConfig, McpServerInput, McpServerPolicy, McpTransportInput, ModelCost,
    ModelCostTier, ModelLimits, ModelMetadata, PermissionAccess, PermissionConfig,
    PermissionProfileConfig, ProjectContextInstructionMode,
};
pub use crate::types::{ConfigScope, ConfiguredModel, ModelCatalogEntry, ModelCatalogProvider};

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
    pub(crate) codex_plugins: CodexPluginsConfig,
    pub(crate) plugins: PluginPolicyConfig,
    pub(crate) builtin_plugins: BuiltinPluginPolicyConfig,
}

// Configuration internals are split by loading, parsing, resolution, and catalog concerns.
#[path = "config/types.rs"]
pub(crate) mod config_types;
pub(crate) use config_types::{
    AuxiliaryConfig, AuxiliaryTaskConfig, BuiltinPluginPolicyConfig, ChannelConnectionConfig,
    ChannelPlatform, ChannelTransport, ChannelsConfig, CompressionConfig, ConfigModelEntry,
    ConfigProviderEntry, CustomToolsetConfig, ImageGenerationConfig, LoadedRunConfig, LspConfig,
    ModelSelection, PluginPolicyConfig, PluginPolicyEntry, ProjectContextConfig,
    ResolvedRunProvider, ToolModeConfig, ToolSearchConfig, ToolSelectionConfig,
    ToolsetContribution, VoiceConfig, WebConfig, WebSearchBackend, WebSearchConfig,
    WebSearchExecution, WorkspacesConfig,
};
pub use config_types::{
    CodexPluginsConfig, DEFAULT_WORKSPACE_NAME, DEFAULT_WORKSPACE_ROOT, REASONING_EFFORT_VALUES,
    ResolvedImageGenerationConfig, ResolvedVoiceAsrConfig, ResolvedVoiceRealtimeConfig,
    ResolvedVoiceTtsConfig, RuntimeProfileConfig, RuntimeProfileKind,
    generated_runtime_profile_id_for_backend, validate_runtime_profile_backend_ref,
};
#[path = "config/loading.rs"]
pub(crate) mod config_loading;
pub use config_loading::{
    load_agent_backend_configs, load_codex_plugins_profile_config, load_runtime_profile_configs,
    resolve_default_workspace_cwd, resolve_workspace_root, write_codex_plugins_profile_config,
};
pub(crate) use config_loading::{
    load_config_value, load_plugin_policy_config_lenient, load_project_context_instruction_mode,
    load_run_config, load_run_config_from,
};
#[path = "config/file_env.rs"]
pub(crate) mod config_file_env;
pub(crate) use config_file_env::{
    CONFIG_FILE_NAME, deep_merge, load_toml_config_file, resolve_config_path,
    resolve_psychevo_home, valid_env_name, write_toml_config_file,
};
#[path = "config/parse.rs"]
pub(crate) mod config_parse;
pub(crate) use config_parse::{parse_plugin_policy_config, parse_run_config};
#[path = "config/model_metadata.rs"]
pub(crate) mod config_model_metadata;
pub use config_model_metadata::refresh_model_metadata_cache;
#[path = "config/resolution.rs"]
pub(crate) mod config_resolution;
pub(crate) use config_resolution::{
    model_selection_from_raw, resolve_compression_config, resolve_one_provider,
    resolve_run_provider, resolve_title_generation_provider,
};
#[path = "config/catalog_helpers.rs"]
pub(crate) mod config_catalog_helpers;
pub use config_catalog_helpers::normalize_provider_id;
#[path = "config/models.rs"]
pub(crate) mod config_models;
pub use config_models::{
    PROVIDER_MODELS_CACHE_FILE, PROVIDER_MODELS_CACHE_VERSION, configured_models,
    fetch_and_cache_model_catalog, fetch_model_catalog, fetch_model_catalog_with_client,
    model_catalog_endpoint, model_catalog_entry_is_free, model_catalog_provider,
    model_catalog_providers, provider_models_cache_fingerprint,
    provider_models_cache_path_for_home, read_cached_model_catalog, selected_configured_model,
    write_cached_model_catalog,
};
#[path = "config/custom_provider.rs"]
pub(crate) mod config_custom_provider;
pub(crate) use config_custom_provider::valid_provider_id;
pub use config_custom_provider::{
    create_global_custom_provider, create_scoped_custom_provider, custom_provider_api_key_env,
    set_provider_api_key, set_provider_model_config,
};
#[path = "config/default_model.rs"]
pub(crate) mod config_default_model;
pub(crate) use config_default_model::parse_provider_model_spec;
pub use config_default_model::{
    set_auxiliary_model, set_auxiliary_model_with_reasoning, set_default_model,
    set_default_model_with_reasoning,
};
#[path = "config/cli_views.rs"]
pub(crate) mod config_cli_views;
pub use config_cli_views::{
    ConfigRemoveResult, ConfigSetResult, auth_status_value, config_provider_list_value,
    config_show_value, remove_config_value, set_config_value,
};
#[path = "config/permissions.rs"]
pub(crate) mod config_permissions;
pub(crate) use config_permissions::append_local_web_search_grant_with_extends;
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
#[path = "config/mcp_management.rs"]
pub(crate) mod config_mcp_management;
pub use config_mcp_management::{
    MCP_OAUTH_KEYRING_SERVICE, McpOAuthCredentialStore, McpServerConfigInput, McpToolPolicyInput,
    SystemMcpOAuthCredentialStore, clear_mcp_oauth_access_token,
    clear_mcp_oauth_access_token_with_store, load_mcp_oauth_access_token,
    load_mcp_oauth_access_token_with_store, mcp_oauth_keyring_account, mcp_server_value,
    mcp_servers_value, remove_mcp_server, save_mcp_oauth_access_token,
    save_mcp_oauth_access_token_with_store, set_mcp_server_enabled, set_mcp_server_tool_policy,
    upsert_mcp_server,
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
