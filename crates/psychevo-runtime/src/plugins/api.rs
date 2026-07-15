use std::fs;
use std::path::Path;

use chrono::Utc;
use serde_json::{Map, Value, json};

use super::inspect::{PluginMaterializedSource, inspect_materialized_source, inspection_value};
use super::manifest::load_plugin_manifest;
use super::records::{
    all_records, canonical_record_selector, plugin_scope_name, policy_entry, policy_key_for_record,
    select_record,
};
use super::store::PluginStore;
use super::types::{
    LoadedPluginManifest, PluginAdapterMode, PluginInstallRecord, PluginManifestKind, PluginScope,
    PluginTrustRecord,
};
use super::util::directory_fingerprint;
use super::worker::worker_tools;
use crate::config::{
    BuiltinPluginPolicyConfig, CONFIG_FILE_NAME, PluginPolicyConfig, PluginPolicyEntry,
    load_run_config, load_toml_config_file, parse_plugin_policy_config, resolve_config_path,
    resolve_psychevo_home, write_toml_config_file,
};
use crate::error::{Error, Result};
use crate::paths::canonical_cwd;
use crate::types::RunOptions;

const BUILTIN_BROWSER_PLUGIN_SELECTOR: &str = "builtin:browser";
const BUILTIN_BROWSER_PLUGIN_POLICY_KEY: &str = "browser";

pub fn plugin_list_value(options: &RunOptions) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let home = resolve_psychevo_home(&loaded.env)?;
    let records = all_records(&home, &cwd)?;
    let project_policy = project_plugin_policy(options, &cwd)?;
    let builtin_scope = builtin_browser_policy_scope(options, &cwd)?;
    let mut plugins = vec![builtin_browser_plugin_value(
        &cwd,
        &loaded.config.builtin_plugins,
        builtin_scope,
    )];
    plugins.extend(records.iter().map(|record| {
        record_value(
            &home,
            &cwd,
            &records,
            record,
            &loaded.config.plugins,
            &project_policy,
        )
    }));
    Ok(json!({
        "plugins": plugins,
        "count": plugins.len(),
    }))
}

pub fn plugin_view_value(options: &RunOptions, selector: &str) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let home = resolve_psychevo_home(&loaded.env)?;
    if is_builtin_browser_selector(selector) {
        let builtin_scope = builtin_browser_policy_scope(options, &cwd)?;
        let enabled = builtin_browser_enabled(&loaded.config.builtin_plugins);
        let status = builtin_browser_status(enabled);
        let plugin =
            builtin_browser_plugin_value(&cwd, &loaded.config.builtin_plugins, builtin_scope);
        return Ok(json!({
            "plugin": plugin,
            "manifest": builtin_browser_manifest_value(),
            "inspection": {
                "name": "Browser",
                "framework": "psychevo",
                "status": status,
                "source_kind": "built_in",
                "adapter_mode": "built_in",
                "target_lanes": ["browser", "plugins"],
                "projected_contributions": ["right_workspace_browser", "desktop_browser_host", "annotation_context"],
                "unsupported_lanes": [],
                "stages": [],
            },
        }));
    }
    let records = all_records(&home, &cwd)?;
    let project_policy = project_plugin_policy(options, &cwd)?;
    let record = select_record(&records, selector)?;
    let inspection = inspect_record(record)?;
    let manifest = load_plugin_manifest(&record.package_root, true).ok();
    Ok(json!({
        "plugin": record_value(
            &home,
            &cwd,
            &records,
            record,
            &loaded.config.plugins,
            &project_policy,
        ),
        "manifest": manifest.as_ref().map(manifest_value).unwrap_or_else(|| inspection_value(&inspection)),
        "inspection": inspection,
    }))
}

