use std::fs;
use std::path::Path;

use chrono::Utc;
use serde_json::{Map, Value, json};

use super::inspect::{PluginMaterializedSource, inspect_materialized_source, inspection_value};
use super::manifest::load_plugin_manifest;
use super::records::{all_records, policy_entry, policy_key_for_selector, select_record};
use super::store::PluginStore;
use super::types::{
    LoadedPluginManifest, PluginAdapterMode, PluginInstallRecord, PluginManifestKind, PluginScope,
    PluginTrustRecord,
};
use super::util::directory_fingerprint;
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
        .map(|record| record_value(&home, &cwd, record, &loaded.config.plugins))
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
    let inspection = inspect_record(record)?;
    let manifest = load_plugin_manifest(&record.package_root, true).ok();
    Ok(json!({
        "plugin": record_value(&home, &cwd, record, &loaded.config.plugins),
        "manifest": manifest.as_ref().map(manifest_value).unwrap_or_else(|| inspection_value(&inspection)),
        "inspection": inspection,
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
        let inspection = inspect_record(&record)?;
        let manifest = load_plugin_manifest(&record.package_root, true).ok();
        let policy = policy_entry(&loaded.config.plugins, &record);
        let mut worker = json!({
            "configured": manifest.as_ref().and_then(|manifest| manifest.worker.as_ref()).is_some(),
            "tools": [],
        });
        if let Some(manifest) = manifest.as_ref()
            && policy.is_some_and(PluginPolicyEntry::plugin_enabled)
            && let Some(spec) = &manifest.worker
        {
            worker = match worker_tools(&record, manifest, spec, &loaded.env) {
                Ok(tools) => json!({"configured": true, "tools": tools, "status": "ok"}),
                Err(err) => {
                    json!({"configured": true, "tools": [], "status": "failed", "error": err})
                }
            };
        }
        plugins.push(json!({
            "plugin": record_value(&home, &cwd, &record, &loaded.config.plugins),
            "manifest": manifest.as_ref().map(manifest_value).unwrap_or_else(|| inspection_value(&inspection)),
            "inspection": inspection,
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

pub fn plugin_set_trust_value(
    home: &Path,
    cwd: &Path,
    scope: PluginScope,
    selector: &str,
    trusted: bool,
) -> Result<Value> {
    let records = match scope {
        PluginScope::Global => PluginStore::new(home, cwd, PluginScope::Global)?.records()?,
        PluginScope::Local => all_records(home, cwd)?,
    };
    let record = select_record(&records, selector)?.clone();
    let store = PluginStore::new(home, cwd, record.scope)?;
    let key = trust_key(&record);
    let fingerprint =
        current_fingerprint(&record).unwrap_or_else(|| record.package_fingerprint.clone());
    if fingerprint.is_empty() {
        return Err(Error::Config(format!(
            "plugin `{}` has no package fingerprint to trust",
            record.name
        )));
    }
    let mut trust = store.trust_records()?;
    trust.retain(|entry| entry.key != key);
    if trusted {
        trust.push(PluginTrustRecord {
            key: key.clone(),
            fingerprint: fingerprint.clone(),
            trusted_at_ms: Utc::now().timestamp_millis(),
        });
    }
    store.write_trust_records(&trust)?;
    Ok(json!({
        "success": true,
        "scope": record.scope.as_str(),
        "plugin": record.name,
        "source": record.source_slug,
        "trusted": trusted,
        "trust": trust_value(home, cwd, &record),
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

fn record_value(
    home: &Path,
    cwd: &Path,
    record: &PluginInstallRecord,
    policy: &PluginPolicyConfig,
) -> Value {
    let policy = policy_entry(policy, record);
    let enabled = policy.is_some_and(PluginPolicyEntry::plugin_enabled);
    let trust = trust_value(home, cwd, record);
    let readiness = readiness_status(record, enabled, &trust);
    json!({
        "name": record.name,
        "version": record.version,
        "description": record.description,
        "source_id": record.source_id,
        "source": record.source_slug,
        "source_kind": record.source_kind.as_str(),
        "npm_registry": record.npm_registry,
        "scope": record.scope.as_str(),
        "package_root": record.package_root,
        "data_root": record.data_root,
        "manifest_path": record.manifest_path,
        "manifest_kind": record.manifest_kind.as_str(),
        "package_fingerprint": current_fingerprint(record).unwrap_or_else(|| record.package_fingerprint.clone()),
        "adapter_mode": record.adapter_mode.as_str(),
        "manifest_resources": record.manifest_resources,
        "psychevo_extensions": record.psychevo_extensions,
        "enabled": enabled,
        "readiness": readiness,
        "status": readiness,
        "trust": trust,
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

fn inspect_record(record: &PluginInstallRecord) -> Result<super::types::PluginInspection> {
    let materialized = PluginMaterializedSource {
        root: record.package_root.clone(),
        source_id: record.source_id.clone(),
        source_kind: record.source_kind,
        npm_registry: record.npm_registry.clone(),
        temp_dir: None,
    };
    inspect_materialized_source(&materialized, record.adapter_mode, "Installed")
}

fn current_fingerprint(record: &PluginInstallRecord) -> Option<String> {
    directory_fingerprint(&record.package_root).ok()
}

fn trust_key(record: &PluginInstallRecord) -> String {
    format!("{}@{}", record.name, record.source_slug)
}

fn trust_value(home: &Path, cwd: &Path, record: &PluginInstallRecord) -> Value {
    let key = trust_key(record);
    let current = current_fingerprint(record).unwrap_or_else(|| record.package_fingerprint.clone());
    let trust = PluginStore::new(home, cwd, record.scope)
        .and_then(|store| store.trust_records())
        .ok()
        .and_then(|entries| entries.into_iter().find(|entry| entry.key == key));
    let required = trust_required(record);
    let status = if !required {
        "not_required"
    } else {
        match trust.as_ref() {
            Some(entry) if !current.is_empty() && entry.fingerprint == current => "trusted",
            Some(_) => "modified",
            None => "untrusted",
        }
    };
    json!({
        "required": required,
        "key": key,
        "status": status,
        "fingerprint": current,
        "trusted_fingerprint": trust.as_ref().map(|entry| entry.fingerprint.clone()),
        "trusted_at_ms": trust.as_ref().map(|entry| entry.trusted_at_ms),
    })
}

fn trust_required(record: &PluginInstallRecord) -> bool {
    matches!(
        record.manifest_kind,
        PluginManifestKind::Hermes | PluginManifestKind::OpenCode
    ) && record.adapter_mode == PluginAdapterMode::AdapterHost
}

fn readiness_status(record: &PluginInstallRecord, enabled: bool, trust: &Value) -> &'static str {
    if !enabled {
        return "Disabled";
    }
    if record
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == "invalid")
    {
        return "Failed";
    }
    if matches!(
        record.manifest_kind,
        PluginManifestKind::Hermes | PluginManifestKind::OpenCode
    ) {
        if record.adapter_mode == PluginAdapterMode::Disabled {
            return "Unsupported target";
        }
        if trust
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && trust.get("status").and_then(Value::as_str) != Some("trusted")
        {
            return "Needs trust";
        }
    }
    if record.manifest_resources.iter().any(|lane| lane == "apps") {
        return "Needs setup";
    }
    "Installed"
}
