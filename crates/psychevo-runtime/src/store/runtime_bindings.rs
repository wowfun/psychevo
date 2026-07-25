use std::collections::BTreeMap;

use psychevo_agent_core::now_ms;
use serde_json::Value;
use sqlx::Row;

use crate::error::{Error, Result};

use super::{
    GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership, GatewayRuntimeBindingRecord,
    GatewayRuntimeBindingStatus, GatewayRuntimeControlStatePatch, StateRuntime,
};

impl StateRuntime {
    pub async fn create_gateway_runtime_binding(
        &self,
        input: GatewayRuntimeBindingInput<'_>,
    ) -> Result<GatewayRuntimeBindingRecord> {
        validate_runtime_binding_input(&input)?;
        validate_runtime_binding_threads(self, &input).await?;

        let now = now_ms();
        let inserted = self
            .runtime_binding_write(
                sqlx::query(
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
                    ?11, ?12, ?13, ?14, ?15, ?16, 1, NULL, NULL, 1, NULL,
                    ?17, ?17
                )
                ON CONFLICT(thread_id) DO NOTHING
                "#,
                )
                .bind(input.thread_id)
                .bind(input.agent_ref)
                .bind(input.agent_fingerprint)
                .bind(input.agent_definition_json)
                .bind(input.runtime_ref)
                .bind(input.backend_kind)
                .bind(input.native_kind)
                .bind(input.native_session_id)
                .bind(input.cwd)
                .bind(input.profile_fingerprint)
                .bind(input.profile_revision)
                .bind(input.profile_config_json)
                .bind(input.adapter_kind)
                .bind(input.adapter_revision)
                .bind(input.ownership.as_str())
                .bind(input.parent_thread_id)
                .bind(now),
            )
            .await?;

