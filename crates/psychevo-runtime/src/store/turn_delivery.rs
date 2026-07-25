use psychevo_agent_core::now_ms;
use serde_json::Value;
use sqlx::Row;

use crate::error::{Error, Result};

use super::{
    GatewayChannelOutboxInput, GatewayChannelOutboxRecord, GatewayTurnDeliveryInput,
    GatewayTurnDeliveryRecord, StateRuntime,
};

impl StateRuntime {
    pub async fn insert_gateway_turn_delivery(
        &self,
        input: GatewayTurnDeliveryInput<'_>,
    ) -> Result<GatewayTurnDeliveryRecord> {
        let now = now_ms();
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            sqlx::query(
                r#"
                INSERT INTO gateway_turn_deliveries (
                    turn_id, thread_id, runtime_ref, status, input_json,
                    input_hash, created_at_ms, updated_at_ms,
                    delivery_confirmed_at_ms, terminal_at_ms
                ) VALUES (?1, ?2, ?3, 'not_delivered', ?4, ?5, ?6, ?6, NULL, NULL)
                "#,
            )
            .bind(input.turn_id)
            .bind(input.thread_id)
            .bind(input.runtime_ref)
            .bind(input.input_json)
            .bind(input.input_hash)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(())
        })
        .await?;
        self.gateway_turn_delivery(input.turn_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "turn delivery not found after insert: {}",
                    input.turn_id
                ))
            })
    }

    pub async fn mark_gateway_turn_delivery_unknown(&self, turn_id: &str) -> Result<bool> {
        let now = now_ms();
        self.delivery_update(
            sqlx::query(
                r#"
                UPDATE gateway_turn_deliveries
                SET status = 'unknown', updated_at_ms = ?2
                WHERE turn_id = ?1 AND status = 'not_delivered'
                "#,
            )
            .bind(turn_id)
            .bind(now),
        )
        .await
    }

    pub async fn confirm_gateway_turn_delivery(&self, turn_id: &str) -> Result<bool> {
        let now = now_ms();
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = sqlx::query(
                r#"
                UPDATE gateway_turn_deliveries
                SET status = 'delivered', input_json = NULL,
                    delivery_confirmed_at_ms = COALESCE(delivery_confirmed_at_ms, ?2),
                    updated_at_ms = ?2
                WHERE turn_id = ?1 AND status IN ('not_delivered', 'unknown', 'delivered')
                "#,
            )
            .bind(turn_id)
            .bind(now)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed > 0 {
                scrub_gateway_activity_turn_input(&mut tx, turn_id, now).await?;
            }
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(changed > 0)
        })
        .await
    }

    pub async fn finish_gateway_turn_delivery(&self, turn_id: &str) -> Result<bool> {
        let now = now_ms();
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = sqlx::query(
                r#"
                UPDATE gateway_turn_deliveries
                SET status = 'terminal', input_json = NULL,
                    terminal_at_ms = COALESCE(terminal_at_ms, ?2),
                    updated_at_ms = ?2
                WHERE turn_id = ?1 AND status != 'unknown'
                "#,
            )
            .bind(turn_id)
            .bind(now)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed > 0 {
                scrub_gateway_activity_turn_input(&mut tx, turn_id, now).await?;
            }
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(changed > 0)
        })
        .await
    }

    pub async fn unknown_gateway_turn_deliveries_for_thread(
        &self,
        thread_id: &str,
        exclude_turn_id: &str,
    ) -> Result<Vec<GatewayTurnDeliveryRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT turn_id, thread_id, runtime_ref, status, input_json,
                   input_hash, created_at_ms, updated_at_ms,
                   delivery_confirmed_at_ms, terminal_at_ms
            FROM gateway_turn_deliveries
            WHERE thread_id = ?1 AND status = 'unknown' AND turn_id != ?2
            ORDER BY created_at_ms ASC, turn_id ASC
            LIMIT 2
            "#,
            )
            .bind(thread_id)
            .bind(exclude_turn_id)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| gateway_turn_delivery_from_row(&row))
                .collect()
        })
        .await
    }

    /// Atomically resolves delivery ambiguity after Agent-owned history proves
    /// that the prior turn reached a normal terminal. This is deliberately a
    /// distinct transition from `confirm_gateway_turn_delivery`: only replay
    /// reconciliation may move `unknown` directly to `terminal` and scrub the
    /// retained recovery input.
    pub async fn reconcile_unknown_gateway_turn_delivery(
        &self,
        turn_id: &str,
        thread_id: &str,
        metadata: Option<&Value>,
    ) -> Result<bool> {
        let now = now_ms();
        let metadata_json = metadata.map(serde_json::to_string).transpose()?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = sqlx::query(
                r#"
                UPDATE gateway_turn_deliveries
                SET status = 'terminal', input_json = NULL,
                    delivery_confirmed_at_ms = COALESCE(delivery_confirmed_at_ms, ?3),
                    terminal_at_ms = COALESCE(terminal_at_ms, ?3),
                    updated_at_ms = ?3
                WHERE turn_id = ?1 AND thread_id = ?2 AND status = 'unknown'
                "#,
            )
            .bind(turn_id)
            .bind(thread_id)
            .bind(now)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed == 0 {
                return Ok(false);
            }
            scrub_gateway_activity_turn_input(&mut tx, turn_id, now).await?;
            sqlx::query(
                r#"
                INSERT INTO gateway_turn_terminals (
                    turn_id, thread_id, status, outcome, error_message,
                    started_at_ms, completed_at_ms, metadata_json
                ) VALUES (?1, ?2, 'completed', 'normal', NULL, NULL, ?3, ?4)
                ON CONFLICT(turn_id) DO UPDATE SET
                    thread_id = excluded.thread_id,
                    status = 'completed',
                    outcome = 'normal',
                    error_message = NULL,
                    completed_at_ms = excluded.completed_at_ms,
                    metadata_json = excluded.metadata_json
                "#,
            )
            .bind(turn_id)
            .bind(thread_id)
            .bind(now)
            .bind(metadata_json)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(true)
        })
        .await
    }

    pub async fn gateway_turn_delivery(
        &self,
        turn_id: &str,
    ) -> Result<Option<GatewayTurnDeliveryRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
            SELECT turn_id, thread_id, runtime_ref, status, input_json,
                   input_hash, created_at_ms, updated_at_ms,
                   delivery_confirmed_at_ms, terminal_at_ms
            FROM gateway_turn_deliveries
            WHERE turn_id = ?1
            "#,
            )
            .bind(turn_id)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| gateway_turn_delivery_from_row(&row))
            .transpose()
        })
        .await
    }

    pub async fn upsert_gateway_channel_outbox(
        &self,
        input: GatewayChannelOutboxInput<'_>,
    ) -> Result<GatewayChannelOutboxRecord> {
        let now = now_ms();
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            sqlx::query(
                r#"
                INSERT INTO gateway_channel_outbox (
                    delivery_id, thread_id, turn_id, connection_id, source_key,
                    status, payload_text, payload_hash, created_at_ms,
                    updated_at_ms, acknowledged_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7, ?8, ?8, NULL)
                ON CONFLICT(delivery_id) DO UPDATE SET
                    payload_text = CASE
                        WHEN gateway_channel_outbox.status = 'acknowledged' THEN NULL
                        ELSE excluded.payload_text
                    END,
                    payload_hash = excluded.payload_hash,
                    updated_at_ms = excluded.updated_at_ms
                "#,
            )
            .bind(input.delivery_id)
            .bind(input.thread_id)
            .bind(input.turn_id)
            .bind(input.connection_id)
            .bind(input.source_key)
            .bind(input.payload_text)
            .bind(input.payload_hash)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(())
        })
        .await?;
        self.gateway_channel_outbox(input.delivery_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "channel outbox row not found after upsert: {}",
                    input.delivery_id
                ))
            })
    }

    pub async fn acknowledge_gateway_channel_outbox(&self, delivery_id: &str) -> Result<bool> {
        let now = now_ms();
        self.delivery_update(
            sqlx::query(
                r#"
                UPDATE gateway_channel_outbox
                SET status = 'acknowledged', payload_text = NULL,
                    acknowledged_at_ms = COALESCE(acknowledged_at_ms, ?2),
                    updated_at_ms = ?2
                WHERE delivery_id = ?1 AND status != 'acknowledged'
                "#,
            )
            .bind(delivery_id)
            .bind(now),
        )
        .await
    }

    pub async fn fail_gateway_channel_outbox(&self, delivery_id: &str) -> Result<bool> {
        let now = now_ms();
        self.delivery_update(
            sqlx::query(
                r#"
                UPDATE gateway_channel_outbox
                SET status = 'failed', updated_at_ms = ?2
                WHERE delivery_id = ?1 AND status = 'pending'
                "#,
            )
            .bind(delivery_id)
            .bind(now),
        )
        .await
    }

    pub async fn gateway_channel_outbox(
        &self,
        delivery_id: &str,
    ) -> Result<Option<GatewayChannelOutboxRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
            SELECT delivery_id, thread_id, turn_id, connection_id, source_key,
                   status, payload_text, payload_hash, created_at_ms,
                   updated_at_ms, acknowledged_at_ms
            FROM gateway_channel_outbox
            WHERE delivery_id = ?1
            "#,
            )
            .bind(delivery_id)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| gateway_channel_outbox_record(&row))
            .transpose()
        })
        .await
    }

    pub async fn retryable_gateway_channel_outbox(
        &self,
        connection_id: &str,
    ) -> Result<Vec<GatewayChannelOutboxRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT delivery_id, thread_id, turn_id, connection_id, source_key,
                   status, payload_text, payload_hash, created_at_ms,
                   updated_at_ms, acknowledged_at_ms
            FROM gateway_channel_outbox
            WHERE connection_id = ?1
              AND status IN ('pending', 'failed')
              AND payload_text IS NOT NULL
            ORDER BY created_at_ms ASC, delivery_id ASC
            LIMIT 32
            "#,
            )
            .bind(connection_id)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| gateway_channel_outbox_record(&row))
                .collect()
        })
        .await
    }

    async fn delivery_update<'q>(
        &self,
        query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    ) -> Result<bool> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = query.execute(&mut *tx).await?.rows_affected();
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(changed > 0)
        })
        .await
    }
}

