use psychevo_agent_core::now_ms;
use sqlx::Row;

use crate::error::{Error, Result};

use super::{
    GatewayControlCommandInput, GatewayControlCommandRecord, GatewayTurnTerminalInput,
    GatewayTurnTerminalRecord, StateRuntime,
};

impl StateRuntime {
    pub async fn enqueue_gateway_control_command(
        &self,
        input: GatewayControlCommandInput<'_>,
    ) -> Result<i64> {
        let payload_json = serde_json::to_string(&input.payload)?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let id = sqlx::query(
                r#"
                INSERT INTO gateway_control_commands (
                    activity_id, owner_id, command_kind, status, payload_json,
                    created_at_ms, updated_at_ms, error
                ) VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?5, NULL)
                "#,
            )
            .bind(input.activity_id)
            .bind(input.owner_id)
            .bind(input.command_kind)
            .bind(payload_json)
            .bind(now_ms())
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
            tx.commit().await?;
            Ok(id)
        })
        .await
    }

    pub async fn pending_gateway_control_commands(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<GatewayControlCommandRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
                SELECT id, activity_id, owner_id, command_kind, status, payload_json,
                       created_at_ms, updated_at_ms, error
                FROM gateway_control_commands
                WHERE owner_id = ?1 AND status = 'pending'
                ORDER BY id ASC
                LIMIT ?2
                "#,
            )
            .bind(owner_id)
            .bind(limit.clamp(1, 100) as i64)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| gateway_control_command_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn mark_gateway_control_command_applied(&self, id: i64) -> Result<bool> {
        self.update_gateway_control_command_status(id, "applied", None)
            .await
    }

    pub async fn mark_gateway_control_command_failed(&self, id: i64, error: &str) -> Result<bool> {
        self.update_gateway_control_command_status(id, "failed", Some(error))
            .await
    }

    async fn update_gateway_control_command_status(
        &self,
        id: i64,
        status: &str,
        error: Option<&str>,
    ) -> Result<bool> {
        self.gateway_control_write(
            sqlx::query(
                r#"
                UPDATE gateway_control_commands
                SET status = ?2, updated_at_ms = ?3, error = ?4
                WHERE id = ?1 AND status = 'pending'
                "#,
            )
            .bind(id)
            .bind(status)
            .bind(now_ms())
            .bind(error),
        )
        .await
    }

    pub async fn upsert_gateway_turn_terminal(
        &self,
        input: GatewayTurnTerminalInput<'_>,
    ) -> Result<GatewayTurnTerminalRecord> {
        let metadata_json = input
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        self.gateway_control_write(
            sqlx::query(
                r#"
                INSERT INTO gateway_turn_terminals (
                    turn_id, thread_id, status, outcome, error_message,
                    started_at_ms, completed_at_ms, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(turn_id) DO UPDATE SET
                    thread_id = excluded.thread_id,
                    status = excluded.status,
                    outcome = excluded.outcome,
                    error_message = excluded.error_message,
                    started_at_ms = COALESCE(excluded.started_at_ms, gateway_turn_terminals.started_at_ms),
                    completed_at_ms = excluded.completed_at_ms,
                    metadata_json = excluded.metadata_json
                "#,
            )
            .bind(input.turn_id)
            .bind(input.thread_id)
            .bind(input.status)
            .bind(input.outcome)
            .bind(input.error_message)
            .bind(input.started_at_ms)
            .bind(input.completed_at_ms)
            .bind(metadata_json),
        )
        .await?;
        self.gateway_turn_terminal(input.turn_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "gateway turn terminal not found after write: {}",
                    input.turn_id
                ))
            })
    }

    pub async fn gateway_turn_terminal(
        &self,
        turn_id: &str,
    ) -> Result<Option<GatewayTurnTerminalRecord>> {
        let sql = gateway_turn_terminal_select_sql("WHERE turn_id = ?1");
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(turn_id)
                .fetch_optional(&mut *conn)
                .await?
                .map(|row| gateway_turn_terminal_from_row(&row))
                .transpose()
        })
        .await
    }

    pub async fn list_gateway_turn_terminals_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<GatewayTurnTerminalRecord>> {
        let sql = gateway_turn_terminal_select_sql(
            "WHERE thread_id = ?1 ORDER BY completed_at_ms ASC, turn_id ASC",
        );
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(thread_id)
                .fetch_all(&mut *conn)
                .await?;
            rows.into_iter()
                .map(|row| gateway_turn_terminal_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn list_gateway_turn_terminals_for_thread_window(
        &self,
        thread_id: &str,
        lower_session_seq: i64,
        before_session_seq: Option<i64>,
        limit: usize,
    ) -> Result<Vec<GatewayTurnTerminalRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        const STRUCTURAL_BOUNDARY_SQL: &str = r#"
            COALESCE(
                json_extract(metadata_json, '$.lastCommittedSeq'),
                json_extract(metadata_json, '$.last_committed_seq'),
                json_extract(metadata_json, '$.firstCommittedSeq') - 1,
                json_extract(metadata_json, '$.first_committed_seq') - 1,
                9223372036854775807
            )
        "#;
        let upper_predicate = if before_session_seq.is_some() {
            format!("AND {STRUCTURAL_BOUNDARY_SQL} < ?3")
        } else {
            String::new()
        };
        let sql = format!(
            r#"
            SELECT turn_id, thread_id, status, outcome, error_message,
                   started_at_ms, completed_at_ms, metadata_json
            FROM gateway_turn_terminals
            WHERE thread_id = ?1
              AND {STRUCTURAL_BOUNDARY_SQL} >= ?2
              {upper_predicate}
            ORDER BY {STRUCTURAL_BOUNDARY_SQL} DESC, completed_at_ms DESC, turn_id DESC
            LIMIT ?4
            "#
        );
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(thread_id)
                .bind(lower_session_seq);
            query = if let Some(before_session_seq) = before_session_seq {
                query.bind(before_session_seq)
            } else {
                query.bind(Option::<i64>::None)
            };
            let rows = query
                .bind(i64::try_from(limit).unwrap_or(i64::MAX))
                .fetch_all(&mut *conn)
                .await?;
            let mut records = rows
                .into_iter()
                .map(|row| gateway_turn_terminal_from_row(&row))
                .collect::<Result<Vec<_>>>()?;
            records.reverse();
            Ok(records)
        })
        .await
    }

    async fn gateway_control_write<'q>(
        &self,
        query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    ) -> Result<bool> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = query.execute(&mut *tx).await?.rows_affected();
            tx.commit().await?;
            Ok(changed > 0)
        })
        .await
    }
}

