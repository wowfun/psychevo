use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use super::manifest::load_plugin_manifest;
use super::store::PluginStore;
use super::types::{PluginInstallOptions, PluginInstallRecord};
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
    let store = PluginStore::new(home, cwd, options.scope)?;
    store.ensure()?;
    let materialized = materialize_source(&store, &options)?;
    let manifest = load_plugin_manifest(&materialized.root, false)?;
    let invalid = manifest
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == "invalid")
        .map(|diagnostic| diagnostic.message.clone())
        .collect::<Vec<_>>();
    if !invalid.is_empty() {
        return Err(Error::Config(format!(
            "plugin manifest is invalid: {}",
            invalid.join("; ")
        )));
    }
    let version = manifest
        .version
        .clone()
        .ok_or_else(|| Error::Config("plugin version is required for install".to_string()))?;
    let description = manifest.description.clone().unwrap_or_default();
    let source_slug = source_slug(&materialized.source_id);
    let package_root = store
        .cache
        .join(package_dir_name(&manifest.name, &version, &source_slug));
    if package_root.exists() {
        if options.force {
            fs::remove_dir_all(&package_root)?;
        } else {
            return Err(Error::Config(format!(
                "plugin `{}` from `{}` is already installed; use --force to replace it",
                manifest.name, materialized.source_id
            )));
        }
    }
    if let Err(err) = copy_dir(&materialized.root, &package_root) {
        let _ = fs::remove_dir_all(&package_root);
        return Err(err);
    }
    let installed_manifest = load_plugin_manifest(&package_root, false)?;
    let data_root = store.data.join(format!(
        "{}-{}",
        sanitize_path_segment(&installed_manifest.name),
        source_slug
    ));
    fs::create_dir_all(&data_root)?;
    let record = PluginInstallRecord {
        name: installed_manifest.name.clone(),
        version,
        description,
        source_id: materialized.source_id,
        source_slug,
        scope: options.scope,
        package_root,
        data_root,
        manifest_path: installed_manifest.manifest_path,
        manifest_kind: installed_manifest.kind,
        manifest_resources: installed_manifest
            .manifest_resources
            .iter()
            .cloned()
            .collect(),
        psychevo_extensions: installed_manifest
            .psychevo_extensions
            .iter()
            .cloned()
            .collect(),
        diagnostics: installed_manifest.diagnostics,
    };
    store.write_record(&record)?;
    Ok(record)
}

struct MaterializedSource {
    root: PathBuf,
    source_id: String,
}

fn materialize_source(
    store: &PluginStore,
    options: &PluginInstallOptions,
) -> Result<MaterializedSource> {
    let source_path = PathBuf::from(&options.source);
    if source_path.exists() {
        let root = source_path.canonicalize()?;
        return Ok(MaterializedSource {
            root,
            source_id: format!("local:{}", source_path.display()),
        });
    }
    if looks_like_git_source(&options.source) {
        let incoming = store.cache.join(format!(
            "incoming-{}",
            source_slug(&format!("{}{:?}", options.source, options.git_ref))
        ));
        if incoming.exists() {
            fs::remove_dir_all(&incoming)?;
        }
        fs::create_dir_all(&store.cache)?;
        let status = Command::new("git")
            .arg("clone")
            .arg(&options.source)
            .arg(&incoming)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(Error::Config(format!(
                "git clone failed for {}",
                options.source
            )));
        }
        if let Some(git_ref) = &options.git_ref {
            let status = Command::new("git")
                .arg("-C")
                .arg(&incoming)
                .arg("checkout")
                .arg(git_ref)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if !status.success() {
                return Err(Error::Config(format!(
                    "git checkout `{git_ref}` failed for {}",
                    options.source
                )));
            }
        }
        return Ok(MaterializedSource {
            root: incoming,
            source_id: format!(
                "git:{}{}",
                options.source,
                options
                    .git_ref
                    .as_ref()
                    .map(|git_ref| format!("#{git_ref}"))
                    .unwrap_or_default()
            ),
        });
    }
    Err(Error::Config(format!(
        "plugin source not found: {}",
        options.source
    )))
}

fn looks_like_git_source(source: &str) -> bool {
    source.starts_with("file://")
        || source.contains("://")
        || source.ends_with(".git")
        || source.starts_with("git@")
}

fn copy_dir(source: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let metadata = fs::symlink_metadata(&path)?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(Error::Config(format!(
                "plugin package contains unsupported symlink: {}",
                path.display()
            )));
        }
        if file_type.is_dir() {
            copy_dir(&path, &dest_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &dest_path)?;
        }
    }
    Ok(())
}

fn package_dir_name(name: &str, version: &str, source_slug: &str) -> String {
    format!(
        "{}-{}-{}",
        sanitize_path_segment(name),
        sanitize_path_segment(version),
        source_slug
    )
}
