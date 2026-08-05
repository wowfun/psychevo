use psychevo_agent_core::{Message, now_ms};
use serde_json::{Value, json};
use sqlx::Row;

use crate::error::{Error, Result};
#[cfg(test)]
use crate::messages::sanitize_message_for_output;
use crate::messages::sanitize_message_for_tui_history;
#[cfg(test)]
use crate::types::SanitizedMessageSummary;
use crate::types::{CostStatus, MessageAccounting, SessionExportMessageSummary, TuiMessageSummary};

use super::store_context_evidence::{insert_context_evidence_rows, prepare_context_evidence};
use super::store_message_fields::{message_fields, optional_json_string, parse_optional_json};
use super::{
    ContextEvidenceInput, MESSAGE_PRE_SNAPSHOT_KEY, MESSAGE_UNDO_METADATA_KEY, StateRuntime,
};

pub(crate) struct AppendMessageParams<'a> {
    pub(crate) session_id: &'a str,
    pub(crate) message: &'a Message,
    pub(crate) usage: Option<Value>,
    pub(crate) metadata: Option<Value>,
    pub(crate) accounting: Option<MessageAccounting>,
    pub(crate) context_evidence: &'a [ContextEvidenceInput],
    pub(crate) content_text_override: Option<String>,
}

pub(super) async fn insert_inherited_message_in_tx(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
    session_seq: i64,
    message: &Message,
    metadata_json: &str,
) -> Result<i64> {
    let fields = message_fields(message)?;
    let message_json = serde_json::to_string(message)?;
    sqlx::query(
        r#"
        INSERT INTO messages (
            session_id, session_seq, role, timestamp_ms, message_json,
            content_text, tool_call_id, tool_name, tool_calls_json,
            finish_reason, outcome, model, provider, usage_json, metadata_json
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL, ?14
        )
        "#,
    )
    .bind(session_id)
    .bind(session_seq)
    .bind(&fields.role)
    .bind(fields.timestamp_ms)
    .bind(message_json)
    .bind(&fields.content_text)
    .bind(&fields.tool_call_id)
    .bind(&fields.tool_name)
    .bind(&fields.tool_calls_json)
    .bind(&fields.finish_reason)
    .bind(&fields.outcome)
    .bind(&fields.model)
    .bind(&fields.provider)
    .bind(metadata_json)
    .execute(conn)
    .await?;
    Ok(fields.tool_call_count)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredHistoryReplayDecodeField {
    Message,
    Usage,
    Metadata,
    Accounting,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StoredHistoryReplayItem {
    Available(Box<TuiMessageSummary>),
    Unavailable {
        session_seq: i64,
        invalid_fields: Vec<StoredHistoryReplayDecodeField>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredHistoryReplayPage {
    pub(crate) items: Vec<StoredHistoryReplayItem>,
    pub(crate) has_more: bool,
}

impl StateRuntime {
    pub async fn resume_session(&self, session_id: &str) -> Result<()> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let exists = sqlx::query_scalar::<_, i64>("SELECT 1 FROM sessions WHERE id = ?1")
                .bind(session_id)
                .fetch_optional(&mut *conn)
                .await?;
            if exists.is_none() {
                return Err(Error::Message(format!("session not found: {session_id}")));
            }
            Ok(())
        }
        .await;
        operation.finish(&result);
        result
    }

    #[cfg(test)]
    pub async fn load_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query_scalar::<_, String>(
                "SELECT message_json FROM messages WHERE session_id = ?1 ORDER BY session_seq ASC",
            )
            .bind(session_id)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|message_json| serde_json::from_str(&message_json).map_err(Into::into))
                .collect()
        }
        .await;
        operation.finish(&result);
        result
    }

    #[cfg(test)]
    pub async fn load_sanitized_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        Ok(self
            .load_messages(session_id)
            .await?
            .iter()
            .map(sanitize_message_for_output)
            .collect())
    }

    #[cfg(test)]
    pub async fn load_sanitized_message_summaries(
        &self,
        session_id: &str,
    ) -> Result<Vec<SanitizedMessageSummary>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
                SELECT message_json, usage_json, metadata_json
                FROM messages
                WHERE session_id = ?1
                ORDER BY session_seq ASC
                "#,
            )
            .bind(session_id)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| {
                    let message_json: String = row.try_get(0)?;
                    let message = serde_json::from_str::<Message>(&message_json)?;
                    Ok(SanitizedMessageSummary {
                        message: sanitize_message_for_output(&message),
                        usage: parse_optional_json(row.try_get(1)?)?,
                        metadata: parse_optional_json(row.try_get(2)?)?,
                    })
                })
                .collect()
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn load_export_message_summaries(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionExportMessageSummary>> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
                SELECT session_seq, message_json, usage_json, metadata_json
                FROM messages
                WHERE session_id = ?1 AND session_seq < ?2
                ORDER BY session_seq ASC
                "#,
            )
            .bind(session_id)
            .bind(boundary)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| {
                    let message_json: String = row.try_get(1)?;
                    Ok(SessionExportMessageSummary {
                        session_seq: row.try_get(0)?,
                        message: serde_json::from_str(&message_json)?,
                        usage: parse_optional_json(row.try_get(2)?)?,
                        metadata: parse_optional_json(row.try_get(3)?)?,
                    })
                })
                .collect()
        }
        .await;
        operation.finish(&result);
        result
    }

    #[cfg(test)]
    pub async fn load_tui_message_summaries(
        &self,
        session_id: &str,
    ) -> Result<Vec<TuiMessageSummary>> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
                SELECT session_seq, message_json, usage_json, metadata_json,
                       context_input_tokens, billable_input_tokens, billable_output_tokens,
                       reasoning_tokens, cache_read_tokens, cache_write_tokens,
                       reported_total_tokens, estimated_cost_nanodollars,
                       pricing_source, pricing_tier, cost_status,
                       pricing_missing_reason, pricing_version
                FROM messages
                WHERE session_id = ?1 AND session_seq < ?2
                ORDER BY session_seq ASC
                "#,
            )
            .bind(session_id)
            .bind(boundary)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| {
                    let message_json: String = row.try_get(1)?;
                    let message = serde_json::from_str::<Message>(&message_json)?;
                    Ok(TuiMessageSummary {
                        session_seq: row.try_get(0)?,
                        message: sanitize_message_for_tui_history(&message),
                        usage: parse_optional_json(row.try_get(2)?)?,
                        metadata: parse_optional_json(row.try_get(3)?)?,
                        accounting: accounting_json_from_row(&row, 4)?,
                    })
                })
                .collect()
        }
        .await;
        operation.finish(&result);
        result
    }

    pub async fn load_tui_message_summaries_before(
        &self,
        session_id: &str,
        before_session_seq: Option<i64>,
        limit: usize,
    ) -> Result<Vec<TuiMessageSummary>> {
        self.load_tui_message_summaries_before_with_visibility(
            session_id,
            before_session_seq,
            limit,
            false,
        )
        .await
    }

    pub(crate) async fn load_visible_tui_message_summaries_before(
        &self,
        session_id: &str,
        before_session_seq: Option<i64>,
        limit: usize,
    ) -> Result<Vec<TuiMessageSummary>> {
        self.load_tui_message_summaries_before_with_visibility(
            session_id,
            before_session_seq,
            limit,
            true,
        )
        .await
    }

    async fn load_tui_message_summaries_before_with_visibility(
        &self,
        session_id: &str,
        before_session_seq: Option<i64>,
        limit: usize,
        visible_only: bool,
    ) -> Result<Vec<TuiMessageSummary>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let revert_boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let boundary = before_session_seq.unwrap_or(i64::MAX).min(revert_boundary);
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let visibility_predicate = if visible_only {
                "AND json_type(metadata_json, '$.side_inherited.hidden') IS NOT 'true'"
            } else {
                ""
            };
            let sql = format!(
                r#"
                SELECT session_seq, message_json, usage_json, metadata_json,
                       context_input_tokens, billable_input_tokens, billable_output_tokens,
                       reasoning_tokens, cache_read_tokens, cache_write_tokens,
                       reported_total_tokens, estimated_cost_nanodollars,
                       pricing_source, pricing_tier, cost_status,
                       pricing_missing_reason, pricing_version
                FROM messages
                WHERE session_id = ?1
                  AND session_seq < ?2
                  {visibility_predicate}
                ORDER BY session_seq DESC
                LIMIT ?3
                "#
            );
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(session_id)
                .bind(boundary)
                .bind(i64::try_from(limit).unwrap_or(i64::MAX))
                .fetch_all(&mut *conn)
                .await?;
            let mut summaries = rows
                .into_iter()
                .map(|row| {
                    let message_json: String = row.try_get(1)?;
                    let message = serde_json::from_str::<Message>(&message_json)?;
                    Ok(TuiMessageSummary {
                        session_seq: row.try_get(0)?,
                        message: sanitize_message_for_tui_history(&message),
                        usage: parse_optional_json(row.try_get(2)?)?,
                        metadata: parse_optional_json(row.try_get(3)?)?,
                        accounting: accounting_json_from_row(&row, 4)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            summaries.reverse();
            Ok(summaries)
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn latest_assistant_usage(&self, session_id: &str) -> Result<Option<Value>> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let usage_json = sqlx::query_scalar::<_, String>(
                r#"
                SELECT usage_json
                FROM messages
                WHERE session_id = ?1
                  AND session_seq < ?2
                  AND role = 'assistant'
                  AND usage_json IS NOT NULL
                ORDER BY session_seq DESC
                LIMIT 1
                "#,
            )
            .bind(session_id)
            .bind(boundary)
            .fetch_optional(&mut *conn)
            .await?;
            usage_json
                .map(|value| serde_json::from_str(&value).map_err(Into::into))
                .transpose()
        })
        .await
    }

    pub(crate) async fn latest_assistant_effective_usage_after(
        &self,
        session_id: &str,
        after_session_seq: Option<i64>,
    ) -> Result<Option<(i64, Value)>> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let after_session_seq = after_session_seq.unwrap_or(i64::MIN);
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let row = sqlx::query(
                r#"
                WITH assistant_usage AS (
                    SELECT
                        session_seq,
                        usage_json,
                        COALESCE(
                            json_extract(usage_json, '$.total_tokens'),
                            json_extract(usage_json, '$.reported_total_tokens'),
                            json_extract(usage_json, '$.totalTokens'),
                            json_extract(usage_json, '$.total')
                        ) AS reported,
                        COALESCE(
                            json_extract(usage_json, '$.input_tokens'),
                            json_extract(usage_json, '$.prompt_tokens'),
                            json_extract(usage_json, '$.context_input_tokens'),
                            json_extract(usage_json, '$.inputTokens'),
                            json_extract(usage_json, '$.input')
                        ) AS input,
                        COALESCE(
                            json_extract(usage_json, '$.output_tokens'),
                            json_extract(usage_json, '$.completion_tokens'),
                            json_extract(usage_json, '$.outputTokens'),
                            json_extract(usage_json, '$.output')
                        ) AS output
                    FROM messages
                    WHERE session_id = ?1
                      AND session_seq > ?2
                      AND session_seq < ?3
                      AND role = 'assistant'
                      AND usage_json IS NOT NULL
                )
                SELECT session_seq, usage_json
                FROM assistant_usage
                WHERE (typeof(reported) = 'integer' AND reported >= 0)
                   OR (typeof(input) = 'integer' AND input >= 0)
                   OR (typeof(output) = 'integer' AND output >= 0)
                ORDER BY session_seq DESC
                LIMIT 1
                "#,
            )
            .bind(session_id)
            .bind(after_session_seq)
            .bind(boundary)
            .fetch_optional(&mut *conn)
            .await?;
            row.map(|row| {
                let session_seq = row.try_get(0)?;
                let usage_json: String = row.try_get(1)?;
                Ok((session_seq, serde_json::from_str(&usage_json)?))
            })
            .transpose()
        })
        .await
    }

    pub(crate) async fn display_message_count(&self, session_id: &str) -> Result<usize> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let count = self
            .observe_sqlx(async {
                let mut conn = self.acquire_sqlx().await?;
                let count = sqlx::query_scalar::<_, i64>(
                    r#"
                    SELECT COALESCE(SUM(
                        CASE
                            WHEN COALESCE(
                                json_extract(
                                    CASE WHEN json_valid(metadata_json)
                                         THEN metadata_json ELSE '{}' END,
                                    '$.side_inherited.hidden'
                                ),
                                0
                            ) = 1 THEN 0
                            WHEN role = 'user'
                              AND json_type(
                                  CASE WHEN json_valid(metadata_json)
                                       THEN metadata_json ELSE '{}' END,
                                  '$.agent_notification'
                              ) IS NULL
                              AND COALESCE(content_text, '') <> '' THEN 1
                            WHEN role = 'assistant'
                              AND COALESCE(content_text, '') <> '' THEN 1
                            ELSE 0
                        END
                    ), 0)
                    FROM messages
                    WHERE session_id = ?1 AND session_seq < ?2
                    "#,
                )
                .bind(session_id)
                .bind(boundary)
                .fetch_one(&mut *conn)
                .await?;
                Ok(count)
            })
            .await?;
        usize::try_from(count).map_err(|_| {
            Error::Message(format!(
                "display message count is out of range for session {session_id}"
            ))
        })
    }

    pub(crate) async fn load_history_replay_after(
        &self,
        session_id: &str,
        after_session_seq: Option<i64>,
        limit: usize,
    ) -> Result<StoredHistoryReplayPage> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let after_session_seq = after_session_seq.unwrap_or(i64::MIN);
        let fetch_limit = limit.saturating_add(1);
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
                SELECT session_seq, message_json, usage_json, metadata_json,
                       context_input_tokens, billable_input_tokens, billable_output_tokens,
                       reasoning_tokens, cache_read_tokens, cache_write_tokens,
                       reported_total_tokens, estimated_cost_nanodollars,
                       pricing_source, pricing_tier, cost_status,
                       pricing_missing_reason, pricing_version
                FROM messages
                WHERE session_id = ?1 AND session_seq > ?2 AND session_seq < ?3
                ORDER BY session_seq ASC
                LIMIT ?4
                "#,
            )
            .bind(session_id)
            .bind(after_session_seq)
            .bind(boundary)
            .bind(i64::try_from(fetch_limit).unwrap_or(i64::MAX))
            .fetch_all(&mut *conn)
            .await?;
            let has_more = rows.len() > limit;
            let items = rows
                .into_iter()
                .take(limit)
                .map(decode_history_replay_row)
                .collect::<Result<Vec<_>>>()?;
            Ok(StoredHistoryReplayPage { items, has_more })
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn visible_history_message_exists(
        &self,
        session_id: &str,
        session_seq: i64,
    ) -> Result<bool> {
        let revert_boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            Ok(sqlx::query_scalar::<_, i64>(
                r#"
                SELECT 1
                FROM messages
                WHERE session_id = ?1
                  AND session_seq = ?2
                  AND session_seq < ?3
                  AND json_type(metadata_json, '$.side_inherited.hidden') IS NOT 'true'
                LIMIT 1
                "#,
            )
            .bind(session_id)
            .bind(session_seq)
            .bind(revert_boundary)
            .fetch_optional(&mut *conn)
            .await?
            .is_some())
        })
        .await
    }

    pub(crate) async fn latest_message_session_seq(&self, session_id: &str) -> Result<i64> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            Ok(sqlx::query_scalar(
                "SELECT COALESCE(MAX(session_seq), 0) FROM messages WHERE session_id = ?1",
            )
            .bind(session_id)
            .fetch_one(&mut *conn)
            .await?)
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn append_framework_message(
        &self,
        session_id: &str,
        message: &Message,
        usage: Option<Value>,
        metadata: Option<Value>,
    ) -> Result<i64> {
        self.append_message_with_metrics_accounting_and_context_evidence(AppendMessageParams {
            session_id,
            message,
            usage,
            metadata,
            accounting: None,
            context_evidence: &[],
            content_text_override: None,
        })
        .await
    }

    pub async fn append_message(&self, session_id: &str, message: &Message) -> Result<()> {
        self.append_message_with_metrics(session_id, message, None, None)
            .await
    }

    pub async fn append_message_with_undo_snapshot(
        &self,
        session_id: &str,
        message: &Message,
        snapshot: Option<String>,
    ) -> Result<()> {
        self.append_message_with_undo_snapshot_and_context_evidence(
            session_id,
            message,
            snapshot,
            &[],
        )
        .await
        .map(|_| ())
    }

    pub async fn append_message_with_undo_snapshot_and_context_evidence(
        &self,
        session_id: &str,
        message: &Message,
        snapshot: Option<String>,
        context_evidence: &[ContextEvidenceInput],
    ) -> Result<i64> {
        let metadata = snapshot.map(|snapshot| {
            json!({
                MESSAGE_UNDO_METADATA_KEY: {
                    MESSAGE_PRE_SNAPSHOT_KEY: snapshot
                }
            })
        });
        self.append_message_with_undo_snapshot_metadata_and_context_evidence(
            session_id,
            message,
            metadata,
            None,
            context_evidence,
        )
        .await
    }

    pub async fn append_message_with_undo_snapshot_metadata_and_context_evidence(
        &self,
        session_id: &str,
        message: &Message,
        metadata: Option<Value>,
        content_text_override: Option<String>,
        context_evidence: &[ContextEvidenceInput],
    ) -> Result<i64> {
        self.append_message_with_metrics_accounting_and_context_evidence(AppendMessageParams {
            session_id,
            message,
            usage: None,
            metadata,
            accounting: None,
            context_evidence,
            content_text_override,
        })
        .await
    }

    pub async fn append_message_with_metrics(
        &self,
        session_id: &str,
        message: &Message,
        usage: Option<Value>,
        metadata: Option<Value>,
    ) -> Result<()> {
        self.append_message_with_metrics_and_accounting(session_id, message, usage, metadata, None)
            .await
    }

    pub async fn append_message_with_metrics_and_accounting(
        &self,
        session_id: &str,
        message: &Message,
        usage: Option<Value>,
        metadata: Option<Value>,
        accounting: Option<MessageAccounting>,
    ) -> Result<()> {
        self.append_message_with_metrics_accounting_and_context_evidence(AppendMessageParams {
            session_id,
            message,
            usage,
            metadata,
            accounting,
            context_evidence: &[],
            content_text_override: None,
        })
        .await
        .map(|_| ())
    }

    pub(crate) async fn append_message_with_metrics_accounting_and_context_evidence(
        &self,
        params: AppendMessageParams<'_>,
    ) -> Result<i64> {
        let AppendMessageParams {
            session_id,
            message,
            usage,
            metadata,
            accounting,
            context_evidence,
            content_text_override,
        } = params;
        let mut fields = message_fields(message)?;
        if fields.role == "user"
            && let Some(content_text) = content_text_override
        {
            fields.content_text = Some(content_text);
        }
        let message_json = serde_json::to_string(message)?;
        let usage_json = optional_json_string(&usage)?;
        let metadata_json = optional_json_string(&metadata)?;
        let context_evidence = prepare_context_evidence(context_evidence)?;
        let now = now_ms();
        let accounting_values = (
            accounting
                .as_ref()
                .and_then(|value| value.context_input_tokens)
                .map(|value| value as i64),
            accounting
                .as_ref()
                .and_then(|value| value.billable_input_tokens)
                .map(|value| value as i64),
            accounting
                .as_ref()
                .and_then(|value| value.billable_output_tokens)
                .map(|value| value as i64),
            accounting
                .as_ref()
                .and_then(|value| value.reasoning_tokens)
                .map(|value| value as i64),
            accounting
                .as_ref()
                .and_then(|value| value.cache_read_tokens)
                .map(|value| value as i64),
            accounting
                .as_ref()
                .and_then(|value| value.cache_write_tokens)
                .map(|value| value as i64),
            accounting
                .as_ref()
                .and_then(|value| value.reported_total_tokens)
                .map(|value| value as i64),
            accounting
                .as_ref()
                .and_then(|value| value.estimated_cost_nanodollars),
            accounting
                .as_ref()
                .and_then(|value| value.pricing_source.clone()),
            accounting
                .as_ref()
                .and_then(|value| value.pricing_tier.clone()),
            accounting
                .as_ref()
                .and_then(|value| value.cost_status.map(|status| status.as_str().to_string())),
            accounting
                .as_ref()
                .and_then(|value| value.pricing_missing_reason.clone()),
            accounting
                .as_ref()
                .and_then(|value| value.pricing_version.clone()),
        );
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut tx = self.begin_sqlx_write().await?;
            let seq: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(session_seq), 0) + 1 FROM messages WHERE session_id = ?1",
            )
            .bind(session_id)
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO messages (
                    session_id, session_seq, role, timestamp_ms, message_json,
                    content_text, tool_call_id, tool_name, tool_calls_json,
                    finish_reason, outcome, model, provider, usage_json, metadata_json,
                    context_input_tokens, billable_input_tokens, billable_output_tokens,
                    reasoning_tokens, cache_read_tokens, cache_write_tokens,
                    reported_total_tokens, estimated_cost_nanodollars,
                    pricing_source, pricing_tier, cost_status,
                    pricing_missing_reason, pricing_version
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                    ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28
                )
                "#,
            )
            .bind(session_id)
            .bind(seq)
            .bind(&fields.role)
            .bind(fields.timestamp_ms)
            .bind(&message_json)
            .bind(&fields.content_text)
            .bind(&fields.tool_call_id)
            .bind(&fields.tool_name)
            .bind(&fields.tool_calls_json)
            .bind(&fields.finish_reason)
            .bind(&fields.outcome)
            .bind(&fields.model)
            .bind(&fields.provider)
            .bind(&usage_json)
            .bind(&metadata_json)
            .bind(accounting_values.0)
            .bind(accounting_values.1)
            .bind(accounting_values.2)
            .bind(accounting_values.3)
            .bind(accounting_values.4)
            .bind(accounting_values.5)
            .bind(accounting_values.6)
            .bind(accounting_values.7)
            .bind(&accounting_values.8)
            .bind(&accounting_values.9)
            .bind(&accounting_values.10)
            .bind(&accounting_values.11)
            .bind(&accounting_values.12)
            .execute(&mut *tx)
            .await?;
            insert_context_evidence_rows(&mut tx, session_id, seq, now, &context_evidence).await?;
            sqlx::query(
                r#"
                UPDATE sessions
                SET updated_at_ms = ?1,
                    ended_at_ms = NULL,
                    end_reason = NULL,
                    archived_at_ms = NULL,
                    message_count = message_count + 1,
                    tool_call_count = tool_call_count + ?2
                WHERE id = ?3
                "#,
            )
            .bind(now)
            .bind(fields.tool_call_count)
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(seq)
        }
        .await;
        operation.finish(&result);
        result
    }
}

