use std::sync::{Arc, Mutex};

use super::{Result, SkillCatalog, SkillDiscoveryOptions, discover_skills};

#[derive(Clone)]
pub(crate) struct SkillRuntime {
    options: SkillDiscoveryOptions,
    catalog: Arc<Mutex<Option<SkillCatalog>>>,
}

impl SkillRuntime {
    pub(crate) fn new(options: SkillDiscoveryOptions) -> Self {
        Self {
            options,
            catalog: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn from_catalog(options: SkillDiscoveryOptions, catalog: SkillCatalog) -> Self {
        Self {
            options,
            catalog: Arc::new(Mutex::new(Some(catalog))),
        }
    }

    pub(crate) fn options(&self) -> &SkillDiscoveryOptions {
        &self.options
    }

    pub(crate) fn catalog(&self) -> Result<SkillCatalog> {
        let mut catalog = self
            .catalog
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if catalog.is_none() {
            *catalog = Some(discover_skills(&self.options)?);
        }
        Ok(catalog.as_ref().expect("catalog initialized").clone())
    }

    pub(crate) fn refresh(&self) -> Result<SkillCatalog> {
        let refreshed = discover_skills(&self.options)?;
        *self
            .catalog
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(refreshed.clone());
        Ok(refreshed)
    }
}
