impl SqliteStore {
    pub fn load_context_evidence(
        &self,
        session_id: &str,
        prompt_session_seq: i64,
    ) -> Result<Vec<ContextEvidenceRecord>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, session_id, prompt_session_seq, context_seq, role,
                   source_kind, source_name, source_path, timestamp_ms,
                   content_text, metadata_json
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
                row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, Option<String>>(10)?,
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
                timestamp_ms,
                content_text,
                metadata: parse_optional_json(metadata_json)?,
            });
        }
        Ok(evidence)
    }
}

#[derive(Debug)]
struct PreparedContextEvidence {
    role: String,
    source_kind: String,
    source_name: Option<String>,
    source_path: Option<String>,
    content_text: String,
    metadata_json: Option<String>,
}

fn prepare_context_evidence(
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
                content_text: item.content_text.clone(),
                metadata_json: optional_json_string(&item.metadata)?,
            })
        })
        .collect()
}

fn insert_context_evidence_rows(
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
                source_name, source_path, timestamp_ms, content_text, metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                session_id,
                prompt_session_seq,
                index as i64 + 1,
                &item.role,
                &item.source_kind,
                &item.source_name,
                &item.source_path,
                timestamp_ms,
                &item.content_text,
                &item.metadata_json,
            ],
        )?;
    }
    Ok(())
}
