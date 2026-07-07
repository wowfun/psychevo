use std::fs;
use std::path::{Path, PathBuf};

use super::types::{PluginInstallRecord, PluginMarketplaceEntry, PluginScope, PluginTrustRecord};
use super::util::sanitize_path_segment;
use crate::error::Result;
use crate::paths::canonical_cwd;

const RECORDS_DIR: &str = "records";
const CATALOG_FILE: &str = "catalogs.json";
const TRUST_FILE: &str = "trust.json";

pub(crate) struct PluginStore {
    pub(crate) cache: PathBuf,
    pub(crate) data: PathBuf,
}

impl PluginStore {
    pub(crate) fn new(home: &Path, cwd: &Path, scope: PluginScope) -> Result<Self> {
        let root = match scope {
            PluginScope::Global => home.join("plugins"),
            PluginScope::Local => canonical_cwd(cwd)?.join(".psychevo").join("plugins"),
        };
        Ok(Self {
            cache: root.join("cache"),
            data: root.join("data"),
        })
    }

    pub(crate) fn ensure(&self) -> Result<()> {
        fs::create_dir_all(self.cache.join(RECORDS_DIR))?;
        fs::create_dir_all(&self.data)?;
        Ok(())
    }

    pub(crate) fn records(&self) -> Result<Vec<PluginInstallRecord>> {
        let dir = self.cache.join(RECORDS_DIR);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            records.push(serde_json::from_str::<PluginInstallRecord>(&text)?);
        }
        Ok(records)
    }

    pub(crate) fn write_record(&self, record: &PluginInstallRecord) -> Result<()> {
        self.ensure()?;
        let path = self.record_path(record);
        fs::write(path, serde_json::to_string_pretty(record)?)?;
        Ok(())
    }

    pub(crate) fn record_path(&self, record: &PluginInstallRecord) -> PathBuf {
        self.cache.join(RECORDS_DIR).join(format!(
            "{}-{}.json",
            sanitize_path_segment(&record.name),
            record.source_slug
        ))
    }

    pub(crate) fn catalog_entries(&self) -> Result<Vec<PluginMarketplaceEntry>> {
        let path = self.cache.join(CATALOG_FILE);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub(crate) fn write_catalog_entries(&self, entries: &[PluginMarketplaceEntry]) -> Result<()> {
        self.ensure()?;
        fs::write(
            self.cache.join(CATALOG_FILE),
            serde_json::to_string_pretty(entries)?,
        )?;
        Ok(())
    }

    pub(crate) fn trust_records(&self) -> Result<Vec<PluginTrustRecord>> {
        let path = self.cache.join(TRUST_FILE);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub(crate) fn write_trust_records(&self, records: &[PluginTrustRecord]) -> Result<()> {
        self.ensure()?;
        fs::write(
            self.cache.join(TRUST_FILE),
            serde_json::to_string_pretty(records)?,
        )?;
        Ok(())
    }
}
