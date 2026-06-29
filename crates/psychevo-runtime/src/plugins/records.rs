use std::path::Path;

use super::store::PluginStore;
use super::types::{PluginInstallRecord, PluginScope};
use crate::config::{PluginPolicyConfig, PluginPolicyEntry};
use crate::error::{Error, Result};

pub(crate) fn all_records(home: &Path, cwd: &Path) -> Result<Vec<PluginInstallRecord>> {
    let mut records = PluginStore::new(home, cwd, PluginScope::Global)?.records()?;
    records.extend(PluginStore::new(home, cwd, PluginScope::Local)?.records()?);
    records.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.scope.as_str().cmp(b.scope.as_str()))
            .then_with(|| a.source_slug.cmp(&b.source_slug))
    });
    Ok(records)
}

pub(crate) fn select_record<'a>(
    records: &'a [PluginInstallRecord],
    selector: &str,
) -> Result<&'a PluginInstallRecord> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(Error::Config(
            "plugin selector must not be empty".to_string(),
        ));
    }
    let matches = if let Some((name, source)) = selector.split_once('@') {
        records
            .iter()
            .filter(|record| {
                record.name == name && (record.source_slug == source || record.source_id == source)
            })
            .collect::<Vec<_>>()
    } else {
        records
            .iter()
            .filter(|record| record.name == selector)
            .collect::<Vec<_>>()
    };
    match matches.as_slice() {
        [record] => Ok(record),
        [] => Err(Error::Config(format!("plugin not found: {selector}"))),
        _ => Err(Error::Config(format!(
            "plugin selector `{selector}` is ambiguous; use name@source"
        ))),
    }
}

pub(crate) fn policy_entry<'a>(
    policy: &'a PluginPolicyConfig,
    record: &PluginInstallRecord,
) -> Option<&'a PluginPolicyEntry> {
    policy
        .plugins
        .get(&format!("{}@{}", record.name, record.source_slug))
        .or_else(|| policy.plugins.get(&record.name))
}

pub(crate) fn policy_key_for_selector(
    records: &[PluginInstallRecord],
    selector: &str,
    record: &PluginInstallRecord,
) -> String {
    if selector.contains('@')
        || records
            .iter()
            .filter(|candidate| candidate.name == record.name)
            .count()
            > 1
    {
        format!("{}@{}", record.name, record.source_slug)
    } else {
        record.name.clone()
    }
}
