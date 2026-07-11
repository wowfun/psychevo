#[allow(unused_imports)]
pub(crate) use super::*;
impl SqliteStore {
    pub fn append_session_compaction(
        &self,
        input: SessionCompactionInput,
    ) -> Result<SessionCompactionRecord> {
        let now = now_ms();
        let metadata_json = optional_json_string(&input.metadata)?;
        let id = self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO session_compactions (
                    session_id, created_at_ms, reason, summary_text,
                    first_kept_session_seq, created_after_session_seq,
                    tokens_before, tokens_after, summary_provider, summary_model,
                    instructions, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                "#,
                params![
                    input.session_id,
                    now,
                    input.reason,
                    input.summary_text,
                    input.first_kept_session_seq,
                    input.created_after_session_seq,
                    input.tokens_before.map(|value| value as i64),
                    input.tokens_after.map(|value| value as i64),
                    input.summary_provider,
                    input.summary_model,
                    input.instructions,
                    metadata_json,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })?;
        self.session_compaction(id)?.ok_or_else(|| {
            Error::Message(format!("session compaction not found after insert: {id}"))
        })
    }

    pub fn latest_valid_session_compaction(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionCompactionRecord>> {
        let boundary = self
            .session_revert_state(session_id)?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, session_id, created_at_ms, reason, summary_text,
                   first_kept_session_seq, created_after_session_seq,
                   tokens_before, tokens_after, summary_provider, summary_model,
                   instructions, metadata_json
            FROM session_compactions
            WHERE session_id = ?1 AND created_after_session_seq < ?2
            ORDER BY created_at_ms DESC, id DESC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id, boundary], compaction_from_row)?;
        for row in rows {
            let record = row?;
            if !compaction_is_projection_only(&record) {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    pub fn list_valid_session_compactions(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionCompactionRecord>> {
        let boundary = self
            .session_revert_state(session_id)?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, session_id, created_at_ms, reason, summary_text,
                   first_kept_session_seq, created_after_session_seq,
                   tokens_before, tokens_after, summary_provider, summary_model,
                   instructions, metadata_json
            FROM session_compactions
            WHERE session_id = ?1 AND created_after_session_seq < ?2
            ORDER BY created_at_ms ASC, id ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id, boundary], compaction_from_row)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    pub fn load_message_records(&self, session_id: &str) -> Result<Vec<SessionMessageRecord>> {
        let boundary = self
            .session_revert_state(session_id)?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT session_seq, message_json
            FROM messages
            WHERE session_id = ?1 AND session_seq < ?2
            ORDER BY session_seq ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id, boundary], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (session_seq, message_json) = row?;
            records.push(SessionMessageRecord {
                session_seq,
                message: serde_json::from_str(&message_json)?,
            });
        }
        Ok(records)
    }

    pub fn delete_messages_from_seq(&self, session_id: &str, start_seq: i64) -> Result<usize> {
        self.write_retry(|conn| {
            let removed = conn.execute(
                "DELETE FROM messages WHERE session_id = ?1 AND session_seq >= ?2",
                params![session_id, start_seq],
            )?;
            let message_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )?;
            let tool_call_count = session_tool_call_count(conn, session_id)?;
            conn.execute(
                r#"
                UPDATE sessions
                SET message_count = ?1,
                    tool_call_count = ?2,
                    updated_at_ms = ?3
                WHERE id = ?4
                "#,
                params![message_count, tool_call_count, now_ms(), session_id],
            )?;
            Ok(removed)
        })
    }

    pub fn session_compaction(&self, id: i64) -> Result<Option<SessionCompactionRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            r#"
            SELECT id, session_id, created_at_ms, reason, summary_text,
                   first_kept_session_seq, created_after_session_seq,
                   tokens_before, tokens_after, summary_provider, summary_model,
                   instructions, metadata_json
            FROM session_compactions
            WHERE id = ?1
            "#,
            params![id],
            compaction_from_row,
        )
        .optional()
        .map_err(Into::into)
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
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<SessionCompactionRecord> {
    let metadata_json = row.get::<_, Option<String>>(12)?;
    let metadata = parse_optional_json(metadata_json).map_err(json_result_to_sql)?;
    Ok(SessionCompactionRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        created_at_ms: row.get(2)?,
        reason: row.get(3)?,
        summary_text: row.get(4)?,
        first_kept_session_seq: row.get(5)?,
        created_after_session_seq: row.get(6)?,
        tokens_before: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
        tokens_after: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
        summary_provider: row.get(9)?,
        summary_model: row.get(10)?,
        instructions: row.get(11)?,
        metadata,
    })
}

pub(crate) fn json_result_to_sql(err: Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(err))
}
