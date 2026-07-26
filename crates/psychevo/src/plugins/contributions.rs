use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::manifest::load_plugin_manifest;
use super::records::{all_records, policy_entry};
use super::types::{
    EnabledPluginManifest, LoadedPluginManifest, PluginInstallRecord, PluginRuntimeAssembly,
};
use super::worker::{PluginWorkerTool, worker_tools};
use crate::config::{PluginPolicyConfig, PluginPolicyEntry, ToolsetContribution};
use crate::contribution_projection::{
    ContributionFact, ContributionProjection, ContributionStatus,
};
use crate::hooks::{HookSourceDescriptor, HookWorkerAdapter};
use crate::types::{McpServerInput, RuntimeTool};

pub(crate) fn load_enabled_plugin_contributions(
    home: &Path,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    policy: &PluginPolicyConfig,
) -> PluginRuntimeAssembly {
    let mut assembly = PluginRuntimeAssembly {
        skill_inputs: Vec::new(),
        agent_inputs: Vec::new(),
        hook_sources: Vec::new(),
        mcp_servers: Vec::new(),
        toolsets: Vec::new(),
        runtime_tools: Vec::new(),
        warnings: Vec::new(),
        projection: ContributionProjection::new(),
    };
    let enabled = enabled_plugin_manifests(home, cwd, policy, &mut assembly.warnings);
    for enabled in enabled {
        add_static_contributions(
            &mut assembly,
            &enabled.record,
            &enabled.manifest,
            &enabled.policy,
            env,
        );
        if let Some(worker) = enabled.manifest.worker.clone() {
            match worker_tools(&enabled.record, &enabled.manifest, &worker, env) {
                Ok(tools) => {
                    for tool in tools {
                        let tool_name = tool.name.clone();
                        assembly.projection.record(ContributionFact::new(
                            plugin_source_id(&enabled.record),
                            "plugin",
                            "worker_tool",
                            "tool_surface",
                            format!("tool:{tool_name}"),
                            ContributionStatus::Accepted,
                        ));
                        assembly.runtime_tools.push(RuntimeTool::with_source(
                            Arc::new(PluginWorkerTool {
                                record: enabled.record.clone(),
                                spec: worker.clone(),
                                descriptor: tool,
                                env: env.clone(),
                            }),
                            plugin_source_id(&enabled.record),
                            "plugin",
                        ));
                    }
                }
                Err(err) => {
                    assembly.projection.record(
                        ContributionFact::new(
                            plugin_source_id(&enabled.record),
                            "plugin",
                            "worker_tool",
                            "tool_surface",
                            "worker".to_string(),
                            ContributionStatus::Unavailable,
                        )
                        .with_reason(err.clone()),
                    );
                    assembly.warnings.push(plugin_warning(format!(
                        "plugin `{}` worker unavailable: {err}",
                        enabled.record.name
                    )));
                }
            }
        }
    }
    assembly
}

pub(crate) fn load_enabled_plugin_hook_sources(
    home: &Path,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    policy: &PluginPolicyConfig,
) -> Vec<HookSourceDescriptor> {
    let mut warnings = Vec::new();
    enabled_plugin_manifests(home, cwd, policy, &mut warnings)
        .into_iter()
        .filter_map(|enabled| {
            hook_source_from_manifest(&enabled.record, &enabled.manifest, &enabled.policy, env)
        })
        .collect()
}

fn enabled_plugin_manifests(
    home: &Path,
    cwd: &Path,
    policy: &PluginPolicyConfig,
    warnings: &mut Vec<crate::types::RunWarning>,
) -> Vec<EnabledPluginManifest> {
    let records = match all_records(home, cwd) {
        Ok(records) => records,
        Err(err) => {
            warnings.push(plugin_warning(format!(
                "failed to load plugin records: {err}"
            )));
            return Vec::new();
        }
    };
    let mut enabled = Vec::new();
    for record in &records {
        let Some(policy) = policy_entry(policy, &records, record).cloned() else {
            continue;
        };
        if !policy.plugin_enabled() {
            continue;
        }
        let manifest = match load_plugin_manifest(&record.package_root, true) {
            Ok(manifest) => manifest,
            Err(err) => {
                warnings.push(plugin_warning(format!(
                    "plugin `{}` skipped: {err}",
                    record.name
                )));
                continue;
            }
        };
        enabled.push(EnabledPluginManifest {
            record: record.clone(),
            manifest,
            policy,
        });
    }
    enabled
}