        let record = self
            .gateway_runtime_binding(input.thread_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "runtime binding not found after create: {}",
                    input.thread_id
                ))
            })?;
        if inserted == 0 && !runtime_binding_matches_input(&record, &input) {
            return Err(Error::Message(format!(
                "runtime binding conflict for thread `{}`: bindings are immutable",
                input.thread_id
            )));
        }
        Ok(record)
    }

    /// Creates a fresh Thread binding from a resolved parent snapshot.
    ///
    /// Immutable Agent/Profile identity is copied, while runtime session identity
    /// and adapter observations are intentionally reset. The caller supplies the
    /// parent's resolved live controls so the child requests the same effective
    /// values from its new runtime session.
    pub async fn create_gateway_runtime_binding_from_parent_snapshot(
        &self,
        parent_thread_id: &str,
        child_thread_id: &str,
        effective_controls: &BTreeMap<String, Value>,
    ) -> Result<GatewayRuntimeBindingRecord> {
        if parent_thread_id == child_thread_id {
            return Err(Error::Message(
                "runtime binding snapshot requires a distinct child Thread".to_string(),
            ));
        }
        let parent = self
            .gateway_runtime_binding(parent_thread_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "resolved runtime binding not found for parent Thread `{parent_thread_id}`"
                ))
            })?;
        if parent.status != GatewayRuntimeBindingStatus::Resolved {
            return Err(Error::Message(format!(
                "runtime binding for parent Thread `{parent_thread_id}` is unresolved"
            )));
        }
        let child = self
            .session_summary(child_thread_id)
            .await?
            .ok_or_else(|| Error::Message(format!("session not found: {child_thread_id}")))?;
        if child.cwd != parent.cwd {
            return Err(Error::Message(format!(
                "runtime binding snapshot cwd does not match child Thread `{child_thread_id}`"
            )));
        }

        validate_runtime_control_map("inherited preference", effective_controls)?;
        let inherited_preferences_json = (!effective_controls.is_empty())
            .then(|| serde_json::to_string(effective_controls))
            .transpose()?;
        let now = now_ms();
        let inserted = self
            .runtime_binding_write(
                sqlx::query(
                    r#"
                INSERT INTO gateway_runtime_bindings (
                    thread_id, resolution_status, agent_ref, agent_fingerprint,
                    agent_definition_json, runtime_ref, backend_kind, native_kind,
                    native_session_id, cwd, profile_fingerprint, profile_revision,
                    profile_config_json, adapter_kind, adapter_revision, ownership,
                    parent_thread_id, binding_revision, thread_preferences_json,
                    runtime_observed_json, control_revision, unresolved_reason,
                    created_at_ms, updated_at_ms
                )
                SELECT ?1, resolution_status, agent_ref, agent_fingerprint,
                       agent_definition_json, runtime_ref, backend_kind, native_kind,
                       NULL, cwd, profile_fingerprint, profile_revision,
                       profile_config_json, adapter_kind, adapter_revision, ownership,
                       NULL, 1, ?2, NULL, 1, NULL, ?3, ?3
                FROM gateway_runtime_bindings
                WHERE thread_id = ?4 AND resolution_status = 'resolved'
                ON CONFLICT(thread_id) DO NOTHING
                "#,
                )
                .bind(child_thread_id)
                .bind(inherited_preferences_json)
                .bind(now)
                .bind(parent_thread_id),
            )
            .await?;
        if inserted != 1 {
            return Err(Error::Message(format!(
                "runtime binding conflict for child Thread `{child_thread_id}`"
            )));
        }
        self.gateway_runtime_binding(child_thread_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "runtime binding not found after snapshot: {child_thread_id}"
                ))
            })
    }

    pub async fn resolve_gateway_runtime_binding(
        &self,
        input: GatewayRuntimeBindingInput<'_>,
        expected_binding_revision: i64,
    ) -> Result<GatewayRuntimeBindingRecord> {
        validate_runtime_binding_input(&input)?;
        validate_runtime_binding_threads(self, &input).await?;
        if expected_binding_revision < 1 {
            return Err(Error::Message(
                "expected binding revision must be positive".to_string(),
            ));
        }

        let now = now_ms();
        let changed = self
            .runtime_binding_write(
                sqlx::query(
                    r#"
                UPDATE gateway_runtime_bindings
                SET resolution_status = 'resolved',
                    agent_ref = ?2,
                    agent_fingerprint = ?3,
                    agent_definition_json = ?4,
                    runtime_ref = ?5,
                    backend_kind = ?6,
                    native_kind = ?7,
                    native_session_id = ?8,
                    cwd = ?9,
                    profile_fingerprint = ?10,
                    profile_revision = ?11,
                    profile_config_json = ?12,
                    adapter_kind = ?13,
                    adapter_revision = ?14,
                    ownership = ?15,
                    parent_thread_id = ?16,
                    binding_revision = binding_revision + 1,
                    unresolved_reason = NULL,
                    updated_at_ms = ?17
                WHERE thread_id = ?1
                  AND resolution_status = 'unresolved'
                  AND binding_revision = ?18
                  AND (agent_ref IS NULL OR agent_ref IS ?2)
                  AND (agent_fingerprint IS NULL OR agent_fingerprint = ?3)
                  AND (agent_definition_json IS NULL OR agent_definition_json = ?4)
                  AND (runtime_ref IS NULL OR runtime_ref = ?5)
                  AND (backend_kind IS NULL OR backend_kind = ?6)
                  AND (native_kind IS NULL OR native_kind = ?7)
                  AND (native_session_id IS NULL OR native_session_id IS ?8)
                  AND cwd = ?9
                  AND ownership = ?15
                  AND (parent_thread_id IS NULL OR parent_thread_id IS ?16)
                "#,
                )
                .bind(input.thread_id)
                .bind(input.agent_ref)
                .bind(input.agent_fingerprint)
                .bind(input.agent_definition_json)
                .bind(input.runtime_ref)
                .bind(input.backend_kind)
                .bind(input.native_kind)
                .bind(input.native_session_id)
                .bind(input.cwd)
                .bind(input.profile_fingerprint)
                .bind(input.profile_revision)
                .bind(input.profile_config_json)
                .bind(input.adapter_kind)
                .bind(input.adapter_revision)
                .bind(input.ownership.as_str())
                .bind(input.parent_thread_id)
                .bind(now)
                .bind(expected_binding_revision),
            )
            .await?;

        let record = self
            .gateway_runtime_binding(input.thread_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "runtime binding not found for thread `{}`",
                    input.thread_id
                ))
            })?;
        if changed > 0 {
            return Ok(record);
        }
        if record.binding_revision != expected_binding_revision {
            return Err(Error::Message(format!(
                "stale runtime binding revision for thread `{}`: expected {expected_binding_revision}, current {}",
                input.thread_id, record.binding_revision
            )));
        }
        if runtime_binding_matches_input(&record, &input) {
            return Ok(record);
        }
        Err(Error::Message(format!(
            "legacy runtime binding evidence conflicts with the requested binding for thread `{}`",
            input.thread_id
        )))
    }

    pub async fn gateway_runtime_binding(
        &self,
        thread_id: &str,
    ) -> Result<Option<GatewayRuntimeBindingRecord>> {
        let sql = runtime_binding_select_sql("WHERE thread_id = ?1");
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(thread_id)
                .fetch_optional(&mut *conn)
                .await?
                .map(|row| gateway_runtime_binding_from_row(&row))
                .transpose()
        })
        .await
    }

    pub async fn gateway_runtime_binding_by_native_session(
        &self,
        runtime_ref: &str,
        native_session_id: &str,
    ) -> Result<Option<GatewayRuntimeBindingRecord>> {
        let sql = runtime_binding_select_sql(
            "WHERE resolution_status = 'resolved' AND runtime_ref = ?1 AND native_session_id = ?2",
        );
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(runtime_ref)
                .bind(native_session_id)
                .fetch_optional(&mut *conn)
                .await?
                .map(|row| gateway_runtime_binding_from_row(&row))
                .transpose()
        })
        .await
    }

    pub async fn gateway_runtime_bindings_for_runtime(
        &self,
        runtime_ref: &str,
    ) -> Result<Vec<GatewayRuntimeBindingRecord>> {
        let sql = runtime_binding_select_sql(
            "WHERE resolution_status = 'resolved' AND runtime_ref = ?1 ORDER BY created_at_ms ASC, thread_id ASC",
        );
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(runtime_ref)
                .fetch_all(&mut *conn)
                .await?;
            rows.into_iter()
                .map(|row| gateway_runtime_binding_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn gateway_runtime_child_bindings(
        &self,
        parent_thread_id: &str,
    ) -> Result<Vec<GatewayRuntimeBindingRecord>> {
        let sql = runtime_binding_select_sql(
            "WHERE resolution_status = 'resolved' AND parent_thread_id = ?1 ORDER BY created_at_ms ASC, thread_id ASC",
        );
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(parent_thread_id)
                .fetch_all(&mut *conn)
                .await?;
            rows.into_iter()
                .map(|row| gateway_runtime_binding_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn attach_gateway_runtime_native_session(
        &self,
        thread_id: &str,
        expected_binding_revision: i64,
        native_session_id: &str,
    ) -> Result<GatewayRuntimeBindingRecord> {
        let native_session_id = native_session_id.trim();
        if native_session_id.is_empty() {
            return Err(Error::Message(
                "native session id must not be empty".to_string(),
            ));
        }
        if expected_binding_revision < 1 {
            return Err(Error::Message(
                "expected binding revision must be positive".to_string(),
            ));
        }

        let now = now_ms();
        let changed = self
            .runtime_binding_write(
                sqlx::query(
                    r#"
                UPDATE gateway_runtime_bindings
                SET native_session_id = ?1,
                    binding_revision = binding_revision + 1,
                    updated_at_ms = ?2
                WHERE thread_id = ?3
                  AND resolution_status = 'resolved'
                  AND binding_revision = ?4
                  AND native_session_id IS NULL
                "#,
                )
                .bind(native_session_id)
                .bind(now)
                .bind(thread_id)
                .bind(expected_binding_revision),
            )
            .await?;

        let record = self
            .gateway_runtime_binding(thread_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "runtime binding not found for thread `{thread_id}`"
                ))
            })?;
        if changed > 0 {
            return Ok(record);
        }
        if record.status != GatewayRuntimeBindingStatus::Resolved {
            return Err(Error::Message(format!(
                "runtime binding for thread `{thread_id}` is unresolved"
            )));
        }
        // A successful attach advances the durable revision. The adapter and the
        // post-turn reconciliation path may both acknowledge the same native id
        // while still holding the pre-attach revision; identical immutable
        // identity is therefore idempotent across that revision edge.
        if record.native_session_id.as_deref() == Some(native_session_id) {
            return Ok(record);
        }
        if record.binding_revision != expected_binding_revision {
            return Err(Error::Message(format!(
                "stale runtime binding revision for thread `{thread_id}`: expected {expected_binding_revision}, current {}",
                record.binding_revision
            )));
        }
        Err(Error::Message(format!(
            "runtime binding conflict for thread `{thread_id}`: native session identity is immutable"
        )))
    }

    pub async fn compare_and_set_gateway_runtime_control_state(
        &self,
        thread_id: &str,
        expected_binding_revision: i64,
        expected_control_revision: i64,
        patch: GatewayRuntimeControlStatePatch<'_>,
    ) -> Result<GatewayRuntimeBindingRecord> {
        if expected_binding_revision < 1 {
            return Err(Error::Message(
                "expected binding revision must be positive".to_string(),
            ));
        }
        if expected_control_revision < 1 {
            return Err(Error::Message(
                "expected control revision must be positive".to_string(),
            ));
        }
        if patch.thread_preferences.is_none() && patch.runtime_observed.is_none() {
            return Err(Error::Message(
                "runtime control state patch must contain preferences or observations".to_string(),
            ));
        }
        if let Some(values) = patch.thread_preferences {
            validate_runtime_control_map("thread preference", values)?;
        }
        if let Some(values) = patch.runtime_observed {
            validate_runtime_control_map("runtime observation", values)?;
        }

        let before = self
            .gateway_runtime_binding(thread_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "runtime binding not found for thread `{thread_id}`"
                ))
            })?;
        validate_runtime_control_cas(
            &before,
            expected_binding_revision,
            expected_control_revision,
        )?;
        let preferences_unchanged = patch
            .thread_preferences
            .is_none_or(|values| *values == before.thread_preferences);
        let observed_unchanged = patch
            .runtime_observed
            .is_none_or(|values| *values == before.runtime_observed);
        if preferences_unchanged && observed_unchanged {
            return Ok(before);
        }

        let preferences_json = patch
            .thread_preferences
            .map(serde_json::to_string)
            .transpose()?;
        let observed_json = patch
            .runtime_observed
            .map(serde_json::to_string)
            .transpose()?;
        let now = now_ms();
        let changed = self
            .runtime_binding_write(
                sqlx::query(
                    r#"
                UPDATE gateway_runtime_bindings
                SET thread_preferences_json = CASE WHEN ?1 THEN ?2 ELSE thread_preferences_json END,
                    runtime_observed_json = CASE WHEN ?3 THEN ?4 ELSE runtime_observed_json END,
                    control_revision = control_revision + 1,
                    updated_at_ms = ?5
                WHERE thread_id = ?6
                  AND resolution_status = 'resolved'
                  AND ownership = 'read_write'
                  AND binding_revision = ?7
                  AND control_revision = ?8
                "#,
                )
                .bind(patch.thread_preferences.is_some())
                .bind(preferences_json)
                .bind(patch.runtime_observed.is_some())
                .bind(observed_json)
                .bind(now)
                .bind(thread_id)
                .bind(expected_binding_revision)
                .bind(expected_control_revision),
            )
            .await?;
        let after = self
            .gateway_runtime_binding(thread_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "runtime binding not found for thread `{thread_id}`"
                ))
            })?;
        if changed > 0 {
            return Ok(after);
        }
        validate_runtime_control_cas(&after, expected_binding_revision, expected_control_revision)?;
        Err(Error::Message(format!(
            "runtime control state for thread `{thread_id}` was not updated"
        )))
    }

    async fn runtime_binding_write<'q>(
        &self,
        query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    ) -> Result<u64> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = query.execute(&mut *tx).await?.rows_affected();
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(changed)
        })
        .await
    }
}

