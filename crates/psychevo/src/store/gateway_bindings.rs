use std::collections::BTreeMap;

use psychevo_agent_core::now_ms;
use serde_json::Value;
use sqlx::Row;

use crate::error::{Error, Result};

use super::{
    GatewaySourceBindingInput, GatewaySourceBindingRecord, GatewaySourceLaneInput,
    GatewaySourceLaneRecord, StateRuntime,
};

impl StateRuntime {
    pub async fn upsert_gateway_source_binding(
        &self,
        input: GatewaySourceBindingInput<'_>,
    ) -> Result<GatewaySourceBindingRecord> {
        self.resume_session(input.thread_id).await?;
        let now = now_ms();
        let raw_identity_json = serde_json::to_string(&input.raw_identity)?;
        let lineage_json = input
            .lineage
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            sqlx::query(
                r#"
                INSERT INTO gateway_source_bindings (
                    source_key, source_kind, raw_identity_json, visible_name,
                    thread_id, backend_kind, backend_native_id, draft_agent_ref, draft_profile_ref,
                    draft_control_values_json, created_at_ms, updated_at_ms, lineage_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL, ?8, ?8, ?9)
                ON CONFLICT(source_key) DO UPDATE SET
                    source_kind = excluded.source_kind,
                    raw_identity_json = excluded.raw_identity_json,
                    visible_name = excluded.visible_name,
                    thread_id = excluded.thread_id,
                    backend_kind = excluded.backend_kind,
                    backend_native_id = excluded.backend_native_id,
                    draft_agent_ref = NULL,
                    draft_profile_ref = NULL,
                    draft_control_values_json = NULL,
                    updated_at_ms = excluded.updated_at_ms,
                    lineage_json = excluded.lineage_json
                "#,
            )
            .bind(input.source_key)
            .bind(input.source_kind)
            .bind(raw_identity_json)
            .bind(input.visible_name)
            .bind(input.thread_id)
            .bind(input.backend_kind)
            .bind(input.backend_native_id)
            .bind(now)
            .bind(lineage_json)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(())
        })
        .await?;
        self.gateway_source_binding(input.source_key)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "gateway source binding not found: {}",
                    input.source_key
                ))
            })
    }

    pub async fn gateway_source_binding(
        &self,
        source_key: &str,
    ) -> Result<Option<GatewaySourceBindingRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
            SELECT source_key, source_kind, raw_identity_json, visible_name,
                   thread_id, COALESCE(backend_kind, 'unresolved'),
                   backend_native_id, draft_agent_ref, draft_profile_ref, draft_control_values_json,
                   created_at_ms, updated_at_ms, lineage_json
            FROM gateway_source_bindings
            WHERE source_key = ?1 AND thread_id IS NOT NULL
            "#,
            )
            .bind(source_key)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| gateway_source_binding_from_row(&row))
            .transpose()
        })
        .await
    }

    pub async fn gateway_source_bindings_for_connection_id(
        &self,
        connection_id: &str,
    ) -> Result<Vec<GatewaySourceBindingRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT source_key, source_kind, raw_identity_json, visible_name,
                   thread_id, COALESCE(backend_kind, 'unresolved'),
                   backend_native_id, draft_agent_ref, draft_profile_ref, draft_control_values_json,
                   created_at_ms, updated_at_ms, lineage_json
            FROM gateway_source_bindings
            WHERE source_kind LIKE 'im.%' AND thread_id IS NOT NULL
            ORDER BY updated_at_ms DESC
            "#,
            )
            .fetch_all(&mut *conn)
            .await?;
            let mut bindings = Vec::new();
            for row in rows {
                let binding = gateway_source_binding_from_row(&row)?;
                if binding
                    .raw_identity
                    .get("connectionId")
                    .and_then(Value::as_str)
                    == Some(connection_id)
                {
                    bindings.push(binding);
                }
            }
            Ok(bindings)
        })
        .await
    }

    pub async fn gateway_source_bindings_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<GatewaySourceBindingRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT source_key, source_kind, raw_identity_json, visible_name,
                   thread_id, COALESCE(backend_kind, 'unresolved'),
                   backend_native_id, draft_agent_ref, draft_profile_ref, draft_control_values_json,
                   created_at_ms, updated_at_ms, lineage_json
            FROM gateway_source_bindings
            WHERE thread_id = ?1
            ORDER BY updated_at_ms DESC, source_key ASC
            "#,
            )
            .bind(thread_id)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| gateway_source_binding_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn upsert_gateway_source_lane(
        &self,
        input: GatewaySourceLaneInput<'_>,
    ) -> Result<GatewaySourceLaneRecord> {
        if let Some(thread_id) = input.thread_id {
            self.resume_session(thread_id).await?;
        }
        let now = now_ms();
        let raw_identity_json = serde_json::to_string(&input.raw_identity)?;
        let lineage_json = input
            .lineage
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let draft_profile_ref = input
            .draft_profile_ref
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let draft_agent_ref = input
            .draft_agent_ref
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let draft_control_values_json = (!input.draft_control_values.is_empty())
            .then(|| serde_json::to_string(input.draft_control_values))
            .transpose()?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            sqlx::query(
                r#"
                INSERT INTO gateway_source_bindings (
                    source_key, source_kind, raw_identity_json, visible_name,
                    thread_id, backend_kind, backend_native_id, draft_agent_ref, draft_profile_ref,
                    draft_control_values_json, created_at_ms, updated_at_ms, lineage_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, ?7, ?8, ?9, ?9, ?10)
                ON CONFLICT(source_key) DO UPDATE SET
                    source_kind = excluded.source_kind,
                    raw_identity_json = excluded.raw_identity_json,
                    visible_name = excluded.visible_name,
                    thread_id = excluded.thread_id,
                    backend_kind = NULL,
                    backend_native_id = NULL,
                    draft_agent_ref = excluded.draft_agent_ref,
                    draft_profile_ref = excluded.draft_profile_ref,
                    draft_control_values_json = excluded.draft_control_values_json,
                    updated_at_ms = excluded.updated_at_ms,
                    lineage_json = excluded.lineage_json
                "#,
            )
            .bind(input.source_key)
            .bind(input.source_kind)
            .bind(raw_identity_json)
            .bind(input.visible_name)
            .bind(input.thread_id)
            .bind(draft_agent_ref)
            .bind(draft_profile_ref)
            .bind(draft_control_values_json)
            .bind(now)
            .bind(lineage_json)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(())
        })
        .await?;
        self.gateway_source_lane(input.source_key)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "gateway source lane not found after write: {}",
                    input.source_key
                ))
            })
    }

    pub async fn gateway_source_lane(
        &self,
        source_key: &str,
    ) -> Result<Option<GatewaySourceLaneRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(
                r#"
            SELECT source_key, source_kind, raw_identity_json, visible_name,
                   thread_id, draft_agent_ref, draft_profile_ref, created_at_ms, updated_at_ms,
                   draft_control_values_json, lineage_json
            FROM gateway_source_bindings
            WHERE source_key = ?1
            "#,
            )
            .bind(source_key)
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| gateway_source_lane_from_row(&row))
            .transpose()
        })
        .await
    }

    pub async fn clear_gateway_source_lane_thread(&self, source_key: &str) -> Result<bool> {
        let now = now_ms();
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = sqlx::query(
                r#"
                UPDATE gateway_source_bindings
                SET thread_id = NULL,
                    backend_kind = NULL,
                    backend_native_id = NULL,
                    updated_at_ms = ?2
                WHERE source_key = ?1 AND thread_id IS NOT NULL
                "#,
            )
            .bind(source_key)
            .bind(now)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            tx.commit().await?;
            Ok(changed > 0)
        })
        .await
    }

    pub async fn delete_gateway_source_binding(&self, source_key: &str) -> Result<bool> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = sqlx::query("DELETE FROM gateway_source_bindings WHERE source_key = ?1")
                .bind(source_key)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            tx.commit().await?;
            Ok(changed > 0)
        })
        .await
    }

    pub async fn mark_session_ended_with_reason(
        &self,
        session_id: &str,
        reason: &str,
    ) -> Result<()> {
        let now = now_ms();
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = sqlx::query(
                "UPDATE sessions SET updated_at_ms = ?1, ended_at_ms = ?1, end_reason = ?2 WHERE id = ?3",
            )
            .bind(now)
            .bind(reason)
            .bind(session_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed == 0 {
                return Err(Error::Message(format!("session not found: {session_id}")));
            }
            tx.commit().await?;
            Ok(())
        })
        .await
    }
}

