use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use super::inspect::{
    PluginMaterializedSource, SourceRequest, inspect_materialized_source, materialize_source_in_dir,
};
use super::install::install_materialized_plugin;
use super::materialization::{bounded_tree, copy_tree_bounded};
use super::store::PluginStore;
use super::types::{PluginMarketplaceEntry, PluginScope, PluginSourceKind};
use super::util::sanitize_path_segment;
use crate::error::{Error, Result};

const MARKETPLACE_MANIFESTS: &[&str] = &[
    ".agents/plugins/marketplace.json",
    ".agents/plugins/api_marketplace.json",
    ".claude-plugin/marketplace.json",
    ".cursor-plugin/marketplace.json",
];

#[derive(Debug, Deserialize)]
struct MarketplaceManifest {
    name: String,
    #[serde(default)]
    plugins: Vec<MarketplacePlugin>,
}

#[derive(Debug, Deserialize)]
struct MarketplacePlugin {
    name: String,
    source: MarketplacePluginSource,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    keywords: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MarketplacePluginSource {
    LocalPath(String),
    Detailed(MarketplacePluginSourceObject),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MarketplacePluginSourceObject {
    source: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default, rename = "ref")]
    ref_name: Option<String>,
    #[serde(default)]
    sha: Option<String>,
    #[serde(default)]
    package: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    registry: Option<String>,
}

#[derive(Debug)]
struct LoadedMarketplace {
    root: PathBuf,
    path: PathBuf,
    manifest: MarketplaceManifest,
}

#[derive(Debug)]
struct ResolvedPluginSource {
    source: String,
    kind: PluginSourceKind,
    git_ref: Option<String>,
    npm_version: Option<String>,
    npm_registry: Option<String>,
    subdir: Option<String>,
    expected_sha: Option<String>,
}

pub fn plugin_marketplace_list_value(home: &Path, cwd: &Path, scope: PluginScope) -> Result<Value> {
    let store = PluginStore::new(home, cwd, scope)?;
    let mut rows = Vec::new();
    for entry in store.catalog_entries()? {
        let root = marketplace_root(&store, cwd, &entry)?;
        let loaded = load_marketplace(&root)?;
        if loaded.manifest.name != entry.name {
            return Err(Error::Config(format!(
                "plugin marketplace `{}` snapshot declares name `{}`",
                entry.name, loaded.manifest.name
            )));
        }
        rows.push(json!({
            "name": entry.name,
            "source": entry.source,
            "kind": entry.kind,
            "git_ref": entry.git_ref,
            "npm_version": entry.npm_version,
            "npm_registry": entry.npm_registry,
            "root": loaded.root,
            "manifest": loaded.path,
            "plugin_count": loaded.manifest.plugins.len(),
        }));
    }
    Ok(json!({
        "scope": scope.as_str(),
        "marketplaces": rows,
    }))
}

pub fn plugin_marketplace_add_value(
    home: &Path,
    cwd: &Path,
    scope: PluginScope,
    mut entry: PluginMarketplaceEntry,
) -> Result<Value> {
    validate_marketplace_entry(&entry)?;
    let store = PluginStore::new(home, cwd, scope)?;
    store.ensure()?;
    let mut entries = store.catalog_entries()?;
    let materialized = materialize_marketplace(home, cwd, &store, &entry)?;
    let loaded = load_marketplace(&materialized.root)?;
    if entry.name.trim().is_empty() {
        entry.name = loaded.manifest.name.clone();
    }
    validate_marketplace_name(&entry.name)?;
    if loaded.manifest.name != entry.name {
        return Err(Error::Config(format!(
            "plugin marketplace source declares `{}`, not `{}`",
            loaded.manifest.name, entry.name
        )));
    }
    if let Some(existing) = entries.iter().find(|existing| existing.name == entry.name) {
        if existing == &entry {
            return Ok(json!({
                "success": true,
                "scope": scope.as_str(),
                "marketplace": entry,
                "root": marketplace_root(&store, cwd, existing)?,
                "already_added": true,
            }));
        }
        return Err(Error::Config(format!(
            "plugin marketplace `{}` is already configured from a different source; remove it first",
            entry.name
        )));
    }
    let root = if entry.kind == "local" {
        materialized.root.clone()
    } else {
        publish_marketplace_snapshot(&store, &entry.name, &materialized.root)?
    };
    entries.push(entry.clone());
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    if let Err(err) = store.write_catalog_entries(&entries) {
        if entry.kind != "local" {
            let _ = fs::remove_dir_all(&root);
        }
        return Err(err);
    }
    Ok(json!({
        "success": true,
        "scope": scope.as_str(),
        "marketplace": entry,
        "root": root,
        "already_added": false,
    }))
}

