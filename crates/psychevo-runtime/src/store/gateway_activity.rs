use std::collections::HashSet;

use psychevo_agent_core::now_ms;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use crate::error::{Error, Result};

use super::{
    GatewayActivityClaimInput, GatewayActivityRecord, GatewayControlCommandInput,
    GatewayControlCommandRecord, GatewayLiveEventRecord, GatewayLiveSnapshotInput,
    GatewayLiveSnapshotRecord, GatewayTurnStartReceiptRecord, GatewayTurnTerminalInput,
    GatewayTurnTerminalRecord, StateRuntime,
};

const TURN_START_RECEIPTS_METADATA_KEY: &str = "gatewayTurnStartReceipts";
const MAX_TURN_START_RECEIPTS: usize = 32;

impl StateRuntime {
    pub fn record_gateway_turn_start_receipt(
        &self,
        thread_id: &str,
        client_turn_id: &str,
        turn_id: &str,
    ) -> Result<()> {
        let changed = self.write_retry(|conn| {
            let metadata_json = conn
                .query_row(
                    "SELECT metadata_json FROM sessions WHERE id = ?1",
                    params![thread_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?;
            let Some(metadata_json) = metadata_json else {
                return Ok(0);
            };
            let mut metadata = metadata_json
                .as_deref()
                .map(serde_json::from_str::<serde_json::Map<String, Value>>)
                .transpose()
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?
                .unwrap_or_default();
            let mut receipts = metadata
                .remove(TURN_START_RECEIPTS_METADATA_KEY)
                .and_then(|value| value.as_array().cloned())
                .unwrap_or_default();
            receipts.retain(|receipt| {
                receipt.get("clientTurnId").and_then(Value::as_str) != Some(client_turn_id)
            });
            receipts.push(serde_json::json!({
                "clientTurnId": client_turn_id,
                "turnId": turn_id,
            }));
            if receipts.len() > MAX_TURN_START_RECEIPTS {
                receipts.drain(..receipts.len() - MAX_TURN_START_RECEIPTS);
            }
            metadata.insert(
                TURN_START_RECEIPTS_METADATA_KEY.to_string(),
                Value::Array(receipts),
            );
            conn.execute(
                "UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![Value::Object(metadata).to_string(), now_ms(), thread_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {thread_id}")));
        }
        Ok(())
    }

    pub fn gateway_turn_start_receipts(
        &self,
        thread_id: &str,
    ) -> Result<Vec<GatewayTurnStartReceiptRecord>> {
        let Some(metadata) = self.session_metadata(thread_id)? else {
            return Ok(Vec::new());
        };
        Ok(metadata
            .get(TURN_START_RECEIPTS_METADATA_KEY)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|receipt| {
                Some(GatewayTurnStartReceiptRecord {
                    client_turn_id: receipt.get("clientTurnId")?.as_str()?.to_string(),
                    turn_id: receipt.get("turnId")?.as_str()?.to_string(),
                })
            })
            .collect())
    }

