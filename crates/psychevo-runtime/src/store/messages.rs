use psychevo_agent_core::{Message, now_ms};
use serde_json::{Value, json};
use sqlx::Row;

use crate::error::{Error, Result};
use crate::messages::{sanitize_message_for_output, sanitize_message_for_tui_history};
use crate::types::{
    CostStatus, MessageAccounting, SanitizedMessageSummary, SessionExportMessageSummary,
    TuiMessageSummary,
};

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

    pub async fn load_sanitized_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        Ok(self
            .load_messages(session_id)
            .await?
            .iter()
            .map(sanitize_message_for_output)
            .collect())
    }

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
            self.finish_sqlx_write().await;
            Ok(seq)
        }
        .await;
        operation.finish(&result);
        result
    }
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
