use psychevo_agent_core::{TerminalReason, now_ms};
use psychevo_ai::Outcome;
use serde_json::{Map, Value};
use sqlx::Row;

use crate::error::Result;

use super::{StateRuntime, UndoTarget};

impl StateRuntime {
    pub async fn finish_session(
        &self,
        session_id: &str,
        outcome: Outcome,
        terminal_reason: Option<TerminalReason>,
    ) -> Result<()> {
        let now = now_ms();
        let metadata_json = match terminal_reason {
            Some(reason) => {
                let mut metadata = self
                    .session_metadata(session_id)
                    .await?
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
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            if let Some(metadata_json) = metadata_json.as_deref() {
                sqlx::query(
                    "UPDATE sessions SET updated_at_ms = ?1, ended_at_ms = ?1, end_reason = ?2, metadata_json = ?3 WHERE id = ?4",
                )
                .bind(now)
                .bind(outcome.as_str())
                .bind(metadata_json)
                .bind(session_id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(
                    "UPDATE sessions SET updated_at_ms = ?1, ended_at_ms = ?1, end_reason = ?2 WHERE id = ?3",
                )
                .bind(now)
                .bind(outcome.as_str())
                .bind(session_id)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(())
        })
        .await
    }

    pub(crate) async fn user_target_before(
        &self,
        session_id: &str,
        boundary: i64,
    ) -> Result<Option<UndoTarget>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
            SELECT session_seq, message_json, content_text, metadata_json
            FROM messages
            WHERE session_id = ?1 AND role = 'user' AND session_seq < ?2
            ORDER BY session_seq DESC
            LIMIT 1
            "#,
            )
            .bind(session_id)
            .bind(boundary)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| undo_target_from_sqlx_row(&row))
            .transpose()
        })
        .await
    }

    pub(crate) async fn user_target_after(
        &self,
        session_id: &str,
        boundary: i64,
    ) -> Result<Option<UndoTarget>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
            SELECT session_seq, message_json, content_text, metadata_json
            FROM messages
            WHERE session_id = ?1 AND role = 'user' AND session_seq > ?2
            ORDER BY session_seq ASC
            LIMIT 1
            "#,
            )
            .bind(session_id)
            .bind(boundary)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| undo_target_from_sqlx_row(&row))
            .transpose()
        })
        .await
    }
}

fn undo_target_from_sqlx_row(row: &sqlx::sqlite::SqliteRow) -> Result<UndoTarget> {
    let seq = row.try_get(0)?;
    let message_json: String = row.try_get(1)?;
    let content_text: Option<String> = row.try_get(2)?;
    let metadata_json: Option<String> = row.try_get(3)?;
    let prompt = content_text
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            super::store_undo_helpers::user_prompt_from_message_json(&message_json)
                .unwrap_or_default()
        });
    Ok(UndoTarget {
        seq,
        prompt,
        snapshot: super::store_undo_helpers::undo_snapshot_from_metadata(metadata_json.as_deref()),
    })
}
