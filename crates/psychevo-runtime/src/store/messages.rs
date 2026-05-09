impl SqliteStore {
    pub fn resume_session(&self, session_id: &str) -> Result<()> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET updated_at_ms = ?1, ended_at_ms = NULL, end_reason = NULL, archived_at_ms = NULL WHERE id = ?2",
                params![now, session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn load_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
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
        let conn = self.conn.lock().expect("sqlite lock poisoned");
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

    pub fn load_tui_message_summaries(&self, session_id: &str) -> Result<Vec<TuiMessageSummary>> {
        let boundary = self
            .session_revert_state(session_id)?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT message_json, usage_json, metadata_json
            FROM messages
            WHERE session_id = ?1 AND session_seq < ?2
            ORDER BY session_seq ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id, boundary], |row| {
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
            messages.push(TuiMessageSummary {
                message: sanitize_message_for_tui_history(&message),
                usage,
                metadata,
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
        let metadata = snapshot.map(|snapshot| {
            json!({
                MESSAGE_UNDO_METADATA_KEY: {
                    MESSAGE_PRE_SNAPSHOT_KEY: snapshot
                }
            })
        });
        self.append_message_with_metrics(session_id, message, None, metadata)
    }

    pub fn append_message_with_metrics(
        &self,
        session_id: &str,
        message: &Message,
        usage: Option<Value>,
        metadata: Option<Value>,
    ) -> Result<()> {
        let fields = message_fields(message)?;
        let message_json = serde_json::to_string(message)?;
        let usage_json = optional_json_string(&usage)?;
        let metadata_json = optional_json_string(&metadata)?;
        let now = now_ms();
        self.write_retry(|conn| {
            let seq = next_session_seq(conn, session_id)?;
            conn.execute(
                r#"
                INSERT INTO messages (
                    session_id, session_seq, role, timestamp_ms, message_json,
                    content_text, tool_call_id, tool_name, tool_calls_json,
                    finish_reason, outcome, model, provider, usage_json, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
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
                    metadata_json
                ],
            )?;
            conn.execute(
                r#"
                UPDATE sessions
                SET updated_at_ms = ?1,
                    message_count = message_count + 1,
                    tool_call_count = tool_call_count + ?2
                WHERE id = ?3
                "#,
                params![now, fields.tool_call_count, session_id],
            )?;
            Ok(())
        })
    }

}