pub fn plugin_marketplace_upgrade_value(
    home: &Path,
    cwd: &Path,
    scope: PluginScope,
    name: Option<&str>,
) -> Result<Value> {
    let store = PluginStore::new(home, cwd, scope)?;
    let entries = store.catalog_entries()?;
    let selected = entries
        .iter()
        .filter(|entry| name.is_none_or(|name| entry.name == name))
        .collect::<Vec<_>>();
    if let Some(name) = name
        && selected.is_empty()
    {
        return Err(Error::Config(format!(
            "plugin marketplace `{name}` is not configured"
        )));
    }
    let mut upgraded = Vec::new();
    for entry in selected {
        let materialized = materialize_marketplace(home, cwd, &store, entry)?;
        let loaded = load_marketplace(&materialized.root)?;
        if loaded.manifest.name != entry.name {
            return Err(Error::Config(format!(
                "plugin marketplace source declares `{}`, not `{}`",
                loaded.manifest.name, entry.name
            )));
        }
        let root = if entry.kind == "local" {
            loaded.root
        } else {
            publish_marketplace_snapshot(&store, &entry.name, &loaded.root)?
        };
        upgraded.push(json!({
            "name": entry.name,
            "status": if entry.kind == "local" { "validated" } else { "upgraded" },
            "root": root,
            "plugin_count": loaded.manifest.plugins.len(),
        }));
    }
    Ok(json!({
        "success": true,
        "scope": scope.as_str(),
        "marketplaces": upgraded,
    }))
}

pub fn plugin_marketplace_install_value(
    home: &Path,
    cwd: &Path,
    scope: PluginScope,
    plugin_name: &str,
    marketplace_name: &str,
) -> Result<Value> {
    validate_marketplace_name(marketplace_name)?;
    validate_plugin_name(plugin_name)?;
    let store = PluginStore::new(home, cwd, scope)?;
    let entry = store
        .catalog_entries()?
        .into_iter()
        .find(|entry| entry.name == marketplace_name)
        .ok_or_else(|| {
            Error::Config(format!(
                "plugin marketplace `{marketplace_name}` is not configured"
            ))
        })?;
    let root = marketplace_root(&store, cwd, &entry)?;
    let loaded = load_marketplace(&root)?;
    if loaded.manifest.name != marketplace_name {
        return Err(Error::Config(format!(
            "plugin marketplace `{marketplace_name}` snapshot declares `{}`",
            loaded.manifest.name
        )));
    }
    let plugin = loaded
        .manifest
        .plugins
        .iter()
        .find(|plugin| plugin.name == plugin_name)
        .ok_or_else(|| {
            Error::Config(format!(
                "plugin `{plugin_name}` is absent from marketplace `{marketplace_name}`"
            ))
        })?;
    let source = resolve_plugin_source(&loaded.root, &plugin.source)?;
    let staging = tempfile::TempDir::new()?;
    let staging_root = staging.path().to_path_buf();
    let mut materialized = materialize_source_in_dir(
        home,
        cwd,
        &staging_root,
        &SourceRequest {
            source: source.source,
            source_kind: Some(source.kind),
            git_ref: source.git_ref,
            npm_version: source.npm_version,
            npm_registry: source.npm_registry,
        },
        Some(staging),
    )?;
    if let Some(expected) = source.expected_sha {
        let actual = materialized.resolved_revision.as_deref().ok_or_else(|| {
            Error::Config("marketplace Git source did not resolve a commit".to_string())
        })?;
        if actual != expected {
            return Err(Error::Config(format!(
                "plugin `{plugin_name}` Git commit mismatch: expected `{expected}`, received `{actual}`"
            )));
        }
    }
    if let Some(subdir) = source.subdir {
        let root = resolve_relative_inside(&materialized.root, &subdir, "plugin Git subdirectory")?;
        if !root.is_dir() {
            return Err(Error::Config(format!(
                "plugin `{plugin_name}` Git subdirectory `{subdir}` is unavailable"
            )));
        }
        materialized.root = root;
        materialized.source_id.push_str(&format!("#path={subdir}"));
    }
    let inspection = inspect_materialized_source(&materialized)?;
    if inspection.name != plugin_name {
        return Err(Error::Config(format!(
            "marketplace entry `{plugin_name}` materializes Plugin `{}`",
            inspection.name
        )));
    }
    let record = install_materialized_plugin(
        home,
        cwd,
        scope,
        false,
        materialized,
        Some(marketplace_name),
    )?;
    Ok(json!({
        "success": true,
        "plugin": record,
        "marketplace": marketplace_name,
        "enabled": false,
        "catalog": {
            "version": plugin.version,
            "description": plugin.description,
            "keywords": plugin.keywords,
        },
    }))
}

