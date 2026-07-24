use psychevo_agent_core::{TerminalReason, now_ms};
use psychevo_ai::Outcome;
use rusqlite::{OptionalExtension, params};
use serde_json::{Map, Value};

use crate::error::Result;

use super::store_undo_helpers::undo_target_from_row;
use super::{StateRuntime, UndoTarget};

impl StateRuntime {
    pub fn finish_session(
        &self,
        session_id: &str,
        outcome: Outcome,
        terminal_reason: Option<TerminalReason>,
    ) -> Result<()> {
        let now = now_ms();
        let metadata_json = match terminal_reason {
            Some(reason) => {
                let mut metadata = self
                    .session_metadata(session_id)?
                    .unwrap_or_else(|| Value::Object(Map::new()));
                if !metadata.is_object() {
                    metadata = Value::Object(Map::new());
                }
                metadata
                    .as_object_mut()
                    .expect("metadata object")
                    .insert("terminal_reason".to_string(), serde_json::to_value(reason)?);
                Some(serde_json::to_string(&metadata)?)
            }
            None => None,
        };
        self.write_retry(|conn| {
            if let Some(metadata_json) = metadata_json.as_deref() {
                conn.execute(
                    "UPDATE sessions SET updated_at_ms = ?1, ended_at_ms = ?1, end_reason = ?2, metadata_json = ?3 WHERE id = ?4",
                    params![now, outcome.as_str(), metadata_json, session_id],
                )?;
            } else {
                conn.execute(
                    "UPDATE sessions SET updated_at_ms = ?1, ended_at_ms = ?1, end_reason = ?2 WHERE id = ?3",
                    params![now, outcome.as_str(), session_id],
                )?;
            }
            Ok(())
        })
    }

    pub(crate) fn user_target_before(
        &self,
        session_id: &str,
        boundary: i64,
    ) -> Result<Option<UndoTarget>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT session_seq, message_json, content_text, metadata_json
            FROM messages
            WHERE session_id = ?1 AND role = 'user' AND session_seq < ?2
            ORDER BY session_seq DESC
            LIMIT 1
            "#,
        )?;
        stmt.query_row(params![session_id, boundary], undo_target_from_row)
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn user_target_after(
        &self,
        session_id: &str,
        boundary: i64,
    ) -> Result<Option<UndoTarget>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT session_seq, message_json, content_text, metadata_json
            FROM messages
            WHERE session_id = ?1 AND role = 'user' AND session_seq > ?2
            ORDER BY session_seq ASC
            LIMIT 1
            "#,
        )?;
        stmt.query_row(params![session_id, boundary], undo_target_from_row)
            .optional()
            .map_err(Into::into)
    }
}
