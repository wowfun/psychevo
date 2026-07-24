use std::path::Path;

use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::state::StateRuntime;

pub fn session_exists(state: &StateRuntime, session_id: &str) -> Result<bool> {
    state.with_conn(|conn| {
        let found = conn
            .query_row(
                "SELECT 1 FROM sessions WHERE id = ?1",
                params![session_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        Ok(found)
    })
}

pub fn latest_run_session_for_cwd(state: &StateRuntime, cwd: &Path) -> Result<Option<String>> {
    state.latest_run_session_for_cwd(cwd)
}
