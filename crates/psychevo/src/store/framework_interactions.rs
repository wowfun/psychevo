use psychevo_agent_core::now_ms;
use serde_json::{Value, json};
use sqlx::Row;

use crate::error::Result;

use super::{
    FrameworkInteractionRecord, GatewayTurnTerminalInput, StateRuntime,
    store_turn_delivery::scrub_gateway_activity_turn_input,
};

impl StateRuntime {
    #[cfg(test)]
    pub(crate) fn fail_next_framework_terminal_for_test(&self) {
        self.inner
            .fail_next_framework_terminal
            .store(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) async fn request_framework_interaction(
        &self,
        interaction_id: &str,
        thread_id: &str,
        turn_id: &str,
        kind: &str,
        payload: Value,
    ) -> Result<bool> {
        let requested_at_ms = now_ms();
        let payload_json = serde_json::to_string(&payload)?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = sqlx::query(
                r#"
                INSERT INTO framework_interactions (
                    interaction_id, thread_id, turn_id, kind, status,
                    payload_json, resolution_json, requested_at_ms, resolved_at_ms
                )
                SELECT ?1, ?2, ?3, ?4, 'pending', ?5, NULL, ?6, NULL
                WHERE NOT EXISTS (
                    SELECT 1 FROM gateway_turn_terminals WHERE turn_id = ?3
                )
                ON CONFLICT(turn_id, interaction_id) DO NOTHING
                "#,
            )
            .bind(interaction_id)
            .bind(thread_id)
            .bind(turn_id)
            .bind(kind)
            .bind(payload_json)
            .bind(requested_at_ms)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            tx.commit().await?;
            Ok(changed == 1)
        })
        .await
    }

    pub(crate) async fn resolve_framework_interaction(
        &self,
        interaction_id: &str,
        thread_id: &str,
        turn_id: &str,
        kind: &str,
        status: &str,
        resolution: Value,
    ) -> Result<bool> {
        let resolved_at_ms = now_ms();
        debug_assert!(matches!(status, "resolved" | "cancelled"));
        let resolution_json = serde_json::to_string(&resolution)?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = sqlx::query(
                r#"
                UPDATE framework_interactions
                SET status = ?5,
                    resolution_json = ?6,
                    resolved_at_ms = ?7
                WHERE interaction_id = ?1
                  AND thread_id = ?2
                  AND turn_id = ?3
                  AND kind = ?4
                  AND status = 'pending'
                "#,
            )
            .bind(interaction_id)
            .bind(thread_id)
            .bind(turn_id)
            .bind(kind)
            .bind(status)
            .bind(resolution_json)
            .bind(resolved_at_ms)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            tx.commit().await?;
            Ok(changed > 0)
        })
        .await
    }

    pub(crate) async fn pending_framework_interaction_kind(
        &self,
        interaction_id: &str,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<Option<String>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            Ok(sqlx::query_scalar(
                r#"
                SELECT kind
                FROM framework_interactions
                WHERE interaction_id = ?1
                  AND thread_id = ?2
                  AND turn_id = ?3
                  AND status = 'pending'
                "#,
            )
            .bind(interaction_id)
            .bind(thread_id)
            .bind(turn_id)
            .fetch_optional(&mut *conn)
            .await?)
        })
        .await
    }

    pub(crate) async fn finalize_framework_turn(
        &self,
        input: GatewayTurnTerminalInput<'_>,
        interaction_reason: &str,
    ) -> Result<()> {
        #[cfg(test)]
        if self
            .inner
            .fail_next_framework_terminal
            .swap(0, std::sync::atomic::Ordering::SeqCst)
            > 0
        {
            return Err(crate::Error::Message(
                "injected Framework terminal persistence failure".to_string(),
            ));
        }
        let metadata_json = input
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let resolution_json =
            serde_json::to_string(&json!({ "reason": interaction_reason }))?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
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
                    started_at_ms = COALESCE(
                        excluded.started_at_ms,
                        gateway_turn_terminals.started_at_ms
                    ),
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
            .bind(metadata_json)
            .execute(&mut *tx)
            .await?;
            let delivery_changed = sqlx::query(
                r#"
                UPDATE gateway_turn_deliveries
                SET status = 'terminal', input_json = NULL,
                    terminal_at_ms = COALESCE(terminal_at_ms, ?2),
                    updated_at_ms = ?2
                WHERE turn_id = ?1 AND status != 'unknown'
                "#,
            )
            .bind(input.turn_id)
            .bind(input.completed_at_ms)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if delivery_changed > 0 {
                scrub_gateway_activity_turn_input(
                    &mut tx,
                    input.turn_id,
                    input.completed_at_ms,
                )
                .await?;
            }
            sqlx::query(
                r#"
                UPDATE framework_interactions
                SET status = 'cancelled',
                    resolution_json = ?2,
                    resolved_at_ms = ?3
                WHERE turn_id = ?1 AND status = 'pending'
                "#,
            )
            .bind(input.turn_id)
            .bind(resolution_json)
            .bind(input.completed_at_ms)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(())
        })
        .await
    }

    pub(crate) async fn framework_interactions_for_thread(
        &self,
        thread_id: &str,
        pending_only: bool,
    ) -> Result<Vec<FrameworkInteractionRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
                SELECT interaction_id, thread_id, turn_id, kind, status,
                       payload_json, resolution_json, requested_at_ms, resolved_at_ms
                FROM framework_interactions
                WHERE thread_id = ?1 AND (?2 = 0 OR status = 'pending')
                ORDER BY requested_at_ms ASC, rowid ASC
                "#,
            )
            .bind(thread_id)
            .bind(i64::from(pending_only))
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter().map(interaction_from_row).collect()
        })
        .await
    }
}

fn interaction_from_row(row: sqlx::sqlite::SqliteRow) -> Result<FrameworkInteractionRecord> {
    let payload_json = row.try_get::<String, _>(5)?;
    let resolution_json = row.try_get::<Option<String>, _>(6)?;
    Ok(FrameworkInteractionRecord {
        interaction_id: row.try_get(0)?,
        thread_id: row.try_get(1)?,
        turn_id: row.try_get(2)?,
        kind: row.try_get(3)?,
        status: row.try_get(4)?,
        payload: serde_json::from_str(&payload_json)?,
        resolution: resolution_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
        requested_at_ms: row.try_get(7)?,
        resolved_at_ms: row.try_get(8)?,
    })
}
