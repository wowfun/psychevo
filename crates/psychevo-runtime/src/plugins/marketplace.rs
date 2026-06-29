use std::path::Path;

use serde_json::{Value, json};

use super::store::PluginStore;
use super::types::{PluginMarketplaceEntry, PluginScope};
use crate::error::{Error, Result};

pub fn plugin_marketplace_list_value(home: &Path, cwd: &Path, scope: PluginScope) -> Result<Value> {
    let store = PluginStore::new(home, cwd, scope)?;
    Ok(json!({
        "scope": scope.as_str(),
        "marketplaces": store.catalog_entries()?,
    }))
}

pub fn plugin_marketplace_add_value(
    home: &Path,
    cwd: &Path,
    scope: PluginScope,
    entry: PluginMarketplaceEntry,
) -> Result<Value> {
    validate_marketplace_entry(&entry)?;
    let store = PluginStore::new(home, cwd, scope)?;
    store.ensure()?;
    let mut entries = store.catalog_entries()?;
    entries.retain(|existing| existing.name != entry.name);
    entries.push(entry.clone());
    store.write_catalog_entries(&entries)?;
    Ok(json!({
        "success": true,
        "scope": scope.as_str(),
        "marketplace": entry,
    }))
}

fn validate_marketplace_entry(entry: &PluginMarketplaceEntry) -> Result<()> {
    if entry.name.trim().is_empty() {
        return Err(Error::Config(
            "plugin marketplace entry name must not be empty".to_string(),
        ));
    }
    if entry.source.trim().is_empty() {
        return Err(Error::Config(
            "plugin marketplace entry source must not be empty".to_string(),
        ));
    }
    if !matches!(entry.kind.as_str(), "local" | "git") {
        return Err(Error::Config(format!(
            "plugin marketplace entry `{}` has unsupported kind `{}`; expected local or git",
            entry.name, entry.kind
        )));
    }
    Ok(())
}

pub fn plugin_marketplace_remove_value(
    home: &Path,
    cwd: &Path,
    scope: PluginScope,
    name: &str,
) -> Result<Value> {
    let store = PluginStore::new(home, cwd, scope)?;
    let mut entries = store.catalog_entries()?;
    let before = entries.len();
    entries.retain(|entry| entry.name != name);
    store.write_catalog_entries(&entries)?;
    Ok(json!({
        "success": true,
        "scope": scope.as_str(),
        "removed": before != entries.len(),
        "name": name,
    }))
}
