use sqlx::Row;

use crate::error::Result;

use super::{PromptPrefixRecord, StateRuntime};

impl StateRuntime {
    pub async fn load_session_prompt_prefix(
        &self,
        session_id: &str,
    ) -> Result<Option<PromptPrefixRecord>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
                SELECT session_id, version, created_at_ms, provider, model,
                       prefix_hash, tool_declarations_hash, invalidation_reason,
                       slots_json, metadata_json
                FROM session_prompt_prefixes
                WHERE session_id = ?1
                ORDER BY version DESC
                LIMIT 1
                "#,
            )
            .bind(session_id)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| prompt_prefix_record_from_row(&row))
            .transpose()
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn load_session_prompt_prefix_version(
        &self,
        session_id: &str,
        version: i64,
    ) -> Result<Option<PromptPrefixRecord>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
                SELECT session_id, version, created_at_ms, provider, model,
                       prefix_hash, tool_declarations_hash, invalidation_reason,
                       slots_json, metadata_json
                FROM session_prompt_prefixes
                WHERE session_id = ?1 AND version = ?2
                "#,
            )
            .bind(session_id)
            .bind(version)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| prompt_prefix_record_from_row(&row))
            .transpose()
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn upsert_session_prompt_prefix(
        &self,
        mut record: PromptPrefixRecord,
    ) -> Result<PromptPrefixRecord> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let slots_json = serde_json::to_string(&record.slots)?;
            let metadata_json = record
                .metadata
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            let mut tx = self.begin_sqlx_write().await?;
            let next_version: i64 = sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(MAX(version), 0) FROM session_prompt_prefixes WHERE session_id = ?1",
            )
            .bind(&record.session_id)
            .fetch_one(&mut *tx)
            .await?
            .saturating_add(1);
            record.version = next_version;
            sqlx::query(
                r#"
                INSERT INTO session_prompt_prefixes (
                    session_id, version, created_at_ms, provider, model,
                    prefix_hash, tool_declarations_hash, invalidation_reason,
                    slots_json, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
            )
            .bind(&record.session_id)
            .bind(record.version)
            .bind(record.created_at_ms)
            .bind(&record.provider)
            .bind(&record.model)
            .bind(&record.prefix_hash)
            .bind(&record.tool_declarations_hash)
            .bind(&record.invalidation_reason)
            .bind(&slots_json)
            .bind(&metadata_json)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(record)
        }
        .await;
        operation.finish(&result);
        result
    }
}

pub(crate) fn prompt_prefix_record_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<PromptPrefixRecord> {
    let slots_json: String = row.try_get(8)?;
    let slots = serde_json::from_str(&slots_json)?;
    let metadata_json: Option<String> = row.try_get(9)?;
    let metadata = metadata_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    Ok(PromptPrefixRecord {
        session_id: row.try_get(0)?,
        version: row.try_get(1)?,
        created_at_ms: row.try_get(2)?,
        provider: row.try_get(3)?,
        model: row.try_get(4)?,
        prefix_hash: row.try_get(5)?,
        tool_declarations_hash: row.try_get(6)?,
        invalidation_reason: row.try_get(7)?,
        slots,
        metadata,
    })
}
