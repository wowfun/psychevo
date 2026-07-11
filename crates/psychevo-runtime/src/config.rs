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
pub(crate) use config_types::*;
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
pub(crate) use config_file_env::*;
#[path = "config/parse.rs"]
pub(crate) mod config_parse;
#[allow(unused_imports)]
pub(crate) use config_parse::*;
#[path = "config/model_metadata.rs"]
pub(crate) mod config_model_metadata;
pub use config_model_metadata::*;
#[path = "config/resolution.rs"]
pub(crate) mod config_resolution;
#[allow(unused_imports)]
pub(crate) use config_resolution::*;
#[path = "config/catalog_helpers.rs"]
pub(crate) mod config_catalog_helpers;
pub use config_catalog_helpers::normalize_provider_id;
#[allow(unused_imports)]
pub(crate) use config_catalog_helpers::*;
#[path = "config/models.rs"]
pub(crate) mod config_models;
pub use config_models::*;
#[path = "config/custom_provider.rs"]
pub(crate) mod config_custom_provider;
pub use config_custom_provider::*;
#[path = "config/default_model.rs"]
pub(crate) mod config_default_model;
pub use config_default_model::*;
#[path = "config/cli_views.rs"]
pub(crate) mod config_cli_views;
pub use config_cli_views::*;
#[path = "config/permissions.rs"]
pub(crate) mod config_permissions;
pub use config_permissions::*;
#[path = "config/toolsets.rs"]
pub(crate) mod config_toolsets;
pub use config_toolsets::*;
#[path = "config/mcp_management.rs"]
pub(crate) mod config_mcp_management;
pub use config_mcp_management::*;
#[path = "config/channels.rs"]
pub(crate) mod config_channels;
pub use config_channels::*;
#[path = "config/voice.rs"]
pub(crate) mod config_voice;
pub use config_voice::*;
#[path = "config/image_generation.rs"]
pub(crate) mod config_image_generation;
pub use config_image_generation::*;
