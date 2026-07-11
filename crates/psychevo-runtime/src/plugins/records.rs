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
    let matches = records
        .iter()
        .filter(|record| {
            selector == record.name
                || selector == canonical_record_selector(record)
                || selector == format!("{}@{}", record.name, record.source_slug)
                || selector == format!("{}@{}", record.name, record.source_id)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [record] => Ok(record),
        [] => Err(Error::Config(format!("plugin not found: {selector}"))),
        _ => Err(Error::Config(format!(
            "plugin selector `{selector}` is ambiguous; use profile:name@source or project:name@source"
        ))),
    }
}

pub(crate) fn canonical_record_selector(record: &PluginInstallRecord) -> String {
    format!(
        "{}:{}@{}",
        plugin_scope_name(record.scope),
        record.name,
        record.source_slug
    )
}

pub(crate) fn plugin_scope_name(scope: PluginScope) -> &'static str {
    match scope {
        PluginScope::Global => "profile",
        PluginScope::Local => "project",
    }
}

pub(crate) fn policy_entry<'a>(
    policy: &'a PluginPolicyConfig,
    records: &[PluginInstallRecord],
    record: &PluginInstallRecord,
) -> Option<&'a PluginPolicyEntry> {
    policy
        .plugins
        .get(&canonical_record_selector(record))
        .or_else(|| {
            let key = format!("{}@{}", record.name, record.source_slug);
            (records
                .iter()
                .filter(|candidate| {
                    candidate.name == record.name && candidate.source_slug == record.source_slug
                })
                .count()
                == 1)
                .then(|| policy.plugins.get(&key))
                .flatten()
        })
        .or_else(|| {
            (records
                .iter()
                .filter(|candidate| candidate.name == record.name)
                .count()
                == 1)
                .then(|| policy.plugins.get(&record.name))
                .flatten()
        })
}

pub(crate) fn policy_key_for_record(record: &PluginInstallRecord) -> String {
    canonical_record_selector(record)
}
