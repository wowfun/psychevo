use psychevo_agent_core::now_ms;
use serde_json::Value;
use sqlx::Row;

use crate::error::{Error, Result};

use super::{
    GatewayLiveEventCommit, GatewayLiveEventRecord, GatewayLiveSnapshotInput,
    GatewayLiveSnapshotRecord, StateRuntime,
};

impl StateRuntime {
    pub async fn append_gateway_live_event(
        &self,
        activity_id: Option<&str>,
        owner_id: Option<&str>,
        thread_id: Option<&str>,
        turn_id: Option<&str>,
        idempotency_key: Option<&str>,
        event: &Value,
    ) -> Result<GatewayLiveEventCommit> {
        let event_json = serde_json::to_string(event)?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let insert = sqlx::query(
                r#"
                INSERT INTO gateway_live_events (
                    activity_id, owner_id, thread_id, turn_id, idempotency_key,
                    event_json, created_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(activity_id)
            .bind(owner_id)
            .bind(thread_id)
            .bind(turn_id)
            .bind(idempotency_key)
            .bind(&event_json)
            .bind(now_ms())
            .execute(&mut *tx)
            .await?;
            let inserted = insert.rows_affected() == 1;
            let seq = if inserted {
                insert.last_insert_rowid()
            } else {
                let Some(idempotency_key) = idempotency_key else {
                    return Err(Error::Message(
                        "gateway live event append conflicted without an idempotency key"
                            .to_string(),
                    ));
                };
                let row = sqlx::query(
                    r#"
                    SELECT seq, activity_id, owner_id, thread_id, turn_id, event_json
                    FROM gateway_live_events
                    WHERE idempotency_key = ?1
                    "#,
                )
                .bind(idempotency_key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| {
                    Error::Message(format!(
                        "gateway live event idempotency conflict has no committed row: {idempotency_key}"
                    ))
                })?;
                let replay_matches = row.try_get::<Option<String>, _>(1)?.as_deref()
                    == activity_id
                    && row.try_get::<Option<String>, _>(2)?.as_deref() == owner_id
                    && row.try_get::<Option<String>, _>(3)?.as_deref() == thread_id
                    && row.try_get::<Option<String>, _>(4)?.as_deref() == turn_id
                    && row.try_get::<String, _>(5)? == event_json;
                if !replay_matches {
                    return Err(Error::Message(format!(
                        "gateway live event idempotency key was reused for a different envelope: {idempotency_key}"
                    )));
                }
                row.try_get(0)?
            };
            tx.commit().await?;
            Ok(GatewayLiveEventCommit {
                seq,
                idempotency_key: idempotency_key.map(str::to_string),
                inserted,
            })
        })
        .await
    }

