use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::error::Result;
use crate::session_trace::{
    SessionTraceReadOptions, SessionTraceReadResult, read_session_trace, remove_session_trace_dir,
};
use crate::store::SqliteStore;

#[derive(Clone)]
pub struct StateRuntime {
    pub(crate) db_path: PathBuf,
    pub(crate) store: SqliteStore,
    filesystem_grants: Arc<Mutex<HashMap<String, crate::sandbox::SandboxWriteGrants>>>,
}

impl StateRuntime {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let store = SqliteStore::open(&db_path)?;
        Ok(Self {
            db_path,
            store,
            filesystem_grants: Arc::new(Mutex::new(HashMap::new())),
        })
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
        self.clear_session_filesystem_grants(session_id);
        let _ = remove_session_trace_dir(&self.db_path, session_id);
        Ok(())
    }

    pub fn delete_sessions_for_cwd_with_source(&self, cwd: &Path, source: &str) -> Result<usize> {
        let ids = self.store.session_ids_for_cwd_with_source(cwd, source)?;
        let count = self
            .store
            .delete_sessions_for_cwd_with_source(cwd, source)?;
        for id in ids {
            self.clear_session_filesystem_grants(&id);
            let _ = remove_session_trace_dir(&self.db_path, &id);
        }
        Ok(count)
    }

    pub(crate) fn from_store(db_path: PathBuf, store: SqliteStore) -> Self {
        Self {
            db_path,
            store,
            filesystem_grants: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn filesystem_grants(&self, session_id: &str) -> crate::sandbox::SandboxWriteGrants {
        let mut grants = self
            .filesystem_grants
            .lock()
            .expect("filesystem grant map poisoned");
        grants.entry(session_id.to_string()).or_default().clone()
    }

    pub(crate) fn turn_filesystem_grant_guard(
        &self,
        session_id: impl Into<String>,
    ) -> TurnFilesystemGrantGuard {
        TurnFilesystemGrantGuard {
            state: self.clone(),
            session_id: session_id.into(),
        }
    }

    fn clear_turn_filesystem_grants(&self, session_id: &str) {
        if let Ok(grants) = self.filesystem_grants.lock()
            && let Some(grants) = grants.get(session_id)
        {
            grants.clear_turn_scopes();
        }
    }

    fn clear_session_filesystem_grants(&self, session_id: &str) {
        if let Ok(mut grants) = self.filesystem_grants.lock()
            && let Some(grants) = grants.remove(session_id)
        {
            grants.clear_session_scopes();
        }
    }
}

pub(crate) struct TurnFilesystemGrantGuard {
    state: StateRuntime,
    session_id: String,
}

impl Drop for TurnFilesystemGrantGuard {
    fn drop(&mut self) {
        self.state.clear_turn_filesystem_grants(&self.session_id);
    }
}

impl fmt::Debug for StateRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateRuntime")
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FilesystemApprovalLifetime, FilesystemApprovalScope};

    #[test]
    fn filesystem_grants_follow_turn_and_session_lifecycles() {
        let temp = tempfile::tempdir().expect("temp");
        let turn_root = temp.path().join("turn");
        let session_root = temp.path().join("session");
        std::fs::create_dir_all(&turn_root).expect("turn root");
        std::fs::create_dir_all(&session_root).expect("session root");
        let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let grants = state.filesystem_grants("session-1");
        let turn_guard = state.turn_filesystem_grant_guard("session-1");
        grants
            .grant_scope(&FilesystemApprovalScope {
                directory: turn_root.display().to_string(),
                lifetime: FilesystemApprovalLifetime::Turn,
            })
            .expect("turn grant");
        grants
            .grant_scope(&FilesystemApprovalScope {
                directory: session_root.display().to_string(),
                lifetime: FilesystemApprovalLifetime::Session,
            })
            .expect("session grant");

        drop(turn_guard);

        assert_eq!(
            grants.scoped_roots(),
            vec![session_root.canonicalize().unwrap()]
        );
        state.clear_session_filesystem_grants("session-1");
        assert!(grants.scoped_roots().is_empty());
    }
}
