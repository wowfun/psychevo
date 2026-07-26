use psychevo_agent_core::now_ms;
use serde_json::Value;
use sqlx::Row;

use crate::error::{Error, Result};

use super::store_message_fields::{optional_json_string, parse_optional_json};
use super::{SessionCompactionInput, SessionCompactionRecord, SessionMessageRecord, StateRuntime};

impl StateRuntime {
    pub async fn append_session_compaction(
        &self,
        input: SessionCompactionInput,
    ) -> Result<SessionCompactionRecord> {
        let now = now_ms();
        let metadata_json = optional_json_string(&input.metadata)?;
        let id = self
            .observe_sqlx(async {
                let mut tx = self.begin_sqlx_write().await?;
                let id = sqlx::query(
                    r#"
                INSERT INTO session_compactions (
                    session_id, created_at_ms, reason, summary_text,
                    first_kept_session_seq, created_after_session_seq,
                    tokens_before, tokens_after, summary_provider, summary_model,
                    instructions, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                "#,
                )
                .bind(input.session_id)
                .bind(now)
                .bind(input.reason)
                .bind(input.summary_text)
                .bind(input.first_kept_session_seq)
                .bind(input.created_after_session_seq)
                .bind(input.tokens_before.map(|value| value as i64))
                .bind(input.tokens_after.map(|value| value as i64))
                .bind(input.summary_provider)
                .bind(input.summary_model)
                .bind(input.instructions)
                .bind(metadata_json)
                .execute(&mut *tx)
                .await?
                .last_insert_rowid();
                tx.commit().await?;
                Ok(id)
            })
            .await?;
        self.session_compaction(id).await?.ok_or_else(|| {
            Error::Message(format!("session compaction not found after insert: {id}"))
        })
    }

    pub async fn latest_valid_session_compaction(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionCompactionRecord>> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT id, session_id, created_at_ms, reason, summary_text,
                   first_kept_session_seq, created_after_session_seq,
                   tokens_before, tokens_after, summary_provider, summary_model,
                   instructions, metadata_json
            FROM session_compactions
            WHERE session_id = ?1 AND created_after_session_seq < ?2
            ORDER BY created_at_ms DESC, id DESC
            "#,
            )
            .bind(session_id)
            .bind(boundary)
            .fetch_all(&mut *conn)
            .await?;
            for row in rows {
                let record = compaction_from_row(&row)?;
                if !compaction_is_projection_only(&record) {
                    return Ok(Some(record));
                }
            }
            Ok(None)
        })
        .await
    }

    pub async fn list_valid_session_compactions(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionCompactionRecord>> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT id, session_id, created_at_ms, reason, summary_text,
                   first_kept_session_seq, created_after_session_seq,
                   tokens_before, tokens_after, summary_provider, summary_model,
                   instructions, metadata_json
            FROM session_compactions
            WHERE session_id = ?1 AND created_after_session_seq < ?2
            ORDER BY created_at_ms ASC, id ASC
            "#,
            )
            .bind(session_id)
            .bind(boundary)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| compaction_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn list_valid_session_compactions_between(
        &self,
        session_id: &str,
        lower_session_seq: i64,
        before_session_seq: Option<i64>,
        limit: usize,
    ) -> Result<Vec<SessionCompactionRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let revert_boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let upper_session_seq = before_session_seq
            .unwrap_or(i64::MAX)
            .min(revert_boundary);
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT id, session_id, created_at_ms, reason, summary_text,
                   first_kept_session_seq, created_after_session_seq,
                   tokens_before, tokens_after, summary_provider, summary_model,
                   instructions, metadata_json
            FROM session_compactions
            WHERE session_id = ?1
              AND created_after_session_seq >= ?2
              AND created_after_session_seq < ?3
              AND COALESCE(json_extract(metadata_json, '$.projection_only'), 0) != 1
            ORDER BY created_after_session_seq DESC, id DESC
            LIMIT ?4
            "#,
            )
            .bind(session_id)
            .bind(lower_session_seq)
            .bind(upper_session_seq)
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .fetch_all(&mut *conn)
            .await?;
            let mut records = rows
                .into_iter()
                .map(|row| compaction_from_row(&row))
                .collect::<Result<Vec<_>>>()?;
            records.reverse();
            Ok(records)
        })
        .await
    }

    pub async fn load_message_records(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionMessageRecord>> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT session_seq, message_json
            FROM messages
            WHERE session_id = ?1 AND session_seq < ?2
            ORDER BY session_seq ASC
            "#,
            )
            .bind(session_id)
            .bind(boundary)
            .fetch_all(&mut *conn)
            .await?;
            let mut records = Vec::new();
            for row in rows {
                let session_seq = row.try_get(0)?;
                let message_json: String = row.try_get(1)?;
                records.push(SessionMessageRecord {
                    session_seq,
                    message: serde_json::from_str(&message_json)?,
                });
            }
            Ok(records)
        })
        .await
    }

    pub async fn delete_messages_from_seq(
        &self,
        session_id: &str,
        start_seq: i64,
    ) -> Result<usize> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let removed =
                sqlx::query("DELETE FROM messages WHERE session_id = ?1 AND session_seq >= ?2")
                    .bind(session_id)
                    .bind(start_seq)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
            let message_count =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages WHERE session_id = ?1")
                    .bind(session_id)
                    .fetch_one(&mut *tx)
                    .await?;
            let tool_call_count = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COALESCE(SUM(
                    CASE WHEN json_valid(tool_calls_json)
                         THEN json_array_length(tool_calls_json)
                         ELSE 0 END
                ), 0)
                FROM messages
                WHERE session_id = ?1 AND tool_calls_json IS NOT NULL
                "#,
            )
            .bind(session_id)
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query(
                r#"
                UPDATE sessions
                SET message_count = ?1,
                    tool_call_count = ?2,
                    updated_at_ms = ?3
                WHERE id = ?4
                "#,
            )
            .bind(message_count)
            .bind(tool_call_count)
            .bind(now_ms())
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(removed as usize)
        })
        .await
    }

    pub async fn session_compaction(&self, id: i64) -> Result<Option<SessionCompactionRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
            SELECT id, session_id, created_at_ms, reason, summary_text,
                   first_kept_session_seq, created_after_session_seq,
                   tokens_before, tokens_after, summary_provider, summary_model,
                   instructions, metadata_json
            FROM session_compactions
            WHERE id = ?1
            "#,
            )
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| compaction_from_row(&row))
            .transpose()
        })
        .await
    }
}

fn compaction_is_projection_only(record: &SessionCompactionRecord) -> bool {
    record
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("projection_only"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn compaction_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<SessionCompactionRecord> {
    let metadata_json = row.try_get::<Option<String>, _>(12)?;
    let metadata = parse_optional_json(metadata_json)?;
    Ok(SessionCompactionRecord {
        id: row.try_get(0)?,
        session_id: row.try_get(1)?,
        created_at_ms: row.try_get(2)?,
        reason: row.try_get(3)?,
        summary_text: row.try_get(4)?,
        first_kept_session_seq: row.try_get(5)?,
        created_after_session_seq: row.try_get(6)?,
        tokens_before: row.try_get::<Option<i64>, _>(7)?.map(|value| value as u64),
        tokens_after: row.try_get::<Option<i64>, _>(8)?.map(|value| value as u64),
        summary_provider: row.try_get(9)?,
        summary_model: row.try_get(10)?,
        instructions: row.try_get(11)?,
        metadata,
    })
}
