use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::paths::canonical_workdir;
use crate::types::{
    ApprovalMode, ConfigScope, ConfiguredModel, CustomProviderInput, CustomProviderResult,
    ModelCapabilities, ModelCatalogEntry, ModelCatalogProvider, ModelCost, ModelCostTier,
    ModelLimits, ModelMetadata, ModelMetadataCacheTarget, PermissionConfig, PermissionMode,
    RunMode, RunOptions, ScopedCustomProviderInput,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct RunConfig {
    model: ModelSelection,
    provider: BTreeMap<String, ConfigProviderEntry>,
    pub(crate) compression: CompressionConfig,
    pub(crate) permissions: PermissionConfig,
    pub(crate) lsp: LspConfig,
    pub(crate) tools: ToolSelectionConfig,
    pub(crate) toolsets: BTreeMap<String, CustomToolsetConfig>,
}

// Configuration internals are split by loading, parsing, resolution, and catalog concerns.
include!("config/types.rs");
include!("config/file_env.rs");
include!("config/parse.rs");
include!("config/model_metadata.rs");
include!("config/resolution.rs");
include!("config/catalog_helpers.rs");
include!("config/models.rs");
include!("config/custom_provider.rs");
include!("config/cli_views.rs");
include!("config/permissions.rs");
include!("config/toolsets.rs");
