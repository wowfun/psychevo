use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::PluginPolicyEntry;
use crate::hooks::HookSourceDescriptor;
use crate::types::RuntimeTool;

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
    pub scope: PluginScope,
    pub git_ref: Option<String>,
    pub force: bool,
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
}

impl PluginManifestKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Psychevo => "psychevo",
            Self::Codex => "codex",
            Self::Claude => "claude",
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
    pub diagnostics: Vec<PluginDiagnostic>,
    pub ignored_manifest_paths: Vec<PathBuf>,
    pub skill_roots: Vec<PathBuf>,
    pub agent_roots: Vec<PathBuf>,
    pub hooks: Option<Value>,
    pub worker: Option<PluginWorkerSpec>,
    pub interface: Option<Value>,
    pub manifest_resources: BTreeSet<String>,
    pub psychevo_extensions: BTreeSet<String>,
    pub supported_fields: BTreeSet<String>,
    pub ignored_fields: BTreeSet<String>,
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
    pub scope: PluginScope,
    pub package_root: PathBuf,
    pub data_root: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest_kind: PluginManifestKind,
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
}

#[derive(Debug, Clone)]
pub(crate) struct PluginRuntimeAssembly {
    pub(crate) skill_inputs: Vec<PathBuf>,
    pub(crate) agent_inputs: Vec<String>,
    pub(crate) hook_sources: Vec<HookSourceDescriptor>,
    pub(crate) runtime_tools: Vec<RuntimeTool>,
    pub(crate) warnings: Vec<crate::types::RunWarning>,
}

pub(crate) struct EnabledPluginManifest {
    pub(crate) record: PluginInstallRecord,
    pub(crate) manifest: LoadedPluginManifest,
    pub(crate) policy: PluginPolicyEntry,
}
