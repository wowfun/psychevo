#[allow(unused_imports)]
pub(crate) use super::*;
impl SqliteStore {
    pub fn create_session(&self, cwd: &Path) -> Result<String> {
        self.create_session_with_metadata(cwd, "smoke", "fake-coding-model", "fake", None)
    }

    pub fn create_session_with_metadata(
        &self,
        cwd: &Path,
        source: &str,
        model: &str,
        provider: &str,
        metadata: Option<Value>,
    ) -> Result<String> {
        self.create_session_with_parent_and_metadata(cwd, source, None, model, provider, metadata)
    }

    pub fn create_child_session_with_metadata(
        &self,
        parent_session_id: &str,
        cwd: &Path,
        source: &str,
        model: &str,
        provider: &str,
        metadata: Option<Value>,
    ) -> Result<String> {
        self.create_session_with_parent_and_metadata(
            cwd,
            source,
            Some(parent_session_id),
            model,
            provider,
            metadata,
        )
    }

    pub fn create_child_session_from_parent_snapshot(
        &self,
        input: ChildSessionSnapshotInput<'_>,
    ) -> Result<String> {
        let parent_messages = crate::context::prune_context(
            self.load_messages(input.parent_session_id)?,
            input.max_context_messages,
        );
        let child_session = self.create_child_session_with_metadata(
            input.parent_session_id,
            input.cwd,
            input.source,
            input.model,
            input.provider,
            input.metadata,
        )?;
        for message in parent_messages {
            self.append_message_with_metrics(
                &child_session,
                &message,
                None,
                Some(input.inherited_message_metadata.clone()),
            )?;
        }
        self.append_message_with_metrics(
            &child_session,
            &user_text_message(input.boundary_text),
            None,
            Some(input.inherited_message_metadata),
        )?;
        Ok(child_session)
    }

