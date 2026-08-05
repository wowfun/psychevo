use std::collections::BTreeMap;

use psychevo_agent_core::now_ms;
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};

use crate::error::{Error, Result};

use super::{
    ExistingFrameworkThreadTurnInput, GatewayChannelOutboxInput, GatewayChannelOutboxRecord,
    GatewayChannelOutboxStatus, GatewayRuntimeBindingInput, GatewayTurnDeliveryInput,
    GatewayTurnDeliveryRecord, GatewayTurnDeliveryStatus, NewFrameworkThreadTurnInput,
    StateRuntime, invalid_persisted_domain_value,
    store_agents::insert_agent_mission_registration_in_tx,
    store_gateway_activity::record_gateway_turn_start_receipt_in_tx,
    store_runtime_bindings::validate_runtime_binding_input,
};

impl StateRuntime {
    pub(crate) async fn accept_new_framework_thread_turn(
        &self,
        input: NewFrameworkThreadTurnInput<'_>,
    ) -> Result<()> {
        if input.delivery.thread_id != input.thread_id {
            return Err(Error::Message(
                "new Thread delivery identity does not match the Thread".to_string(),
            ));
        }
        if let Some(source) = input.source_lane.as_ref()
            && source.thread_id != Some(input.thread_id)
        {
            return Err(Error::Message(
                "new Thread source association does not match the Thread".to_string(),
            ));
        }
        if let Some(binding) = input.runtime_binding.as_ref() {
            validate_runtime_binding_input(binding)?;
            if binding.thread_id != input.thread_id {
                return Err(Error::Message(
                    "new Thread runtime binding does not match the Thread".to_string(),
                ));
            }
        } else if !input.initial_thread_preferences.is_empty() {
            return Err(Error::Message(
                "initial Thread preferences require a runtime binding".to_string(),
            ));
        }

        #[cfg(test)]
        {
            let barrier = self
                .inner
                .gateway_turn_acceptance_barrier
                .lock()
                .expect("Gateway Turn acceptance barrier poisoned")
                .take();
            if let Some((entered, release)) = barrier {
                entered.notify_one();
                release.notified().await;
            }
        }

        let now = now_ms();
        let cwd = input.cwd.to_string_lossy().into_owned();
        let metadata_json = input
            .metadata
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        let preferences_json = (!input.initial_thread_preferences.is_empty())
            .then(|| serde_json::to_string(input.initial_thread_preferences))
            .transpose()?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            sqlx::query(
                r#"
                INSERT INTO sessions (
                    id, source, parent_session_id, cwd, model, provider,
                    started_at_ms, updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                    message_count, tool_call_count, title, metadata_json
                ) VALUES (?1, ?2, NULL, ?3, 'pending', 'pending',
                    ?4, ?4, NULL, NULL, NULL, 0, 0, NULL, ?5)
                "#,
            )
            .bind(input.thread_id)
            .bind(input.source)
            .bind(&cwd)
            .bind(now)
            .bind(metadata_json)
            .execute(&mut *tx)
            .await?;

            if let Some(binding) = input.runtime_binding {
                insert_initial_runtime_binding_in_tx(&mut tx, binding, preferences_json, now)
                    .await?;
            }

            if let Some(source) = input.source_lane {
                let raw_identity_json = serde_json::to_string(&source.raw_identity)?;
                let lineage_json = source
                    .lineage
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?;
                sqlx::query(
                    r#"
                    INSERT INTO gateway_source_bindings (
                        source_key, source_kind, raw_identity_json, visible_name,
                        thread_id, backend_kind, backend_native_id, draft_agent_ref,
                        draft_profile_ref, draft_control_values_json, created_at_ms,
                        updated_at_ms, lineage_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, NULL, NULL, ?6, ?6, ?7)
                    ON CONFLICT(source_key) DO UPDATE SET
                        source_kind = excluded.source_kind,
                        raw_identity_json = excluded.raw_identity_json,
                        visible_name = excluded.visible_name,
                        thread_id = excluded.thread_id,
                        backend_kind = NULL,
                        backend_native_id = NULL,
                        draft_agent_ref = NULL,
                        draft_profile_ref = NULL,
                        draft_control_values_json = NULL,
                        updated_at_ms = excluded.updated_at_ms,
                        lineage_json = excluded.lineage_json
                    "#,
                )
                .bind(source.source_key)
                .bind(source.source_kind)
                .bind(raw_identity_json)
                .bind(source.visible_name)
                .bind(source.thread_id)
                .bind(now)
                .bind(lineage_json)
                .execute(&mut *tx)
                .await?;
            }

            if let Some(mission) = input.mission.as_ref() {
                insert_agent_mission_registration_in_tx(&mut tx, input.thread_id, mission, now)
                    .await?;
            }

            sqlx::query(
                r#"
                INSERT INTO gateway_turn_deliveries (
                    turn_id, thread_id, runtime_ref, status, input_json,
                    input_hash, created_at_ms, updated_at_ms,
                    delivery_confirmed_at_ms, terminal_at_ms
                ) VALUES (?1, ?2, ?3, 'not_delivered', ?4, ?5, ?6, ?6, NULL, NULL)
                "#,
            )
            .bind(input.delivery.turn_id)
            .bind(input.delivery.thread_id)
            .bind(input.delivery.runtime_ref)
            .bind(input.delivery.input_json)
            .bind(input.delivery.input_hash)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            if let Some(client_turn_id) = input.client_turn_id {
                record_gateway_turn_start_receipt_in_tx(
                    &mut tx,
                    input.thread_id,
                    client_turn_id,
                    input.delivery.turn_id,
                )
                .await?;
            }
            tx.commit().await?;
            Ok(())
        })
        .await
    }

    #[cfg(test)]
    pub(crate) fn set_gateway_turn_acceptance_barrier_for_test(
        &self,
        entered: std::sync::Arc<tokio::sync::Notify>,
        release: std::sync::Arc<tokio::sync::Notify>,
    ) {
        *self
            .inner
            .gateway_turn_acceptance_barrier
            .lock()
            .expect("Gateway Turn acceptance barrier poisoned") = Some((entered, release));
    }

    pub async fn insert_gateway_turn_delivery(
        &self,
        input: GatewayTurnDeliveryInput<'_>,
    ) -> Result<GatewayTurnDeliveryRecord> {
        let turn_id = input.turn_id.to_string();
        let empty_preferences = BTreeMap::new();
        self.accept_framework_turn(ExistingFrameworkThreadTurnInput {
            delivery: input,
            client_turn_id: None,
            runtime_binding: None,
            initial_thread_preferences: &empty_preferences,
            mission: None,
        })
        .await?;
        self.gateway_turn_delivery(&turn_id).await?.ok_or_else(|| {
            Error::Message(format!("turn delivery not found after insert: {turn_id}"))
        })
    }

    pub(crate) async fn accept_framework_turn(
        &self,
        input: ExistingFrameworkThreadTurnInput<'_>,
    ) -> Result<()> {
        if let Some(binding) = input.runtime_binding.as_ref() {
            validate_runtime_binding_input(binding)?;
            if binding.thread_id != input.delivery.thread_id {
                return Err(Error::Message(
                    "runtime binding does not match the accepted Turn's Thread".to_string(),
                ));
            }
        } else if !input.initial_thread_preferences.is_empty() {
            return Err(Error::Message(
                "initial Thread preferences require a runtime binding".to_string(),
            ));
        }
        #[cfg(test)]
        {
            let barrier = self
                .inner
                .gateway_turn_acceptance_barrier
                .lock()
                .expect("Gateway Turn acceptance barrier poisoned")
                .take();
            if let Some((entered, release)) = barrier {
                entered.notify_one();
                release.notified().await;
            }
        }
        let now = now_ms();
        let preferences_json = (!input.initial_thread_preferences.is_empty())
            .then(|| serde_json::to_string(input.initial_thread_preferences))
            .transpose()?;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            if let Some(binding) = input.runtime_binding {
                insert_initial_runtime_binding_in_tx(&mut tx, binding, preferences_json, now)
                    .await?;
            }
            if let Some(mission) = input.mission.as_ref() {
                insert_agent_mission_registration_in_tx(
                    &mut tx,
                    input.delivery.thread_id,
                    mission,
                    now,
                )
                .await?;
            }
            sqlx::query(
                r#"
                INSERT INTO gateway_turn_deliveries (
                    turn_id, thread_id, runtime_ref, status, input_json,
                    input_hash, created_at_ms, updated_at_ms,
                    delivery_confirmed_at_ms, terminal_at_ms
                ) VALUES (?1, ?2, ?3, 'not_delivered', ?4, ?5, ?6, ?6, NULL, NULL)
                "#,
            )
            .bind(input.delivery.turn_id)
            .bind(input.delivery.thread_id)
            .bind(input.delivery.runtime_ref)
            .bind(input.delivery.input_json)
            .bind(input.delivery.input_hash)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            if let Some(client_turn_id) = input.client_turn_id {
                record_gateway_turn_start_receipt_in_tx(
                    &mut tx,
                    input.delivery.thread_id,
                    client_turn_id,
                    input.delivery.turn_id,
                )
                .await?;
            }
            tx.commit().await?;
            Ok(())
        })
        .await
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
                    started_at_ms, completed_at_ms, boundary_session_seq, metadata_json
                ) VALUES (
                    ?1, ?2, 'completed', 'normal', NULL, NULL, ?3,
                    (SELECT COALESCE(MAX(session_seq), 0)
                     FROM messages WHERE session_id = ?2),
                    ?4
                )
                ON CONFLICT(turn_id) DO UPDATE SET
                    thread_id = excluded.thread_id,
                    status = 'completed',
                    outcome = 'normal',
                    error_message = NULL,
                    completed_at_ms = excluded.completed_at_ms,
                    boundary_session_seq = excluded.boundary_session_seq,
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
            Ok(changed > 0)
        })
        .await
    }
}

