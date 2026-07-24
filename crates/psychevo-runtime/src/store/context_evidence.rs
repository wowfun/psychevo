use rusqlite::{Connection, params};

use crate::error::Result;

use super::store_message_fields::{optional_json_string, parse_optional_json};
use super::{ContextEvidenceInput, ContextEvidenceRecord, StateRuntime};

impl StateRuntime {
    pub fn load_context_evidence(
        &self,
        session_id: &str,
        prompt_session_seq: i64,
    ) -> Result<Vec<ContextEvidenceRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, session_id, prompt_session_seq, context_seq, role,
                   source_kind, source_name, source_path, provider_group,
                   provider_block_index, context_kind, timestamp_ms, content_text,
                   metadata_json
            FROM context_evidence
            WHERE session_id = ?1 AND prompt_session_seq = ?2
            ORDER BY context_seq ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id, prompt_session_seq], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, i64>(11)?,
                row.get::<_, String>(12)?,
                row.get::<_, Option<String>>(13)?,
            ))
        })?;
        let mut evidence = Vec::new();
        for row in rows {
            let (
                id,
                session_id,
                prompt_session_seq,
                context_seq,
                role,
                source_kind,
                source_name,
                source_path,
                provider_group,
                provider_block_index,
                context_kind,
                timestamp_ms,
                content_text,
                metadata_json,
            ) = row?;
            evidence.push(ContextEvidenceRecord {
                id,
                session_id,
                prompt_session_seq,
                context_seq,
                role,
                source_kind,
                source_name,
                source_path,
                provider_group,
                provider_block_index,
                context_kind,
                timestamp_ms,
                content_text,
                metadata: parse_optional_json(metadata_json)?,
            });
        }
        Ok(evidence)
    }
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

pub(crate) fn insert_context_evidence_rows(
    conn: &Connection,
    session_id: &str,
    prompt_session_seq: i64,
    timestamp_ms: i64,
    evidence: &[PreparedContextEvidence],
) -> rusqlite::Result<()> {
    for (index, item) in evidence.iter().enumerate() {
        conn.execute(
            r#"
            INSERT INTO context_evidence (
                session_id, prompt_session_seq, context_seq, role, source_kind,
                source_name, source_path, provider_group, provider_block_index,
                context_kind, timestamp_ms, content_text, metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            params![
                session_id,
                prompt_session_seq,
                index as i64 + 1,
                &item.role,
                &item.source_kind,
                &item.source_name,
                &item.source_path,
                &item.provider_group,
                &item.provider_block_index,
                &item.context_kind,
                timestamp_ms,
                &item.content_text,
                &item.metadata_json,
            ],
        )?;
    }
    Ok(())
}
