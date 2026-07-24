use rusqlite::{Connection, OptionalExtension, params};

use crate::error::Result;
use crate::types::SessionSummary;

use super::StateRuntime;

pub(crate) fn sqlite_table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
        params![table],
        |_| Ok(()),
    )
    .optional()
    .map(|value| value.is_some())
}

impl StateRuntime {
    pub(crate) fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        f(&conn)
    }
}

pub(crate) fn is_busy(err: &rusqlite::Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("busy") || msg.contains("locked")
}

pub(crate) fn session_summary_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        id: row.get(0)?,
        source: row.get(1)?,
        parent_session_id: row.get(2)?,
        cwd: row.get(3)?,
        model: row.get(4)?,
        provider: row.get(5)?,
        started_at_ms: row.get(6)?,
        updated_at_ms: row.get(7)?,
        ended_at_ms: row.get(8)?,
        end_reason: row.get(9)?,
        archived_at_ms: row.get(10)?,
        message_count: row.get(11)?,
        tool_call_count: row.get(12)?,
        title: row.get(13)?,
    })
}

pub(crate) fn next_session_seq(conn: &Connection, session_id: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(session_seq), 0) + 1 FROM messages WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )
}
