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
use super::store_messages::insert_inherited_message_in_tx;
use super::store_runtime_bindings::snapshot_resolved_writable_runtime_binding_in_tx;
use super::{
    ChildSessionSnapshotInput, SessionBrowserRequest, SessionBrowserWorkspaceProjection,
    SessionListCursor, SessionListProjection, SessionListProjectionPage, SessionSummaryPage,
    StateRuntime,
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
        let child_session_id = Uuid::now_v7().to_string();
        let now = now_ms();
        let cwd = input.cwd.to_string_lossy().into_owned();
        let metadata_json = input
            .metadata
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        let inherited_message_metadata_json =
            serde_json::to_string(&input.inherited_message_metadata)?;

        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let inserted = sqlx::query(
                r#"
                INSERT INTO sessions (
                    id, source, parent_session_id, cwd, model, provider,
                    started_at_ms, updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                    message_count, tool_call_count, title, metadata_json
                )
                SELECT ?1, ?2, ?3, ?4, ?5, ?6,
                       ?7, ?7, NULL, NULL, NULL, 0, 0, NULL, ?8
                FROM sessions
                WHERE id = ?3 AND cwd = ?4
                "#,
            )
            .bind(&child_session_id)
            .bind(input.source)
            .bind(input.parent_session_id)
            .bind(&cwd)
            .bind(input.model)
            .bind(input.provider)
            .bind(now)
            .bind(&metadata_json)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if inserted != 1 {
                return Err(Error::Message(format!(
                    "parent Thread `{}` was not found in `{cwd}`",
                    input.parent_session_id
                )));
            }

            let inherited_count = sqlx::query(
                r#"
                INSERT INTO messages (
                    session_id, session_seq, role, timestamp_ms, message_json,
                    content_text, tool_call_id, tool_name, tool_calls_json,
                    finish_reason, outcome, model, provider, usage_json, metadata_json
                )
                SELECT
                    ?1,
                    ROW_NUMBER() OVER (ORDER BY session_seq),
                    role, timestamp_ms, message_json,
                    content_text, tool_call_id, tool_name, tool_calls_json,
                    finish_reason, outcome, model, provider, NULL, ?2
                FROM messages
                WHERE session_id = ?3
                ORDER BY session_seq ASC
                "#,
            )
            .bind(&child_session_id)
            .bind(&inherited_message_metadata_json)
            .bind(input.parent_session_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            let inherited_count = i64::try_from(inherited_count).map_err(|_| {
                Error::Message("side conversation history is too large".to_string())
            })?;
            insert_inherited_message_in_tx(
                &mut tx,
                &child_session_id,
                inherited_count.saturating_add(1),
                &user_text_message(input.boundary_text),
                &inherited_message_metadata_json,
            )
            .await?;
            let message_count = inherited_count.saturating_add(1);
            let tool_call_count = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COALESCE(SUM(json_array_length(tool_calls_json)), 0)
                FROM messages
                WHERE session_id = ?1 AND tool_calls_json IS NOT NULL
                "#,
            )
            .bind(&child_session_id)
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE sessions SET message_count = ?1, tool_call_count = ?2 WHERE id = ?3",
            )
            .bind(message_count)
            .bind(tool_call_count)
            .bind(&child_session_id)
            .execute(&mut *tx)
            .await?;

            if let Some(binding) = input.runtime_binding
                && let Err(error) = snapshot_resolved_writable_runtime_binding_in_tx(
                    &mut tx,
                    input.parent_session_id,
                    &child_session_id,
                    binding.expected_binding_revision,
                    binding.expected_control_revision,
                    binding.effective_controls,
                    now,
                )
                .await
            {
                tx.rollback().await?;
                return Err(error);
            }

            tx.commit().await?;
            Ok(child_session_id)
        })
        .await
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
                   message_count, tool_call_count, title,
                   json_extract(metadata_json, '$.forkedFromThreadId') AS forked_from_thread_id
            FROM sessions
            WHERE (?1 IS NULL OR cwd = ?1)
              AND ((?2 = 0 AND archived_at_ms IS NULL) OR (?2 = 1 AND archived_at_ms IS NOT NULL))
            ORDER BY updated_at_ms DESC, message_count DESC, started_at_ms DESC, id DESC
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

    pub(crate) async fn list_session_summary_page(
        &self,
        cwd: Option<&str>,
        sources: &[String],
        archived: bool,
        cursor: Option<&SessionListCursor>,
        limit: usize,
    ) -> Result<SessionSummaryPage> {
        let limit = limit.clamp(1, 200);
        let sources_json = serde_json::to_string(sources)?;
        let source_filter_disabled = i64::from(sources.is_empty());
        let cursor_updated_at_ms = cursor.map(|cursor| cursor.updated_at_ms);
        let cursor_id = cursor.map(|cursor| cursor.id.as_str());
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
                SELECT id, source, parent_session_id, cwd, model, provider, started_at_ms,
                       updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                       message_count, tool_call_count, title,
                       json_extract(metadata_json, '$.forkedFromThreadId') AS forked_from_thread_id
                FROM sessions
                WHERE (?1 IS NULL OR cwd = ?1)
                  AND ((?2 = 0 AND archived_at_ms IS NULL)
                    OR (?2 = 1 AND archived_at_ms IS NOT NULL))
                  AND (?3 = 1 OR source IN (SELECT value FROM json_each(?4)))
                  AND (
                    ?5 IS NULL
                    OR updated_at_ms < ?5
                    OR (updated_at_ms = ?5 AND id < ?6)
                  )
                ORDER BY updated_at_ms DESC, id DESC
                LIMIT ?7
                "#,
            )
            .bind(cwd)
            .bind(i64::from(archived))
            .bind(source_filter_disabled)
            .bind(sources_json)
            .bind(cursor_updated_at_ms)
            .bind(cursor_id)
            .bind(i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX))
            .fetch_all(&mut *conn)
            .await?;
            let mut summaries = rows
                .into_iter()
                .map(|row| session_summary_from_sqlx_row(&row))
                .collect::<Result<Vec<_>>>()?;
            let has_more = summaries.len() > limit;
            summaries.truncate(limit);
            let next_cursor = has_more.then(|| {
                let last = summaries.last().expect("non-empty paged session result");
                SessionListCursor {
                    updated_at_ms: last.updated_at_ms,
                    id: last.id.clone(),
                }
            });
            Ok(SessionSummaryPage {
                summaries,
                next_cursor,
            })
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn browse_human_sessions(
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
                   json_extract(r.metadata_json, '$.forkedFromThreadId') AS forked_from_thread_id,
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

    pub(crate) async fn list_human_session_projections(
        &self,
        cwd: Option<&str>,
        archived: bool,
        cursor: Option<&SessionListCursor>,
        limit: usize,
    ) -> Result<SessionListProjectionPage> {
        let internal_sources_json = serde_json::to_string(SIDE_CONVERSATION_SESSION_SOURCES)?;
        let archived = i64::from(archived);
        let cursor_updated_at_ms = cursor.map(|cursor| cursor.updated_at_ms);
        let cursor_id = cursor.map(|cursor| cursor.id.as_str());
        let limit = limit.clamp(1, 200);
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT s.id, s.source, s.parent_session_id, s.cwd, s.model, s.provider,
                   s.started_at_ms, s.updated_at_ms, s.ended_at_ms, s.end_reason,
                   s.archived_at_ms, s.message_count, s.tool_call_count, s.title,
                   json_extract(s.metadata_json, '$.forkedFromThreadId') AS forked_from_thread_id,
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
              AND (
                ?4 IS NULL
                OR s.updated_at_ms < ?4
                OR (s.updated_at_ms = ?4 AND s.id < ?5)
              )
            ORDER BY s.updated_at_ms DESC, s.id DESC
            LIMIT ?6
            "#,
            )
            .bind(cwd)
            .bind(archived)
            .bind(internal_sources_json)
            .bind(cursor_updated_at_ms)
            .bind(cursor_id)
            .bind(i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX))
            .fetch_all(&mut *conn)
            .await?;
            let mut sessions = rows
                .into_iter()
                .map(|row| {
                    projection_from_raw(session_projection_from_row(&row)?).map(|value| value.0)
                })
                .collect::<Result<Vec<_>>>()?;
            let has_more = sessions.len() > limit;
            sessions.truncate(limit);
            let next_cursor = has_more.then(|| {
                let last = sessions.last().expect("non-empty paged session result");
                SessionListCursor {
                    updated_at_ms: last.summary.updated_at_ms,
                    id: last.summary.id.clone(),
                }
            });
            Ok(SessionListProjectionPage {
                sessions,
                next_cursor,
            })
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn session_list_projection(
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
                       json_extract(s.metadata_json, '$.forkedFromThreadId') AS forked_from_thread_id,
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
                       message_count, tool_call_count, title,
                       json_extract(metadata_json, '$.forkedFromThreadId') AS forked_from_thread_id
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

    pub(crate) async fn session_summaries_by_ids(
        &self,
        session_ids: &[String],
    ) -> Result<Vec<SessionSummary>> {
        if session_ids.is_empty() {
            return Ok(Vec::new());
        }
        if session_ids.len() > 200 {
            return Err(Error::Message(
                "session summary batch exceeds the 200-Thread limit".to_string(),
            ));
        }
        let ids = serde_json::to_string(session_ids)?;
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
                SELECT id, source, parent_session_id, cwd, model, provider, started_at_ms,
                       updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                       message_count, tool_call_count, title,
                       json_extract(metadata_json, '$.forkedFromThreadId') AS forked_from_thread_id
                FROM sessions
                WHERE id IN (SELECT value FROM json_each(?1))
                "#,
            )
            .bind(ids)
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(|row| session_summary_from_sqlx_row(&row))
            .collect()
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn session_composer_model_selection(
        &self,
        session_id: &str,
    ) -> Result<Option<(String, String, Option<String>)>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
                SELECT provider, model,
                       json_extract(metadata_json, '$.composerModel.reasoningEffort')
                FROM sessions
                WHERE id = ?1
                "#,
            )
            .bind(session_id)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| {
                Ok((
                    row.try_get(0)?,
                    row.try_get(1)?,
                    row.try_get::<Option<String>, _>(2)?,
                ))
            })
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

    pub(crate) async fn acknowledged_agent_delete_thread_ids(&self) -> Result<Vec<String>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            Ok(sqlx::query_scalar::<_, String>(
                r#"
                SELECT id
                FROM sessions
                WHERE json_extract(
                    metadata_json,
                    '$.agentSessionDeleteIntent.state'
                ) = 'remoteAcknowledged'
                ORDER BY id ASC
                "#,
            )
            .fetch_all(&mut *conn)
            .await?)
        })
        .await
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

    pub(crate) async fn set_session_composer_model(
        &self,
        session_id: &str,
        provider: &str,
        model: &str,
        reasoning_effort: Option<&str>,
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
            let qualified_model = format!("{provider}/{model}");
            let mut composer_model = Map::new();
            composer_model.insert("model".to_string(), Value::String(qualified_model));
            if let Some(reasoning_effort) = reasoning_effort {
                composer_model.insert(
                    "reasoningEffort".to_string(),
                    Value::String(reasoning_effort.to_string()),
                );
            }
            metadata.insert(
                crate::model_state::SESSION_COMPOSER_MODEL_METADATA_KEY.to_string(),
                Value::Object(composer_model),
            );
            let metadata_json = encode_metadata(metadata)?;
            sqlx::query(
                "UPDATE sessions SET provider = ?1, model = ?2, metadata_json = ?3, updated_at_ms = ?4 WHERE id = ?5",
            )
            .bind(provider)
            .bind(model)
            .bind(metadata_json)
            .bind(now_ms())
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(())
        }
        .await;
        operation.finish(&result);
        result
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
        row.try_get(15)?,
        row.try_get(16)?,
        row.try_get(17)?,
        row.try_get(18)?,
        false,
        0,
    ))
}