async fn insert_initial_runtime_binding_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    binding: GatewayRuntimeBindingInput<'_>,
    preferences_json: Option<String>,
    now: i64,
) -> Result<()> {
    let inserted = sqlx::query(
        r#"
        INSERT INTO gateway_runtime_bindings (
            thread_id, resolution_status, agent_ref, agent_fingerprint,
            agent_definition_json, runtime_ref, backend_kind, native_kind,
            native_session_id, cwd, profile_fingerprint, profile_revision,
            profile_config_json, adapter_kind, adapter_revision, ownership,
            parent_thread_id, binding_revision, thread_preferences_json,
            runtime_observed_json, control_revision, unresolved_reason,
            created_at_ms, updated_at_ms
        ) VALUES (
            ?1, 'resolved', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, ?14, ?15, ?16, 1, ?17, NULL, 1, NULL,
            ?18, ?18
        )
        ON CONFLICT(thread_id) DO NOTHING
        "#,
    )
    .bind(binding.thread_id)
    .bind(binding.agent_ref)
    .bind(binding.agent_fingerprint)
    .bind(binding.agent_definition_json)
    .bind(binding.runtime_ref)
    .bind(binding.backend_kind)
    .bind(binding.native_kind)
    .bind(binding.native_session_id)
    .bind(binding.cwd)
    .bind(binding.profile_fingerprint)
    .bind(binding.profile_revision)
    .bind(binding.profile_config_json)
    .bind(binding.adapter_kind)
    .bind(binding.adapter_revision)
    .bind(binding.ownership.as_str())
    .bind(binding.parent_thread_id)
    .bind(preferences_json.as_deref())
    .bind(now)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        let matches: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM gateway_runtime_bindings
                WHERE thread_id = ?1
                  AND resolution_status = 'resolved'
                  AND agent_ref IS ?2
                  AND agent_fingerprint IS ?3
                  AND agent_definition_json IS ?4
                  AND runtime_ref IS ?5
                  AND backend_kind IS ?6
                  AND native_kind IS ?7
                  AND native_session_id IS ?8
                  AND cwd IS ?9
                  AND profile_fingerprint IS ?10
                  AND profile_revision IS ?11
                  AND profile_config_json IS ?12
                  AND adapter_kind IS ?13
                  AND adapter_revision IS ?14
                  AND ownership IS ?15
                  AND parent_thread_id IS ?16
                  AND thread_preferences_json IS ?17
            )
            "#,
        )
        .bind(binding.thread_id)
        .bind(binding.agent_ref)
        .bind(binding.agent_fingerprint)
        .bind(binding.agent_definition_json)
        .bind(binding.runtime_ref)
        .bind(binding.backend_kind)
        .bind(binding.native_kind)
        .bind(binding.native_session_id)
        .bind(binding.cwd)
        .bind(binding.profile_fingerprint)
        .bind(binding.profile_revision)
        .bind(binding.profile_config_json)
        .bind(binding.adapter_kind)
        .bind(binding.adapter_revision)
        .bind(binding.ownership.as_str())
        .bind(binding.parent_thread_id)
        .bind(preferences_json.as_deref())
        .fetch_one(&mut **tx)
        .await?;
        if !matches {
            return Err(Error::Message(format!(
                "runtime binding conflict for thread `{}`: bindings and initial preferences are immutable",
                binding.thread_id
            )));
        }
    }
    Ok(())
}

