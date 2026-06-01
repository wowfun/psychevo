#[allow(unused_imports)]
pub(crate) use super::*;

impl SqliteStore {
    pub fn upsert_capability_snapshot(&self, snapshot: &CapabilitySnapshot) -> Result<()> {
        let snapshot_json = serde_json::to_string(snapshot)?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO capability_snapshots (
                    session_id, prompt_prefix_version, created_at_ms, snapshot_hash,
                    snapshot_json
                ) VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(session_id, prompt_prefix_version) DO UPDATE SET
                    created_at_ms = excluded.created_at_ms,
                    snapshot_hash = excluded.snapshot_hash,
                    snapshot_json = excluded.snapshot_json
                "#,
                params![
                    &snapshot.session_id,
                    snapshot.prompt_prefix_version,
                    snapshot.created_at_ms,
                    &snapshot.snapshot_hash,
                    &snapshot_json,
                ],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn load_capability_snapshot(
        &self,
        session_id: &str,
        prompt_prefix_version: i64,
    ) -> Result<Option<CapabilitySnapshot>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            r#"
            SELECT snapshot_json
            FROM capability_snapshots
            WHERE session_id = ?1 AND prompt_prefix_version = ?2
            "#,
            params![session_id, prompt_prefix_version],
            capability_snapshot_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn load_latest_capability_snapshot(
        &self,
        session_id: &str,
    ) -> Result<Option<CapabilitySnapshot>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            r#"
            SELECT snapshot_json
            FROM capability_snapshots
            WHERE session_id = ?1
            ORDER BY prompt_prefix_version DESC
            LIMIT 1
            "#,
            params![session_id],
            capability_snapshot_from_row,
        )
        .optional()
        .map_err(Into::into)
    }
}

fn capability_snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CapabilitySnapshot> {
    let snapshot_json: String = row.get(0)?;
    serde_json::from_str(&snapshot_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
    })
}