fn gateway_control_command_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GatewayControlCommandRecord> {
    let payload_json: String = row.try_get(5)?;
    Ok(GatewayControlCommandRecord {
        id: row.try_get(0)?,
        activity_id: row.try_get(1)?,
        owner_id: row.try_get(2)?,
        command_kind: row.try_get(3)?,
        status: row.try_get(4)?,
        payload: serde_json::from_str(&payload_json)?,
        created_at_ms: row.try_get(6)?,
        updated_at_ms: row.try_get(7)?,
        error: row.try_get(8)?,
    })
}

fn gateway_turn_terminal_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        SELECT turn_id, thread_id, status, outcome, error_message,
               started_at_ms, completed_at_ms, metadata_json
        FROM gateway_turn_terminals
        {where_clause}
        "#
    )
}

fn gateway_turn_terminal_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GatewayTurnTerminalRecord> {
    let metadata_json: Option<String> = row.try_get(7)?;
    Ok(GatewayTurnTerminalRecord {
        turn_id: row.try_get(0)?,
        thread_id: row.try_get(1)?,
        status: row.try_get(2)?,
        outcome: row.try_get(3)?,
        error_message: row.try_get(4)?,
        started_at_ms: row.try_get(5)?,
        completed_at_ms: row.try_get(6)?,
        metadata: metadata_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
    })
}