    pub fn claim_gateway_activity(
        &self,
        input: GatewayActivityClaimInput<'_>,
    ) -> Result<GatewayActivityRecord> {
        let now = now_ms();
        let existing = {
            let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
            conn.query_row(
                gateway_activity_select_sql(
                    "WHERE status IN ('running', 'queued')
                     AND ((?1 IS NOT NULL AND thread_id = ?1)
                       OR (?2 IS NOT NULL AND source_key = ?2))
                     ORDER BY generation DESC, updated_at_ms DESC
                     LIMIT 1",
                )
                .as_str(),
                params![input.thread_id, input.source_key],
                gateway_activity_from_row,
            )
            .optional()?
        };
        if let Some(existing) = existing.as_ref()
            && existing.lease_expires_at_ms >= now
            && existing.owner_id != input.owner_id
        {
            return Err(Error::Message(format!(
                "gateway activity already owned by {} until {}",
                existing.owner_id, existing.lease_expires_at_ms
            )));
        }
        let superseded_activity_id = input
            .superseded_activity_id
            .or_else(|| existing.as_ref().map(|record| record.activity_id.as_str()));
        let intent_json = input
            .intent
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let generation = self.write_retry(|conn| {
            let previous_generation: i64 = conn.query_row(
                r#"
                SELECT COALESCE(MAX(generation), 0)
                FROM gateway_activities
                WHERE (?1 IS NOT NULL AND thread_id = ?1)
                   OR (?2 IS NOT NULL AND source_key = ?2)
                "#,
                params![input.thread_id, input.source_key],
                |row| row.get(0),
            )?;
            let generation = previous_generation.saturating_add(1);
            conn.execute(
                r#"
                UPDATE gateway_activities
                SET status = 'superseded',
                    updated_at_ms = ?3,
                    lease_expires_at_ms = ?3,
                    superseded_activity_id = ?4
                WHERE status IN ('running', 'queued')
                  AND lease_expires_at_ms < ?3
                  AND ((?1 IS NOT NULL AND thread_id = ?1)
                    OR (?2 IS NOT NULL AND source_key = ?2))
                "#,
                params![input.thread_id, input.source_key, now, input.activity_id],
            )?;
            conn.execute(
                r#"
                INSERT INTO gateway_activities (
                    activity_id, thread_id, source_key, turn_id, kind, status,
                    owner_id, owner_surface, generation, started_at_ms, updated_at_ms,
                    lease_expires_at_ms, queued_turns, superseded_activity_id, intent_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, 'running',
                    ?6, ?7, ?8, ?9, ?9, ?10, ?11, ?12, ?13)
                "#,
                params![
                    input.activity_id,
                    input.thread_id,
                    input.source_key,
                    input.turn_id,
                    input.kind,
                    input.owner_id,
                    input.owner_surface,
                    generation,
                    now,
                    input.lease_expires_at_ms,
                    input.queued_turns as i64,
                    superseded_activity_id,
                    intent_json,
                ],
            )?;
            Ok(generation)
        })?;
        self.gateway_activity(input.activity_id)?
            .ok_or_else(|| {
                Error::Message(format!(
                    "gateway activity not found after claim: {}",
                    input.activity_id
                ))
            })
            .map(|mut record| {
                record.generation = generation;
                record
            })
    }