fn add_static_contributions(
    assembly: &mut PluginRuntimeAssembly,
    record: &PluginInstallRecord,
    manifest: &LoadedPluginManifest,
    policy: &PluginPolicyEntry,
    env: &BTreeMap<String, String>,
) {
    let source_id = plugin_source_id(record);
    for diagnostic in &manifest.diagnostics {
        if diagnostic.kind == "invalid" {
            assembly.projection.record(
                ContributionFact::new(
                    source_id.clone(),
                    "plugin",
                    "manifest",
                    "plugin_runtime",
                    record.manifest_path.display().to_string(),
                    ContributionStatus::Invalid,
                )
                .with_reason(diagnostic.message.clone()),
            );
        }
    }
    for root in &manifest.skill_roots {
        assembly.skill_inputs.push(root.clone());
        assembly.projection.record(ContributionFact::new(
            source_id.clone(),
            "plugin",
            "skill_root",
            "skills",
            root.display().to_string(),
            ContributionStatus::Accepted,
        ));
    }
    for agent in agent_files_from_roots(&manifest.agent_roots) {
        assembly.projection.record(ContributionFact::new(
            source_id.clone(),
            "plugin",
            "agent_root",
            "agents",
            agent.clone(),
            ContributionStatus::Accepted,
        ));
        assembly.agent_inputs.push(agent);
    }
    if let Some(source) = hook_source_from_manifest(record, manifest, policy, env) {
        assembly.projection.record(ContributionFact::new(
            source_id.clone(),
            "plugin",
            "hook_source",
            "hook_runtime",
            source.source_id.clone(),
            ContributionStatus::Accepted,
        ));
        assembly.hook_sources.push(source);
    }
    for server in &manifest.mcp_servers {
        assembly.mcp_servers.push(
            McpServerInput::with_source(
                server.name.clone(),
                server.transport.clone(),
                source_id.clone(),
                "plugin",
            )
            .with_policy(server.policy.clone()),
        );
        assembly.projection.record(ContributionFact::new(
            source_id.clone(),
            "plugin",
            "mcp_server",
            "mcp",
            format!("mcp:{}", server.name),
            ContributionStatus::Accepted,
        ));
    }
    for (name, config) in &manifest.toolsets {
        assembly.toolsets.push(ToolsetContribution {
            source_id: source_id.clone(),
            source_kind: "plugin".to_string(),
            name: name.clone(),
            config: config.clone(),
        });
        assembly.projection.record(ContributionFact::new(
            source_id.clone(),
            "plugin",
            "toolset",
            "tool_surface",
            format!("toolset:{name}"),
            ContributionStatus::Accepted,
        ));
    }
    for family in inert_descriptor_families(manifest) {
        assembly.projection.record(
            ContributionFact::new(
                source_id.clone(),
                "plugin",
                family,
                "plugin_runtime",
                family,
                ContributionStatus::Unsupported,
            )
            .with_reason("descriptor recognized but no owning runtime registry is implemented"),
        );
    }
}

fn hook_source_from_manifest(
    record: &PluginInstallRecord,
    manifest: &LoadedPluginManifest,
    policy: &PluginPolicyEntry,
    env: &BTreeMap<String, String>,
) -> Option<HookSourceDescriptor> {
    if !policy.plugin_enabled() {
        return None;
    }
    let hooks = manifest.hooks.clone()?;
    Some(HookSourceDescriptor {
        source_id: format!("plugin:{}@{}", record.name, record.source_slug),
        source_kind: "plugin".to_string(),
        display_name: Some(record.name.clone()),
        path: Some(record.manifest_path.clone()),
        hooks,
        worker: manifest.worker.clone().map(|worker| HookWorkerAdapter {
            plugin_name: record.name.clone(),
            plugin_version: record.version.clone(),
            plugin_source: record.source_slug.clone(),
            plugin_root: record.package_root.clone(),
            plugin_data: record.data_root.clone(),
            manifest_path: record.manifest_path.clone(),
            manifest_resources: manifest.manifest_resources.iter().cloned().collect(),
            psychevo_extensions: manifest.psychevo_extensions.iter().cloned().collect(),
            command: worker.command,
            args: worker.args,
            env: env.clone(),
        }),
    })
}

fn agent_files_from_roots(roots: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for root in roots {
        collect_agent_files(root, &mut out);
    }
    out
}

fn collect_agent_files(path: &Path, out: &mut Vec<String>) {
    if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md") {
        out.push(path.display().to_string());
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        collect_agent_files(&entry.path(), out);
    }
}

fn plugin_warning(message: String) -> crate::types::RunWarning {
    crate::types::RunWarning {
        kind: "plugin".to_string(),
        message,
        source_path: None,
        suggestion: None,
    }
}

fn plugin_source_id(record: &PluginInstallRecord) -> String {
    format!("plugin:{}@{}", record.name, record.source_slug)
}

fn inert_descriptor_families(manifest: &LoadedPluginManifest) -> Vec<&'static str> {
    let mut families = Vec::new();
    if manifest.manifest_resources.contains("apps") {
        families.push("apps");
    }
    if manifest.manifest_resources.contains("interface") {
        families.push("interface");
    }
    if manifest.psychevo_extensions.contains("commands") {
        families.push("commands");
    }
    if manifest.psychevo_extensions.contains("providers") {
        families.push("providers");
    }
    families
}
