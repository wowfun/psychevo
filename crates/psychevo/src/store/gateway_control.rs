use psychevo_agent_core::now_ms;
use sqlx::Row;

use crate::error::{Error, Result};

use super::{
    GatewayControlCommandInput, GatewayControlCommandKind, GatewayControlCommandRecord,
    GatewayControlCommandStatus, GatewayTurnTerminalInput, GatewayTurnTerminalRecord, StateRuntime,
    invalid_persisted_domain_value,
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
            .bind(input.command_kind.as_str())
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

    pub async fn gateway_control_command(
        &self,
        id: i64,
    ) -> Result<Option<GatewayControlCommandRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
                SELECT id, activity_id, owner_id, command_kind, status, payload_json,
                       created_at_ms, updated_at_ms, error
                FROM gateway_control_commands
                WHERE id = ?1
                "#,
            )
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| gateway_control_command_from_row(&row))
            .transpose()
        })
        .await
    }

    pub async fn claim_pending_gateway_control_commands(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<GatewayControlCommandRecord>> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
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
            .fetch_all(&mut *tx)
            .await?;
            let mut commands = Vec::with_capacity(rows.len());
            for row in rows {
                let mut command = gateway_control_command_from_row(&row)?;
                let changed = sqlx::query(
                    r#"
                    UPDATE gateway_control_commands
                    SET status = 'applying', updated_at_ms = ?2, error = NULL
                    WHERE id = ?1 AND status = 'pending'
                    "#,
                )
                .bind(command.id)
                .bind(now_ms())
                .execute(&mut *tx)
                .await?
                .rows_affected();
                if changed == 1 {
                    command.status = GatewayControlCommandStatus::Applying;
                    commands.push(command);
                }
            }
            tx.commit().await?;
            Ok(commands)
        })
        .await
    }

    pub async fn recover_indeterminate_gateway_control_commands(
        &self,
        now_ms: i64,
    ) -> Result<Vec<GatewayControlCommandRecord>> {
        const INDETERMINATE_ERROR: &str =
            "control side effect outcome is indeterminate after owner loss";
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let rows = sqlx::query(
                r#"
                SELECT c.id, c.activity_id, c.owner_id, c.command_kind, c.status,
                       c.payload_json, c.created_at_ms, c.updated_at_ms, c.error
                FROM gateway_control_commands c
                WHERE c.status = 'applying'
                  AND NOT EXISTS (
                      SELECT 1
                      FROM gateway_activities a
                      WHERE a.activity_id = c.activity_id
                        AND a.owner_id = c.owner_id
                        AND a.status IN ('running', 'queued')
                        AND a.lease_expires_at_ms >= ?1
                  )
                ORDER BY c.id ASC
                LIMIT 100
                "#,
            )
            .bind(now_ms)
            .fetch_all(&mut *tx)
            .await?;
            let mut recovered = Vec::with_capacity(rows.len());
            for row in rows {
                let mut command = gateway_control_command_from_row(&row)?;
                let changed = sqlx::query(
                    r#"
                    UPDATE gateway_control_commands
                    SET status = 'outcome_indeterminate', updated_at_ms = ?2, error = ?3
                    WHERE id = ?1 AND status = 'applying'
                    "#,
                )
                .bind(command.id)
                .bind(now_ms)
                .bind(INDETERMINATE_ERROR)
                .execute(&mut *tx)
                .await?
                .rows_affected();
                if changed == 1 {
                    command.status = GatewayControlCommandStatus::OutcomeIndeterminate;
                    command.updated_at_ms = now_ms;
                    command.error = Some(INDETERMINATE_ERROR.to_string());
                    recovered.push(command);
                }
            }
            tx.commit().await?;
            Ok(recovered)
        })
        .await
    }

    pub async fn mark_gateway_control_command_applied(&self, id: i64) -> Result<bool> {
        self.update_gateway_control_command_status(id, GatewayControlCommandStatus::Applied, None)
            .await
    }

    pub async fn mark_gateway_control_command_failed(&self, id: i64, error: &str) -> Result<bool> {
        self.update_gateway_control_command_status(
            id,
            GatewayControlCommandStatus::Failed,
            Some(error),
        )
        .await
    }

    pub async fn retry_gateway_control_command(&self, id: i64) -> Result<bool> {
        self.gateway_control_write(
            sqlx::query(
                r#"
                UPDATE gateway_control_commands
                SET status = 'pending', updated_at_ms = ?2, error = NULL
                WHERE id = ?1 AND status = 'applying'
                "#,
            )
            .bind(id)
            .bind(now_ms()),
        )
        .await
    }

    async fn update_gateway_control_command_status(
        &self,
        id: i64,
        status: GatewayControlCommandStatus,
        error: Option<&str>,
    ) -> Result<bool> {
        self.gateway_control_write(
            sqlx::query(
                r#"
                UPDATE gateway_control_commands
                SET status = ?2, updated_at_ms = ?3, error = ?4
                WHERE id = ?1 AND status = 'applying'
                "#,
            )
            .bind(id)
            .bind(status.as_str())
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
                    started_at_ms, completed_at_ms, boundary_session_seq, metadata_json
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                    COALESCE(
                        ?8,
                        (SELECT COALESCE(MAX(session_seq), 0)
                         FROM messages WHERE session_id = ?2)
                    ),
                    ?9
                )
                ON CONFLICT(turn_id) DO UPDATE SET
                    thread_id = excluded.thread_id,
                    status = excluded.status,
                    outcome = excluded.outcome,
                    error_message = excluded.error_message,
                    started_at_ms = COALESCE(excluded.started_at_ms, gateway_turn_terminals.started_at_ms),
                    completed_at_ms = excluded.completed_at_ms,
                    boundary_session_seq = excluded.boundary_session_seq,
                    metadata_json = excluded.metadata_json
                "#,
            )
            .bind(input.turn_id)
            .bind(input.thread_id)
            .bind(input.status.as_str())
            .bind(input.outcome.map(|outcome| outcome.as_str()))
            .bind(input.error_message)
            .bind(input.started_at_ms)
            .bind(input.completed_at_ms)
            .bind(input.boundary_session_seq)
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

    pub async fn list_valid_gateway_turn_terminals_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<GatewayTurnTerminalRecord>> {
        let revert_boundary = self
            .session_revert_state(thread_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let sql = gateway_turn_terminal_select_sql(
            r#"WHERE thread_id = ?1
               AND status IN ('failed', 'interrupted')
               AND boundary_session_seq < ?2
               ORDER BY boundary_session_seq ASC, completed_at_ms ASC, turn_id ASC"#,
        );
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(thread_id)
                .bind(revert_boundary)
                .fetch_all(&mut *conn)
                .await?;
            rows.into_iter()
                .map(|row| gateway_turn_terminal_from_row(&row))
                .collect()
        })
        .await
    }

    pub(crate) async fn gateway_turn_terminal_exists_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<bool> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            Ok(sqlx::query_scalar::<_, i64>(
                r#"
                SELECT 1
                FROM gateway_turn_terminals
                WHERE thread_id = ?1
                LIMIT 1
                "#,
            )
            .bind(thread_id)
            .fetch_optional(&mut *conn)
            .await?
            .is_some())
        })
        .await
    }

    pub async fn list_valid_gateway_turn_terminals_for_thread_window(
        &self,
        thread_id: &str,
        lower_session_seq: i64,
        before_session_seq: Option<i64>,
        before_structural_entry: Option<(i64, &str)>,
        limit: usize,
    ) -> Result<Vec<GatewayTurnTerminalRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let revert_boundary = self
            .session_revert_state(thread_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let sql = r#"
            SELECT turn_id, thread_id, status, outcome, error_message,
                   started_at_ms, completed_at_ms, boundary_session_seq, metadata_json
            FROM gateway_turn_terminals
            WHERE thread_id = ?1
              AND status IN ('failed', 'interrupted')
              AND boundary_session_seq >= ?2
              AND boundary_session_seq < ?8
              AND (
                  ?3 IS NULL
                  OR boundary_session_seq < ?3
                  OR (
                      ?4 = 1
                      AND boundary_session_seq = ?3
                      AND (
                          completed_at_ms < ?5
                          OR (
                              completed_at_ms = ?5
                              AND ('turn:' || turn_id || ':terminal') < ?6
                          )
                      )
                  )
              )
            ORDER BY boundary_session_seq DESC, completed_at_ms DESC, turn_id DESC
            LIMIT ?7
            "#;
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let (before_created_at_ms, before_entry_id) =
                before_structural_entry.unwrap_or((0, ""));
            let rows = sqlx::query(sql)
                .bind(thread_id)
                .bind(lower_session_seq)
                .bind(before_session_seq)
                .bind(if before_structural_entry.is_some() {
                    1_i64
                } else {
                    0_i64
                })
                .bind(before_created_at_ms)
                .bind(before_entry_id)
                .bind(i64::try_from(limit).unwrap_or(i64::MAX))
                .bind(revert_boundary)
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
    let command_kind: String = row.try_get(3)?;
    let status: String = row.try_get(4)?;
    Ok(GatewayControlCommandRecord {
        id: row.try_get(0)?,
        activity_id: row.try_get(1)?,
        owner_id: row.try_get(2)?,
        command_kind: GatewayControlCommandKind::parse(&command_kind).ok_or_else(|| {
            invalid_persisted_domain_value(
                "gateway_control_commands",
                "command_kind",
                &command_kind,
            )
        })?,
        status: GatewayControlCommandStatus::parse(&status).ok_or_else(|| {
            invalid_persisted_domain_value("gateway_control_commands", "status", &status)
        })?,
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
               started_at_ms, completed_at_ms, boundary_session_seq, metadata_json
        FROM gateway_turn_terminals
        {where_clause}
        "#
    )
}

fn gateway_turn_terminal_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GatewayTurnTerminalRecord> {
    let metadata_json: Option<String> = row.try_get(8)?;
    let status: String = row.try_get(2)?;
    let outcome: Option<String> = row.try_get(3)?;
    Ok(GatewayTurnTerminalRecord {
        turn_id: row.try_get(0)?,
        thread_id: row.try_get(1)?,
        status: crate::application::FrameworkTurnTerminalStatus::parse_persisted(&status)
            .ok_or_else(|| {
                invalid_persisted_domain_value("gateway_turn_terminals", "status", &status)
            })?,
        outcome: outcome
            .as_deref()
            .map(|value| {
                crate::application::FrameworkTurnTerminalOutcome::parse_persisted(value).ok_or_else(
                    || invalid_persisted_domain_value("gateway_turn_terminals", "outcome", value),
                )
            })
            .transpose()?,
        error_message: row.try_get(4)?,
        started_at_ms: row.try_get(5)?,
        completed_at_ms: row.try_get(6)?,
        boundary_session_seq: row.try_get(7)?,
        metadata: metadata_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
    })
}
