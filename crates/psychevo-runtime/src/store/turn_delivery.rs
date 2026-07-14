use psychevo_agent_core::now_ms;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::error::{Error, Result};

use super::{
    GatewayChannelOutboxInput, GatewayChannelOutboxRecord, GatewayTurnDeliveryInput,
    GatewayTurnDeliveryRecord, SqliteStore,
};

impl SqliteStore {
    pub fn insert_gateway_turn_delivery(
        &self,
        input: GatewayTurnDeliveryInput<'_>,
    ) -> Result<GatewayTurnDeliveryRecord> {
        let now = now_ms();
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO gateway_turn_deliveries (
                    turn_id, thread_id, runtime_ref, status, input_json,
                    input_hash, created_at_ms, updated_at_ms,
                    delivery_confirmed_at_ms, terminal_at_ms
                ) VALUES (?1, ?2, ?3, 'not_delivered', ?4, ?5, ?6, ?6, NULL, NULL)
                "#,
                params![
                    input.turn_id,
                    input.thread_id,
                    input.runtime_ref,
                    input.input_json,
                    input.input_hash,
                    now,
                ],
            )?;
            Ok(())
        })?;
        self.gateway_turn_delivery(input.turn_id)?.ok_or_else(|| {
            Error::Message(format!(
                "turn delivery not found after insert: {}",
                input.turn_id
            ))
        })
    }

    pub fn mark_gateway_turn_delivery_unknown(&self, turn_id: &str) -> Result<bool> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                r#"
                UPDATE gateway_turn_deliveries
                SET status = 'unknown', updated_at_ms = ?2
                WHERE turn_id = ?1 AND status = 'not_delivered'
                "#,
                params![turn_id, now],
            )
        })?;
        Ok(changed > 0)
    }

    pub fn confirm_gateway_turn_delivery(&self, turn_id: &str) -> Result<bool> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            let changed = conn.execute(
                r#"
                UPDATE gateway_turn_deliveries
                SET status = 'delivered', input_json = NULL,
                    delivery_confirmed_at_ms = COALESCE(delivery_confirmed_at_ms, ?2),
                    updated_at_ms = ?2
                WHERE turn_id = ?1 AND status IN ('not_delivered', 'unknown', 'delivered')
                "#,
                params![turn_id, now],
            )?;
            if changed > 0 {
                scrub_gateway_activity_turn_input(conn, turn_id, now)?;
            }
            Ok(changed)
        })?;
        Ok(changed > 0)
    }

    pub fn finish_gateway_turn_delivery(&self, turn_id: &str) -> Result<bool> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            let changed = conn.execute(
                r#"
                UPDATE gateway_turn_deliveries
                SET status = 'terminal', input_json = NULL,
                    terminal_at_ms = COALESCE(terminal_at_ms, ?2),
                    updated_at_ms = ?2
                WHERE turn_id = ?1 AND status != 'unknown'
                "#,
                params![turn_id, now],
            )?;
            if changed > 0 {
                scrub_gateway_activity_turn_input(conn, turn_id, now)?;
            }
            Ok(changed)
        })?;
        Ok(changed > 0)
    }

    pub fn unknown_gateway_turn_deliveries_for_thread(
        &self,
        thread_id: &str,
        exclude_turn_id: &str,
    ) -> Result<Vec<GatewayTurnDeliveryRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut statement = conn.prepare(
            r#"
            SELECT turn_id, thread_id, runtime_ref, status, input_json,
                   input_hash, created_at_ms, updated_at_ms,
                   delivery_confirmed_at_ms, terminal_at_ms
            FROM gateway_turn_deliveries
            WHERE thread_id = ?1 AND status = 'unknown' AND turn_id != ?2
            ORDER BY created_at_ms ASC, turn_id ASC
            LIMIT 2
            "#,
        )?;
        let rows = statement.query_map(
            params![thread_id, exclude_turn_id],
            gateway_turn_delivery_from_row,
        )?;
        let mut deliveries = Vec::new();
        for row in rows {
            deliveries.push(row?);
        }
        Ok(deliveries)
    }

    /// Atomically resolves delivery ambiguity after Agent-owned history proves
    /// that the prior turn reached a normal terminal. This is deliberately a
    /// distinct transition from `confirm_gateway_turn_delivery`: only replay
    /// reconciliation may move `unknown` directly to `terminal` and scrub the
    /// retained recovery input.
    pub fn reconcile_unknown_gateway_turn_delivery(
        &self,
        turn_id: &str,
        thread_id: &str,
        metadata: Option<&Value>,
    ) -> Result<bool> {
        let now = now_ms();
        let metadata_json = metadata.map(serde_json::to_string).transpose()?;
        self.write_retry(|conn| {
            let changed = conn.execute(
                r#"
                UPDATE gateway_turn_deliveries
                SET status = 'terminal', input_json = NULL,
                    delivery_confirmed_at_ms = COALESCE(delivery_confirmed_at_ms, ?3),
                    terminal_at_ms = COALESCE(terminal_at_ms, ?3),
                    updated_at_ms = ?3
                WHERE turn_id = ?1 AND thread_id = ?2 AND status = 'unknown'
                "#,
                params![turn_id, thread_id, now],
            )?;
            if changed == 0 {
                return Ok(false);
            }
            scrub_gateway_activity_turn_input(conn, turn_id, now)?;
            conn.execute(
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
                params![turn_id, thread_id, now, metadata_json],
            )?;
            Ok(true)
        })
    }

    pub fn gateway_turn_delivery(
        &self,
        turn_id: &str,
    ) -> Result<Option<GatewayTurnDeliveryRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            r#"
            SELECT turn_id, thread_id, runtime_ref, status, input_json,
                   input_hash, created_at_ms, updated_at_ms,
                   delivery_confirmed_at_ms, terminal_at_ms
            FROM gateway_turn_deliveries
            WHERE turn_id = ?1
            "#,
            params![turn_id],
            gateway_turn_delivery_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn upsert_gateway_channel_outbox(
        &self,
        input: GatewayChannelOutboxInput<'_>,
    ) -> Result<GatewayChannelOutboxRecord> {
        let now = now_ms();
        self.write_retry(|conn| {
            conn.execute(
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
                params![
                    input.delivery_id,
                    input.thread_id,
                    input.turn_id,
                    input.connection_id,
                    input.source_key,
                    input.payload_text,
                    input.payload_hash,
                    now,
                ],
            )?;
            Ok(())
        })?;
        self.gateway_channel_outbox(input.delivery_id)?
            .ok_or_else(|| {
                Error::Message(format!(
                    "channel outbox row not found after upsert: {}",
                    input.delivery_id
                ))
            })
    }

    pub fn acknowledge_gateway_channel_outbox(&self, delivery_id: &str) -> Result<bool> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                r#"
                UPDATE gateway_channel_outbox
                SET status = 'acknowledged', payload_text = NULL,
                    acknowledged_at_ms = COALESCE(acknowledged_at_ms, ?2),
                    updated_at_ms = ?2
                WHERE delivery_id = ?1 AND status != 'acknowledged'
                "#,
                params![delivery_id, now],
            )
        })?;
        Ok(changed > 0)
    }

    pub fn fail_gateway_channel_outbox(&self, delivery_id: &str) -> Result<bool> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                r#"
                UPDATE gateway_channel_outbox
                SET status = 'failed', updated_at_ms = ?2
                WHERE delivery_id = ?1 AND status = 'pending'
                "#,
                params![delivery_id, now],
            )
        })?;
        Ok(changed > 0)
    }

    pub fn gateway_channel_outbox(
        &self,
        delivery_id: &str,
    ) -> Result<Option<GatewayChannelOutboxRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            r#"
            SELECT delivery_id, thread_id, turn_id, connection_id, source_key,
                   status, payload_text, payload_hash, created_at_ms,
                   updated_at_ms, acknowledged_at_ms
            FROM gateway_channel_outbox
            WHERE delivery_id = ?1
            "#,
            params![delivery_id],
            gateway_channel_outbox_record,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn retryable_gateway_channel_outbox(
        &self,
        connection_id: &str,
    ) -> Result<Vec<GatewayChannelOutboxRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut statement = conn.prepare(
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
        )?;
        statement
            .query_map(params![connection_id], gateway_channel_outbox_record)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

