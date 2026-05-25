#[allow(unused_imports)]
pub(crate) use super::*;

impl SqliteStore {
    pub fn append_display_block(&self, input: DisplayBlockInput) -> Result<i64> {
        let metadata_json = optional_json_string(&input.metadata)?;
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let block_seq: i64 = conn.query_row(
            "SELECT COALESCE(MAX(block_seq), 0) + 1 FROM display_blocks WHERE session_id = ?1",
            params![&input.session_id],
            |row| row.get(0),
        )?;
        conn.execute(
            r#"
            INSERT INTO display_blocks (
                session_id, block_seq, kind, surface, source, message_session_seq,
                title, content_text, metadata_json, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                &input.session_id,
                block_seq,
                input.kind.as_str(),
                &input.surface,
                &input.source,
                input.message_session_seq,
                &input.title,
                &input.content_text,
                &metadata_json,
                now_ms(),
            ],
        )?;
        Ok(block_seq)
    }

    pub fn load_display_blocks(&self, session_id: &str) -> Result<Vec<DisplayBlockRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, session_id, block_seq, kind, surface, source,
                   message_session_seq, title, content_text, metadata_json,
                   created_at_ms
            FROM display_blocks
            WHERE session_id = ?1
            ORDER BY block_seq ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, i64>(10)?,
            ))
        })?;
        let mut blocks = Vec::new();
        for row in rows {
            let (
                id,
                session_id,
                block_seq,
                kind,
                surface,
                source,
                message_session_seq,
                title,
                content_text,
                metadata_json,
                created_at_ms,
            ) = row?;
            let Some(kind) = DisplayBlockKind::parse(&kind) else {
                return Err(Error::Message(format!(
                    "unknown display block kind: {kind}"
                )));
            };
            blocks.push(DisplayBlockRecord {
                id,
                session_id,
                block_seq,
                kind,
                surface,
                source,
                message_session_seq,
                title,
                content_text,
                metadata: parse_optional_json(metadata_json)?,
                created_at_ms,
            });
        }
        Ok(blocks)
    }
}