    pub fn gateway_activity(&self, activity_id: &str) -> Result<Option<GatewayActivityRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            gateway_activity_select_sql("WHERE activity_id = ?1").as_str(),
            params![activity_id],
            gateway_activity_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn active_gateway_activity_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Option<GatewayActivityRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            gateway_activity_select_sql(
                "WHERE thread_id = ?1 AND status IN ('running', 'queued') ORDER BY generation DESC, updated_at_ms DESC LIMIT 1",
            )
            .as_str(),
            params![thread_id],
            gateway_activity_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn active_gateway_activity_for_source(
        &self,
        source_key: &str,
    ) -> Result<Option<GatewayActivityRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            gateway_activity_select_sql(
                "WHERE source_key = ?1 AND status IN ('running', 'queued') ORDER BY generation DESC, updated_at_ms DESC LIMIT 1",
            )
            .as_str(),
            params![source_key],
            gateway_activity_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn active_gateway_activities(&self) -> Result<Vec<GatewayActivityRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            gateway_activity_select_sql(
                "WHERE thread_id IS NOT NULL AND status IN ('running', 'queued')
                 ORDER BY thread_id ASC, generation DESC, updated_at_ms DESC",
            )
            .as_str(),
        )?;
        let rows = stmt.query_map([], gateway_activity_from_row)?;
        let mut seen = HashSet::new();
        let mut activities = Vec::new();
        for row in rows {
            let activity = row?;
            let Some(thread_id) = activity.thread_id.as_ref() else {
                continue;
            };
            if seen.insert(thread_id.clone()) {
                activities.push(activity);
            }
        }
        Ok(activities)
    }

    pub fn update_gateway_activity_thread(
        &self,
        activity_id: &str,
        owner_id: &str,
        generation: i64,
        thread_id: &str,
        lease_expires_at_ms: i64,
    ) -> Result<bool> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                r#"
                UPDATE gateway_activities
                SET thread_id = ?4, updated_at_ms = ?5, lease_expires_at_ms = ?6
                WHERE activity_id = ?1 AND owner_id = ?2 AND generation = ?3
                  AND (thread_id IS NULL OR thread_id = ?4)
                "#,
                params![
                    activity_id,
                    owner_id,
                    generation,
                    thread_id,
                    now,
                    lease_expires_at_ms,
                ],
            )
        })?;
        Ok(changed > 0)
    }

    pub fn heartbeat_gateway_activity(
        &self,
        activity_id: &str,
        owner_id: &str,
        generation: i64,
        lease_expires_at_ms: i64,
    ) -> Result<bool> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                r#"
                UPDATE gateway_activities
                SET updated_at_ms = ?4, lease_expires_at_ms = ?5
                WHERE activity_id = ?1
                  AND owner_id = ?2
                  AND generation = ?3
                  AND status IN ('running', 'queued')
                "#,
                params![activity_id, owner_id, generation, now, lease_expires_at_ms],
            )
        })?;
        Ok(changed > 0)
    }

    pub fn set_gateway_activity_queued_turns(
        &self,
        activity_id: &str,
        queued_turns: usize,
    ) -> Result<bool> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                r#"
                UPDATE gateway_activities
                SET queued_turns = ?2, updated_at_ms = ?3
                WHERE activity_id = ?1 AND status IN ('running', 'queued')
                "#,
                params![activity_id, queued_turns as i64, now],
            )
        })?;
        Ok(changed > 0)
    }

    pub fn finish_gateway_activity(
        &self,
        activity_id: &str,
        owner_id: &str,
        generation: i64,
        status: &str,
    ) -> Result<bool> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                r#"
                UPDATE gateway_activities
                SET status = ?4,
                    updated_at_ms = ?5,
                    lease_expires_at_ms = ?5,
                    queued_turns = 0
                WHERE activity_id = ?1 AND owner_id = ?2 AND generation = ?3
                "#,
                params![activity_id, owner_id, generation, status, now],
            )
        })?;
        Ok(changed > 0)
    }

    pub fn supersede_stale_gateway_activity(
        &self,
        activity_id: &str,
        superseded_by_activity_id: &str,
    ) -> Result<bool> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                r#"
                UPDATE gateway_activities
                SET status = 'superseded',
                    updated_at_ms = ?3,
                    lease_expires_at_ms = ?3,
                    superseded_activity_id = ?2
                WHERE activity_id = ?1
                  AND status IN ('running', 'queued')
                  AND lease_expires_at_ms < ?3
                "#,
                params![activity_id, superseded_by_activity_id, now],
            )
        })?;
        Ok(changed > 0)
    }

    pub fn append_gateway_live_event(
        &self,
        activity_id: Option<&str>,
        owner_id: Option<&str>,
        thread_id: Option<&str>,
        turn_id: Option<&str>,
        event: &Value,
    ) -> Result<i64> {
        let now = now_ms();
        let event_json = serde_json::to_string(event)?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO gateway_live_events (
                    activity_id, owner_id, thread_id, turn_id, event_json, created_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![activity_id, owner_id, thread_id, turn_id, event_json, now],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    pub fn latest_gateway_live_event_seq(&self) -> Result<i64> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM gateway_live_events",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
    }

    pub fn list_gateway_live_events_after(
        &self,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<GatewayLiveEventRecord>> {
        let limit = limit.clamp(1, 500) as i64;
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT seq, activity_id, owner_id, thread_id, turn_id, event_json, created_at_ms
            FROM gateway_live_events
            WHERE seq > ?1
            ORDER BY seq ASC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![after_seq, limit], gateway_live_event_from_row)?;
        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    pub fn cleanup_gateway_live_events_before(&self, before_ms: i64) -> Result<usize> {
        self.write_retry(|conn| {
            conn.execute(
                "DELETE FROM gateway_live_events WHERE created_at_ms < ?1",
                params![before_ms],
            )
        })
    }

    pub fn delete_gateway_live_events_for_activity(&self, activity_id: &str) -> Result<usize> {
        self.write_retry(|conn| {
            conn.execute(
                "DELETE FROM gateway_live_events WHERE activity_id = ?1",
                params![activity_id],
            )
        })
    }

    pub fn upsert_gateway_live_snapshot(&self, input: GatewayLiveSnapshotInput<'_>) -> Result<i64> {
        let now = now_ms();
        let event_json = serde_json::to_string(&input.event)?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO gateway_live_snapshots (
                    snapshot_key, activity_id, owner_id, thread_id, turn_id,
                    event_kind, event_json, revision, created_at_ms, updated_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8)
                ON CONFLICT(snapshot_key) DO UPDATE SET
                    activity_id = excluded.activity_id,
                    owner_id = excluded.owner_id,
                    thread_id = excluded.thread_id,
                    turn_id = excluded.turn_id,
                    event_kind = excluded.event_kind,
                    event_json = excluded.event_json,
                    revision = gateway_live_snapshots.revision + 1,
                    updated_at_ms = excluded.updated_at_ms
                "#,
                params![
                    input.snapshot_key,
                    input.activity_id,
                    input.owner_id,
                    input.thread_id,
                    input.turn_id,
                    input.event_kind,
                    event_json,
                    now,
                ],
            )?;
            conn.query_row(
                "SELECT revision FROM gateway_live_snapshots WHERE snapshot_key = ?1",
                params![input.snapshot_key],
                |row| row.get(0),
            )
        })
    }

    pub fn list_gateway_live_snapshots(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayLiveSnapshotRecord>> {
        let limit = limit.clamp(1, 1000) as i64;
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT snapshot_key, activity_id, owner_id, thread_id, turn_id,
                   event_kind, event_json, revision, created_at_ms, updated_at_ms
            FROM gateway_live_snapshots
            ORDER BY updated_at_ms ASC, snapshot_key ASC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit], gateway_live_snapshot_from_row)?;
        let mut snapshots = Vec::new();
        for row in rows {
            snapshots.push(row?);
        }
        Ok(snapshots)
    }

    pub fn list_gateway_live_snapshots_for_thread(
        &self,
        thread_id: &str,
        turn_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GatewayLiveSnapshotRecord>> {
        let limit = limit.clamp(1, 1000) as i64;
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT snapshot_key, activity_id, owner_id, thread_id, turn_id,
                   event_kind, event_json, revision, created_at_ms, updated_at_ms
            FROM gateway_live_snapshots
            WHERE thread_id = ?1
              AND (?2 IS NULL OR turn_id = ?2)
            ORDER BY updated_at_ms ASC, snapshot_key ASC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(
            params![thread_id, turn_id, limit],
            gateway_live_snapshot_from_row,
        )?;
        let mut snapshots = Vec::new();
        for row in rows {
            snapshots.push(row?);
        }
        Ok(snapshots)
    }

    pub fn delete_gateway_live_snapshots_for_activity(&self, activity_id: &str) -> Result<usize> {
        self.write_retry(|conn| {
            conn.execute(
                "DELETE FROM gateway_live_snapshots WHERE activity_id = ?1",
                params![activity_id],
            )
        })
    }

    pub fn cleanup_gateway_live_snapshots_before(&self, before_ms: i64) -> Result<usize> {
        self.write_retry(|conn| {
            conn.execute(
                "DELETE FROM gateway_live_snapshots WHERE updated_at_ms < ?1",
                params![before_ms],
            )
        })
    }

    pub fn enqueue_gateway_control_command(
        &self,
        input: GatewayControlCommandInput<'_>,
    ) -> Result<i64> {
        let now = now_ms();
        let payload_json = serde_json::to_string(&input.payload)?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO gateway_control_commands (
                    activity_id, owner_id, command_kind, status, payload_json,
                    created_at_ms, updated_at_ms, error
                ) VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?5, NULL)
                "#,
                params![
                    input.activity_id,
                    input.owner_id,
                    input.command_kind,
                    payload_json,
                    now,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    pub fn pending_gateway_control_commands(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<GatewayControlCommandRecord>> {
        let limit = limit.clamp(1, 100) as i64;
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, activity_id, owner_id, command_kind, status, payload_json,
                   created_at_ms, updated_at_ms, error
            FROM gateway_control_commands
            WHERE owner_id = ?1 AND status = 'pending'
            ORDER BY id ASC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![owner_id, limit], gateway_control_command_from_row)?;
        let mut commands = Vec::new();
        for row in rows {
            commands.push(row?);
        }
        Ok(commands)
    }

    pub fn mark_gateway_control_command_applied(&self, id: i64) -> Result<bool> {
        self.update_gateway_control_command_status(id, "applied", None)
    }

    pub fn mark_gateway_control_command_failed(&self, id: i64, error: &str) -> Result<bool> {
        self.update_gateway_control_command_status(id, "failed", Some(error))
    }

    fn update_gateway_control_command_status(
        &self,
        id: i64,
        status: &str,
        error: Option<&str>,
    ) -> Result<bool> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                r#"
                UPDATE gateway_control_commands
                SET status = ?2, updated_at_ms = ?3, error = ?4
                WHERE id = ?1 AND status = 'pending'
                "#,
                params![id, status, now, error],
            )
        })?;
        Ok(changed > 0)
    }

    pub fn upsert_gateway_turn_terminal(
        &self,
        input: GatewayTurnTerminalInput<'_>,
    ) -> Result<GatewayTurnTerminalRecord> {
        let metadata_json = input
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        self.write_retry(|conn| {
            conn.execute(
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
                params![
                    input.turn_id,
                    input.thread_id,
                    input.status,
                    input.outcome,
                    input.error_message,
                    input.started_at_ms,
                    input.completed_at_ms,
                    metadata_json,
                ],
            )
        })?;
        self.gateway_turn_terminal(input.turn_id)?.ok_or_else(|| {
            Error::Message(format!(
                "gateway turn terminal not found after write: {}",
                input.turn_id
            ))
        })
    }

    pub fn gateway_turn_terminal(
        &self,
        turn_id: &str,
    ) -> Result<Option<GatewayTurnTerminalRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            gateway_turn_terminal_select_sql("WHERE turn_id = ?1").as_str(),
            params![turn_id],
            gateway_turn_terminal_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_gateway_turn_terminals_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<GatewayTurnTerminalRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            gateway_turn_terminal_select_sql(
                "WHERE thread_id = ?1 ORDER BY completed_at_ms ASC, turn_id ASC",
            )
            .as_str(),
        )?;
        let rows = stmt.query_map(params![thread_id], gateway_turn_terminal_from_row)?;
        let mut terminals = Vec::new();
        for row in rows {
            terminals.push(row?);
        }
        Ok(terminals)
    }
}

