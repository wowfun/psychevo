use sqlx::Row;

use crate::error::Result;

use super::store_message_fields::{optional_json_string, parse_optional_json};
use super::{ContextEvidenceInput, ContextEvidenceRecord, StateRuntime};

impl StateRuntime {
    pub async fn load_context_evidence(
        &self,
        session_id: &str,
        prompt_session_seq: i64,
    ) -> Result<Vec<ContextEvidenceRecord>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
                SELECT id, session_id, prompt_session_seq, context_seq, role,
                       source_kind, source_name, source_path, provider_group,
                       provider_block_index, context_kind, timestamp_ms, content_text,
                       metadata_json
                FROM context_evidence
                WHERE session_id = ?1 AND prompt_session_seq = ?2
                ORDER BY context_seq ASC
                "#,
            )
            .bind(session_id)
            .bind(prompt_session_seq)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| {
                    Ok(ContextEvidenceRecord {
                        id: row.try_get(0)?,
                        session_id: row.try_get(1)?,
                        prompt_session_seq: row.try_get(2)?,
                        context_seq: row.try_get(3)?,
                        role: row.try_get(4)?,
                        source_kind: row.try_get(5)?,
                        source_name: row.try_get(6)?,
                        source_path: row.try_get(7)?,
                        provider_group: row.try_get(8)?,
                        provider_block_index: row.try_get(9)?,
                        context_kind: row.try_get(10)?,
                        timestamp_ms: row.try_get(11)?,
                        content_text: row.try_get(12)?,
                        metadata: parse_optional_json(row.try_get(13)?)?,
                    })
                })
                .collect()
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn load_export_context_evidence(
        &self,
        session_id: &str,
    ) -> Result<Vec<ContextEvidenceRecord>> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
                SELECT evidence.id, evidence.session_id,
                       evidence.prompt_session_seq, evidence.context_seq,
                       evidence.role, evidence.source_kind, evidence.source_name,
                       evidence.source_path, evidence.provider_group,
                       evidence.provider_block_index, evidence.context_kind,
                       evidence.timestamp_ms, evidence.content_text,
                       evidence.metadata_json
                FROM context_evidence AS evidence
                WHERE evidence.session_id = ?1
                  AND evidence.prompt_session_seq < ?2
                ORDER BY evidence.prompt_session_seq ASC, evidence.context_seq ASC
                "#,
            )
            .bind(session_id)
            .bind(boundary)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| {
                    Ok(ContextEvidenceRecord {
                        id: row.try_get(0)?,
                        session_id: row.try_get(1)?,
                        prompt_session_seq: row.try_get(2)?,
                        context_seq: row.try_get(3)?,
                        role: row.try_get(4)?,
                        source_kind: row.try_get(5)?,
                        source_name: row.try_get(6)?,
                        source_path: row.try_get(7)?,
                        provider_group: row.try_get(8)?,
                        provider_block_index: row.try_get(9)?,
                        context_kind: row.try_get(10)?,
                        timestamp_ms: row.try_get(11)?,
                        content_text: row.try_get(12)?,
                        metadata: parse_optional_json(row.try_get(13)?)?,
                    })
                })
                .collect()
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn load_export_context_evidence_page(
        &self,
        session_id: &str,
        after: Option<(i64, i64, i64)>,
        before_session_seq: i64,
        through_id: i64,
        limit: usize,
    ) -> Result<Vec<ContextEvidenceRecord>> {
        let (after_prompt_session_seq, after_context_seq, after_id) =
            after.unwrap_or((i64::MIN, i64::MIN, i64::MIN));
        let limit = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
                SELECT evidence.id, evidence.session_id,
                       evidence.prompt_session_seq, evidence.context_seq,
                       evidence.role, evidence.source_kind, evidence.source_name,
                       evidence.source_path, evidence.provider_group,
                       evidence.provider_block_index, evidence.context_kind,
                       evidence.timestamp_ms, evidence.content_text,
                       evidence.metadata_json
                FROM context_evidence AS evidence
                WHERE evidence.session_id = ?1
                  AND evidence.prompt_session_seq < ?2
                  AND evidence.id <= ?6
                  AND (
                    evidence.prompt_session_seq > ?3
                    OR (
                      evidence.prompt_session_seq = ?3
                      AND evidence.context_seq > ?4
                    )
                    OR (
                      evidence.prompt_session_seq = ?3
                      AND evidence.context_seq = ?4
                      AND evidence.id > ?5
                    )
                  )
                ORDER BY evidence.prompt_session_seq ASC,
                         evidence.context_seq ASC,
                         evidence.id ASC
                LIMIT ?7
                "#,
            )
            .bind(session_id)
            .bind(before_session_seq)
            .bind(after_prompt_session_seq)
            .bind(after_context_seq)
            .bind(after_id)
            .bind(through_id)
            .bind(limit)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| context_evidence_from_row(&row))
                .collect()
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn latest_export_context_evidence_id(
        &self,
        session_id: &str,
        before_session_seq: i64,
    ) -> Result<i64> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            Ok(sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(MAX(id), 0) FROM context_evidence WHERE session_id = ?1 AND prompt_session_seq < ?2",
            )
            .bind(session_id)
            .bind(before_session_seq)
            .fetch_one(&mut *conn)
            .await?)
        })
        .await
    }
}