    pub(crate) fn create_session_with_parent_and_metadata(
        &self,
        cwd: &Path,
        source: &str,
        parent_session_id: Option<&str>,
        model: &str,
        provider: &str,
        metadata: Option<Value>,
    ) -> Result<String> {
        let id = Uuid::now_v7().to_string();
        let now = now_ms();
        let cwd = cwd.to_string_lossy().to_string();
        let metadata_json = metadata
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO sessions (
                    id, source, parent_session_id, cwd, model, provider,
                    started_at_ms, updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                    message_count, tool_call_count, title, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6,
                    ?7, ?7, NULL, NULL, NULL, 0, 0, NULL, ?8)
                "#,
                params![
                    &id,
                    source,
                    parent_session_id,
                    &cwd,
                    model,
                    provider,
                    now,
                    &metadata_json
                ],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn latest_run_session_for_cwd(&self, cwd: &Path) -> Result<Option<String>> {
        self.latest_session_for_cwd_with_sources(cwd, &["run"])
    }

    pub fn latest_session_for_cwd_with_sources(
        &self,
        cwd: &Path,
        sources: &[&str],
    ) -> Result<Option<String>> {
        Ok(self
            .list_sessions_for_cwd_with_sources(cwd, sources)?
            .into_iter()
            .next()
            .map(|session| session.id))
    }

    pub fn list_sessions_for_cwd_with_sources(
        &self,
        cwd: &Path,
        sources: &[&str],
    ) -> Result<Vec<SessionSummary>> {
        let cwd = cwd.to_string_lossy().to_string();
        self.list_sessions_with_sources_and_archive(Some(&cwd), sources, false)
    }

    pub fn list_archived_sessions_for_cwd_with_sources(
        &self,
        cwd: &Path,
        sources: &[&str],
    ) -> Result<Vec<SessionSummary>> {
        let cwd = cwd.to_string_lossy().to_string();
        self.list_sessions_with_sources_and_archive(Some(&cwd), sources, true)
    }

    pub fn list_sessions_with_sources(&self, sources: &[&str]) -> Result<Vec<SessionSummary>> {
        self.list_sessions_with_sources_and_archive(None, sources, false)
    }

    pub fn list_archived_sessions_with_sources(
        &self,
        sources: &[&str],
    ) -> Result<Vec<SessionSummary>> {
        self.list_sessions_with_sources_and_archive(None, sources, true)
    }

    pub(crate) fn list_sessions_with_sources_and_archive(
        &self,
        cwd: Option<&str>,
        sources: &[&str],
        archived: bool,
    ) -> Result<Vec<SessionSummary>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, source, parent_session_id, cwd, model, provider, started_at_ms,
                   updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                   message_count, tool_call_count, title
            FROM sessions
            WHERE (?1 IS NULL OR cwd = ?1)
              AND ((?2 = 0 AND archived_at_ms IS NULL) OR (?2 = 1 AND archived_at_ms IS NOT NULL))
            ORDER BY updated_at_ms DESC, started_at_ms DESC
            "#,
        )?;
        let archived = if archived { 1 } else { 0 };
        let rows = stmt.query_map(params![cwd, archived], session_summary_from_row)?;
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
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        Ok(conn
            .query_row(
                r#"
                SELECT id, source, parent_session_id, cwd, model, provider, started_at_ms,
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

    pub fn session_metadata(&self, session_id: &str) -> Result<Option<Value>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let metadata = conn
            .query_row(
                "SELECT metadata_json FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        parse_optional_json(metadata)
    }

    pub fn set_session_metadata_field(
        &self,
        session_id: &str,
        key: &str,
        value: Option<Value>,
    ) -> Result<()> {
        let changed = self.write_retry(|conn| {
            let metadata_row = conn
                .query_row(
                    "SELECT metadata_json FROM sessions WHERE id = ?1",
                    params![session_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?;
            let Some(metadata_json) = metadata_row else {
                return Ok(0);
            };
            let mut metadata = metadata_object_sql(metadata_json.as_deref())?;
            if let Some(value) = &value {
                metadata.insert(key.to_string(), value.clone());
            } else {
                metadata.remove(key);
            }
            let metadata_json = metadata_json_sql(metadata)?;
            conn.execute(
                "UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![metadata_json, now_ms(), session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn set_session_metadata(&self, session_id: &str, metadata: Option<Value>) -> Result<()> {
        let metadata_json = metadata
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![metadata_json, now_ms(), session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn set_session_model(&self, session_id: &str, provider: &str, model: &str) -> Result<()> {
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET provider = ?1, model = ?2, updated_at_ms = ?3 WHERE id = ?4",
                params![provider, model, now_ms(), session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
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

    pub fn set_session_title_if_empty(
        &self,
        session_id: &str,
        title: &str,
    ) -> Result<Option<String>> {
        let title = normalize_session_title(title)
            .ok_or_else(|| Error::Message("session title is empty".to_string()))?;
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET title = ?1 WHERE id = ?2 AND (title IS NULL OR trim(title) = '')",
                params![&title, session_id],
            )
        })?;
        if changed > 0 {
            return Ok(Some(title));
        }
        if self.session_summary(session_id)?.is_none() {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(None)
    }

    pub fn archive_session(&self, session_id: &str) -> Result<()> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET archived_at_ms = ?1 WHERE id = ?2",
                params![now, session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn restore_session(&self, session_id: &str) -> Result<()> {
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET archived_at_ms = NULL WHERE id = ?1",
                params![session_id],
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

    pub fn delete_sessions_for_cwd_with_source(&self, cwd: &Path, source: &str) -> Result<usize> {
        let ids = self.session_ids_for_cwd_with_source(cwd, source)?;
        self.write_retry(|conn| {
            for id in &ids {
                conn.execute("DELETE FROM messages WHERE session_id = ?1", params![id])?;
                conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
            }
            Ok(ids.len())
        })
    }

    pub fn session_ids_for_cwd_with_source(&self, cwd: &Path, source: &str) -> Result<Vec<String>> {
        let cwd = cwd.to_string_lossy().to_string();
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt =
            conn.prepare("SELECT id FROM sessions WHERE cwd = ?1 AND source = ?2 ORDER BY id ASC")?;
        let rows = stmt.query_map(params![&cwd, source], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }
}