fn gateway_source_binding_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GatewaySourceBindingRecord> {
    let raw_identity_json: String = row.try_get(2)?;
    let draft_control_values_json: Option<String> = row.try_get(9)?;
    let lineage_json: Option<String> = row.try_get(12)?;
    let raw_identity = serde_json::from_str(&raw_identity_json)?;
    let lineage = lineage_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    Ok(GatewaySourceBindingRecord {
        source_key: row.try_get(0)?,
        source_kind: row.try_get(1)?,
        raw_identity,
        visible_name: row.try_get(3)?,
        thread_id: row.try_get(4)?,
        backend_kind: row.try_get(5)?,
        backend_native_id: row.try_get(6)?,
        draft_agent_ref: row.try_get(7)?,
        draft_profile_ref: row.try_get(8)?,
        draft_control_values: decode_draft_control_values(draft_control_values_json.as_deref())?,
        created_at_ms: row.try_get(10)?,
        updated_at_ms: row.try_get(11)?,
        lineage,
    })
}

fn gateway_source_lane_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<GatewaySourceLaneRecord> {
    let raw_identity_json: String = row.try_get(2)?;
    let draft_control_values_json: Option<String> = row.try_get(9)?;
    let lineage_json: Option<String> = row.try_get(10)?;
    let raw_identity = serde_json::from_str(&raw_identity_json)?;
    let lineage = lineage_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    Ok(GatewaySourceLaneRecord {
        source_key: row.try_get(0)?,
        source_kind: row.try_get(1)?,
        raw_identity,
        visible_name: row.try_get(3)?,
        thread_id: row.try_get(4)?,
        draft_agent_ref: row.try_get(5)?,
        draft_profile_ref: row.try_get(6)?,
        draft_control_values: decode_draft_control_values(draft_control_values_json.as_deref())?,
        created_at_ms: row.try_get(7)?,
        updated_at_ms: row.try_get(8)?,
        lineage,
    })
}

fn decode_draft_control_values(value: Option<&str>) -> Result<BTreeMap<String, String>> {
    value
        .map(serde_json::from_str)
        .transpose()
        .map(Option::unwrap_or_default)
        .map_err(Into::into)
}
