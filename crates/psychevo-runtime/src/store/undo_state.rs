use psychevo_agent_core::now_ms;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use crate::error::{Error, Result};

use super::store_metadata::{
    json_to_sql, metadata_json_sql, metadata_object_sql, parse_session_revert,
    parse_session_revert_sql, session_metadata_json,
};
use super::store_undo_helpers::session_tool_call_count;
use super::{
    SESSION_REVERT_METADATA_KEY, SessionRevertKind, SessionRevertState, StateRuntime, UndoTarget,
};

impl StateRuntime {
    pub fn session_revert_state(&self, session_id: &str) -> Result<Option<SessionRevertState>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let metadata_json = conn
            .query_row(
                "SELECT metadata_json FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))?;
        parse_session_revert(metadata_json.as_deref())
    }

    pub fn set_session_revert_state(
        &self,
        session_id: &str,
        revert: SessionRevertState,
    ) -> Result<()> {
        let changed = self.write_retry(|conn| {
            let metadata_json = session_metadata_json(conn, session_id)?;
            let mut metadata = metadata_object_sql(metadata_json.as_deref())?;
            let value = match &revert.kind {
                SessionRevertKind::WorkspaceUndo { original_snapshot } => json!({
                    "kind": "workspaceUndo",
                    "start_seq": revert.start_seq,
                    "original_snapshot": original_snapshot,
                }),
                SessionRevertKind::ConversationEdit {
                    boundary_message_id,
                    draft,
                } => json!({
                    "kind": "conversationEdit",
                    "start_seq": revert.start_seq,
                    "boundary_message_id": boundary_message_id,
                    "draft": draft,
                }),
            };
            metadata.insert(SESSION_REVERT_METADATA_KEY.to_string(), value);
            let metadata_json =
                serde_json::to_string(&Value::Object(metadata)).map_err(json_to_sql)?;
            conn.execute(
                "UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![metadata_json, now_ms(), session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn clear_session_revert_state(&self, session_id: &str) -> Result<()> {
        let changed = self.write_retry(|conn| {
            let metadata_json = session_metadata_json(conn, session_id)?;
            let mut metadata = metadata_object_sql(metadata_json.as_deref())?;
            metadata.remove(SESSION_REVERT_METADATA_KEY);
            let metadata_json = metadata_json_sql(metadata)?;
            conn.execute(
                "UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![metadata_json, now_ms(), session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn latest_undo_target(&self, session_id: &str) -> Result<Option<UndoTarget>> {
        let boundary = self
            .session_revert_state(session_id)?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        self.user_target_before(session_id, boundary)
    }

    pub fn next_redo_target(&self, session_id: &str) -> Result<Option<UndoTarget>> {
        let Some(revert) = self.session_revert_state(session_id)? else {
            return Ok(None);
        };
        self.user_target_after(session_id, revert.start_seq)
    }

    pub fn messages_from_count(&self, session_id: &str, start_seq: i64) -> Result<usize> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id = ?1 AND session_seq >= ?2",
            params![session_id, start_seq],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as usize)
    }

    pub fn cleanup_reverted_messages(&self, session_id: &str) -> Result<usize> {
        self.write_retry(|conn| {
            let metadata_json = session_metadata_json(conn, session_id)?;
            let Some(revert) = parse_session_revert_sql(metadata_json.as_deref())? else {
                return Ok(0);
            };
            let removed = conn.execute(
                "DELETE FROM messages WHERE session_id = ?1 AND session_seq >= ?2",
                params![session_id, revert.start_seq],
            )?;
            let mut metadata = metadata_object_sql(metadata_json.as_deref())?;
            metadata.remove(SESSION_REVERT_METADATA_KEY);
            let metadata_json = metadata_json_sql(metadata)?;
            let message_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )?;
            let tool_call_count = session_tool_call_count(conn, session_id)?;
            conn.execute(
                r#"
                UPDATE sessions
                SET metadata_json = ?1,
                    message_count = ?2,
                    tool_call_count = ?3,
                    updated_at_ms = ?4
                WHERE id = ?5
                "#,
                params![
                    metadata_json,
                    message_count,
                    tool_call_count,
                    now_ms(),
                    session_id
                ],
            )?;
            Ok(removed)
        })
    }
}
