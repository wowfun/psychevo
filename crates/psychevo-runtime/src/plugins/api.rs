use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};

use super::manifest::load_plugin_manifest;
use super::records::{all_records, policy_entry, policy_key_for_selector, select_record};
use super::store::PluginStore;
use super::types::{LoadedPluginManifest, PluginInstallRecord, PluginScope};
use super::worker::worker_tools;
use crate::config::{
    CONFIG_FILE_NAME, PluginPolicyConfig, PluginPolicyEntry, load_run_config,
    load_toml_config_file, resolve_psychevo_home, write_toml_config_file,
};
use crate::error::{Error, Result};
use crate::paths::canonical_cwd;
use crate::types::RunOptions;

pub fn plugin_list_value(options: &RunOptions) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let home = resolve_psychevo_home(&loaded.env)?;
    let records = all_records(&home, &cwd)?;
    let plugins = records
        .iter()
        .map(|record| record_value(record, &loaded.config.plugins))
        .collect::<Vec<_>>();
    Ok(json!({
        "plugins": plugins,
        "count": plugins.len(),
    }))
}

pub fn plugin_view_value(options: &RunOptions, selector: &str) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let home = resolve_psychevo_home(&loaded.env)?;
    let records = all_records(&home, &cwd)?;
    let record = select_record(&records, selector)?;
    let manifest = load_plugin_manifest(&record.package_root, true)?;
    Ok(json!({
        "plugin": record_value(record, &loaded.config.plugins),
        "manifest": manifest_value(&manifest),
    }))
}

pub fn plugin_doctor_value(options: &RunOptions, selector: Option<&str>) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let home = resolve_psychevo_home(&loaded.env)?;
    let records = all_records(&home, &cwd)?;
    let selected = if let Some(selector) = selector {
        vec![select_record(&records, selector)?.clone()]
    } else {
        records
    };
    let mut plugins = Vec::new();
    for record in selected {
        let manifest = load_plugin_manifest(&record.package_root, true)?;
        let policy = policy_entry(&loaded.config.plugins, &record);
        let mut worker = json!({"configured": manifest.worker.is_some(), "tools": []});
        if policy.is_some_and(PluginPolicyEntry::plugin_enabled)
            && let Some(spec) = &manifest.worker
        {
            worker = match worker_tools(&record, &manifest, spec, &loaded.env) {
                Ok(tools) => json!({"configured": true, "tools": tools, "status": "ok"}),
                Err(err) => {
                    json!({"configured": true, "tools": [], "status": "failed", "error": err})
                }
            };
        }
        plugins.push(json!({
            "plugin": record_value(&record, &loaded.config.plugins),
            "manifest": manifest_value(&manifest),
            "worker": worker,
            "sandbox": {
                "worker_process_confined": false,
                "message": "plugin workers and hook commands are not whole-process sandboxed in V1",
            }
        }));
    }
    Ok(json!({ "plugins": plugins }))
}

pub fn plugin_uninstall_value(
    home: &Path,
    cwd: &Path,
    scope: PluginScope,
    selector: &str,
) -> Result<Value> {
    let store = PluginStore::new(home, cwd, scope)?;
    let records = store.records()?;
    let record = select_record(&records, selector)?.clone();
    let record_path = store.record_path(&record);
    if record_path.exists() {
        fs::remove_file(record_path)?;
    }
    if record.package_root.exists() {
        fs::remove_dir_all(&record.package_root)?;
    }
    Ok(json!({
        "success": true,
        "scope": scope.as_str(),
        "plugin": record.name,
        "source": record.source_slug,
    }))
}

pub fn plugin_set_enabled_value(
    home: &Path,
    cwd: &Path,
    scope: PluginScope,
    selector: &str,
    enabled: bool,
) -> Result<Value> {
    let records = match scope {
        PluginScope::Global => PluginStore::new(home, cwd, PluginScope::Global)?.records()?,
        PluginScope::Local => all_records(home, cwd)?,
    };
    let record = select_record(&records, selector)?.clone();
    let config_dir = match scope {
        PluginScope::Global => home.to_path_buf(),
        PluginScope::Local => canonical_cwd(cwd)?.join(".psychevo"),
    };
    fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut document = load_toml_config_file(&config_path, false)?;
    if !document.is_object() {
        document = json!({});
    }
    let key = policy_key_for_selector(&records, selector, &record);
    set_plugin_policy_document_value(
        &mut document,
        &key,
        PluginPolicyEntry {
            enabled: Some(enabled),
        },
    )?;
    write_toml_config_file(&config_path, &document)?;
    Ok(json!({
        "success": true,
        "scope": scope.as_str(),
        "path": config_path,
        "plugin": record.name,
        "source": record.source_slug,
        "enabled": enabled,
        "manifest_resources": record.manifest_resources,
        "psychevo_extensions": record.psychevo_extensions,
    }))
}

fn set_plugin_policy_document_value(
    root: &mut Value,
    key: &str,
    entry: PluginPolicyEntry,
) -> Result<()> {
    let root = root
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let plugins = root
        .entry("plugins".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| Error::Config("plugins must be an object".to_string()))?;
    let value = json!({ "enabled": entry.enabled });
    plugins.insert(key.to_string(), value);
    Ok(())
}

fn record_value(record: &PluginInstallRecord, policy: &PluginPolicyConfig) -> Value {
    let policy = policy_entry(policy, record);
    json!({
        "name": record.name,
        "version": record.version,
        "description": record.description,
        "source_id": record.source_id,
        "source": record.source_slug,
        "scope": record.scope.as_str(),
        "package_root": record.package_root,
        "data_root": record.data_root,
        "manifest_path": record.manifest_path,
        "manifest_kind": record.manifest_kind.as_str(),
        "manifest_resources": record.manifest_resources,
        "psychevo_extensions": record.psychevo_extensions,
        "enabled": policy.is_some_and(PluginPolicyEntry::plugin_enabled),
        "diagnostics": record.diagnostics,
    })
}

fn manifest_value(manifest: &LoadedPluginManifest) -> Value {
    json!({
        "name": manifest.name,
        "version": manifest.version,
        "description": manifest.description,
        "path": manifest.manifest_path,
        "kind": manifest.kind.as_str(),
        "supported_fields": manifest.supported_fields.iter().cloned().collect::<Vec<_>>(),
        "ignored_fields": manifest.ignored_fields.iter().cloned().collect::<Vec<_>>(),
        "ignored_manifest_paths": manifest.ignored_manifest_paths,
        "manifest_resources": manifest.manifest_resources.iter().cloned().collect::<Vec<_>>(),
        "psychevo_extensions": manifest.psychevo_extensions.iter().cloned().collect::<Vec<_>>(),
        "skill_roots": manifest.skill_roots,
        "agent_roots": manifest.agent_roots,
        "hooks": manifest.hooks.is_some(),
        "worker": manifest.worker,
        "interface": manifest.interface,
        "diagnostics": manifest.diagnostics,
    })
}