pub fn plugin_doctor_value(options: &RunOptions, selector: Option<&str>) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let home = resolve_psychevo_home(&loaded.env)?;
    let builtin_scope = builtin_browser_policy_scope(options, &cwd)?;
    if selector.is_some_and(is_builtin_browser_selector) {
        return Ok(json!({
            "plugins": [builtin_browser_doctor_value(
                &cwd,
                &loaded.config.builtin_plugins,
                builtin_scope,
            )]
        }));
    }
    let records = all_records(&home, &cwd)?;
    let project_policy = project_plugin_policy(options, &cwd)?;
    let selected = if let Some(selector) = selector {
        vec![select_record(&records, selector)?]
    } else {
        records.iter().collect::<Vec<_>>()
    };
    let mut plugins = Vec::new();
    for record in selected {
        let inspection = inspect_record(record)?;
        let manifest = load_plugin_manifest(&record.package_root, true).ok();
        let policy = policy_entry(&loaded.config.plugins, &records, record);
        let mut worker = json!({
            "configured": manifest.as_ref().and_then(|manifest| manifest.worker.as_ref()).is_some(),
            "tools": [],
        });
        if let Some(manifest) = manifest.as_ref()
            && policy.is_some_and(PluginPolicyEntry::plugin_enabled)
            && let Some(spec) = &manifest.worker
        {
            worker = match worker_tools(record, manifest, spec, &loaded.env) {
                Ok(tools) => json!({"configured": true, "tools": tools, "status": "ok"}),
                Err(err) => {
                    json!({"configured": true, "tools": [], "status": "failed", "error": err})
                }
            };
        }
        plugins.push(json!({
            "plugin": record_value(
                &home,
                &cwd,
                &records,
                record,
                &loaded.config.plugins,
                &project_policy,
            ),
            "manifest": manifest.as_ref().map(manifest_value).unwrap_or_else(|| inspection_value(&inspection)),
            "inspection": inspection,
            "worker": worker,
            "sandbox": {
                "worker_process_confined": false,
                "message": "plugin workers and hook commands are not whole-process sandboxed in V1",
            }
        }));
    }
    if selector.is_none() {
        plugins.insert(
            0,
            builtin_browser_doctor_value(&cwd, &loaded.config.builtin_plugins, builtin_scope),
        );
    }
    Ok(json!({ "plugins": plugins }))
}

