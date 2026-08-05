use std::collections::{HashMap, HashSet};

use psychevo_agent_core::now_ms;
use serde_json::Value;
use sqlx::{Arguments, Row, Sqlite, Transaction};

use crate::error::{Error, Result};

use super::{
    GatewayActivityClaimInput, GatewayActivityKind, GatewayActivityRecord, GatewayActivityState,
    GatewayActivityTerminalStatus, GatewayTurnStartReceiptRecord, StateRuntime,
    invalid_persisted_domain_value,
};

const TURN_START_RECEIPTS_METADATA_KEY: &str = "gatewayTurnStartReceipts";
const MAX_TURN_START_RECEIPTS: usize = 32;
const MAX_GATEWAY_ACTIVITY_BATCH_IDS: usize = 1_500;

impl StateRuntime {
    pub async fn record_gateway_turn_start_receipt(
        &self,
        thread_id: &str,
        client_turn_id: &str,
        turn_id: &str,
    ) -> Result<()> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            record_gateway_turn_start_receipt_in_tx(&mut tx, thread_id, client_turn_id, turn_id)
                .await?;
            tx.commit().await?;
            Ok(())
        })
        .await
    }

    pub(crate) async fn gateway_turn_start_receipts(
        &self,
        thread_id: &str,
    ) -> Result<Vec<GatewayTurnStartReceiptRecord>> {
        let Some(metadata) = self.session_metadata(thread_id).await? else {
            return Ok(Vec::new());
        };
        let metadata = metadata
            .as_object()
            .ok_or_else(|| corrupt_turn_start_receipts(thread_id))?;
        let Some(receipts) = metadata.get(TURN_START_RECEIPTS_METADATA_KEY) else {
            return Ok(Vec::new());
        };
        let receipts = receipts
            .as_array()
            .filter(|receipts| receipts.len() <= MAX_TURN_START_RECEIPTS)
            .ok_or_else(|| corrupt_turn_start_receipts(thread_id))?;
        receipts
            .iter()
            .map(|receipt| {
                let client_turn_id = receipt
                    .get("clientTurnId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| corrupt_turn_start_receipts(thread_id))?;
                let turn_id = receipt
                    .get("turnId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| corrupt_turn_start_receipts(thread_id))?;
                Ok(GatewayTurnStartReceiptRecord {
                    client_turn_id: client_turn_id.to_string(),
                    turn_id: turn_id.to_string(),
                })
            })
            .collect()
    }

    pub async fn claim_gateway_activity(
        &self,
        input: GatewayActivityClaimInput<'_>,
    ) -> Result<GatewayActivityRecord> {
        let now = now_ms();
        let intent_json = input
            .intent
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let (generation, superseded_activity_id) = self
            .observe_sqlx(async {
                let mut tx = self.begin_sqlx_write().await?;
                let select = gateway_activity_select_sql(
                    "WHERE status IN ('running', 'queued')
                     AND ((?1 IS NOT NULL AND thread_id = ?1)
                       OR (?2 IS NOT NULL AND source_key = ?2))
                     ORDER BY generation DESC, updated_at_ms DESC
                     LIMIT 1",
                );
                let existing = sqlx::query(sqlx::AssertSqlSafe(select))
                    .bind(input.thread_id)
                    .bind(input.source_key)
                    .fetch_optional(&mut *tx)
                    .await?
                    .map(|row| gateway_activity_from_row(&row))
                    .transpose()?;
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
                    .map(str::to_string)
                    .or_else(|| existing.as_ref().map(|record| record.activity_id.clone()));
                let previous_generation = sqlx::query_scalar::<_, i64>(
                    r#"
                    SELECT COALESCE(MAX(generation), 0)
                    FROM gateway_activities
                    WHERE (?1 IS NOT NULL AND thread_id = ?1)
                       OR (?2 IS NOT NULL AND source_key = ?2)
                    "#,
                )
                .bind(input.thread_id)
                .bind(input.source_key)
                .fetch_one(&mut *tx)
                .await?;
                let generation = previous_generation.saturating_add(1);
                sqlx::query(
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
                )
                .bind(input.thread_id)
                .bind(input.source_key)
                .bind(now)
                .bind(input.activity_id)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    r#"
                    INSERT INTO gateway_activities (
                        activity_id, thread_id, source_key, turn_id, kind, status,
                        owner_id, owner_surface, generation, started_at_ms, updated_at_ms,
                        lease_expires_at_ms, queued_turns, superseded_activity_id, intent_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, 'running',
                        ?6, ?7, ?8, ?9, ?9, ?10, ?11, ?12, ?13)
                    "#,
                )
                .bind(input.activity_id)
                .bind(input.thread_id)
                .bind(input.source_key)
                .bind(input.turn_id)
                .bind(input.kind.as_str())
                .bind(input.owner_id)
                .bind(input.owner_surface)
                .bind(generation)
                .bind(now)
                .bind(input.lease_expires_at_ms)
                .bind(input.queued_turns as i64)
                .bind(superseded_activity_id.as_deref())
                .bind(intent_json)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok((generation, superseded_activity_id))
            })
            .await?;
        let mut record = self
            .gateway_activity(input.activity_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "gateway activity not found after claim: {}",
                    input.activity_id
                ))
            })?;
        record.generation = generation;
        record.superseded_activity_id = superseded_activity_id;
        Ok(record)
    }

    pub async fn gateway_activity(
        &self,
        activity_id: &str,
    ) -> Result<Option<GatewayActivityRecord>> {
        let mut arguments = sqlx::sqlite::SqliteArguments::default();
        arguments
            .add(activity_id)
            .map_err(|error| Error::Message(error.to_string()))?;
        self.gateway_activity_optional("WHERE activity_id = ?1", arguments)
            .await
    }

    pub async fn gateway_activities_by_id(
        &self,
        activity_ids: &[String],
    ) -> Result<HashMap<String, GatewayActivityRecord>> {
        let mut activity_ids = activity_ids
            .iter()
            .filter(|activity_id| !activity_id.is_empty())
            .cloned()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if activity_ids.is_empty() {
            return Ok(HashMap::new());
        }
        if activity_ids.len() > MAX_GATEWAY_ACTIVITY_BATCH_IDS {
            return Err(Error::Message(format!(
                "gateway activity batch exceeds {MAX_GATEWAY_ACTIVITY_BATCH_IDS} ids"
            )));
        }
        activity_ids.sort();
        let placeholders = (1..=activity_ids.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = gateway_activity_select_sql(&format!(
            "WHERE activity_id IN ({placeholders}) ORDER BY activity_id ASC"
        ));
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
            for activity_id in activity_ids {
                query = query.bind(activity_id);
            }
            let rows = query.fetch_all(&mut *conn).await?;
            rows.into_iter()
                .map(|row| {
                    let activity = gateway_activity_from_row(&row)?;
                    Ok((activity.activity_id.clone(), activity))
                })
                .collect()
        })
        .await
    }

    pub async fn active_gateway_activity_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Option<GatewayActivityRecord>> {
        let mut arguments = sqlx::sqlite::SqliteArguments::default();
        arguments
            .add(thread_id)
            .map_err(|error| Error::Message(error.to_string()))?;
        self.gateway_activity_optional(
            "WHERE thread_id = ?1 AND status IN ('running', 'queued') ORDER BY generation DESC, updated_at_ms DESC LIMIT 1",
            arguments,
        )
        .await
    }

    pub async fn active_gateway_activity_for_source(
        &self,
        source_key: &str,
    ) -> Result<Option<GatewayActivityRecord>> {
        let mut arguments = sqlx::sqlite::SqliteArguments::default();
        arguments
            .add(source_key)
            .map_err(|error| Error::Message(error.to_string()))?;
        self.gateway_activity_optional(
            "WHERE source_key = ?1 AND status IN ('running', 'queued') ORDER BY generation DESC, updated_at_ms DESC LIMIT 1",
            arguments,
        )
        .await
    }

    pub async fn latest_gateway_activity_for_source(
        &self,
        source_key: &str,
    ) -> Result<Option<GatewayActivityRecord>> {
        let mut arguments = sqlx::sqlite::SqliteArguments::default();
        arguments
            .add(source_key)
            .map_err(|error| Error::Message(error.to_string()))?;
        self.gateway_activity_optional(
            "WHERE source_key = ?1 ORDER BY generation DESC, updated_at_ms DESC LIMIT 1",
            arguments,
        )
        .await
    }

    pub async fn active_gateway_activities(&self) -> Result<Vec<GatewayActivityRecord>> {
        let sql = gateway_activity_select_sql(
            "WHERE thread_id IS NOT NULL AND status IN ('running', 'queued')
             ORDER BY thread_id ASC, generation DESC, updated_at_ms DESC",
        );
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .fetch_all(&mut *conn)
                .await?;
            let mut seen = HashSet::new();
            let mut activities = Vec::new();
            for row in rows {
                let activity = gateway_activity_from_row(&row)?;
                let Some(thread_id) = activity.thread_id.as_ref() else {
                    continue;
                };
                if seen.insert(thread_id.clone()) {
                    activities.push(activity);
                }
            }
            Ok(activities)
        })
        .await
    }

    pub async fn update_gateway_activity_thread(
        &self,
        activity_id: &str,
        owner_id: &str,
        generation: i64,
        thread_id: &str,
        lease_expires_at_ms: i64,
    ) -> Result<bool> {
        self.gateway_activity_update(
            sqlx::query(
                r#"
                UPDATE gateway_activities
                SET thread_id = ?4, updated_at_ms = ?5, lease_expires_at_ms = ?6
                WHERE activity_id = ?1 AND owner_id = ?2 AND generation = ?3
                  AND (thread_id IS NULL OR thread_id = ?4)
                "#,
            )
            .bind(activity_id)
            .bind(owner_id)
            .bind(generation)
            .bind(thread_id)
            .bind(now_ms())
            .bind(lease_expires_at_ms),
        )
        .await
    }

    pub async fn heartbeat_gateway_activities(
        &self,
        owner_id: &str,
        activities: &[(String, i64)],
        lease_expires_at_ms: i64,
    ) -> Result<Vec<String>> {
        if activities.is_empty() {
            return Ok(Vec::new());
        }
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let updated_at_ms = now_ms();
            let mut refreshed = Vec::with_capacity(activities.len());
            for (activity_id, generation) in activities {
                let changed = sqlx::query(
                    r#"
                    UPDATE gateway_activities
                    SET updated_at_ms = ?4, lease_expires_at_ms = ?5
                    WHERE activity_id = ?1
                      AND owner_id = ?2
                      AND generation = ?3
                      AND status IN ('running', 'queued')
                    "#,
                )
                .bind(activity_id)
                .bind(owner_id)
                .bind(generation)
                .bind(updated_at_ms)
                .bind(lease_expires_at_ms)
                .execute(&mut *tx)
                .await?
                .rows_affected();
                if changed == 1 {
                    refreshed.push(activity_id.clone());
                }
            }
            tx.commit().await?;
            Ok(refreshed)
        })
        .await
    }

    pub async fn set_gateway_activity_queued_turns(
        &self,
        activity_id: &str,
        queued_turns: usize,
    ) -> Result<bool> {
        self.gateway_activity_update(
            sqlx::query(
                r#"
                UPDATE gateway_activities
                SET queued_turns = ?2, updated_at_ms = ?3
                WHERE activity_id = ?1 AND status IN ('running', 'queued')
                "#,
            )
            .bind(activity_id)
            .bind(queued_turns as i64)
            .bind(now_ms()),
        )
        .await
    }

    pub async fn finish_gateway_activity(
        &self,
        activity_id: &str,
        owner_id: &str,
        generation: i64,
        status: GatewayActivityTerminalStatus,
    ) -> Result<bool> {
        let now = now_ms();
        self.gateway_activity_update(
            sqlx::query(
                r#"
                UPDATE gateway_activities
                SET status = ?4,
                    updated_at_ms = ?5,
                    lease_expires_at_ms = ?5,
                    queued_turns = 0
                WHERE activity_id = ?1 AND owner_id = ?2 AND generation = ?3
                  AND status IN ('running', 'queued')
                "#,
            )
            .bind(activity_id)
            .bind(owner_id)
            .bind(generation)
            .bind(status.as_str())
            .bind(now),
        )
        .await
    }

    async fn gateway_activity_optional(
        &self,
        where_clause: &'static str,
        arguments: sqlx::sqlite::SqliteArguments,
    ) -> Result<Option<GatewayActivityRecord>> {
        let sql = gateway_activity_select_sql(where_clause);
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query_with(sqlx::AssertSqlSafe(sql), arguments)
                .fetch_optional(&mut *conn)
                .await?
                .map(|row| gateway_activity_from_row(&row))
                .transpose()
        })
        .await
    }

    async fn gateway_activity_update<'q>(
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