fn decode_history_replay_row(row: sqlx::sqlite::SqliteRow) -> Result<StoredHistoryReplayItem> {
    let session_seq = row.try_get(0)?;
    let mut invalid_fields = Vec::new();

    let message = match row
        .try_get::<String, _>(1)
        .map_err(Error::from)
        .and_then(|value| serde_json::from_str::<Message>(&value).map_err(Into::into))
    {
        Ok(message) => Some(message),
        Err(_) => {
            invalid_fields.push(StoredHistoryReplayDecodeField::Message);
            None
        }
    };
    let usage = match row
        .try_get::<Option<String>, _>(2)
        .map_err(Error::from)
        .and_then(parse_optional_json)
    {
        Ok(usage) => Some(usage),
        Err(_) => {
            invalid_fields.push(StoredHistoryReplayDecodeField::Usage);
            None
        }
    };
    let metadata = match row
        .try_get::<Option<String>, _>(3)
        .map_err(Error::from)
        .and_then(parse_optional_json)
    {
        Ok(metadata) => Some(metadata),
        Err(_) => {
            invalid_fields.push(StoredHistoryReplayDecodeField::Metadata);
            None
        }
    };
    let accounting = match accounting_json_from_row(&row, 4) {
        Ok(accounting) => Some(accounting),
        Err(_) => {
            invalid_fields.push(StoredHistoryReplayDecodeField::Accounting);
            None
        }
    };

    if let (Some(message), Some(usage), Some(metadata), Some(accounting)) =
        (message, usage, metadata, accounting)
    {
        return Ok(StoredHistoryReplayItem::Available(Box::new(
            TuiMessageSummary {
                session_seq,
                message: sanitize_message_for_tui_history(&message),
                usage,
                metadata,
                accounting,
            },
        )));
    }
    Ok(StoredHistoryReplayItem::Unavailable {
        session_seq,
        invalid_fields,
    })
}