pub fn plugin_uninstall_value(
    home: &Path,
    cwd: &Path,
    scope: PluginScope,
    selector: &str,
) -> Result<Value> {
    if is_builtin_browser_selector(selector) {
        return Err(Error::Config(
            "built-in plugin `builtin:browser` cannot be uninstalled".to_string(),
        ));
    }
    let records = all_records(home, cwd)?;
    let record = select_record(&records, selector)?.clone();
    ensure_record_scope(selector, &record, scope)?;
    let store = PluginStore::new(home, cwd, scope)?;
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
    if is_builtin_browser_selector(selector) {
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
        set_plugin_policy_document_value(
            &mut document,
            "builtin_plugins",
            BUILTIN_BROWSER_PLUGIN_POLICY_KEY,
            PluginPolicyEntry {
                enabled: Some(enabled),
            },
        )?;
        write_toml_config_file(&config_path, &document)?;
        return Ok(json!({
            "success": true,
            "scope": scope.as_str(),
            "path": config_path,
            "plugin": "Browser",
            "source": "built_in",
            "enabled": enabled,
            "manifest_resources": ["browser"],
            "psychevo_extensions": ["browser"],
        }));
    }
    let records = all_records(home, cwd)?;
    let record = select_record(&records, selector)?.clone();
    if scope == PluginScope::Global && record.scope != PluginScope::Global {
        return Err(Error::Config(format!(
            "plugin `{selector}` is not available to profile policy"
        )));
    }
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
    let key = policy_key_for_record(&record);
    set_plugin_policy_document_value(
        &mut document,
        "plugins",
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

fn is_builtin_browser_selector(selector: &str) -> bool {
    selector.trim() == BUILTIN_BROWSER_PLUGIN_SELECTOR
}

fn builtin_browser_enabled(policy: &BuiltinPluginPolicyConfig) -> bool {
    policy
        .entries
        .get(BUILTIN_BROWSER_PLUGIN_POLICY_KEY)
        .and_then(|entry| entry.enabled)
        .unwrap_or(true)
}

fn builtin_browser_status(enabled: bool) -> &'static str {
    if enabled { "Installed" } else { "Disabled" }
}

fn builtin_browser_policy_scope(options: &RunOptions, cwd: &Path) -> Result<PluginScope> {
    let Some(project_document) = project_config_document(options, cwd)? else {
        return Ok(PluginScope::Global);
    };
    let has_project_override = project_document
        .get("builtin_plugins")
        .or_else(|| project_document.get("builtinPlugins"))
        .and_then(Value::as_object)
        .and_then(|plugins| plugins.get(BUILTIN_BROWSER_PLUGIN_POLICY_KEY))
        .and_then(Value::as_object)
        .is_some_and(|entry| entry.contains_key("enabled"));
    Ok(if has_project_override {
        PluginScope::Local
    } else {
        PluginScope::Global
    })
}

fn project_plugin_policy(options: &RunOptions, cwd: &Path) -> Result<PluginPolicyConfig> {
    let Some(project_document) = project_config_document(options, cwd)? else {
        return Ok(PluginPolicyConfig::default());
    };
    project_document
        .get("plugins")
        .map(parse_plugin_policy_config)
        .transpose()
        .map(Option::unwrap_or_default)
}

fn project_config_document(options: &RunOptions, cwd: &Path) -> Result<Option<Value>> {
    let project_config = cwd.join(".psychevo").join(CONFIG_FILE_NAME);
    let initial_env = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| std::env::vars().collect());
    if let Some(explicit_config) = resolve_config_path(options, &initial_env)?
        && explicit_config != project_config
    {
        return Ok(None);
    }
    load_toml_config_file(&project_config, false).map(Some)
}

fn builtin_browser_plugin_value(
    cwd: &Path,
    policy: &BuiltinPluginPolicyConfig,
    policy_scope: PluginScope,
) -> Value {
    let enabled = builtin_browser_enabled(policy);
    let data_root = cwd
        .join(".psychevo")
        .join("plugins")
        .join("data")
        .join("browser");
    json!({
        "name": "Browser",
        "selector": BUILTIN_BROWSER_PLUGIN_SELECTOR,
        "scope_name": plugin_scope_name(policy_scope),
        "enablement_scope_name": plugin_scope_name(policy_scope),
        "removable": false,
        "package_mutable": false,
        "enablement_mutable": true,
        "version": "built-in",
        "description": "Right-workspace Browser pane, Desktop browser host, and XML-only browser annotation context.",
        "source_id": BUILTIN_BROWSER_PLUGIN_SELECTOR,
        "source": "built_in",
        "source_kind": "built_in",
        "npm_registry": null,
        "scope": "built_in",
        "package_root": null,
        "data_root": data_root,
        "manifest_path": null,
        "manifest_kind": "built_in",
        "package_fingerprint": "builtin-browser",
        "adapter_mode": "built_in",
        "manifest_resources": ["browser"],
        "psychevo_extensions": ["browser"],
        "enabled": enabled,
        "readiness": builtin_browser_status(enabled),
        "status": builtin_browser_status(enabled),
        "trust": {
            "required": false,
            "status": "not_required",
            "fingerprint": "builtin-browser",
        },
        "contributions": {
            "right_workspace": ["browser", "preview"],
            "desktop": ["managed_browser_host"],
            "annotation": ["workspace_comment_context_xml"],
            "toolsets": [],
        },
        "diagnostics": [],
    })
}

