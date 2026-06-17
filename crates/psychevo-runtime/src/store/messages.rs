#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) struct AppendMessageParams<'a> {
    pub(crate) session_id: &'a str,
    pub(crate) message: &'a Message,
    pub(crate) usage: Option<Value>,
    pub(crate) metadata: Option<Value>,
    pub(crate) accounting: Option<MessageAccounting>,
    pub(crate) context_evidence: &'a [ContextEvidenceInput],
    pub(crate) content_text_override: Option<String>,
}

impl SqliteStore {
    pub fn resume_session(&self, session_id: &str) -> Result<()> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let exists = conn
            .query_row(
                "SELECT 1 FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn load_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT message_json FROM messages WHERE session_id = ?1 ORDER BY session_seq ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(serde_json::from_str(&row?)?);
        }
        Ok(messages)
    }

    pub fn load_sanitized_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        Ok(self
            .load_messages(session_id)?
            .iter()
            .map(sanitize_message_for_output)
            .collect())
    }

    pub fn load_sanitized_message_summaries(
        &self,
        session_id: &str,
    ) -> Result<Vec<SanitizedMessageSummary>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT message_json, usage_json, metadata_json
            FROM messages
            WHERE session_id = ?1
            ORDER BY session_seq ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut messages = Vec::new();
        for row in rows {
            let (message_json, usage_json, metadata_json) = row?;
            let message = serde_json::from_str::<Message>(&message_json)?;
            let usage = parse_optional_json(usage_json)?;
            let metadata = parse_optional_json(metadata_json)?;
            messages.push(SanitizedMessageSummary {
                message: sanitize_message_for_output(&message),
                usage,
                metadata,
            });
        }
        Ok(messages)
    }

    pub fn load_export_message_summaries(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionExportMessageSummary>> {
        let boundary = self
            .session_revert_state(session_id)?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT session_seq, message_json, usage_json, metadata_json
            FROM messages
            WHERE session_id = ?1 AND session_seq < ?2
            ORDER BY session_seq ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id, boundary], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut messages = Vec::new();
        for row in rows {
            let (session_seq, message_json, usage_json, metadata_json) = row?;
            messages.push(SessionExportMessageSummary {
                session_seq,
                message: serde_json::from_str(&message_json)?,
                usage: parse_optional_json(usage_json)?,
                metadata: parse_optional_json(metadata_json)?,
            });
        }
        Ok(messages)
    }

    pub fn load_tui_message_summaries(&self, session_id: &str) -> Result<Vec<TuiMessageSummary>> {
        let boundary = self
            .session_revert_state(session_id)?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
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
        )?;
        let rows = stmt.query_map(params![session_id, boundary], |row| {
            let accounting = accounting_json_from_row(row, 4)?;
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                accounting,
            ))
        })?;
        let mut messages = Vec::new();
        for row in rows {
            let (session_seq, message_json, usage_json, metadata_json, accounting) = row?;
            let message = serde_json::from_str::<Message>(&message_json)?;
            let usage = parse_optional_json(usage_json)?;
            let metadata = parse_optional_json(metadata_json)?;
            messages.push(TuiMessageSummary {
                session_seq,
                message: sanitize_message_for_tui_history(&message),
                usage,
                metadata,
                accounting,
            });
        }
        Ok(messages)
    }

    pub fn append_message(&self, session_id: &str, message: &Message) -> Result<()> {
        self.append_message_with_metrics(session_id, message, None, None)
    }

    pub fn append_message_with_undo_snapshot(
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
        .map(|_| ())
    }

    pub fn append_message_with_undo_snapshot_and_context_evidence(
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
    }

    pub fn append_message_with_undo_snapshot_metadata_and_context_evidence(
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
    }

    pub fn append_message_with_metrics(
        &self,
        session_id: &str,
        message: &Message,
        usage: Option<Value>,
        metadata: Option<Value>,
    ) -> Result<()> {
        self.append_message_with_metrics_and_accounting(session_id, message, usage, metadata, None)
    }

    pub fn append_message_with_metrics_and_accounting(
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
        .map(|_| ())
    }

    pub(crate) fn append_message_with_metrics_accounting_and_context_evidence(
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
        self.write_retry(|conn| {
            let seq = next_session_seq(conn, session_id)?;
            conn.execute(
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
                params![
                    session_id,
                    seq,
                    fields.role,
                    fields.timestamp_ms,
                    message_json,
                    fields.content_text,
                    fields.tool_call_id,
                    fields.tool_name,
                    fields.tool_calls_json,
                    fields.finish_reason,
                    fields.outcome,
                    fields.model,
                    fields.provider,
                    usage_json,
                    metadata_json,
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
                    accounting.as_ref().and_then(|value| value
                        .cost_status
                        .map(|status| status.as_str().to_string())),
                    accounting
                        .as_ref()
                        .and_then(|value| value.pricing_missing_reason.clone()),
                    accounting
                        .as_ref()
                        .and_then(|value| value.pricing_version.clone())
                ],
            )?;
            insert_context_evidence_rows(conn, session_id, seq, now, &context_evidence)?;
            conn.execute(
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
                params![now, fields.tool_call_count, session_id],
            )?;
            Ok(seq)
        })
    }
}

pub(crate) fn accounting_json_from_row(
    row: &rusqlite::Row<'_>,
    offset: usize,
) -> rusqlite::Result<Option<Value>> {
    let accounting = MessageAccounting {
        context_input_tokens: row.get::<_, Option<i64>>(offset)?.map(|value| value as u64),
        billable_input_tokens: row
            .get::<_, Option<i64>>(offset + 1)?
            .map(|value| value as u64),
        billable_output_tokens: row
            .get::<_, Option<i64>>(offset + 2)?
            .map(|value| value as u64),
        reasoning_tokens: row
            .get::<_, Option<i64>>(offset + 3)?
            .map(|value| value as u64),
        cache_read_tokens: row
            .get::<_, Option<i64>>(offset + 4)?
            .map(|value| value as u64),
        cache_write_tokens: row
            .get::<_, Option<i64>>(offset + 5)?
            .map(|value| value as u64),
        reported_total_tokens: row
            .get::<_, Option<i64>>(offset + 6)?
            .map(|value| value as u64),
        estimated_cost_nanodollars: row.get(offset + 7)?,
        pricing_source: row.get(offset + 8)?,
        pricing_tier: row.get(offset + 9)?,
        cost_status: row
            .get::<_, Option<String>>(offset + 10)?
            .and_then(|value| CostStatus::from_str(&value)),
        pricing_missing_reason: row.get(offset + 11)?,
        pricing_version: row.get(offset + 12)?,
    };
    let value = accounting.public_json();
    Ok((value.as_object().is_some_and(|object| !object.is_empty())).then_some(value))
}