fn validate_runtime_control_cas(
    record: &GatewayRuntimeBindingRecord,
    expected_binding_revision: i64,
    expected_control_revision: i64,
) -> Result<()> {
    if record.status != GatewayRuntimeBindingStatus::Resolved {
        return Err(Error::Message(format!(
            "runtime binding for thread `{}` is unresolved",
            record.thread_id
        )));
    }
    if record.ownership != GatewayRuntimeBindingOwnership::ReadWrite {
        return Err(Error::Message(format!(
            "runtime binding for thread `{}` is read-only",
            record.thread_id
        )));
    }
    if record.binding_revision != expected_binding_revision {
        return Err(Error::Message(format!(
            "stale runtime binding revision for thread `{}`: expected {expected_binding_revision}, current {}",
            record.thread_id, record.binding_revision
        )));
    }
    if record.control_revision != expected_control_revision {
        return Err(Error::Message(format!(
            "stale runtime control revision for thread `{}`: expected {expected_control_revision}, current {}",
            record.thread_id, record.control_revision
        )));
    }
    Ok(())
}

fn validate_runtime_control_map(label: &str, values: &BTreeMap<String, Value>) -> Result<()> {
    if values.keys().any(|key| key.trim().is_empty()) {
        return Err(Error::Message(format!(
            "{label} control id must not be empty"
        )));
    }
    Ok(())
}

