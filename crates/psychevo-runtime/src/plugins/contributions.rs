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
use crate::config::{PluginPolicyConfig, PluginPolicyEntry};
use crate::hooks::{HookSourceDescriptor, HookWorkerAdapter};
use crate::types::RuntimeTool;

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
        runtime_tools: Vec::new(),
        warnings: Vec::new(),
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
        if enabled.policy.capability_enabled("runtime")
            && let Some(worker) = enabled.manifest.worker.clone()
        {
            match worker_tools(&enabled.record, &enabled.manifest, &worker, env) {
                Ok(tools) => {
                    assembly.runtime_tools.extend(tools.into_iter().map(|tool| {
                        RuntimeTool::new(Arc::new(PluginWorkerTool {
                            record: enabled.record.clone(),
                            spec: worker.clone(),
                            descriptor: tool,
                            env: env.clone(),
                        }))
                    }));
                }
                Err(err) => assembly.warnings.push(plugin_warning(format!(
                    "plugin `{}` worker unavailable: {err}",
                    enabled.record.name
                ))),
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
    for record in records {
        let Some(policy) = policy_entry(policy, &record).cloned() else {
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
            record,
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
    if policy.capability_enabled("skills") {
        assembly
            .skill_inputs
            .extend(manifest.skill_roots.iter().cloned());
    }
    if policy.capability_enabled("agents") {
        assembly
            .agent_inputs
            .extend(agent_files_from_roots(&manifest.agent_roots));
    }
    if policy.capability_enabled("hooks")
        && let Some(source) = hook_source_from_manifest(record, manifest, policy, env)
    {
        assembly.hook_sources.push(source);
    }
}

fn hook_source_from_manifest(
    record: &PluginInstallRecord,
    manifest: &LoadedPluginManifest,
    policy: &PluginPolicyEntry,
    env: &BTreeMap<String, String>,
) -> Option<HookSourceDescriptor> {
    if !policy.capability_enabled("hooks") {
        return None;
    }
    let hooks = manifest.hooks.clone()?;
    Some(HookSourceDescriptor {
        source_id: format!("plugin:{}@{}", record.name, record.source_slug),
        source_kind: "plugin".to_string(),
        display_name: Some(record.name.clone()),
        path: Some(record.manifest_path.clone()),
        hooks,
        worker: manifest
            .worker
            .clone()
            .filter(|_| policy.capability_enabled("runtime"))
            .map(|worker| HookWorkerAdapter {
                plugin_name: record.name.clone(),
                plugin_version: record.version.clone(),
                plugin_source: record.source_slug.clone(),
                plugin_root: record.package_root.clone(),
                plugin_data: record.data_root.clone(),
                manifest_path: record.manifest_path.clone(),
                capability_families: manifest.capability_families.iter().cloned().collect(),
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
