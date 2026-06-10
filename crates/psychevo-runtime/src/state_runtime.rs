use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::session_trace::{
    SessionTraceReadOptions, SessionTraceReadResult, read_session_trace, remove_session_trace_dir,
};
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

    pub fn read_session_trace(
        &self,
        session_id: &str,
        options: SessionTraceReadOptions,
    ) -> SessionTraceReadResult {
        read_session_trace(&self.db_path, session_id, options)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        self.store.delete_session(session_id)?;
        let _ = remove_session_trace_dir(&self.db_path, session_id);
        Ok(())
    }

    pub fn delete_sessions_for_workdir_with_source(
        &self,
        workdir: &Path,
        source: &str,
    ) -> Result<usize> {
        let ids = self
            .store
            .session_ids_for_workdir_with_source(workdir, source)?;
        let count = self
            .store
            .delete_sessions_for_workdir_with_source(workdir, source)?;
        for id in ids {
            let _ = remove_session_trace_dir(&self.db_path, &id);
        }
        Ok(count)
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