fn validate_runtime_binding_input(input: &GatewayRuntimeBindingInput<'_>) -> Result<()> {
    for (field, value) in [
        ("thread_id", input.thread_id),
        ("agent_fingerprint", input.agent_fingerprint),
        ("agent_definition_json", input.agent_definition_json),
        ("runtime_ref", input.runtime_ref),
        ("backend_kind", input.backend_kind),
        ("native_kind", input.native_kind),
        ("cwd", input.cwd),
        ("profile_fingerprint", input.profile_fingerprint),
        ("profile_revision", input.profile_revision),
        ("profile_config_json", input.profile_config_json),
        ("adapter_kind", input.adapter_kind),
        ("adapter_revision", input.adapter_revision),
    ] {
        if value.trim().is_empty() {
            return Err(Error::Message(format!(
                "runtime binding {field} must not be empty"
            )));
        }
    }
    if input.agent_ref.is_some_and(|value| value.trim().is_empty()) {
        return Err(Error::Message(
            "runtime binding agent_ref must not be empty".to_string(),
        ));
    }
    if input
        .native_session_id
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Error::Message(
            "runtime binding native_session_id must not be empty".to_string(),
        ));
    }
    Ok(())
}

async fn validate_runtime_binding_threads(
    store: &StateRuntime,
    input: &GatewayRuntimeBindingInput<'_>,
) -> Result<()> {
    let thread = store
        .session_summary(input.thread_id)
        .await?
        .ok_or_else(|| Error::Message(format!("session not found: {}", input.thread_id)))?;
    if thread.cwd != input.cwd {
        return Err(Error::Message(format!(
            "runtime binding cwd does not match thread `{}`: expected `{}`, got `{}`",
            input.thread_id, thread.cwd, input.cwd
        )));
    }
    if let Some(parent_thread_id) = input.parent_thread_id {
        store
            .session_summary(parent_thread_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!("parent session not found: {parent_thread_id}"))
            })?;
    }
    Ok(())
}

