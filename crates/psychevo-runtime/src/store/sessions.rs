use std::collections::BTreeMap;
use std::path::Path;

use psychevo_agent_core::{now_ms, user_text_message};
use serde_json::{Map, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::run::normalize_session_title;
use crate::thread_lineage::SIDE_CONVERSATION_SESSION_SOURCES;
use crate::types::SessionSummary;

use super::store_message_fields::parse_optional_json;
use super::{
    ChildSessionSnapshotInput, SessionBrowserRequest, SessionBrowserWorkspaceProjection,
    SessionListProjection, StateRuntime,
};

impl StateRuntime {
    pub async fn create_session(&self, cwd: &Path) -> Result<String> {
        self.create_session_with_metadata(cwd, "smoke", "fake-coding-model", "fake", None)
            .await
    }

    pub async fn create_session_with_metadata(
        &self,
        cwd: &Path,
        source: &str,
        model: &str,
        provider: &str,
        metadata: Option<Value>,
    ) -> Result<String> {
        self.create_session_with_parent_and_metadata(cwd, source, None, model, provider, metadata)
            .await
    }

    pub async fn create_child_session_with_metadata(
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
        .await
    }

    pub async fn create_child_session_from_parent_snapshot(
        &self,
        input: ChildSessionSnapshotInput<'_>,
    ) -> Result<String> {
        let parent_messages = crate::context::prune_context(
            self.load_messages(input.parent_session_id).await?,
            input.max_context_messages,
        );
        let child_session = self
            .create_child_session_with_metadata(
                input.parent_session_id,
                input.cwd,
                input.source,
                input.model,
                input.provider,
                input.metadata,
            )
            .await?;
        for message in parent_messages {
            self.append_message_with_metrics(
                &child_session,
                &message,
                None,
                Some(input.inherited_message_metadata.clone()),
            )
            .await?;
        }
        self.append_message_with_metrics(
            &child_session,
            &user_text_message(input.boundary_text),
            None,
            Some(input.inherited_message_metadata),
        )
        .await?;
        Ok(child_session)
    }

    pub(crate) async fn create_session_with_parent_and_metadata(
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
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut tx = self.begin_sqlx_write().await?;
            sqlx::query(
                r#"
                INSERT INTO sessions (
                    id, source, parent_session_id, cwd, model, provider,
                    started_at_ms, updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                    message_count, tool_call_count, title, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6,
                    ?7, ?7, NULL, NULL, NULL, 0, 0, NULL, ?8)
                "#,
            )
            .bind(&id)
            .bind(source)
            .bind(parent_session_id)
            .bind(&cwd)
            .bind(model)
            .bind(provider)
            .bind(now)
            .bind(&metadata_json)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(id)
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn latest_run_session_for_cwd(&self, cwd: &Path) -> Result<Option<String>> {
        self.latest_session_for_cwd_with_sources(cwd, &["run"])
            .await
    }

    pub async fn latest_session_for_cwd_with_sources(
        &self,
        cwd: &Path,
        sources: &[&str],
    ) -> Result<Option<String>> {
        Ok(self
            .list_sessions_for_cwd_with_sources(cwd, sources)
            .await?
            .into_iter()
            .next()
            .map(|session| session.id))
    }

    pub async fn list_sessions_for_cwd_with_sources(
        &self,
        cwd: &Path,
        sources: &[&str],
    ) -> Result<Vec<SessionSummary>> {
        let cwd = cwd.to_string_lossy().to_string();
        self.list_sessions_with_sources_and_archive(Some(&cwd), sources, false)
            .await
    }

    pub async fn list_archived_sessions_for_cwd_with_sources(
        &self,
        cwd: &Path,
        sources: &[&str],
    ) -> Result<Vec<SessionSummary>> {
        let cwd = cwd.to_string_lossy().to_string();
        self.list_sessions_with_sources_and_archive(Some(&cwd), sources, true)
            .await
    }

    pub async fn list_sessions_with_sources(
        &self,
        sources: &[&str],
    ) -> Result<Vec<SessionSummary>> {
        self.list_sessions_with_sources_and_archive(None, sources, false)
            .await
    }

    pub async fn list_archived_sessions_with_sources(
        &self,
        sources: &[&str],
    ) -> Result<Vec<SessionSummary>> {
        self.list_sessions_with_sources_and_archive(None, sources, true)
            .await
    }

    pub(crate) async fn list_sessions_with_sources_and_archive(
        &self,
        cwd: Option<&str>,
        sources: &[&str],
        archived: bool,
    ) -> Result<Vec<SessionSummary>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT id, source, parent_session_id, cwd, model, provider, started_at_ms,
                   updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                   message_count, tool_call_count, title
            FROM sessions
            WHERE (?1 IS NULL OR cwd = ?1)
              AND ((?2 = 0 AND archived_at_ms IS NULL) OR (?2 = 1 AND archived_at_ms IS NOT NULL))
            ORDER BY updated_at_ms DESC, started_at_ms DESC
            "#,
            )
            .bind(cwd)
            .bind(i64::from(archived))
            .fetch_all(&mut *conn)
            .await?;
            let summaries = rows
                .into_iter()
                .map(|row| session_summary_from_sqlx_row(&row))
                .collect::<Result<Vec<_>>>()?;
            Ok(summaries
                .into_iter()
                .filter(|summary| {
                    sources.is_empty() || sources.iter().any(|source| *source == summary.source)
                })
                .collect())
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn browse_human_sessions(
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
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
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
            )
            .bind(request.cwd)
            .bind(archived)
            .bind(internal_sources_json)
            .bind(include_ids_json)
            .bind(active_ids_json)
            .bind(request.recent_since_ms)
            .bind(request.cursor_cwd)
            .bind(cursor_offset)
            .bind(limit)
            .bind(has_cursor)
            .fetch_all(&mut *conn)
            .await?;
            let mut grouped: BTreeMap<String, (Vec<SessionListProjection>, usize, usize)> =
                BTreeMap::new();
            for row in rows {
                let (projection, is_exception, normal_total) =
                    projection_from_raw(session_browser_projection_from_row(&row)?)?;
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
        .await;
        operation.finish(&result);
        result
    }

    pub async fn list_human_session_projections(
        &self,
        cwd: Option<&str>,
        archived: bool,
        limit: usize,
    ) -> Result<Vec<SessionListProjection>> {
        let internal_sources_json = serde_json::to_string(SIDE_CONVERSATION_SESSION_SOURCES)?;
        let archived = i64::from(archived);
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
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
            )
            .bind(cwd)
            .bind(archived)
            .bind(internal_sources_json)
            .bind(limit as i64)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| {
                    projection_from_raw(session_projection_from_row(&row)?).map(|value| value.0)
                })
                .collect()
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn session_list_projection(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionListProjection>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let raw = sqlx::query(
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
            )
            .bind(session_id)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| session_projection_from_row(&row))
            .transpose()?;
            raw.map(projection_from_raw)
                .transpose()
                .map(|projection| projection.map(|value| value.0))
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn session_summary(&self, session_id: &str) -> Result<Option<SessionSummary>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
                SELECT id, source, parent_session_id, cwd, model, provider, started_at_ms,
                       updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                       message_count, tool_call_count, title
                FROM sessions
                WHERE id = ?1
                "#,
            )
            .bind(session_id)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| session_summary_from_sqlx_row(&row))
            .transpose()
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn session_metadata(&self, session_id: &str) -> Result<Option<Value>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let metadata = sqlx::query_scalar::<_, Option<String>>(
                "SELECT metadata_json FROM sessions WHERE id = ?1",
            )
            .bind(session_id)
            .fetch_optional(&mut *conn)
            .await?
            .flatten();
            parse_optional_json(metadata)
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn set_session_metadata_field(
        &self,
        session_id: &str,
        key: &str,
        value: Option<Value>,
    ) -> Result<()> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut tx = self.begin_sqlx_write().await?;
            let metadata_row = sqlx::query_scalar::<_, Option<String>>(
                "SELECT metadata_json FROM sessions WHERE id = ?1",
            )
            .bind(session_id)
            .fetch_optional(&mut *tx)
            .await?;
            let Some(metadata_json) = metadata_row else {
                return Err(Error::Message(format!("session not found: {session_id}")));
            };
            let mut metadata = metadata_object(metadata_json.as_deref())?;
            if let Some(value) = &value {
                metadata.insert(key.to_string(), value.clone());
            } else {
                metadata.remove(key);
            }
            let metadata_json = encode_metadata(metadata)?;
            sqlx::query("UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3")
                .bind(metadata_json)
                .bind(now_ms())
                .bind(session_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(())
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn set_session_metadata(
        &self,
        session_id: &str,
        metadata: Option<Value>,
    ) -> Result<()> {
        let metadata_json = metadata
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        self.execute_session_update(
            sqlx::query("UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3")
                .bind(metadata_json)
                .bind(now_ms())
                .bind(session_id),
            session_id,
        )
        .await
    }

    pub async fn set_session_model(
        &self,
        session_id: &str,
        provider: &str,
        model: &str,
    ) -> Result<()> {
        self.execute_session_update(
            sqlx::query(
                "UPDATE sessions SET provider = ?1, model = ?2, updated_at_ms = ?3 WHERE id = ?4",
            )
            .bind(provider)
            .bind(model)
            .bind(now_ms())
            .bind(session_id),
            session_id,
        )
        .await
    }

    pub async fn set_session_title(&self, session_id: &str, title: &str) -> Result<String> {
        let title = normalize_session_title(title)
            .ok_or_else(|| Error::Message("session title is empty".to_string()))?;
        self.execute_session_update(
            sqlx::query("UPDATE sessions SET title = ?1 WHERE id = ?2")
                .bind(&title)
                .bind(session_id),
            session_id,
        )
        .await?;
        Ok(title)
    }

    pub async fn set_session_title_if_empty(
        &self,
        session_id: &str,
        title: &str,
    ) -> Result<Option<String>> {
        let title = normalize_session_title(title)
            .ok_or_else(|| Error::Message("session title is empty".to_string()))?;
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = sqlx::query(
                "UPDATE sessions SET title = ?1 WHERE id = ?2 AND (title IS NULL OR trim(title) = '')",
            )
            .bind(&title)
            .bind(session_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed == 0 {
                let exists = sqlx::query_scalar::<_, i64>(
                    "SELECT 1 FROM sessions WHERE id = ?1",
                )
                .bind(session_id)
                .fetch_optional(&mut *tx)
                .await?;
                if exists.is_none() {
                    return Err(Error::Message(format!("session not found: {session_id}")));
                }
            }
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok((changed > 0).then_some(title))
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn archive_session(&self, session_id: &str) -> Result<()> {
        let now = now_ms();
        self.execute_session_update(
            sqlx::query("UPDATE sessions SET archived_at_ms = ?1 WHERE id = ?2")
                .bind(now)
                .bind(session_id),
            session_id,
        )
        .await
    }

    pub async fn restore_session(&self, session_id: &str) -> Result<()> {
        self.execute_session_update(
            sqlx::query("UPDATE sessions SET archived_at_ms = NULL WHERE id = ?1").bind(session_id),
            session_id,
        )
        .await
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut tx = self.begin_sqlx_write().await?;
            let messages = sqlx::query("DELETE FROM messages WHERE session_id = ?1")
                .bind(session_id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            let sessions = sqlx::query("DELETE FROM sessions WHERE id = ?1")
                .bind(session_id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if messages + sessions == 0 {
                return Err(Error::Message(format!("session not found: {session_id}")));
            }
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(())
        }
        .await;
        operation.finish(&result);
        result?;
        self.clear_session_filesystem_grants(session_id);
        self.remove_session_trace(session_id);
        Ok(())
    }

    pub async fn delete_sessions_for_cwd_with_source(
        &self,
        cwd: &Path,
        source: &str,
    ) -> Result<usize> {
        let ids = self.session_ids_for_cwd_with_source(cwd, source).await?;
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut tx = self.begin_sqlx_write().await?;
            for id in &ids {
                sqlx::query("DELETE FROM messages WHERE session_id = ?1")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("DELETE FROM sessions WHERE id = ?1")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(ids.len())
        }
        .await;
        operation.finish(&result);
        let count = result?;
        for id in &ids {
            self.clear_session_filesystem_grants(id);
            self.remove_session_trace(id);
        }
        Ok(count)
    }

    pub async fn session_ids_for_cwd_with_source(
        &self,
        cwd: &Path,
        source: &str,
    ) -> Result<Vec<String>> {
        let cwd = cwd.to_string_lossy().to_string();
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM sessions WHERE cwd = ?1 AND source = ?2 ORDER BY id ASC",
            )
            .bind(&cwd)
            .bind(source)
            .fetch_all(&mut *conn)
            .await
            .map_err(Into::into)
        }
        .await;
        operation.finish(&result);
        result
    }

    async fn execute_session_update<'q>(
        &self,
        query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
        session_id: &str,
    ) -> Result<()> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = query.execute(&mut *tx).await?.rows_affected();
            if changed == 0 {
                return Err(Error::Message(format!("session not found: {session_id}")));
            }
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(())
        }
        .await;
        operation.finish(&result);
        result
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