fn gateway_turn_delivery_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GatewayTurnDeliveryRecord> {
    Ok(GatewayTurnDeliveryRecord {
        turn_id: row.try_get(0)?,
        thread_id: row.try_get(1)?,
        runtime_ref: row.try_get(2)?,
        status: row.try_get(3)?,
        input_json: row.try_get(4)?,
        input_hash: row.try_get(5)?,
        created_at_ms: row.try_get(6)?,
        updated_at_ms: row.try_get(7)?,
        delivery_confirmed_at_ms: row.try_get(8)?,
        terminal_at_ms: row.try_get(9)?,
    })
}

fn gateway_channel_outbox_record(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GatewayChannelOutboxRecord> {
    Ok(GatewayChannelOutboxRecord {
        delivery_id: row.try_get(0)?,
        thread_id: row.try_get(1)?,
        turn_id: row.try_get(2)?,
        connection_id: row.try_get(3)?,
        source_key: row.try_get(4)?,
        status: row.try_get(5)?,
        payload_text: row.try_get(6)?,
        payload_hash: row.try_get(7)?,
        created_at_ms: row.try_get(8)?,
        updated_at_ms: row.try_get(9)?,
        acknowledged_at_ms: row.try_get(10)?,
    })
}

async fn scrub_gateway_activity_turn_input(
    tx: &mut sqlx::Transaction<'static, sqlx::Sqlite>,
    turn_id: &str,
    updated_at_ms: i64,
) -> Result<u64> {
    Ok(sqlx::query(
        r#"
        UPDATE gateway_activities
        SET intent_json = CASE
                WHEN json_valid(intent_json) THEN json_remove(intent_json, '$.input')
                ELSE NULL
            END,
            updated_at_ms = ?2
        WHERE turn_id = ?1
        "#,
    )
    .bind(turn_id)
    .bind(updated_at_ms)
    .execute(&mut **tx)
    .await?
    .rows_affected())
}
