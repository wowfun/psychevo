#[allow(unused_imports)]
pub(crate) use super::*;

impl SqliteStore {
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
                    thread_id, resolution_status, runtime_ref, backend_kind,
                    native_kind, native_session_id, cwd, profile_fingerprint,
                    profile_revision, profile_config_json, adapter_kind,
                    adapter_revision, ownership,
                    parent_thread_id, binding_revision, unresolved_reason,
                    created_at_ms, updated_at_ms
                ) VALUES (
                    ?1, 'resolved', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                    ?11, ?12, ?13, 1, NULL, ?14, ?14
                )
                ON CONFLICT(thread_id) DO NOTHING
                "#,
                params![
                    input.thread_id,
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
                    runtime_ref = ?2,
                    backend_kind = ?3,
                    native_kind = ?4,
                    native_session_id = ?5,
                    cwd = ?6,
                    profile_fingerprint = ?7,
                    profile_revision = ?8,
                    profile_config_json = ?9,
                    adapter_kind = ?10,
                    adapter_revision = ?11,
                    ownership = ?12,
                    parent_thread_id = ?13,
                    binding_revision = binding_revision + 1,
                    unresolved_reason = NULL,
                    updated_at_ms = ?14
                WHERE thread_id = ?1
                  AND resolution_status = 'unresolved'
                  AND binding_revision = ?15
                  AND (runtime_ref IS NULL OR runtime_ref = ?2)
                  AND (backend_kind IS NULL OR backend_kind = ?3)
                  AND (native_kind IS NULL OR native_kind = ?4)
                  AND (native_session_id IS NULL OR native_session_id IS ?5)
                  AND cwd = ?6
                  AND ownership = ?12
                  AND (parent_thread_id IS NULL OR parent_thread_id IS ?13)
                "#,
                params![
                    input.thread_id,
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
}

fn validate_runtime_binding_input(input: &GatewayRuntimeBindingInput<'_>) -> Result<()> {
    for (field, value) in [
        ("thread_id", input.thread_id),
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
    store: &SqliteStore,
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
        SELECT thread_id, resolution_status, runtime_ref, backend_kind,
               native_kind, native_session_id, cwd, profile_fingerprint,
               profile_revision, profile_config_json, adapter_kind,
               adapter_revision, ownership,
               parent_thread_id, binding_revision, unresolved_reason,
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
    let ownership_raw: String = row.get(12)?;
    let status = GatewayRuntimeBindingStatus::parse(&status_raw)
        .ok_or_else(|| invalid_runtime_binding_enum(1, "resolution_status", &status_raw))?;
    let ownership = GatewayRuntimeBindingOwnership::parse(&ownership_raw)
        .ok_or_else(|| invalid_runtime_binding_enum(12, "ownership", &ownership_raw))?;
    Ok(GatewayRuntimeBindingRecord {
        thread_id: row.get(0)?,
        status,
        runtime_ref: row.get(2)?,
        backend_kind: row.get(3)?,
        native_kind: row.get(4)?,
        native_session_id: row.get(5)?,
        cwd: row.get(6)?,
        profile_fingerprint: row.get(7)?,
        profile_revision: row.get(8)?,
        profile_config_json: row.get(9)?,
        adapter_kind: row.get(10)?,
        adapter_revision: row.get(11)?,
        ownership,
        parent_thread_id: row.get(13)?,
        binding_revision: row.get(14)?,
        unresolved_reason: row.get(15)?,
        created_at_ms: row.get(16)?,
        updated_at_ms: row.get(17)?,
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