fn runtime_binding_matches_input(
    record: &GatewayRuntimeBindingRecord,
    input: &GatewayRuntimeBindingInput<'_>,
) -> bool {
    record.status == GatewayRuntimeBindingStatus::Resolved
        && record.agent_ref.as_deref() == input.agent_ref
        && record.agent_fingerprint.as_deref() == Some(input.agent_fingerprint)
        && record.agent_definition_json.as_deref() == Some(input.agent_definition_json)
        && record.runtime_ref.as_deref() == Some(input.runtime_ref)
        && record.backend_kind.as_deref() == Some(input.backend_kind)
        && record.native_kind.as_deref() == Some(input.native_kind)
        && record.native_session_id.as_deref() == input.native_session_id
        && record.cwd == input.cwd
        && record.profile_fingerprint.as_deref() == Some(input.profile_fingerprint)
        && record.profile_revision.as_deref() == Some(input.profile_revision)
        && record.profile_config_json.as_deref() == Some(input.profile_config_json)
        && record.adapter_kind.as_deref() == Some(input.adapter_kind)
        && record.adapter_revision.as_deref() == Some(input.adapter_revision)
        && record.ownership == input.ownership
        && record.parent_thread_id.as_deref() == input.parent_thread_id
}

fn runtime_binding_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        SELECT thread_id, resolution_status, agent_ref, agent_fingerprint,
               agent_definition_json, runtime_ref, backend_kind, native_kind,
               native_session_id, cwd, profile_fingerprint, profile_revision,
               profile_config_json, adapter_kind, adapter_revision, ownership,
               parent_thread_id, binding_revision, thread_preferences_json,
               runtime_observed_json, control_revision, unresolved_reason,
               created_at_ms, updated_at_ms
        FROM gateway_runtime_bindings
        {where_clause}
        "#
    )
}

