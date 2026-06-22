#[allow(unused_imports)]
pub(crate) use super::*;

impl SqliteStore {
    pub fn upsert_gateway_source_binding(
        &self,
        input: GatewaySourceBindingInput<'_>,
    ) -> Result<GatewaySourceBindingRecord> {
        self.resume_session(input.thread_id)?;
        let now = now_ms();
        let raw_identity_json = serde_json::to_string(&input.raw_identity)?;
        let lineage_json = input
            .lineage
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO gateway_source_bindings (
                    source_key, source_kind, raw_identity_json, visible_name,
                    thread_id, backend_kind, backend_native_id, created_at_ms,
                    updated_at_ms, lineage_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)
                ON CONFLICT(source_key) DO UPDATE SET
                    source_kind = excluded.source_kind,
                    raw_identity_json = excluded.raw_identity_json,
                    visible_name = excluded.visible_name,
                    thread_id = excluded.thread_id,
                    backend_kind = excluded.backend_kind,
                    backend_native_id = excluded.backend_native_id,
                    updated_at_ms = excluded.updated_at_ms,
                    lineage_json = excluded.lineage_json
                "#,
                params![
                    input.source_key,
                    input.source_kind,
                    raw_identity_json,
                    input.visible_name,
                    input.thread_id,
                    input.backend_kind,
                    input.backend_native_id,
                    now,
                    lineage_json,
                ],
            )?;
            Ok(())
        })?;
        self.gateway_source_binding(input.source_key)?
            .ok_or_else(|| {
                Error::Message(format!(
                    "gateway source binding not found: {}",
                    input.source_key
                ))
            })
    }

    pub fn gateway_source_binding(
        &self,
        source_key: &str,
    ) -> Result<Option<GatewaySourceBindingRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            r#"
            SELECT source_key, source_kind, raw_identity_json, visible_name,
                   thread_id, backend_kind, backend_native_id, created_at_ms,
                   updated_at_ms, lineage_json
            FROM gateway_source_bindings
            WHERE source_key = ?1
            "#,
            params![source_key],
            gateway_source_binding_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn gateway_source_bindings_for_connection_id(
        &self,
        connection_id: &str,
    ) -> Result<Vec<GatewaySourceBindingRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT source_key, source_kind, raw_identity_json, visible_name,
                   thread_id, backend_kind, backend_native_id, created_at_ms,
                   updated_at_ms, lineage_json
            FROM gateway_source_bindings
            WHERE source_kind LIKE 'im.%'
            ORDER BY updated_at_ms DESC
            "#,
        )?;
        let rows = stmt.query_map([], gateway_source_binding_from_row)?;
        let mut bindings = Vec::new();
        for row in rows {
            let binding = row?;
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
    }

    pub fn delete_gateway_source_binding(&self, source_key: &str) -> Result<bool> {
        let changed = self.write_retry(|conn| {
            conn.execute(
                "DELETE FROM gateway_source_bindings WHERE source_key = ?1",
                params![source_key],
            )
        })?;
        Ok(changed > 0)
    }

    pub fn mark_session_ended_with_reason(&self, session_id: &str, reason: &str) -> Result<()> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET updated_at_ms = ?1, ended_at_ms = ?1, end_reason = ?2 WHERE id = ?3",
                params![now, reason, session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }
}

fn gateway_source_binding_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<GatewaySourceBindingRecord> {
    let raw_identity_json: String = row.get(2)?;
    let lineage_json: Option<String> = row.get(9)?;
    let raw_identity = serde_json::from_str(&raw_identity_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let lineage = lineage_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(err))
        })?;
    Ok(GatewaySourceBindingRecord {
        source_key: row.get(0)?,
        source_kind: row.get(1)?,
        raw_identity,
        visible_name: row.get(3)?,
        thread_id: row.get(4)?,
        backend_kind: row.get(5)?,
        backend_native_id: row.get(6)?,
        created_at_ms: row.get(7)?,
        updated_at_ms: row.get(8)?,
        lineage,
    })
}