fn builtin_browser_manifest_value() -> Value {
    json!({
        "name": "Browser",
        "manifest_kind": "built_in",
        "interface": {
            "displayName": "Browser",
            "description": "Right-workspace Browser and rich preview integration.",
            "capabilities": ["browser", "preview", "annotation"],
        },
        "contributions": {
            "right_workspace": ["browser", "preview"],
            "desktop": ["managed_browser_host"],
            "annotation": ["workspace_comment_context_xml"],
        },
    })
}

fn builtin_browser_doctor_value(
    cwd: &Path,
    policy: &BuiltinPluginPolicyConfig,
    policy_scope: PluginScope,
) -> Value {
    let status = builtin_browser_status(builtin_browser_enabled(policy));
    json!({
        "plugin": builtin_browser_plugin_value(cwd, policy, policy_scope),
        "manifest": builtin_browser_manifest_value(),
        "inspection": {
            "name": "Browser",
            "framework": "psychevo",
            "status": status,
            "source_kind": "built_in",
            "adapter_mode": "built_in",
            "target_lanes": ["browser", "plugins"],
            "projected_contributions": ["right_workspace_browser", "desktop_browser_host", "annotation_context"],
            "unsupported_lanes": [],
            "stages": [],
        },
        "worker": {
            "configured": false,
            "tools": [],
            "status": "not_applicable",
        },
        "sandbox": {
            "worker_process_confined": true,
            "message": "Browser is built in; Desktop Browser host automation is not exposed as a plugin worker.",
        }
    })
}

pub fn plugin_set_trust_value(
    home: &Path,
    cwd: &Path,
    scope: PluginScope,
    selector: &str,
    trusted: bool,
) -> Result<Value> {
    let records = all_records(home, cwd)?;
    let record = select_record(&records, selector)?.clone();
    ensure_record_scope(selector, &record, scope)?;
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

fn ensure_record_scope(
    selector: &str,
    record: &PluginInstallRecord,
    requested_scope: PluginScope,
) -> Result<()> {
    if record.scope == requested_scope {
        return Ok(());
    }
    Err(Error::Config(format!(
        "plugin `{selector}` is installed in {} scope, not {} scope",
        plugin_scope_name(record.scope),
        plugin_scope_name(requested_scope)
    )))
}

fn set_plugin_policy_document_value(
    root: &mut Value,
    namespace: &str,
    key: &str,
    entry: PluginPolicyEntry,
) -> Result<()> {
    let root = root
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let plugins = root
        .entry(namespace.to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| Error::Config(format!("{namespace} must be an object")))?;
    let value = json!({ "enabled": entry.enabled });
    plugins.insert(key.to_string(), value);
    Ok(())
}

fn record_value(
    home: &Path,
    cwd: &Path,
    records: &[PluginInstallRecord],
    record: &PluginInstallRecord,
    policy: &PluginPolicyConfig,
    project_policy: &PluginPolicyConfig,
) -> Value {
    let policy = policy_entry(policy, records, record);
    let policy_scope = if record.scope == PluginScope::Local
        || policy_entry(project_policy, records, record)
            .is_some_and(|entry| entry.enabled.is_some())
    {
        PluginScope::Local
    } else {
        PluginScope::Global
    };
    let enabled = policy.is_some_and(PluginPolicyEntry::plugin_enabled);
    let trust = trust_value(home, cwd, record);
    let readiness = readiness_status(record, enabled, &trust);
    json!({
        "name": record.name,
        "authority": {
            "kind": "psychevo",
            "selector": canonical_record_selector(record),
        },
        "selector": canonical_record_selector(record),
        "scope_name": plugin_scope_name(record.scope),
        "enablement_scope_name": plugin_scope_name(policy_scope),
        "removable": true,
        "package_mutable": true,
        "enablement_mutable": true,
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
        "compatibility_profile": record.compatibility_profile,
        "component_statuses": record.component_statuses,
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
        "keywords": manifest.keywords,
        "compatibility_profile": manifest.compatibility_profile,
        "component_statuses": manifest.component_statuses,
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
        "app_resource": manifest.app_resource,
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
