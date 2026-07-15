use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{CustomToolsetConfig, PluginPolicyEntry, ToolsetContribution};
use crate::contribution_projection::ContributionProjection;
use crate::hooks::HookSourceDescriptor;
use crate::types::{McpServerInput, RuntimeTool};

use super::compatibility::PluginComponentStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginScope {
    Global,
    Local,
}

impl PluginScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Local => "local",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInstallOptions {
    pub source: String,
    pub source_kind: Option<PluginSourceKind>,
    pub scope: PluginScope,
    pub git_ref: Option<String>,
    pub npm_version: Option<String>,
    pub npm_registry: Option<String>,
    pub adapter_mode: Option<PluginAdapterMode>,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInspectOptions {
    pub source: String,
    pub source_kind: Option<PluginSourceKind>,
    pub git_ref: Option<String>,
    pub npm_version: Option<String>,
    pub npm_registry: Option<String>,
    pub adapter_mode: Option<PluginAdapterMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDiagnostic {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl PluginDiagnostic {
    pub(crate) fn warning(message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self {
            kind: "warning".to_string(),
            message: message.into(),
            path,
        }
    }

    pub(crate) fn invalid(message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self {
            kind: "invalid".to_string(),
            message: message.into(),
            path,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginManifestKind {
    Psychevo,
    Codex,
    Claude,
    Hermes,
    #[serde(rename = "opencode")]
    OpenCode,
    Unknown,
}

impl PluginManifestKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Psychevo => "psychevo",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Hermes => "hermes",
            Self::OpenCode => "opencode",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSourceKind {
    #[default]
    Local,
    Git,
    Npm,
}

impl PluginSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Git => "git",
            Self::Npm => "npm",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "local" => Some(Self::Local),
            "git" => Some(Self::Git),
            "npm" => Some(Self::Npm),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginAdapterMode {
    AdapterHost,
    #[default]
    ManifestOnly,
    Disabled,
}

impl PluginAdapterMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AdapterHost => "adapter_host",
            Self::ManifestOnly => "manifest_only",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "adapter_host" | "adapter-host" => Some(Self::AdapterHost),
            "manifest_only" | "manifest-only" => Some(Self::ManifestOnly),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedPluginManifest {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub kind: PluginManifestKind,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub compatibility_profile: String,
    pub raw_manifest: Value,
    pub raw_overlay: Option<Value>,
    pub component_statuses: Vec<PluginComponentStatus>,
    pub diagnostics: Vec<PluginDiagnostic>,
    pub ignored_manifest_paths: Vec<PathBuf>,
    pub skill_roots: Vec<PathBuf>,
    pub agent_roots: Vec<PathBuf>,
    pub hooks: Option<Value>,
    pub mcp_servers: Vec<McpServerInput>,
    pub app_resource: Option<PathBuf>,
    pub worker: Option<PluginWorkerSpec>,
    pub(crate) toolsets: BTreeMap<String, CustomToolsetConfig>,
    pub interface: Option<PluginInterfaceMetadata>,
    pub manifest_resources: BTreeSet<String>,
    pub psychevo_extensions: BTreeSet<String>,
    pub supported_fields: BTreeSet<String>,
    pub ignored_fields: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInterfaceMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub developer_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_prompt: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy_policy_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terms_of_service_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brand_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer_icon: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_dark: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshots: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginWorkerSpec {
    pub command: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInstallRecord {
    pub name: String,
    pub version: String,
    pub description: String,
    pub source_id: String,
    pub source_slug: String,
    #[serde(default)]
    pub source_kind: PluginSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm_registry: Option<String>,
    pub scope: PluginScope,
    pub package_root: PathBuf,
    pub data_root: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest_kind: PluginManifestKind,
    #[serde(default)]
    pub compatibility_profile: String,
    #[serde(default)]
    pub component_statuses: Vec<PluginComponentStatus>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub package_fingerprint: String,
    #[serde(default)]
    pub adapter_mode: PluginAdapterMode,
    pub manifest_resources: Vec<String>,
    pub psychevo_extensions: Vec<String>,
    pub diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginMarketplaceEntry {
    pub name: String,
    pub source: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm_registry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_mode: Option<PluginAdapterMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginTrustRecord {
    pub key: String,
    pub fingerprint: String,
    pub trusted_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginStageDiagnostic {
    pub stage: String,
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl PluginStageDiagnostic {
    pub(crate) fn new(
        stage: impl Into<String>,
        status: impl Into<String>,
        message: impl Into<String>,
        path: Option<PathBuf>,
    ) -> Self {
        Self {
            stage: stage.into(),
            status: status.into(),
            message: message.into(),
            path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInspection {
    pub source_kind: PluginSourceKind,
    pub source_id: String,
    pub framework: PluginManifestKind,
    pub canonical_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility_profile: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub manifest_path: PathBuf,
    pub package_root: PathBuf,
    pub package_fingerprint: String,
    pub adapter_mode: PluginAdapterMode,
    pub readiness: String,
    pub status: String,
    pub target_lanes: Vec<String>,
    pub projected_contributions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub component_statuses: Vec<PluginComponentStatus>,
    pub unsupported_lanes: Vec<String>,
    pub diagnostics: Vec<PluginDiagnostic>,
    pub stages: Vec<PluginStageDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<PluginInterfaceMetadata>,
}

#[derive(Debug, Clone)]
pub(crate) struct PluginRuntimeAssembly {
    pub(crate) skill_inputs: Vec<PathBuf>,
    pub(crate) agent_inputs: Vec<String>,
    pub(crate) hook_sources: Vec<HookSourceDescriptor>,
    pub(crate) mcp_servers: Vec<McpServerInput>,
    pub(crate) toolsets: Vec<ToolsetContribution>,
    pub(crate) runtime_tools: Vec<RuntimeTool>,
    pub(crate) warnings: Vec<crate::types::RunWarning>,
    pub(crate) projection: ContributionProjection,
}

pub(crate) struct EnabledPluginManifest {
    pub(crate) record: PluginInstallRecord,
    pub(crate) manifest: LoadedPluginManifest,
    pub(crate) policy: PluginPolicyEntry,
}