fn gateway_turn_delivery_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GatewayTurnDeliveryRecord> {
    let status: String = row.try_get(3)?;
    Ok(GatewayTurnDeliveryRecord {
        turn_id: row.try_get(0)?,
        thread_id: row.try_get(1)?,
        runtime_ref: row.try_get(2)?,
        status: GatewayTurnDeliveryStatus::parse(&status).ok_or_else(|| {
            invalid_persisted_domain_value("gateway_turn_deliveries", "status", &status)
        })?,
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
    let status: String = row.try_get(5)?;
    Ok(GatewayChannelOutboxRecord {
        delivery_id: row.try_get(0)?,
        thread_id: row.try_get(1)?,
        turn_id: row.try_get(2)?,
        connection_id: row.try_get(3)?,
        source_key: row.try_get(4)?,
        status: GatewayChannelOutboxStatus::parse(&status).ok_or_else(|| {
            invalid_persisted_domain_value("gateway_channel_outbox", "status", &status)
        })?,
        payload_text: row.try_get(6)?,
        payload_hash: row.try_get(7)?,
        created_at_ms: row.try_get(8)?,
        updated_at_ms: row.try_get(9)?,
        acknowledged_at_ms: row.try_get(10)?,
    })
}

pub(super) async fn scrub_gateway_activity_turn_input(
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