fn gateway_turn_delivery_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<GatewayTurnDeliveryRecord> {
    Ok(GatewayTurnDeliveryRecord {
        turn_id: row.get(0)?,
        thread_id: row.get(1)?,
        runtime_ref: row.get(2)?,
        status: row.get(3)?,
        input_json: row.get(4)?,
        input_hash: row.get(5)?,
        created_at_ms: row.get(6)?,
        updated_at_ms: row.get(7)?,
        delivery_confirmed_at_ms: row.get(8)?,
        terminal_at_ms: row.get(9)?,
    })
}

fn gateway_channel_outbox_record(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<GatewayChannelOutboxRecord> {
    Ok(GatewayChannelOutboxRecord {
        delivery_id: row.get(0)?,
        thread_id: row.get(1)?,
        turn_id: row.get(2)?,
        connection_id: row.get(3)?,
        source_key: row.get(4)?,
        status: row.get(5)?,
        payload_text: row.get(6)?,
        payload_hash: row.get(7)?,
        created_at_ms: row.get(8)?,
        updated_at_ms: row.get(9)?,
        acknowledged_at_ms: row.get(10)?,
    })
}

fn scrub_gateway_activity_turn_input(
    conn: &Connection,
    turn_id: &str,
    updated_at_ms: i64,
) -> rusqlite::Result<usize> {
    conn.execute(
        r#"
        UPDATE gateway_activities
        SET intent_json = CASE
                WHEN json_valid(intent_json) THEN json_remove(intent_json, '$.input')
                ELSE NULL
            END,
            updated_at_ms = ?2
        WHERE turn_id = ?1
        "#,
        params![turn_id, updated_at_ms],
    )
}