pub(crate) fn accounting_json_from_row(
    row: &sqlx::sqlite::SqliteRow,
    offset: usize,
) -> Result<Option<Value>> {
    let accounting = MessageAccounting {
        context_input_tokens: row
            .try_get::<Option<i64>, _>(offset)?
            .map(|value| value as u64),
        billable_input_tokens: row
            .try_get::<Option<i64>, _>(offset + 1)?
            .map(|value| value as u64),
        billable_output_tokens: row
            .try_get::<Option<i64>, _>(offset + 2)?
            .map(|value| value as u64),
        reasoning_tokens: row
            .try_get::<Option<i64>, _>(offset + 3)?
            .map(|value| value as u64),
        cache_read_tokens: row
            .try_get::<Option<i64>, _>(offset + 4)?
            .map(|value| value as u64),
        cache_write_tokens: row
            .try_get::<Option<i64>, _>(offset + 5)?
            .map(|value| value as u64),
        reported_total_tokens: row
            .try_get::<Option<i64>, _>(offset + 6)?
            .map(|value| value as u64),
        estimated_cost_nanodollars: row.try_get(offset + 7)?,
        pricing_source: row.try_get(offset + 8)?,
        pricing_tier: row.try_get(offset + 9)?,
        cost_status: row
            .try_get::<Option<String>, _>(offset + 10)?
            .and_then(|value| CostStatus::parse(&value)),
        pricing_missing_reason: row.try_get(offset + 11)?,
        pricing_version: row.try_get(offset + 12)?,
    };
    let value = accounting.public_json();
    Ok((value.as_object().is_some_and(|object| !object.is_empty())).then_some(value))
}
