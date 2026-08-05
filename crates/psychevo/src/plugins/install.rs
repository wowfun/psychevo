use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::inspect::{
    PluginMaterializedSource, SourceRequest, inspect_materialized_source, materialize_source_in_dir,
};
use super::materialization::copy_tree_bounded;
use super::store::PluginStore;
use super::types::{PluginInstallOptions, PluginInstallRecord, PluginManifestKind};
use super::util::{sanitize_path_segment, source_slug};
use crate::error::{Error, Result};

pub fn plugin_install_value(
    home: &Path,
    cwd: &Path,
    options: PluginInstallOptions,
) -> Result<Value> {
    let record = install_plugin(home, cwd, options)?;
    Ok(json!({
        "success": true,
        "plugin": record,
    }))
}

pub fn install_plugin(
    home: &Path,
    cwd: &Path,
    options: PluginInstallOptions,
) -> Result<PluginInstallRecord> {
    let staging = tempfile::TempDir::new()?;
    let staging_root = staging.path().to_path_buf();
    let materialized = materialize_source_in_dir(
        home,
        cwd,
        &staging_root,
        &SourceRequest::from_install(&options),
        Some(staging),
    )?;
    let inspection = inspect_materialized_source(&materialized)?;
    if matches!(
        inspection.framework,
        PluginManifestKind::Hermes | PluginManifestKind::OpenCode
    ) {
        return Err(Error::Config(format!(
            "{} plugins are inspection-only; configure the {} ACP Agent runtime profile to execute this source",
            inspection.framework.as_str(),
            inspection.framework.as_str()
        )));
    }
    let invalid = inspection
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == "invalid")
        .map(|diagnostic| diagnostic.message.clone())
        .chain(
            inspection
                .invalid
                .then_some("inspection failed".to_string()),
        )
        .collect::<Vec<_>>();
    if !invalid.is_empty() {
        return Err(Error::Config(format!(
            "plugin manifest is invalid: {}",
            invalid.join("; ")
        )));
    }
    let version = inspection
        .version
        .clone()
        .unwrap_or_else(|| "local".to_string());
    let description = inspection.description.clone().unwrap_or_default();
    let source_slug = source_slug(&materialized.source_id);
    let store = PluginStore::new(home, cwd, options.scope)?;
    store.ensure()?;
    let package_root = store
        .cache
        .join(package_dir_name(&inspection.name, &version, &source_slug));
    if package_root.exists() {
        if options.force {
            fs::remove_dir_all(&package_root)?;
        } else {
            return Err(Error::Config(format!(
                "plugin `{}` from `{}` is already installed; use --force to replace it",
                inspection.name, materialized.source_id
            )));
        }
    }
    if let Err(err) = copy_tree_bounded(&materialized.root, &package_root) {
        let _ = fs::remove_dir_all(&package_root);
        return Err(err);
    }
    let installed = PluginMaterializedSource {
        root: package_root.clone(),
        source_id: materialized.source_id.clone(),
        source_kind: materialized.source_kind,
        npm_registry: materialized.npm_registry.clone(),
        resolved_revision: materialized.resolved_revision.clone(),
        _temp_dir: None,
    };
    let installed_inspection = inspect_materialized_source(&installed)?;
    let installed_manifest = super::manifest::load_plugin_manifest(&package_root, true).ok();
    let (manifest_resources, psychevo_extensions) = match installed_manifest.as_ref() {
        Some(manifest) => (
            manifest.manifest_resources.iter().cloned().collect(),
            manifest.psychevo_extensions.iter().cloned().collect(),
        ),
        None => (installed_inspection.declared_lanes.clone(), Vec::new()),
    };
    let data_root = store.data.join(format!(
        "{}-{}",
        sanitize_path_segment(&installed_inspection.name),
        source_slug
    ));
    fs::create_dir_all(&data_root)?;
    let record = PluginInstallRecord {
        name: installed_inspection.name.clone(),
        version,
        description,
        source_id: materialized.source_id,
        source_slug,
        source_kind: materialized.source_kind,
        npm_registry: materialized.npm_registry,
        resolved_revision: materialized.resolved_revision,
        scope: options.scope,
        package_root,
        data_root,
        manifest_path: installed_inspection.manifest_path,
        manifest_kind: installed_inspection.framework,
        compatibility_profile: installed_manifest
            .as_ref()
            .map(|manifest| manifest.compatibility_profile.clone())
            .unwrap_or_default(),
        component_statuses: installed_manifest
            .as_ref()
            .map(|manifest| manifest.component_statuses.clone())
            .unwrap_or_default(),
        manifest_resources,
        psychevo_extensions,
        diagnostics: installed_inspection.diagnostics,
    };
    store.write_record(&record)?;
    Ok(record)
}

fn package_dir_name(name: &str, version: &str, source_slug: &str) -> String {
    format!(
        "{}-{}-{}",
        sanitize_path_segment(name),
        sanitize_path_segment(version),
        source_slug
    )
}