pub fn plugin_marketplace_remove_value(
    home: &Path,
    cwd: &Path,
    scope: PluginScope,
    name: &str,
) -> Result<Value> {
    let store = PluginStore::new(home, cwd, scope)?;
    let mut entries = store.catalog_entries()?;
    let removed = entries.iter().find(|entry| entry.name == name).cloned();
    let installed = store
        .records()?
        .into_iter()
        .filter(|record| record.marketplace.as_deref() == Some(name))
        .map(|record| record.name)
        .collect::<Vec<_>>();
    if !installed.is_empty() {
        return Err(Error::Config(format!(
            "plugin marketplace `{name}` still owns installed Plugins: {}; remove them first",
            installed.join(", ")
        )));
    }
    entries.retain(|entry| entry.name != name);
    store.write_catalog_entries(&entries)?;
    if let Some(entry) = &removed
        && entry.kind != "local"
    {
        let root = store
            .cache
            .join("marketplaces")
            .join(sanitize_path_segment(name));
        if root.is_dir() {
            fs::remove_dir_all(root)?;
        }
    }
    Ok(json!({
        "success": true,
        "scope": scope.as_str(),
        "removed": removed.is_some(),
        "name": name,
    }))
}

fn validate_marketplace_entry(entry: &PluginMarketplaceEntry) -> Result<()> {
    if !entry.name.trim().is_empty() {
        validate_marketplace_name(&entry.name)?;
    }
    if entry.source.trim().is_empty() {
        return Err(Error::Config(
            "plugin marketplace source must not be empty".to_string(),
        ));
    }
    if !matches!(entry.kind.as_str(), "local" | "git" | "npm") {
        return Err(Error::Config(format!(
            "plugin marketplace entry has unsupported kind `{}`; expected local, git, or npm",
            entry.kind
        )));
    }
    Ok(())
}

fn validate_marketplace_name(name: &str) -> Result<()> {
    if valid_segment(name) {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "invalid plugin marketplace name `{name}`; expected lowercase letters, digits, dots, and hyphens"
        )))
    }
}

fn validate_plugin_name(name: &str) -> Result<()> {
    if valid_segment(name) {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "invalid marketplace Plugin name `{name}`"
        )))
    }
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
}

fn materialize_marketplace(
    home: &Path,
    cwd: &Path,
    store: &PluginStore,
    entry: &PluginMarketplaceEntry,
) -> Result<PluginMaterializedSource> {
    let staging_parent = store.cache.join("marketplace-staging");
    fs::create_dir_all(&staging_parent)?;
    let staging = tempfile::Builder::new()
        .prefix("marketplace-")
        .tempdir_in(staging_parent)?;
    let staging_root = staging.path().to_path_buf();
    materialize_source_in_dir(
        home,
        cwd,
        &staging_root,
        &SourceRequest {
            source: entry.source.clone(),
            source_kind: Some(parse_source_kind(&entry.kind)?),
            git_ref: entry.git_ref.clone(),
            npm_version: entry.npm_version.clone(),
            npm_registry: entry.npm_registry.clone(),
        },
        Some(staging),
    )
}

fn parse_source_kind(kind: &str) -> Result<PluginSourceKind> {
    PluginSourceKind::parse(kind).ok_or_else(|| {
        Error::Config(format!(
            "unsupported marketplace source kind `{kind}`; expected local, git, or npm"
        ))
    })
}

fn marketplace_root(
    store: &PluginStore,
    cwd: &Path,
    entry: &PluginMarketplaceEntry,
) -> Result<PathBuf> {
    if entry.kind == "local" {
        let path = PathBuf::from(&entry.source);
        let path = if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        };
        return path.canonicalize().map_err(|err| {
            Error::Config(format!(
                "failed to resolve local plugin marketplace `{}`: {err}",
                entry.name
            ))
        });
    }
    Ok(store
        .cache
        .join("marketplaces")
        .join(sanitize_path_segment(&entry.name)))
}

fn load_marketplace(root: &Path) -> Result<LoadedMarketplace> {
    let root = root.canonicalize().map_err(|err| {
        Error::Config(format!(
            "failed to resolve plugin marketplace root {}: {err}",
            root.display()
        ))
    })?;
    bounded_tree(&root)?;
    let paths = MARKETPLACE_MANIFESTS
        .iter()
        .map(|relative| root.join(relative))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    let path = match paths.as_slice() {
        [path] => path.clone(),
        [] => {
            return Err(Error::Config(format!(
                "plugin marketplace {} has no recognized marketplace.json",
                root.display()
            )));
        }
        _ => {
            return Err(Error::Config(format!(
                "plugin marketplace {} has multiple recognized marketplace manifests",
                root.display()
            )));
        }
    };
    let bytes = fs::read(&path)?;
    let manifest: MarketplaceManifest = serde_json::from_slice(&bytes).map_err(|err| {
        Error::Config(format!(
            "failed to parse plugin marketplace manifest {}: {err}",
            path.display()
        ))
    })?;
    validate_marketplace_name(&manifest.name)?;
    let mut names = std::collections::BTreeSet::new();
    for plugin in &manifest.plugins {
        validate_plugin_name(&plugin.name)?;
        if !names.insert(plugin.name.as_str()) {
            return Err(Error::Config(format!(
                "plugin marketplace `{}` declares duplicate Plugin `{}`",
                manifest.name, plugin.name
            )));
        }
        resolve_plugin_source(&root, &plugin.source)?;
    }
    Ok(LoadedMarketplace {
        root,
        path,
        manifest,
    })
}