fn context_evidence_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ContextEvidenceRecord> {
    Ok(ContextEvidenceRecord {
        id: row.try_get(0)?,
        session_id: row.try_get(1)?,
        prompt_session_seq: row.try_get(2)?,
        context_seq: row.try_get(3)?,
        role: row.try_get(4)?,
        source_kind: row.try_get(5)?,
        source_name: row.try_get(6)?,
        source_path: row.try_get(7)?,
        provider_group: row.try_get(8)?,
        provider_block_index: row.try_get(9)?,
        context_kind: row.try_get(10)?,
        timestamp_ms: row.try_get(11)?,
        content_text: row.try_get(12)?,
        metadata: parse_optional_json(row.try_get(13)?)?,
    })
}

#[derive(Debug)]
pub(crate) struct PreparedContextEvidence {
    pub(crate) role: String,
    pub(crate) source_kind: String,
    pub(crate) source_name: Option<String>,
    pub(crate) source_path: Option<String>,
    pub(crate) provider_group: Option<String>,
    pub(crate) provider_block_index: Option<i64>,
    pub(crate) context_kind: Option<String>,
    pub(crate) content_text: String,
    pub(crate) metadata_json: Option<String>,
}

pub(crate) fn prepare_context_evidence(
    evidence: &[ContextEvidenceInput],
) -> Result<Vec<PreparedContextEvidence>> {
    evidence
        .iter()
        .map(|item| {
            Ok(PreparedContextEvidence {
                role: item.role.clone(),
                source_kind: item.source_kind.clone(),
                source_name: item.source_name.clone(),
                source_path: item.source_path.clone(),
                provider_group: item.provider_group.clone(),
                provider_block_index: item.provider_block_index,
                context_kind: item.context_kind.clone(),
                content_text: item.content_text.clone(),
                metadata_json: optional_json_string(&item.metadata)?,
            })
        })
        .collect()
}

pub(crate) async fn insert_context_evidence_rows(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
    prompt_session_seq: i64,
    timestamp_ms: i64,
    evidence: &[PreparedContextEvidence],
) -> Result<()> {
    for (index, item) in evidence.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO context_evidence (
                session_id, prompt_session_seq, context_seq, role, source_kind,
                source_name, source_path, provider_group, provider_block_index,
                context_kind, timestamp_ms, content_text, metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
        )
        .bind(session_id)
        .bind(prompt_session_seq)
        .bind(index as i64 + 1)
        .bind(&item.role)
        .bind(&item.source_kind)
        .bind(&item.source_name)
        .bind(&item.source_path)
        .bind(&item.provider_group)
        .bind(item.provider_block_index)
        .bind(&item.context_kind)
        .bind(timestamp_ms)
        .bind(&item.content_text)
        .bind(&item.metadata_json)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}