    pub async fn latest_gateway_live_event_seq(&self) -> Result<i64> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM gateway_live_events")
                .fetch_one(&mut *conn)
                .await
                .map_err(Into::into)
        })
        .await
    }

    pub async fn list_gateway_live_events_after(
        &self,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<GatewayLiveEventRecord>> {
        let limit = limit.clamp(1, 500) as i64;
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
                SELECT seq, activity_id, owner_id, thread_id, turn_id, idempotency_key,
                       event_json, created_at_ms
                FROM gateway_live_events
                WHERE seq > ?1
                ORDER BY seq ASC
                LIMIT ?2
                "#,
            )
            .bind(after_seq)
            .bind(limit)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| gateway_live_event_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn cleanup_gateway_live_events_before(&self, before_ms: i64) -> Result<usize> {
        self.live_state_delete(
            sqlx::query("DELETE FROM gateway_live_events WHERE created_at_ms < ?1").bind(before_ms),
        )
        .await
    }

    pub async fn delete_gateway_live_events_for_activity(
        &self,
        activity_id: &str,
    ) -> Result<usize> {
        self.live_state_delete(
            sqlx::query("DELETE FROM gateway_live_events WHERE activity_id = ?1").bind(activity_id),
        )
        .await
    }

    pub async fn upsert_gateway_live_snapshot(
        &self,
        input: GatewayLiveSnapshotInput<'_>,
    ) -> Result<i64> {
        let inputs = [input];
        let mut revisions = self.upsert_gateway_live_snapshots(&inputs).await?;
        revisions.pop().ok_or_else(|| {
            Error::Message("gateway live snapshot batch returned no revision".to_string())
        })
    }

    pub async fn upsert_gateway_live_snapshots(
        &self,
        inputs: &[GatewayLiveSnapshotInput<'_>],
    ) -> Result<Vec<i64>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let now = now_ms();
        let event_json = inputs
            .iter()
            .map(|input| serde_json::to_string(&input.event))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let mut revisions = Vec::with_capacity(inputs.len());
            for (input, event_json) in inputs.iter().zip(event_json) {
                sqlx::query(
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
                    WHERE gateway_live_snapshots.activity_id IS NOT excluded.activity_id
                       OR gateway_live_snapshots.owner_id IS NOT excluded.owner_id
                       OR gateway_live_snapshots.thread_id IS NOT excluded.thread_id
                       OR gateway_live_snapshots.turn_id IS NOT excluded.turn_id
                       OR gateway_live_snapshots.event_kind IS NOT excluded.event_kind
                       OR gateway_live_snapshots.event_json IS NOT excluded.event_json
                    "#,
                )
                .bind(input.snapshot_key)
                .bind(input.activity_id)
                .bind(input.owner_id)
                .bind(input.thread_id)
                .bind(input.turn_id)
                .bind(input.event_kind)
                .bind(event_json)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                revisions.push(
                    sqlx::query_scalar::<_, i64>(
                        "SELECT revision FROM gateway_live_snapshots WHERE snapshot_key = ?1",
                    )
                    .bind(input.snapshot_key)
                    .fetch_one(&mut *tx)
                    .await?,
                );
            }
            tx.commit().await?;
            Ok(revisions)
        })
        .await
    }

    pub async fn list_gateway_live_snapshots(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayLiveSnapshotRecord>> {
        self.list_gateway_live_snapshots_query(
            sqlx::query(
                r#"
                SELECT snapshot_key, activity_id, owner_id, thread_id, turn_id,
                       event_kind, event_json, revision, created_at_ms, updated_at_ms
                FROM gateway_live_snapshots
                ORDER BY updated_at_ms ASC, snapshot_key ASC
                LIMIT ?1
                "#,
            )
            .bind(limit.clamp(1, 1000) as i64),
        )
        .await
    }

    pub async fn list_gateway_live_snapshots_for_thread(
        &self,
        thread_id: &str,
        turn_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GatewayLiveSnapshotRecord>> {
        self.list_gateway_live_snapshots_query(
            sqlx::query(
                r#"
                SELECT snapshot_key, activity_id, owner_id, thread_id, turn_id,
                       event_kind, event_json, revision, created_at_ms, updated_at_ms
                FROM gateway_live_snapshots
                WHERE thread_id = ?1
                  AND (?2 IS NULL OR turn_id = ?2)
                ORDER BY updated_at_ms ASC, snapshot_key ASC
                LIMIT ?3
                "#,
            )
            .bind(thread_id)
            .bind(turn_id)
            .bind(limit.clamp(1, 1000) as i64),
        )
        .await
    }

    pub async fn delete_gateway_live_snapshots_for_activity(
        &self,
        activity_id: &str,
    ) -> Result<usize> {
        self.live_state_delete(
            sqlx::query("DELETE FROM gateway_live_snapshots WHERE activity_id = ?1")
                .bind(activity_id),
        )
        .await
    }

    pub async fn cleanup_gateway_live_snapshots_before(&self, before_ms: i64) -> Result<usize> {
        self.live_state_delete(
            sqlx::query("DELETE FROM gateway_live_snapshots WHERE updated_at_ms < ?1")
                .bind(before_ms),
        )
        .await
    }

    async fn list_gateway_live_snapshots_query<'q>(
        &self,
        query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    ) -> Result<Vec<GatewayLiveSnapshotRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = query.fetch_all(&mut *conn).await?;
            rows.into_iter()
                .map(|row| gateway_live_snapshot_from_row(&row))
                .collect()
        })
        .await
    }

    async fn live_state_delete<'q>(
        &self,
        query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    ) -> Result<usize> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = query.execute(&mut *tx).await?.rows_affected() as usize;
            tx.commit().await?;
            Ok(changed)
        })
        .await
    }
}

fn gateway_live_event_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<GatewayLiveEventRecord> {
    let event_json: String = row.try_get(6)?;
    Ok(GatewayLiveEventRecord {
        seq: row.try_get(0)?,
        activity_id: row.try_get(1)?,
        owner_id: row.try_get(2)?,
        thread_id: row.try_get(3)?,
        turn_id: row.try_get(4)?,
        idempotency_key: row.try_get(5)?,
        event: serde_json::from_str(&event_json)?,
        created_at_ms: row.try_get(7)?,
    })
}

fn gateway_live_snapshot_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GatewayLiveSnapshotRecord> {
    let event_json: String = row.try_get(6)?;
    Ok(GatewayLiveSnapshotRecord {
        snapshot_key: row.try_get(0)?,
        activity_id: row.try_get(1)?,
        owner_id: row.try_get(2)?,
        thread_id: row.try_get(3)?,
        turn_id: row.try_get(4)?,
        event_kind: row.try_get(5)?,
        event: serde_json::from_str(&event_json)?,
        revision: row.try_get(7)?,
        created_at_ms: row.try_get(8)?,
        updated_at_ms: row.try_get(9)?,
    })
}
