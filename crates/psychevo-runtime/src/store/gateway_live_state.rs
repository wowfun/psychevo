use psychevo_agent_core::now_ms;
use serde_json::Value;
use sqlx::Row;

use crate::error::Result;

use super::{
    GatewayLiveEventRecord, GatewayLiveSnapshotInput, GatewayLiveSnapshotRecord, StateRuntime,
};

impl StateRuntime {
    pub async fn append_gateway_live_event(
        &self,
        activity_id: Option<&str>,
        owner_id: Option<&str>,
        thread_id: Option<&str>,
        turn_id: Option<&str>,
        event: &Value,
    ) -> Result<i64> {
        let event_json = serde_json::to_string(event)?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let seq = sqlx::query(
                r#"
                INSERT INTO gateway_live_events (
                    activity_id, owner_id, thread_id, turn_id, event_json, created_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
            )
            .bind(activity_id)
            .bind(owner_id)
            .bind(thread_id)
            .bind(turn_id)
            .bind(event_json)
            .bind(now_ms())
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(seq)
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
                SELECT seq, activity_id, owner_id, thread_id, turn_id, event_json, created_at_ms
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
        let now = now_ms();
        let event_json = serde_json::to_string(&input.event)?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
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
            let revision = sqlx::query_scalar::<_, i64>(
                "SELECT revision FROM gateway_live_snapshots WHERE snapshot_key = ?1",
            )
            .bind(input.snapshot_key)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(revision)
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
            self.finish_sqlx_write().await;
            Ok(changed)
        })
        .await
    }
}

fn gateway_live_event_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<GatewayLiveEventRecord> {
    let event_json: String = row.try_get(5)?;
    Ok(GatewayLiveEventRecord {
        seq: row.try_get(0)?,
        activity_id: row.try_get(1)?,
        owner_id: row.try_get(2)?,
        thread_id: row.try_get(3)?,
        turn_id: row.try_get(4)?,
        event: serde_json::from_str(&event_json)?,
        created_at_ms: row.try_get(6)?,
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