fn gateway_runtime_binding_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GatewayRuntimeBindingRecord> {
    let status_raw: String = row.try_get(1)?;
    let ownership_raw: String = row.try_get(15)?;
    let status = GatewayRuntimeBindingStatus::parse(&status_raw)
        .ok_or_else(|| invalid_runtime_binding_enum("resolution_status", &status_raw))?;
    let ownership = GatewayRuntimeBindingOwnership::parse(&ownership_raw)
        .ok_or_else(|| invalid_runtime_binding_enum("ownership", &ownership_raw))?;
    Ok(GatewayRuntimeBindingRecord {
        thread_id: row.try_get(0)?,
        status,
        agent_ref: row.try_get(2)?,
        agent_fingerprint: row.try_get(3)?,
        agent_definition_json: row.try_get(4)?,
        runtime_ref: row.try_get(5)?,
        backend_kind: row.try_get(6)?,
        native_kind: row.try_get(7)?,
        native_session_id: row.try_get(8)?,
        cwd: row.try_get(9)?,
        profile_fingerprint: row.try_get(10)?,
        profile_revision: row.try_get(11)?,
        profile_config_json: row.try_get(12)?,
        adapter_kind: row.try_get(13)?,
        adapter_revision: row.try_get(14)?,
        ownership,
        parent_thread_id: row.try_get(16)?,
        binding_revision: row.try_get(17)?,
        thread_preferences: decode_runtime_control_map(
            row.try_get::<Option<String>, _>(18)?.as_deref(),
        )?,
        runtime_observed: decode_runtime_control_map(
            row.try_get::<Option<String>, _>(19)?.as_deref(),
        )?,
        control_revision: row.try_get(20)?,
        unresolved_reason: row.try_get(21)?,
        created_at_ms: row.try_get(22)?,
        updated_at_ms: row.try_get(23)?,
    })
}

fn decode_runtime_control_map(value: Option<&str>) -> Result<BTreeMap<String, Value>> {
    value
        .map(serde_json::from_str)
        .transpose()
        .map(Option::unwrap_or_default)
        .map_err(Into::into)
}

fn invalid_runtime_binding_enum(field: &str, value: &str) -> Error {
    Error::Message(format!("invalid runtime binding {field}: {value}"))
}
