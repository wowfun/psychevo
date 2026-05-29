pub(crate) use std::collections::{BTreeMap, BTreeSet, HashSet};
pub(crate) use std::env;
pub(crate) use std::fs;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::time::Duration;

pub(crate) use serde_json::{Value, json};

pub(crate) use crate::error::{Error, Result};
pub(crate) use crate::paths::canonical_workdir;
pub(crate) use crate::types::{
    ApprovalPolicy, ApprovalsReviewer, AutoReviewConfig, ConfigScope, ConfiguredModel,
    CustomProviderInput, CustomProviderResult, ExecPolicyConfig, ExecPolicyDecision,
    ExecPolicyExample, ExecPolicyHostExecutable, ExecPolicyPatternToken, ExecPolicyRule,
    GranularApprovalConfig, ModelCapabilities, ModelCatalogEntry, ModelCatalogProvider, ModelCost,
    ModelCostTier, ModelLimits, ModelMetadata, ModelMetadataCacheTarget, PermissionAccess,
    PermissionConfig, PermissionProfileConfig, RunMode, RunOptions, ScopedCustomProviderInput,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct RunConfig {
    pub(crate) model: ModelSelection,
    pub(crate) provider: BTreeMap<String, ConfigProviderEntry>,
    pub(crate) compression: CompressionConfig,
    pub(crate) permissions: PermissionConfig,
    pub(crate) lsp: LspConfig,
    pub(crate) tools: ToolSelectionConfig,
    pub(crate) toolsets: BTreeMap<String, CustomToolsetConfig>,
}

// Configuration internals are split by loading, parsing, resolution, and catalog concerns.
#[path = "config/types.rs"]
pub(crate) mod config_types;
#[allow(unused_imports)]
pub(crate) use config_types::*;
#[path = "config/file_env.rs"]
pub(crate) mod config_file_env;
#[allow(unused_imports)]
pub(crate) use config_file_env::*;
#[path = "config/parse.rs"]
pub(crate) mod config_parse;
#[allow(unused_imports)]
use config_parse::*;
#[path = "config/model_metadata.rs"]
pub(crate) mod config_model_metadata;
pub use config_model_metadata::*;
#[path = "config/resolution.rs"]
pub(crate) mod config_resolution;
#[allow(unused_imports)]
pub(crate) use config_resolution::*;
#[path = "config/catalog_helpers.rs"]
pub(crate) mod config_catalog_helpers;
#[allow(unused_imports)]
use config_catalog_helpers::*;
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
