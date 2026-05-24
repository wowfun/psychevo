use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::store::SqliteStore;

#[derive(Clone)]
pub struct StateRuntime {
    pub(crate) db_path: PathBuf,
    pub(crate) store: SqliteStore,
}

impl StateRuntime {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let store = SqliteStore::open(&db_path)?;
        Ok(Self { db_path, store })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn store(&self) -> &SqliteStore {
        &self.store
    }

    pub(crate) fn from_store(db_path: PathBuf, store: SqliteStore) -> Self {
        Self { db_path, store }
    }
}

impl fmt::Debug for StateRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateRuntime")
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}