fn resolve_plugin_source(
    root: &Path,
    source: &MarketplacePluginSource,
) -> Result<ResolvedPluginSource> {
    match source {
        MarketplacePluginSource::LocalPath(path) => Ok(ResolvedPluginSource {
            source: resolve_relative_inside(root, path, "marketplace Plugin path")?
                .display()
                .to_string(),
            kind: PluginSourceKind::Local,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            subdir: None,
            expected_sha: None,
        }),
        MarketplacePluginSource::Detailed(source) => match source.source.as_str() {
            "local" => {
                let path = source.path.as_deref().ok_or_else(|| {
                    Error::Config("local marketplace Plugin source requires `path`".to_string())
                })?;
                Ok(ResolvedPluginSource {
                    source: resolve_relative_inside(root, path, "marketplace Plugin path")?
                        .display()
                        .to_string(),
                    kind: PluginSourceKind::Local,
                    git_ref: None,
                    npm_version: None,
                    npm_registry: None,
                    subdir: None,
                    expected_sha: None,
                })
            }
            "git" | "git-subdir" => {
                let url = source.url.as_deref().ok_or_else(|| {
                    Error::Config("Git marketplace Plugin source requires `url`".to_string())
                })?;
                let url = if !url.contains("://") && url.matches('/').count() == 1 {
                    format!("https://github.com/{url}.git")
                } else {
                    url.to_string()
                };
                if source.source == "git-subdir" && source.path.is_none() {
                    return Err(Error::Config(
                        "git-subdir marketplace Plugin source requires `path`".to_string(),
                    ));
                }
                if let Some(path) = source.path.as_deref() {
                    validate_relative(path, "plugin Git subdirectory")?;
                }
                Ok(ResolvedPluginSource {
                    source: url,
                    kind: PluginSourceKind::Git,
                    git_ref: source.ref_name.clone().or_else(|| source.sha.clone()),
                    npm_version: None,
                    npm_registry: None,
                    subdir: source.path.clone(),
                    expected_sha: source.sha.clone(),
                })
            }
            "npm" => Ok(ResolvedPluginSource {
                source: source.package.clone().ok_or_else(|| {
                    Error::Config("npm marketplace Plugin source requires `package`".to_string())
                })?,
                kind: PluginSourceKind::Npm,
                git_ref: None,
                npm_version: source.version.clone(),
                npm_registry: source.registry.clone(),
                subdir: None,
                expected_sha: None,
            }),
            other => Err(Error::Config(format!(
                "unsupported marketplace Plugin source `{other}`"
            ))),
        },
    }
}

fn resolve_relative_inside(root: &Path, value: &str, context: &str) -> Result<PathBuf> {
    validate_relative(value, context)?;
    let path = root.join(value);
    if path.exists() {
        let path = path.canonicalize()?;
        if !path.starts_with(root) {
            return Err(Error::Config(format!(
                "{context} escapes the marketplace root"
            )));
        }
        Ok(path)
    } else {
        Ok(path)
    }
}

fn validate_relative(value: &str, context: &str) -> Result<()> {
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(Error::Config(format!(
            "{context} must be a package-relative path without .."
        )));
    }
    Ok(())
}

fn publish_marketplace_snapshot(store: &PluginStore, name: &str, source: &Path) -> Result<PathBuf> {
    let root = store.cache.join("marketplaces");
    fs::create_dir_all(&root)?;
    let staging = tempfile::Builder::new()
        .prefix(".marketplace-publish-")
        .tempdir_in(&root)?;
    let incoming = staging.path().join("incoming");
    copy_tree_bounded(source, &incoming)?;
    let destination = root.join(sanitize_path_segment(name));
    let previous = staging.path().join("previous");
    if destination.exists() {
        fs::rename(&destination, &previous)?;
    }
    if let Err(err) = fs::rename(&incoming, &destination) {
        if previous.exists() {
            let _ = fs::rename(&previous, &destination);
        }
        return Err(err.into());
    }
    Ok(destination)
}
