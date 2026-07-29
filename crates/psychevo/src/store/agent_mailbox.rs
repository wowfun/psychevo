use psychevo_agent_core::now_ms;
use sqlx::Row;

use crate::error::{Error, Result};

use super::store_message_fields::optional_json_string;
use super::{AgentMailboxEventInput, AgentMailboxEventRecord, StateRuntime};

impl StateRuntime {
    pub async fn append_agent_mailbox_event(&self, input: AgentMailboxEventInput) -> Result<i64> {
        let now = now_ms();
        let payload_json = serde_json::to_string(&input.payload)?;
        let metadata_json = optional_json_string(&input.metadata)?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let id = sqlx::query(
                r#"
                INSERT INTO agent_mailbox_events (
                    parent_session_id, child_session_id, agent_id, task_name, agent_name,
                    created_at_ms, delivered_at_ms, delivered_prompt_session_seq,
                    delivered_after_session_seq, delivered_tool_call_id, content_text,
                    payload_json, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, NULL, ?7, ?8, ?9)
                "#,
            )
            .bind(input.parent_session_id)
            .bind(input.child_session_id)
            .bind(input.agent_id)
            .bind(input.task_name)
            .bind(input.agent_name)
            .bind(now)
            .bind(input.content_text)
            .bind(payload_json)
            .bind(metadata_json)
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
            tx.commit().await?;
            Ok(id)
        })
        .await
    }

    pub(crate) async fn commit_agent_terminal(
        &self,
        child_session_id: &str,
        mailbox: Option<AgentMailboxEventInput>,
    ) -> Result<()> {
        let mailbox = mailbox.map(|input| (input, psychevo_agent_core::now_ms()));
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let updated = sqlx::query(
                "UPDATE agent_edges SET status = 'closed', updated_at_ms = ?1 WHERE child_session_id = ?2",
            )
            .bind(psychevo_agent_core::now_ms())
            .bind(child_session_id)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(Error::Message(format!(
                    "cannot commit Agent terminal: durable edge {child_session_id} was not found"
                )));
            }
            if let Some((input, created_at_ms)) = mailbox {
                let payload_json = serde_json::to_string(&input.payload)?;
                let metadata_json = optional_json_string(&input.metadata)?;
                sqlx::query(
                    r#"
                    INSERT INTO agent_mailbox_events (
                        parent_session_id, child_session_id, agent_id, task_name, agent_name,
                        created_at_ms, delivered_at_ms, delivered_prompt_session_seq,
                        delivered_after_session_seq, delivered_tool_call_id, content_text,
                        payload_json, metadata_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, NULL, ?7, ?8, ?9)
                    "#,
                )
                .bind(input.parent_session_id)
                .bind(input.child_session_id)
                .bind(input.agent_id)
                .bind(input.task_name)
                .bind(input.agent_name)
                .bind(created_at_ms)
                .bind(input.content_text)
                .bind(payload_json)
                .bind(metadata_json)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            Ok(())
        })
        .await
    }

    pub async fn next_message_seq(&self, session_id: &str) -> Result<i64> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let count =
                sqlx::query_scalar::<_, i64>("SELECT message_count FROM sessions WHERE id = ?1")
                    .bind(session_id)
                    .fetch_one(&mut *conn)
                    .await?;
            Ok(count + 1)
        })
        .await
    }

    pub async fn has_pending_agent_mailbox_events(&self, parent_session_id: &str) -> Result<bool> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let found = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT 1
                FROM agent_mailbox_events
                WHERE parent_session_id = ?1 AND delivered_at_ms IS NULL
                LIMIT 1
                "#,
            )
            .bind(parent_session_id)
            .fetch_optional(&mut *conn)
            .await?;
            Ok(found.is_some())
        })
        .await
    }

    pub async fn deliver_pending_agent_mailbox_events_for_prompt(
        &self,
        parent_session_id: &str,
        prompt_session_seq: i64,
    ) -> Result<Vec<AgentMailboxEventRecord>> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let now = now_ms();
            sqlx::query(
                r#"
                UPDATE agent_mailbox_events
                SET delivered_at_ms = ?1,
                    delivered_prompt_session_seq = ?2
                WHERE parent_session_id = ?3 AND delivered_at_ms IS NULL
                "#,
            )
            .bind(now)
            .bind(prompt_session_seq)
            .bind(parent_session_id)
            .execute(&mut *tx)
            .await?;
            let rows = sqlx::query(AGENT_MAILBOX_EVENTS_QUERY)
                .bind(parent_session_id)
                .fetch_all(&mut *tx)
                .await?;
            tx.commit().await?;
            rows.into_iter()
                .map(|row| agent_mailbox_event_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn deliver_pending_agent_mailbox_events_for_tool(
        &self,
        parent_session_id: &str,
        tool_call_id: &str,
        delivered_after_session_seq: i64,
    ) -> Result<Vec<AgentMailboxEventRecord>> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let now = now_ms();
            sqlx::query(
                r#"
                UPDATE agent_mailbox_events
                SET delivered_at_ms = ?1,
                    delivered_after_session_seq = ?2,
                    delivered_tool_call_id = ?3
                WHERE parent_session_id = ?4 AND delivered_at_ms IS NULL
                "#,
            )
            .bind(now)
            .bind(delivered_after_session_seq)
            .bind(tool_call_id)
            .bind(parent_session_id)
            .execute(&mut *tx)
            .await?;
            let rows = sqlx::query(AGENT_MAILBOX_EVENTS_QUERY)
                .bind(parent_session_id)
                .fetch_all(&mut *tx)
                .await?;
            tx.commit().await?;
            rows.into_iter()
                .map(|row| agent_mailbox_event_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn load_agent_mailbox_events(
        &self,
        parent_session_id: &str,
    ) -> Result<Vec<AgentMailboxEventRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(AGENT_MAILBOX_EVENTS_QUERY)
                .bind(parent_session_id)
                .fetch_all(&mut *conn)
                .await?;
            rows.into_iter()
                .map(|row| agent_mailbox_event_from_row(&row))
                .collect()
        })
        .await
    }
}

const AGENT_MAILBOX_EVENTS_QUERY: &str = r#"
        SELECT id, parent_session_id, child_session_id, agent_id, task_name,
               agent_name, created_at_ms, delivered_at_ms,
               delivered_prompt_session_seq, delivered_after_session_seq,
               delivered_tool_call_id, content_text, payload_json, metadata_json
        FROM agent_mailbox_events
        WHERE parent_session_id = ?1
        ORDER BY created_at_ms ASC, id ASC
        "#;

pub(crate) fn agent_mailbox_event_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<AgentMailboxEventRecord> {
    let payload_json: String = row.try_get(12)?;
    let metadata_json: Option<String> = row.try_get(13)?;
    Ok(AgentMailboxEventRecord {
        id: row.try_get(0)?,
        parent_session_id: row.try_get(1)?,
        child_session_id: row.try_get(2)?,
        agent_id: row.try_get(3)?,
        task_name: row.try_get(4)?,
        agent_name: row.try_get(5)?,
        created_at_ms: row.try_get(6)?,
        delivered_at_ms: row.try_get(7)?,
        delivered_prompt_session_seq: row.try_get(8)?,
        delivered_after_session_seq: row.try_get(9)?,
        delivered_tool_call_id: row.try_get(10)?,
        content_text: row.try_get(11)?,
        payload: serde_json::from_str(&payload_json)?,
        metadata: metadata_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
    })
}