fn corrupt_turn_start_receipts(thread_id: &str) -> Error {
    Error::structured(
        "Persisted Thread Turn-start receipts are invalid.",
        serde_json::json!({
            "kind": "corrupt_thread_turn_start_receipts",
            "threadId": thread_id,
        }),
    )
}

pub(super) async fn record_gateway_turn_start_receipt_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    client_turn_id: &str,
    turn_id: &str,
) -> Result<()> {
    let metadata_json =
        sqlx::query_scalar::<_, Option<String>>("SELECT metadata_json FROM sessions WHERE id = ?1")
            .bind(thread_id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
    let mut metadata = metadata_json
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()?
        .and_then(|value| value.as_object().cloned())
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
    sqlx::query("UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3")
        .bind(Value::Object(metadata).to_string())
        .bind(now_ms())
        .bind(thread_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
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

fn gateway_activity_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<GatewayActivityRecord> {
    let intent_json: Option<String> = row.try_get(14)?;
    let queued_turns: i64 = row.try_get(12)?;
    let kind: String = row.try_get(4)?;
    let status: String = row.try_get(5)?;
    Ok(GatewayActivityRecord {
        activity_id: row.try_get(0)?,
        thread_id: row.try_get(1)?,
        source_key: row.try_get(2)?,
        turn_id: row.try_get(3)?,
        kind: GatewayActivityKind::parse(&kind)
            .ok_or_else(|| invalid_persisted_domain_value("gateway_activities", "kind", &kind))?,
        status: GatewayActivityState::parse(&status).ok_or_else(|| {
            invalid_persisted_domain_value("gateway_activities", "status", &status)
        })?,
        owner_id: row.try_get(6)?,
        owner_surface: row.try_get(7)?,
        generation: row.try_get(8)?,
        started_at_ms: row.try_get(9)?,
        updated_at_ms: row.try_get(10)?,
        lease_expires_at_ms: row.try_get(11)?,
        queued_turns: queued_turns.max(0) as usize,
        superseded_activity_id: row.try_get(13)?,
        intent: intent_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
    })
}