fn session_browser_projection_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<RawSessionProjection> {
    Ok((
        session_summary_from_sqlx_row(row)?,
        row.try_get(15)?,
        row.try_get(16)?,
        row.try_get(17)?,
        row.try_get(18)?,
        row.try_get::<i64, _>(19)? != 0,
        row.try_get::<i64, _>(20)? as usize,
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
    let metadata = parse_optional_json(metadata_json)
        .map_err(|_| corrupt_session_projection(&summary.id, "metadata"))?;
    Ok((
        SessionListProjection {
            summary,
            first_user_text,
            metadata,
            runtime_backend_kind,
            runtime_ref,
        },
        exception,
        total,
    ))
}

fn session_summary_from_sqlx_row(row: &sqlx::sqlite::SqliteRow) -> Result<SessionSummary> {
    let id: String = row.try_get(0)?;
    let forked_from_thread_id: Option<String> = row
        .try_get(14)
        .map_err(|_| corrupt_session_projection(&id, "forkedFromThreadId"))?;
    Ok(SessionSummary {
        id,
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
        forked_from_thread_id,
    })
}

fn corrupt_session_projection(thread_id: &str, field: &'static str) -> Error {
    Error::structured(
        "Persisted Thread presentation data is invalid.",
        serde_json::json!({
            "kind": "corrupt_thread_presentation",
            "threadId": thread_id,
            "field": field,
        }),
    )
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use psychevo_agent_core::{Message, UserContentBlock, user_text_message};
    use serde_json::json;

    use super::*;
    use crate::state::{
        ChildSessionRuntimeBindingSnapshotInput, GatewayRuntimeBindingInput,
        GatewayRuntimeBindingOwnership, GatewayRuntimeBindingRecord,
    };

    const CWD: &str = "/workspace";

    async fn parent_with_history(store: &StateRuntime) -> (String, Vec<Message>) {
        let parent = store
            .create_session_with_metadata(Path::new(CWD), "web", "model", "provider", None)
            .await
            .expect("parent");
        let messages = vec![user_text_message("first"), user_text_message("second")];
        for message in &messages {
            store
                .append_message(&parent, message)
                .await
                .expect("parent message");
        }
        (parent, messages)
    }

    async fn bind_parent(
        store: &StateRuntime,
        parent: &str,
        ownership: GatewayRuntimeBindingOwnership,
    ) -> GatewayRuntimeBindingRecord {
        store
            .create_gateway_runtime_binding(GatewayRuntimeBindingInput {
                thread_id: parent,
                agent_ref: Some("reviewer"),
                agent_fingerprint: "agent-fingerprint",
                agent_definition_json: r#"{"name":"reviewer"}"#,
                runtime_ref: "runtime:reviewer",
                backend_kind: "acp",
                native_kind: "acp",
                native_session_id: Some("native-parent"),
                cwd: CWD,
                profile_fingerprint: "profile-fingerprint",
                profile_revision: "profile-revision",
                profile_config_json: "{}",
                adapter_kind: "acp",
                adapter_revision: "adapter-revision",
                ownership,
                parent_thread_id: None,
            })
            .await
            .expect("parent binding")
    }

    async fn durable_counts(store: &StateRuntime) -> (i64, i64, i64) {
        let sessions = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&store.inner.pool)
            .await
            .expect("session count");
        let messages = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
            .fetch_one(&store.inner.pool)
            .await
            .expect("message count");
        let bindings = sqlx::query_scalar("SELECT COUNT(*) FROM gateway_runtime_bindings")
            .fetch_one(&store.inner.pool)
            .await
            .expect("binding count");
        (sessions, messages, bindings)
    }

    async fn create_side(
        store: &StateRuntime,
        parent: &str,
        binding: Option<ChildSessionRuntimeBindingSnapshotInput<'_>>,
    ) -> Result<String> {
        store
            .create_child_session_from_parent_snapshot(ChildSessionSnapshotInput {
                parent_session_id: parent,
                cwd: Path::new(CWD),
                source: crate::thread_lineage::WEB_SIDE_CONVERSATION_SESSION_SOURCE,
                model: "model",
                provider: "provider",
                metadata: Some(json!({"side_conversation": true})),
                inherited_message_metadata: json!({
                    crate::thread_lineage::SIDE_INHERITED_METADATA_KEY: {
                        "hidden": true,
                        "parent_session_id": parent,
                    }
                }),
                boundary_text: crate::prompt_templates::side_conversation_boundary_prompt(),
                runtime_binding: binding,
            })
            .await
    }

    #[tokio::test]
    async fn side_conversation_snapshot_commits_history_boundary_and_writable_binding_atomically() {
        let store = StateRuntime::open(":memory:").await.expect("store");
        let (parent, parent_messages) = parent_with_history(&store).await;
        let parent_binding =
            bind_parent(&store, &parent, GatewayRuntimeBindingOwnership::ReadWrite).await;
        let effective_controls = BTreeMap::from([
            ("mode".to_string(), json!("plan")),
            ("model".to_string(), json!("live-model")),
        ]);

        let child = create_side(
            &store,
            &parent,
            Some(ChildSessionRuntimeBindingSnapshotInput {
                expected_binding_revision: parent_binding.binding_revision,
                expected_control_revision: parent_binding.control_revision,
                effective_controls: &effective_controls,
            }),
        )
        .await
        .expect("atomic side conversation");

        let history = store.load_messages(&child).await.expect("child history");
        assert_eq!(
            &history[..parent_messages.len()],
            parent_messages.as_slice()
        );
        assert_eq!(history.len(), parent_messages.len() + 1);
        let Message::User { content, .. } = history.last().expect("boundary") else {
            panic!("side conversation boundary must be a user message");
        };
        assert!(content.iter().any(|part| matches!(
            part,
            UserContentBlock::Text(block)
                if block.text == crate::prompt_templates::side_conversation_boundary_prompt()
        )));
        let hidden_messages = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*) FROM messages
            WHERE session_id = ?1
              AND json_extract(metadata_json, '$.side_inherited.hidden') = 1
            "#,
        )
        .bind(&child)
        .fetch_one(&store.inner.pool)
        .await
        .expect("hidden message count");
        assert_eq!(hidden_messages, history.len() as i64);

        let child_binding = store
            .gateway_runtime_binding(&child)
            .await
            .expect("binding read")
            .expect("child binding");
        assert_eq!(child_binding.agent_ref, parent_binding.agent_ref);
        assert_eq!(child_binding.runtime_ref, parent_binding.runtime_ref);
        assert_eq!(
            child_binding.ownership,
            GatewayRuntimeBindingOwnership::ReadWrite
        );
        assert_eq!(child_binding.native_session_id, None);
        assert_eq!(child_binding.parent_thread_id, None);
        assert_eq!(child_binding.binding_revision, 1);
        assert_eq!(child_binding.thread_preferences, effective_controls);
        assert!(child_binding.runtime_observed.is_empty());
        assert_eq!(child_binding.control_revision, 1);
    }

    #[tokio::test]
    async fn side_conversation_snapshot_rolls_back_when_parent_binding_is_missing() {
        let store = StateRuntime::open(":memory:").await.expect("store");
        let (parent, _) = parent_with_history(&store).await;
        let controls = BTreeMap::new();
        let before = durable_counts(&store).await;

        let error = create_side(
            &store,
            &parent,
            Some(ChildSessionRuntimeBindingSnapshotInput {
                expected_binding_revision: 1,
                expected_control_revision: 1,
                effective_controls: &controls,
            }),
        )
        .await
        .expect_err("missing binding must reject the snapshot");

        assert!(
            error
                .to_string()
                .contains("resolved writable runtime binding")
        );
        assert_eq!(durable_counts(&store).await, before);
    }

    #[tokio::test]
    async fn side_conversation_snapshot_rolls_back_for_read_only_parent_binding() {
        let store = StateRuntime::open(":memory:").await.expect("store");
        let (parent, _) = parent_with_history(&store).await;
        let binding = bind_parent(&store, &parent, GatewayRuntimeBindingOwnership::ReadOnly).await;
        let controls = BTreeMap::new();
        let before = durable_counts(&store).await;

        create_side(
            &store,
            &parent,
            Some(ChildSessionRuntimeBindingSnapshotInput {
                expected_binding_revision: binding.binding_revision,
                expected_control_revision: binding.control_revision,
                effective_controls: &controls,
            }),
        )
        .await
        .expect_err("read-only binding must reject the snapshot");

        assert_eq!(durable_counts(&store).await, before);
    }

    #[tokio::test]
    async fn side_conversation_snapshot_rolls_back_for_stale_binding_revision() {
        let store = StateRuntime::open(":memory:").await.expect("store");
        let (parent, _) = parent_with_history(&store).await;
        let binding = bind_parent(&store, &parent, GatewayRuntimeBindingOwnership::ReadWrite).await;
        let controls = BTreeMap::new();
        let before = durable_counts(&store).await;

        create_side(
            &store,
            &parent,
            Some(ChildSessionRuntimeBindingSnapshotInput {
                expected_binding_revision: binding.binding_revision + 1,
                expected_control_revision: binding.control_revision,
                effective_controls: &controls,
            }),
        )
        .await
        .expect_err("stale binding revision must reject the snapshot");

        assert_eq!(durable_counts(&store).await, before);
    }
}
