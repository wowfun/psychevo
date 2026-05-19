impl SqliteStore {
    pub fn append_agent_mailbox_event(&self, input: AgentMailboxEventInput) -> Result<i64> {
        let now = now_ms();
        let payload_json = serde_json::to_string(&input.payload)?;
        let metadata_json = optional_json_string(&input.metadata)?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO agent_mailbox_events (
                    parent_session_id, child_session_id, agent_id, task_name, agent_name,
                    created_at_ms, delivered_at_ms, delivered_prompt_session_seq,
                    delivered_after_session_seq, delivered_tool_call_id, content_text,
                    payload_json, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, NULL, ?7, ?8, ?9)
                "#,
                params![
                    input.parent_session_id,
                    input.child_session_id,
                    input.agent_id,
                    input.task_name,
                    input.agent_name,
                    now,
                    input.content_text,
                    payload_json,
                    metadata_json
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    pub fn next_message_seq(&self, session_id: &str) -> Result<i64> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let count: i64 = conn.query_row(
            "SELECT message_count FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count + 1)
    }

    pub fn has_pending_agent_mailbox_events(&self, parent_session_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let found = conn
            .query_row(
                r#"
                SELECT 1
                FROM agent_mailbox_events
                WHERE parent_session_id = ?1 AND delivered_at_ms IS NULL
                LIMIT 1
                "#,
                params![parent_session_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    pub fn deliver_pending_agent_mailbox_events_for_prompt(
        &self,
        parent_session_id: &str,
        prompt_session_seq: i64,
    ) -> Result<Vec<AgentMailboxEventRecord>> {
        self.write_retry(|conn| {
            let now = now_ms();
            conn.execute(
                r#"
                UPDATE agent_mailbox_events
                SET delivered_at_ms = ?1,
                    delivered_prompt_session_seq = ?2
                WHERE parent_session_id = ?3 AND delivered_at_ms IS NULL
                "#,
                params![now, prompt_session_seq, parent_session_id],
            )?;
            query_agent_mailbox_events_sql(conn, parent_session_id)
        })
    }

    pub fn deliver_pending_agent_mailbox_events_for_tool(
        &self,
        parent_session_id: &str,
        tool_call_id: &str,
        delivered_after_session_seq: i64,
    ) -> Result<Vec<AgentMailboxEventRecord>> {
        self.write_retry(|conn| {
            let now = now_ms();
            conn.execute(
                r#"
                UPDATE agent_mailbox_events
                SET delivered_at_ms = ?1,
                    delivered_after_session_seq = ?2,
                    delivered_tool_call_id = ?3
                WHERE parent_session_id = ?4 AND delivered_at_ms IS NULL
                "#,
                params![
                    now,
                    delivered_after_session_seq,
                    tool_call_id,
                    parent_session_id
                ],
            )?;
            query_agent_mailbox_events_sql(conn, parent_session_id)
        })
    }

    pub fn load_agent_mailbox_events(
        &self,
        parent_session_id: &str,
    ) -> Result<Vec<AgentMailboxEventRecord>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        query_agent_mailbox_events_sql(&conn, parent_session_id).map_err(Into::into)
    }
}

fn query_agent_mailbox_events_sql(
    conn: &Connection,
    parent_session_id: &str,
) -> rusqlite::Result<Vec<AgentMailboxEventRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, parent_session_id, child_session_id, agent_id, task_name,
               agent_name, created_at_ms, delivered_at_ms,
               delivered_prompt_session_seq, delivered_after_session_seq,
               delivered_tool_call_id, content_text, payload_json, metadata_json
        FROM agent_mailbox_events
        WHERE parent_session_id = ?1
        ORDER BY created_at_ms ASC, id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![parent_session_id], agent_mailbox_event_from_row)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

fn agent_mailbox_event_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentMailboxEventRecord> {
    let payload_json: String = row.get(12)?;
    let metadata_json: Option<String> = row.get(13)?;
    Ok(AgentMailboxEventRecord {
        id: row.get(0)?,
        parent_session_id: row.get(1)?,
        child_session_id: row.get(2)?,
        agent_id: row.get(3)?,
        task_name: row.get(4)?,
        agent_name: row.get(5)?,
        created_at_ms: row.get(6)?,
        delivered_at_ms: row.get(7)?,
        delivered_prompt_session_seq: row.get(8)?,
        delivered_after_session_seq: row.get(9)?,
        delivered_tool_call_id: row.get(10)?,
        content_text: row.get(11)?,
        payload: serde_json::from_str(&payload_json).map_err(json_to_sql)?,
        metadata: metadata_json
            .map(|value| serde_json::from_str(&value).map_err(json_to_sql))
            .transpose()?,
    })
}
