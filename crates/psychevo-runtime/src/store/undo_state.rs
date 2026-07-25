use psychevo_agent_core::now_ms;
use serde_json::{Value, json};

use crate::error::{Error, Result};

use super::store_metadata::{metadata_object, parse_session_revert};
use super::{
    SESSION_REVERT_METADATA_KEY, SessionRevertKind, SessionRevertState, StateRuntime, UndoTarget,
};

impl StateRuntime {
    pub async fn session_revert_state(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionRevertState>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let metadata_json = sqlx::query_scalar::<_, Option<String>>(
                "SELECT metadata_json FROM sessions WHERE id = ?1",
            )
            .bind(session_id)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))?;
            parse_session_revert(metadata_json.as_deref())
        })
        .await
    }

    pub async fn set_session_revert_state(
        &self,
        session_id: &str,
        revert: SessionRevertState,
    ) -> Result<()> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let metadata_json = session_metadata_json(&mut tx, session_id).await?;
            let mut metadata = metadata_object(metadata_json.as_deref())?;
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
            let metadata_json = serde_json::to_string(&Value::Object(metadata))?;
            let changed = sqlx::query(
                "UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3",
            )
            .bind(metadata_json)
            .bind(now_ms())
            .bind(session_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed == 0 {
                return Err(Error::Message(format!("session not found: {session_id}")));
            }
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(())
        })
        .await
    }

    pub async fn clear_session_revert_state(&self, session_id: &str) -> Result<()> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let metadata_json = session_metadata_json(&mut tx, session_id).await?;
            let mut metadata = metadata_object(metadata_json.as_deref())?;
            metadata.remove(SESSION_REVERT_METADATA_KEY);
            let metadata_json = encode_metadata(metadata)?;
            let changed = sqlx::query(
                "UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3",
            )
            .bind(metadata_json)
            .bind(now_ms())
            .bind(session_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed == 0 {
                return Err(Error::Message(format!("session not found: {session_id}")));
            }
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(())
        })
        .await
    }

    pub async fn latest_undo_target(&self, session_id: &str) -> Result<Option<UndoTarget>> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        self.user_target_before(session_id, boundary).await
    }

    pub async fn next_redo_target(&self, session_id: &str) -> Result<Option<UndoTarget>> {
        let Some(revert) = self.session_revert_state(session_id).await? else {
            return Ok(None);
        };
        self.user_target_after(session_id, revert.start_seq).await
    }

    pub async fn messages_from_count(&self, session_id: &str, start_seq: i64) -> Result<usize> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1 AND session_seq >= ?2",
            )
            .bind(session_id)
            .bind(start_seq)
            .fetch_one(&mut *conn)
            .await?;
            Ok(count.max(0) as usize)
        })
        .await
    }

    pub async fn cleanup_reverted_messages(&self, session_id: &str) -> Result<usize> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let metadata_json = session_metadata_json(&mut tx, session_id).await?;
            let Some(revert) = parse_session_revert(metadata_json.as_deref())? else {
                return Ok(0);
            };
            let removed =
                sqlx::query("DELETE FROM messages WHERE session_id = ?1 AND session_seq >= ?2")
                    .bind(session_id)
                    .bind(revert.start_seq)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
            let mut metadata = metadata_object(metadata_json.as_deref())?;
            metadata.remove(SESSION_REVERT_METADATA_KEY);
            let metadata_json = encode_metadata(metadata)?;
            let message_count =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages WHERE session_id = ?1")
                    .bind(session_id)
                    .fetch_one(&mut *tx)
                    .await?;
            let tool_call_count = session_tool_call_count(&mut tx, session_id).await?;
            sqlx::query(
                r#"
                UPDATE sessions
                SET metadata_json = ?1,
                    message_count = ?2,
                    tool_call_count = ?3,
                    updated_at_ms = ?4
                WHERE id = ?5
                "#,
            )
            .bind(metadata_json)
            .bind(message_count)
            .bind(tool_call_count)
            .bind(now_ms())
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(removed as usize)
        })
        .await
    }
}

async fn session_metadata_json(
    tx: &mut sqlx::Transaction<'static, sqlx::Sqlite>,
    session_id: &str,
) -> Result<Option<String>> {
    sqlx::query_scalar("SELECT metadata_json FROM sessions WHERE id = ?1")
        .bind(session_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(Into::into)
}

fn encode_metadata(metadata: serde_json::Map<String, Value>) -> Result<Option<String>> {
    (!metadata.is_empty())
        .then(|| serde_json::to_string(&Value::Object(metadata)))
        .transpose()
        .map_err(Into::into)
}

async fn session_tool_call_count(
    tx: &mut sqlx::Transaction<'static, sqlx::Sqlite>,
    session_id: &str,
) -> Result<i64> {
    let values = sqlx::query_scalar::<_, String>(
        "SELECT tool_calls_json FROM messages WHERE session_id = ?1 AND tool_calls_json IS NOT NULL",
    )
    .bind(session_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(values
        .into_iter()
        .filter_map(|value| serde_json::from_str::<Vec<Value>>(&value).ok())
        .map(|calls| calls.len() as i64)
        .sum())
}
