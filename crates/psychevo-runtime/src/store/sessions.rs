impl SqliteStore {
    pub fn create_session(&self, workdir: &Path) -> Result<String> {
        self.create_session_with_metadata(workdir, "smoke", "fake-coding-model", "fake", None)
    }

    pub fn create_session_with_metadata(
        &self,
        workdir: &Path,
        source: &str,
        model: &str,
        provider: &str,
        metadata: Option<Value>,
    ) -> Result<String> {
        let id = Uuid::now_v7().to_string();
        let now = now_ms();
        let workdir = workdir.to_string_lossy().to_string();
        let metadata_json = metadata
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO sessions (
                    id, source, parent_session_id, workdir, model, provider,
                    started_at_ms, updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                    message_count, tool_call_count, title, metadata_json
                ) VALUES (?1, ?2, NULL, ?3, ?4, ?5,
                    ?6, ?6, NULL, NULL, NULL, 0, 0, NULL, ?7)
                "#,
                params![&id, source, &workdir, model, provider, now, &metadata_json],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn latest_run_session_for_workdir(&self, workdir: &Path) -> Result<Option<String>> {
        self.latest_session_for_workdir_with_sources(workdir, &["run"])
    }

    pub fn latest_session_for_workdir_with_sources(
        &self,
        workdir: &Path,
        sources: &[&str],
    ) -> Result<Option<String>> {
        Ok(self
            .list_sessions_for_workdir_with_sources(workdir, sources)?
            .into_iter()
            .next()
            .map(|session| session.id))
    }

    pub fn list_sessions_for_workdir_with_sources(
        &self,
        workdir: &Path,
        sources: &[&str],
    ) -> Result<Vec<SessionSummary>> {
        let workdir = workdir.to_string_lossy().to_string();
        self.list_sessions_for_workdir_with_sources_and_archive(&workdir, sources, false)
    }

    pub fn list_archived_sessions_for_workdir_with_sources(
        &self,
        workdir: &Path,
        sources: &[&str],
    ) -> Result<Vec<SessionSummary>> {
        let workdir = workdir.to_string_lossy().to_string();
        self.list_sessions_for_workdir_with_sources_and_archive(&workdir, sources, true)
    }

    fn list_sessions_for_workdir_with_sources_and_archive(
        &self,
        workdir: &str,
        sources: &[&str],
        archived: bool,
    ) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, source, workdir, model, provider, started_at_ms,
                   updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                   message_count, tool_call_count, title
            FROM sessions
            WHERE workdir = ?1
              AND ((?2 = 0 AND archived_at_ms IS NULL) OR (?2 = 1 AND archived_at_ms IS NOT NULL))
            ORDER BY updated_at_ms DESC, started_at_ms DESC
            "#,
        )?;
        let archived = if archived { 1 } else { 0 };
        let rows = stmt.query_map(params![workdir, archived], session_summary_from_row)?;
        let mut summaries = Vec::new();
        for row in rows {
            let summary = row?;
            if sources.is_empty() || sources.iter().any(|source| *source == summary.source) {
                summaries.push(summary);
            }
        }
        Ok(summaries)
    }

    pub fn session_summary(&self, session_id: &str) -> Result<Option<SessionSummary>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        Ok(conn
            .query_row(
                r#"
                SELECT id, source, workdir, model, provider, started_at_ms,
                       updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                       message_count, tool_call_count, title
                FROM sessions
                WHERE id = ?1
                "#,
                params![session_id],
                session_summary_from_row,
            )
            .optional()?)
    }

    pub fn set_session_title(&self, session_id: &str, title: &str) -> Result<String> {
        let title = normalize_session_title(title)
            .ok_or_else(|| Error::Message("session title is empty".to_string()))?;
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET title = ?1 WHERE id = ?2",
                params![&title, session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(title)
    }

    pub fn archive_session(&self, session_id: &str) -> Result<()> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET archived_at_ms = ?1, updated_at_ms = ?1 WHERE id = ?2",
                params![now, session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn restore_session(&self, session_id: &str) -> Result<()> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET archived_at_ms = NULL, updated_at_ms = ?1 WHERE id = ?2",
                params![now, session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let changed = self.write_retry(|conn| {
            let messages = conn.execute(
                "DELETE FROM messages WHERE session_id = ?1",
                params![session_id],
            )?;
            let sessions =
                conn.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
            Ok(messages + sessions)
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

}
