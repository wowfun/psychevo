use std::collections::BTreeMap;

use psychevo_agent_core::now_ms;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use crate::error::{Error, Result};

use super::{
    GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership, GatewayRuntimeBindingRecord,
    GatewayRuntimeBindingStatus, GatewayRuntimeControlStatePatch, StateRuntime,
};

impl StateRuntime {
    pub fn create_gateway_runtime_binding(
        &self,
        input: GatewayRuntimeBindingInput<'_>,
    ) -> Result<GatewayRuntimeBindingRecord> {
        validate_runtime_binding_input(&input)?;
        validate_runtime_binding_threads(self, &input)?;

        let now = now_ms();
        let inserted = self.write_retry(|conn| {
            conn.execute(
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
                params![
                    input.thread_id,
                    input.agent_ref,
                    input.agent_fingerprint,
                    input.agent_definition_json,
                    input.runtime_ref,
                    input.backend_kind,
                    input.native_kind,
                    input.native_session_id,
                    input.cwd,
                    input.profile_fingerprint,
                    input.profile_revision,
                    input.profile_config_json,
                    input.adapter_kind,
                    input.adapter_revision,
                    input.ownership.as_str(),
                    input.parent_thread_id,
                    now,
                ],
            )
        })?;

        let record = self
            .gateway_runtime_binding(input.thread_id)?
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
    pub fn create_gateway_runtime_binding_from_parent_snapshot(
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
            .gateway_runtime_binding(parent_thread_id)?
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
            .session_summary(child_thread_id)?
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
        let inserted = self.write_retry(|conn| {
            conn.execute(
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
                params![
                    child_thread_id,
                    inherited_preferences_json,
                    now,
                    parent_thread_id,
                ],
            )
        })?;
        if inserted != 1 {
            return Err(Error::Message(format!(
                "runtime binding conflict for child Thread `{child_thread_id}`"
            )));
        }
        self.gateway_runtime_binding(child_thread_id)?
            .ok_or_else(|| {
                Error::Message(format!(
                    "runtime binding not found after snapshot: {child_thread_id}"
                ))
            })
    }

    pub fn resolve_gateway_runtime_binding(
        &self,
        input: GatewayRuntimeBindingInput<'_>,
        expected_binding_revision: i64,
    ) -> Result<GatewayRuntimeBindingRecord> {
        validate_runtime_binding_input(&input)?;
        validate_runtime_binding_threads(self, &input)?;
        if expected_binding_revision < 1 {
            return Err(Error::Message(
                "expected binding revision must be positive".to_string(),
            ));
        }

        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
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
                params![
                    input.thread_id,
                    input.agent_ref,
                    input.agent_fingerprint,
                    input.agent_definition_json,
                    input.runtime_ref,
                    input.backend_kind,
                    input.native_kind,
                    input.native_session_id,
                    input.cwd,
                    input.profile_fingerprint,
                    input.profile_revision,
                    input.profile_config_json,
                    input.adapter_kind,
                    input.adapter_revision,
                    input.ownership.as_str(),
                    input.parent_thread_id,
                    now,
                    expected_binding_revision,
                ],
            )
        })?;

        let record = self
            .gateway_runtime_binding(input.thread_id)?
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

    pub fn gateway_runtime_binding(
        &self,
        thread_id: &str,
    ) -> Result<Option<GatewayRuntimeBindingRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            runtime_binding_select_sql("WHERE thread_id = ?1").as_str(),
            params![thread_id],
            gateway_runtime_binding_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn gateway_runtime_binding_by_native_session(
        &self,
        runtime_ref: &str,
        native_session_id: &str,
    ) -> Result<Option<GatewayRuntimeBindingRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            runtime_binding_select_sql(
                "WHERE resolution_status = 'resolved' AND runtime_ref = ?1 AND native_session_id = ?2",
            )
            .as_str(),
            params![runtime_ref, native_session_id],
            gateway_runtime_binding_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn gateway_runtime_bindings_for_runtime(
        &self,
        runtime_ref: &str,
    ) -> Result<Vec<GatewayRuntimeBindingRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let sql = runtime_binding_select_sql(
            "WHERE resolution_status = 'resolved' AND runtime_ref = ?1 ORDER BY created_at_ms ASC, thread_id ASC",
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![runtime_ref], gateway_runtime_binding_from_row)?;
        let mut bindings = Vec::new();
        for row in rows {
            bindings.push(row?);
        }
        Ok(bindings)
    }

    pub fn gateway_runtime_child_bindings(
        &self,
        parent_thread_id: &str,
    ) -> Result<Vec<GatewayRuntimeBindingRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let sql = runtime_binding_select_sql(
            "WHERE resolution_status = 'resolved' AND parent_thread_id = ?1 ORDER BY created_at_ms ASC, thread_id ASC",
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![parent_thread_id], gateway_runtime_binding_from_row)?;
        let mut bindings = Vec::new();
        for row in rows {
            bindings.push(row?);
        }
        Ok(bindings)
    }

    pub fn attach_gateway_runtime_native_session(
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
        let changed = self.write_retry(|conn| {
            conn.execute(
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
                params![native_session_id, now, thread_id, expected_binding_revision,],
            )
        })?;

        let record = self.gateway_runtime_binding(thread_id)?.ok_or_else(|| {
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

    pub fn compare_and_set_gateway_runtime_control_state(
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

        let before = self.gateway_runtime_binding(thread_id)?.ok_or_else(|| {
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
        let changed = self.write_retry(|conn| {
            conn.execute(
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
                params![
                    patch.thread_preferences.is_some(),
                    preferences_json,
                    patch.runtime_observed.is_some(),
                    observed_json,
                    now,
                    thread_id,
                    expected_binding_revision,
                    expected_control_revision,
                ],
            )
        })?;
        let after = self.gateway_runtime_binding(thread_id)?.ok_or_else(|| {
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

fn validate_runtime_binding_threads(
    store: &StateRuntime,
    input: &GatewayRuntimeBindingInput<'_>,
) -> Result<()> {
    let thread = store
        .session_summary(input.thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {}", input.thread_id)))?;
    if thread.cwd != input.cwd {
        return Err(Error::Message(format!(
            "runtime binding cwd does not match thread `{}`: expected `{}`, got `{}`",
            input.thread_id, thread.cwd, input.cwd
        )));
    }
    if let Some(parent_thread_id) = input.parent_thread_id {
        store.session_summary(parent_thread_id)?.ok_or_else(|| {
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
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<GatewayRuntimeBindingRecord> {
    let status_raw: String = row.get(1)?;
    let ownership_raw: String = row.get(15)?;
    let status = GatewayRuntimeBindingStatus::parse(&status_raw)
        .ok_or_else(|| invalid_runtime_binding_enum(1, "resolution_status", &status_raw))?;
    let ownership = GatewayRuntimeBindingOwnership::parse(&ownership_raw)
        .ok_or_else(|| invalid_runtime_binding_enum(15, "ownership", &ownership_raw))?;
    Ok(GatewayRuntimeBindingRecord {
        thread_id: row.get(0)?,
        status,
        agent_ref: row.get(2)?,
        agent_fingerprint: row.get(3)?,
        agent_definition_json: row.get(4)?,
        runtime_ref: row.get(5)?,
        backend_kind: row.get(6)?,
        native_kind: row.get(7)?,
        native_session_id: row.get(8)?,
        cwd: row.get(9)?,
        profile_fingerprint: row.get(10)?,
        profile_revision: row.get(11)?,
        profile_config_json: row.get(12)?,
        adapter_kind: row.get(13)?,
        adapter_revision: row.get(14)?,
        ownership,
        parent_thread_id: row.get(16)?,
        binding_revision: row.get(17)?,
        thread_preferences: decode_runtime_control_map(
            row.get::<_, Option<String>>(18)?.as_deref(),
            18,
        )?,
        runtime_observed: decode_runtime_control_map(
            row.get::<_, Option<String>>(19)?.as_deref(),
            19,
        )?,
        control_revision: row.get(20)?,
        unresolved_reason: row.get(21)?,
        created_at_ms: row.get(22)?,
        updated_at_ms: row.get(23)?,
    })
}

fn decode_runtime_control_map(
    value: Option<&str>,
    column: usize,
) -> rusqlite::Result<BTreeMap<String, Value>> {
    value
        .map(serde_json::from_str)
        .transpose()
        .map(Option::unwrap_or_default)
        .map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                column,
                rusqlite::types::Type::Text,
                Box::new(err),
            )
        })
}

fn invalid_runtime_binding_enum(index: usize, field: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid runtime binding {field}: {value}"),
        )),
    )
}
