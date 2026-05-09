impl SqliteStore {
    pub fn touch_session(&self, session_id: &str) -> Result<()> {
        let now = now_ms();
        self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET updated_at_ms = ?1 WHERE id = ?2",
                params![now, session_id],
            )?;
            Ok(())
        })
    }

    pub fn finish_session(&self, session_id: &str, outcome: Outcome) -> Result<()> {
        let now = now_ms();
        self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET updated_at_ms = ?1, ended_at_ms = ?1, end_reason = ?2 WHERE id = ?3",
                params![now, outcome.as_str(), session_id],
            )?;
            Ok(())
        })
    }

    fn user_target_before(&self, session_id: &str, boundary: i64) -> Result<Option<UndoTarget>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT session_seq, message_json, content_text, metadata_json
            FROM messages
            WHERE session_id = ?1 AND role = 'user' AND session_seq < ?2
            ORDER BY session_seq DESC
            LIMIT 1
            "#,
        )?;
        stmt.query_row(params![session_id, boundary], undo_target_from_row)
            .optional()
            .map_err(Into::into)
    }

    fn user_target_after(&self, session_id: &str, boundary: i64) -> Result<Option<UndoTarget>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT session_seq, message_json, content_text, metadata_json
            FROM messages
            WHERE session_id = ?1 AND role = 'user' AND session_seq > ?2
            ORDER BY session_seq ASC
            LIMIT 1
            "#,
        )?;
        stmt.query_row(params![session_id, boundary], undo_target_from_row)
            .optional()
            .map_err(Into::into)
    }

}
