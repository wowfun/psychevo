use rusqlite::{OptionalExtension, params};

use crate::error::Result;

use super::{PromptPrefixRecord, StateRuntime};

impl StateRuntime {
    pub fn load_session_prompt_prefix(
        &self,
        session_id: &str,
    ) -> Result<Option<PromptPrefixRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            r#"
            SELECT session_id, version, created_at_ms, provider, model,
                   prefix_hash, tool_declarations_hash, invalidation_reason,
                   slots_json, metadata_json
            FROM session_prompt_prefixes
            WHERE session_id = ?1
            ORDER BY version DESC
            LIMIT 1
            "#,
            params![session_id],
            prompt_prefix_record_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn load_session_prompt_prefix_version(
        &self,
        session_id: &str,
        version: i64,
    ) -> Result<Option<PromptPrefixRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            r#"
            SELECT session_id, version, created_at_ms, provider, model,
                   prefix_hash, tool_declarations_hash, invalidation_reason,
                   slots_json, metadata_json
            FROM session_prompt_prefixes
            WHERE session_id = ?1 AND version = ?2
            "#,
            params![session_id, version],
            prompt_prefix_record_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn upsert_session_prompt_prefix(
        &self,
        mut record: PromptPrefixRecord,
    ) -> Result<PromptPrefixRecord> {
        let next_version = {
            let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
            conn.query_row(
                "SELECT COALESCE(MAX(version), 0) FROM session_prompt_prefixes WHERE session_id = ?1",
                params![&record.session_id],
                |row| row.get::<_, i64>(0),
            )
            ?
            .saturating_add(1)
        };
        record.version = next_version;
        let slots_json = serde_json::to_string(&record.slots)?;
        let metadata_json = record
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO session_prompt_prefixes (
                    session_id, version, created_at_ms, provider, model,
                    prefix_hash, tool_declarations_hash, invalidation_reason,
                    slots_json, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
                params![
                    &record.session_id,
                    record.version,
                    record.created_at_ms,
                    &record.provider,
                    &record.model,
                    &record.prefix_hash,
                    &record.tool_declarations_hash,
                    &record.invalidation_reason,
                    &slots_json,
                    &metadata_json,
                ],
            )?;
            Ok(())
        })?;
        Ok(record)
    }
}

pub(crate) fn prompt_prefix_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<PromptPrefixRecord> {
    let slots_json: String = row.get(8)?;
    let slots = serde_json::from_str(&slots_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let metadata_json: Option<String> = row.get(9)?;
    let metadata = metadata_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(err))
        })?;
    Ok(PromptPrefixRecord {
        session_id: row.get(0)?,
        version: row.get(1)?,
        created_at_ms: row.get(2)?,
        provider: row.get(3)?,
        model: row.get(4)?,
        prefix_hash: row.get(5)?,
        tool_declarations_hash: row.get(6)?,
        invalidation_reason: row.get(7)?,
        slots,
        metadata,
    })
}