fn gateway_activity_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        SELECT activity_id, thread_id, source_key, turn_id, kind, status,
               owner_id, owner_surface, generation, started_at_ms, updated_at_ms,
               lease_expires_at_ms, queued_turns, superseded_activity_id, intent_json
        FROM gateway_activities
        {where_clause}
        "#
    )
}

fn gateway_activity_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GatewayActivityRecord> {
    let intent_json: Option<String> = row.get(14)?;
    let intent = intent_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                14,
                rusqlite::types::Type::Text,
                Box::new(err),
            )
        })?;
    let queued_turns: i64 = row.get(12)?;
    Ok(GatewayActivityRecord {
        activity_id: row.get(0)?,
        thread_id: row.get(1)?,
        source_key: row.get(2)?,
        turn_id: row.get(3)?,
        kind: row.get(4)?,
        status: row.get(5)?,
        owner_id: row.get(6)?,
        owner_surface: row.get(7)?,
        generation: row.get(8)?,
        started_at_ms: row.get(9)?,
        updated_at_ms: row.get(10)?,
        lease_expires_at_ms: row.get(11)?,
        queued_turns: queued_turns.max(0) as usize,
        superseded_activity_id: row.get(13)?,
        intent,
    })
}

fn gateway_live_event_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<GatewayLiveEventRecord> {
    let event_json: String = row.get(5)?;
    let event = serde_json::from_str(&event_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(err))
    })?;
    Ok(GatewayLiveEventRecord {
        seq: row.get(0)?,
        activity_id: row.get(1)?,
        owner_id: row.get(2)?,
        thread_id: row.get(3)?,
        turn_id: row.get(4)?,
        event,
        created_at_ms: row.get(6)?,
    })
}

