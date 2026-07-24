use std::collections::BTreeMap;
use std::path::Path;

use psychevo_agent_core::{now_ms, user_text_message};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::run::normalize_session_title;
use crate::thread_lineage::SIDE_CONVERSATION_SESSION_SOURCES;
use crate::types::SessionSummary;

use super::store_message_fields::parse_optional_json;
use super::store_metadata::{metadata_json_sql, metadata_object_sql};
use super::store_schema_helpers::session_summary_from_row;
use super::{
    ChildSessionSnapshotInput, SessionBrowserRequest, SessionBrowserWorkspaceProjection,
    SessionListProjection, StateRuntime,
};

impl StateRuntime {
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

    pub fn browse_human_sessions(
        &self,
        request: SessionBrowserRequest<'_>,
    ) -> Result<Vec<SessionBrowserWorkspaceProjection>> {
        let internal_sources_json = serde_json::to_string(SIDE_CONVERSATION_SESSION_SOURCES)?;
        let include_ids_json = serde_json::to_string(request.include_session_ids)?;
        let active_ids_json = serde_json::to_string(request.active_session_ids)?;
        let archived = i64::from(request.archived);
        let has_cursor = i64::from(request.cursor_cwd.is_some());
        let cursor_offset = request.cursor_offset as i64;
        let limit = request.limit as i64;
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            WITH visible AS MATERIALIZED (
                SELECT s.*,
                       CASE WHEN s.id IN (SELECT value FROM json_each(?4))
                                  OR s.id IN (SELECT value FROM json_each(?5))
                            THEN 1 ELSE 0 END AS is_exception
                FROM sessions s
                WHERE (?1 IS NULL OR s.cwd = ?1)
                  AND ((?2 = 0 AND s.archived_at_ms IS NULL)
                    OR (?2 = 1 AND s.archived_at_ms IS NOT NULL))
                  AND s.parent_session_id IS NULL
                  AND s.source NOT IN (SELECT value FROM json_each(?3))
                  AND json_type(s.metadata_json, '$.agentSessionImportState') IS NULL
            ),
            ranked AS MATERIALIZED (
                SELECT visible.*,
                       SUM(CASE WHEN is_exception = 0 THEN 1 ELSE 0 END) OVER (
                           PARTITION BY cwd
                           ORDER BY updated_at_ms DESC, id ASC
                           ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
                       ) AS normal_rank,
                       SUM(CASE WHEN is_exception = 0 THEN 1 ELSE 0 END) OVER (
                           PARTITION BY cwd
                       ) AS normal_total
                FROM visible
            )
            SELECT r.id, r.source, r.parent_session_id, r.cwd, r.model, r.provider,
                   r.started_at_ms, r.updated_at_ms, r.ended_at_ms, r.end_reason,
                   r.archived_at_ms, r.message_count, r.tool_call_count, r.title,
                   r.metadata_json,
                   (
                       SELECT m.content_text
                       FROM messages m
                       WHERE m.session_id = r.id
                         AND m.role = 'user'
                         AND trim(COALESCE(m.content_text, '')) != ''
                       ORDER BY m.session_seq ASC
                       LIMIT 1
                   ) AS first_user_text,
                   b.backend_kind, b.runtime_ref, r.is_exception, r.normal_total
            FROM ranked r
            LEFT JOIN gateway_runtime_bindings b ON b.thread_id = r.id
            WHERE (
                    ?10 = 0
                    AND (
                        r.is_exception = 1
                        OR (r.updated_at_ms >= ?6 AND r.normal_rank <= ?9)
                    )
                  )
               OR (
                    ?10 = 1
                    AND r.cwd = ?7
                    AND r.is_exception = 0
                    AND r.normal_rank > ?8
                    AND r.normal_rank <= (?8 + ?9)
                  )
            ORDER BY r.cwd ASC, r.updated_at_ms DESC, r.id ASC
            "#,
        )?;
        let rows = stmt.query_map(
            params![
                request.cwd,
                archived,
                internal_sources_json,
                include_ids_json,
                active_ids_json,
                request.recent_since_ms,
                request.cursor_cwd,
                cursor_offset,
                limit,
                has_cursor,
            ],
            session_browser_projection_from_row,
        )?;
        let mut grouped: BTreeMap<String, (Vec<SessionListProjection>, usize, usize)> =
            BTreeMap::new();
        for row in rows {
            let (projection, is_exception, normal_total) = projection_from_raw(row?)?;
            let workspace = grouped.entry(projection.summary.cwd.clone()).or_default();
            workspace.1 = normal_total;
            if !is_exception {
                workspace.2 += 1;
            }
            workspace.0.push(projection);
        }
        let base_offset = if request.cursor_cwd.is_some() {
            request.cursor_offset
        } else {
            0
        };
        Ok(grouped
            .into_iter()
            .map(|(cwd, (sessions, normal_total, selected_normal_count))| {
                let next_offset = base_offset.saturating_add(selected_normal_count);
                let hidden_count = normal_total.saturating_sub(next_offset);
                SessionBrowserWorkspaceProjection {
                    cwd,
                    sessions,
                    hidden_count,
                    next_offset: (hidden_count > 0).then_some(next_offset),
                }
            })
            .collect())
    }

    pub fn list_human_session_projections(
        &self,
        cwd: Option<&str>,
        archived: bool,
        limit: usize,
    ) -> Result<Vec<SessionListProjection>> {
        let internal_sources_json = serde_json::to_string(SIDE_CONVERSATION_SESSION_SOURCES)?;
        let archived = i64::from(archived);
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT s.id, s.source, s.parent_session_id, s.cwd, s.model, s.provider,
                   s.started_at_ms, s.updated_at_ms, s.ended_at_ms, s.end_reason,
                   s.archived_at_ms, s.message_count, s.tool_call_count, s.title,
                   s.metadata_json,
                   (
                       SELECT m.content_text
                       FROM messages m
                       WHERE m.session_id = s.id
                         AND m.role = 'user'
                         AND trim(COALESCE(m.content_text, '')) != ''
                       ORDER BY m.session_seq ASC
                       LIMIT 1
                   ) AS first_user_text,
                   b.backend_kind, b.runtime_ref
            FROM sessions s
            LEFT JOIN gateway_runtime_bindings b ON b.thread_id = s.id
            WHERE (?1 IS NULL OR s.cwd = ?1)
              AND ((?2 = 0 AND s.archived_at_ms IS NULL)
                OR (?2 = 1 AND s.archived_at_ms IS NOT NULL))
              AND s.parent_session_id IS NULL
              AND s.source NOT IN (SELECT value FROM json_each(?3))
              AND json_type(s.metadata_json, '$.agentSessionImportState') IS NULL
            ORDER BY s.updated_at_ms DESC, s.id ASC
            LIMIT ?4
            "#,
        )?;
        let rows = stmt.query_map(
            params![cwd, archived, internal_sources_json, limit as i64],
            session_projection_from_row,
        )?;
        rows.map(|row| projection_from_raw(row?).map(|value| value.0))
            .collect()
    }

    pub fn session_list_projection(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionListProjection>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let raw = conn
            .query_row(
                r#"
                SELECT s.id, s.source, s.parent_session_id, s.cwd, s.model, s.provider,
                       s.started_at_ms, s.updated_at_ms, s.ended_at_ms, s.end_reason,
                       s.archived_at_ms, s.message_count, s.tool_call_count, s.title,
                       s.metadata_json,
                       (
                           SELECT m.content_text
                           FROM messages m
                           WHERE m.session_id = s.id
                             AND m.role = 'user'
                             AND trim(COALESCE(m.content_text, '')) != ''
                           ORDER BY m.session_seq ASC
                           LIMIT 1
                       ) AS first_user_text,
                       b.backend_kind, b.runtime_ref
                FROM sessions s
                LEFT JOIN gateway_runtime_bindings b ON b.thread_id = s.id
                WHERE s.id = ?1
                "#,
                params![session_id],
                session_projection_from_row,
            )
            .optional()?;
        raw.map(projection_from_raw)
            .transpose()
            .map(|projection| projection.map(|value| value.0))
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
        self.clear_session_filesystem_grants(session_id);
        self.remove_session_trace(session_id);
        Ok(())
    }

    pub fn delete_sessions_for_cwd_with_source(&self, cwd: &Path, source: &str) -> Result<usize> {
        let ids = self.session_ids_for_cwd_with_source(cwd, source)?;
        let count = self.write_retry(|conn| {
            for id in &ids {
                conn.execute("DELETE FROM messages WHERE session_id = ?1", params![id])?;
                conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
            }
            Ok(ids.len())
        })?;
        for id in ids {
            self.clear_session_filesystem_grants(&id);
            self.remove_session_trace(&id);
        }
        Ok(count)
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

type RawSessionProjection = (
    SessionSummary,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
    usize,
);

fn session_projection_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawSessionProjection> {
    Ok((
        session_summary_from_row(row)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        false,
        0,
    ))
}

fn session_browser_projection_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<RawSessionProjection> {
    Ok((
        session_summary_from_row(row)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get::<_, i64>(18)? != 0,
        row.get::<_, i64>(19)? as usize,
    ))
}

fn projection_from_raw(raw: RawSessionProjection) -> Result<(SessionListProjection, bool, usize)> {
    let (
        summary,
        metadata_json,
        first_user_text,
        runtime_backend_kind,
        runtime_ref,
        exception,
        total,
    ) = raw;
    Ok((
        SessionListProjection {
            summary,
            first_user_text,
            metadata: parse_optional_json(metadata_json)?,
            runtime_backend_kind,
            runtime_ref,
        },
        exception,
        total,
    ))
}
