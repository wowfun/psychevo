use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::paths::canonical_workdir;
use crate::types::{
    ConfiguredModel, CustomProviderInput, CustomProviderResult, ModelCapabilities,
    ModelCatalogEntry, ModelCatalogProvider, ModelCost, ModelCostTier, ModelLimits, ModelMetadata,
    RunOptions,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct RunConfig {
    model: ModelSelection,
    provider: BTreeMap<String, ConfigProviderEntry>,
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
