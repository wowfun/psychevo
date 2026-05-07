use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::Result;
use crate::store::SqliteStore;

pub fn session_exists(db_path: &Path, session_id: &str) -> Result<bool> {
    let conn = Connection::open(db_path)?;
    let found = conn
        .query_row(
            "SELECT 1 FROM sessions WHERE id = ?1",
            params![session_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(found)
}

pub fn latest_run_session_for_workdir(db_path: &Path, workdir: &Path) -> Result<Option<String>> {
    SqliteStore::open(db_path)?.latest_run_session_for_workdir(workdir)
}