fn gateway_live_snapshot_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<GatewayLiveSnapshotRecord> {
    let event_json: String = row.get(6)?;
    let event = serde_json::from_str(&event_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(err))
    })?;
    Ok(GatewayLiveSnapshotRecord {
        snapshot_key: row.get(0)?,
        activity_id: row.get(1)?,
        owner_id: row.get(2)?,
        thread_id: row.get(3)?,
        turn_id: row.get(4)?,
        event_kind: row.get(5)?,
        event,
        revision: row.get(7)?,
        created_at_ms: row.get(8)?,
        updated_at_ms: row.get(9)?,
    })
}

fn gateway_control_command_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<GatewayControlCommandRecord> {
    let payload_json: String = row.get(5)?;
    let payload = serde_json::from_str(&payload_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(err))
    })?;
    Ok(GatewayControlCommandRecord {
        id: row.get(0)?,
        activity_id: row.get(1)?,
        owner_id: row.get(2)?,
        command_kind: row.get(3)?,
        status: row.get(4)?,
        payload,
        created_at_ms: row.get(6)?,
        updated_at_ms: row.get(7)?,
        error: row.get(8)?,
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
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<GatewayTurnTerminalRecord> {
    let metadata_json: Option<String> = row.get(7)?;
    let metadata = metadata_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(err))
        })?;
    Ok(GatewayTurnTerminalRecord {
        turn_id: row.get(0)?,
        thread_id: row.get(1)?,
        status: row.get(2)?,
        outcome: row.get(3)?,
        error_message: row.get(4)?,
        started_at_ms: row.get(5)?,
        completed_at_ms: row.get(6)?,
        metadata,
    })
}