fn session_projection_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<RawSessionProjection> {
    Ok((
        session_summary_from_sqlx_row(row)?,
        row.try_get(14)?,
        row.try_get(15)?,
        row.try_get(16)?,
        row.try_get(17)?,
        false,
        0,
    ))
}

fn session_browser_projection_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<RawSessionProjection> {
    Ok((
        session_summary_from_sqlx_row(row)?,
        row.try_get(14)?,
        row.try_get(15)?,
        row.try_get(16)?,
        row.try_get(17)?,
        row.try_get::<i64, _>(18)? != 0,
        row.try_get::<i64, _>(19)? as usize,
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

fn session_summary_from_sqlx_row(row: &sqlx::sqlite::SqliteRow) -> Result<SessionSummary> {
    Ok(SessionSummary {
        id: row.try_get(0)?,
        source: row.try_get(1)?,
        parent_session_id: row.try_get(2)?,
        cwd: row.try_get(3)?,
        model: row.try_get(4)?,
        provider: row.try_get(5)?,
        started_at_ms: row.try_get(6)?,
        updated_at_ms: row.try_get(7)?,
        ended_at_ms: row.try_get(8)?,
        end_reason: row.try_get(9)?,
        archived_at_ms: row.try_get(10)?,
        message_count: row.try_get(11)?,
        tool_call_count: row.try_get(12)?,
        title: row.try_get(13)?,
    })
}

fn metadata_object(value: Option<&str>) -> Result<Map<String, Value>> {
    match value {
        Some(value) => match serde_json::from_str::<Value>(value)? {
            Value::Object(metadata) => Ok(metadata),
            _ => Ok(Map::new()),
        },
        None => Ok(Map::new()),
    }
}

fn encode_metadata(metadata: Map<String, Value>) -> Result<Option<String>> {
    if metadata.is_empty() {
        Ok(None)
    } else {
        Ok(Some(serde_json::to_string(&Value::Object(metadata))?))
    }
}
