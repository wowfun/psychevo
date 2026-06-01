#[allow(unused_imports)]
pub(crate) use super::*;

impl SqliteStore {
    pub fn upsert_timeline_item(&self, input: TimelineItemInput) -> Result<TimelineItemRecord> {
        let artifact_ids_json = serde_json::to_string(&input.artifact_ids)?;
        let metadata_json = optional_json_string(&input.metadata)?;
        let now = now_ms();
        let session_id = input.session_id.clone();
        let item_id = input.item_id.clone();
        self.write_retry(|conn| {
            let existing_seq = conn
                .query_row(
                    "SELECT item_seq FROM timeline_items WHERE session_id = ?1 AND item_id = ?2",
                    params![&input.session_id, &input.item_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            match existing_seq {
                Some(_) => {
                    conn.execute(
                        r#"
                        UPDATE timeline_items
                        SET turn_id = ?3,
                            kind = ?4,
                            status = ?5,
                            source = ?6,
                            title = ?7,
                            body_text = ?8,
                            preview_text = ?9,
                            detail_text = ?10,
                            artifact_ids_json = ?11,
                            metadata_json = ?12,
                            updated_at_ms = ?13
                        WHERE session_id = ?1 AND item_id = ?2
                        "#,
                        params![
                            &input.session_id,
                            &input.item_id,
                            &input.turn_id,
                            input.kind.as_str(),
                            input.status.as_str(),
                            &input.source,
                            &input.title,
                            &input.body_text,
                            &input.preview_text,
                            &input.detail_text,
                            &artifact_ids_json,
                            &metadata_json,
                            now,
                        ],
                    )?;
                }
                None => {
                    let item_seq: i64 = conn.query_row(
                        "SELECT COALESCE(MAX(item_seq), 0) + 1 FROM timeline_items WHERE session_id = ?1",
                        params![&input.session_id],
                        |row| row.get(0),
                    )?;
                    conn.execute(
                        r#"
                        INSERT INTO timeline_items (
                            session_id, item_seq, item_id, turn_id, kind, status, source,
                            title, body_text, preview_text, detail_text, artifact_ids_json,
                            metadata_json, created_at_ms, updated_at_ms
                        ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14
                        )
                        "#,
                        params![
                            &input.session_id,
                            item_seq,
                            &input.item_id,
                            &input.turn_id,
                            input.kind.as_str(),
                            input.status.as_str(),
                            &input.source,
                            &input.title,
                            &input.body_text,
                            &input.preview_text,
                            &input.detail_text,
                            &artifact_ids_json,
                            &metadata_json,
                            now,
                        ],
                    )?;
                }
            }
            Ok(())
        })?;
        self.timeline_item(&session_id, &item_id)?.ok_or_else(|| {
            Error::Message(format!("timeline item not found after upsert: {item_id}"))
        })
    }

    pub fn timeline_item(
        &self,
        session_id: &str,
        item_id: &str,
    ) -> Result<Option<TimelineItemRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let row = conn
            .query_row(
                r#"
                SELECT id, session_id, item_seq, item_id, turn_id, kind, status, source,
                       title, body_text, preview_text, detail_text, artifact_ids_json,
                       metadata_json, created_at_ms, updated_at_ms
                FROM timeline_items
                WHERE session_id = ?1 AND item_id = ?2
                "#,
                params![session_id, item_id],
                timeline_item_row,
            )
            .optional()?;
        row.map(timeline_item_from_row).transpose()
    }

    pub fn load_timeline_items(&self, session_id: &str) -> Result<Vec<TimelineItemRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, session_id, item_seq, item_id, turn_id, kind, status, source,
                   title, body_text, preview_text, detail_text, artifact_ids_json,
                   metadata_json, created_at_ms, updated_at_ms
            FROM timeline_items
            WHERE session_id = ?1
            ORDER BY item_seq ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id], timeline_item_row)?;
        rows.map(|row| timeline_item_from_row(row?)).collect()
    }

    pub fn upsert_timeline_artifact(
        &self,
        input: TimelineArtifactInput,
    ) -> Result<TimelineArtifactRecord> {
        let metadata_json = optional_json_string(&input.metadata)?;
        let now = now_ms();
        let session_id = input.session_id.clone();
        let artifact_id = input.artifact_id.clone();
        self.write_retry(|conn| {
            let exists = conn
                .query_row(
                    "SELECT 1 FROM timeline_artifacts WHERE session_id = ?1 AND artifact_id = ?2",
                    params![&input.session_id, &input.artifact_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if exists {
                conn.execute(
                    r#"
                    UPDATE timeline_artifacts
                    SET kind = ?3,
                        mime_type = ?4,
                        title = ?5,
                        preview_text = ?6,
                        path = ?7,
                        metadata_json = ?8
                    WHERE session_id = ?1 AND artifact_id = ?2
                    "#,
                    params![
                        &input.session_id,
                        &input.artifact_id,
                        &input.kind,
                        &input.mime_type,
                        &input.title,
                        &input.preview_text,
                        &input.path,
                        &metadata_json,
                    ],
                )?;
            } else {
                conn.execute(
                    r#"
                    INSERT INTO timeline_artifacts (
                        session_id, artifact_id, kind, mime_type, title, preview_text,
                        path, metadata_json, created_at_ms
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                    "#,
                    params![
                        &input.session_id,
                        &input.artifact_id,
                        &input.kind,
                        &input.mime_type,
                        &input.title,
                        &input.preview_text,
                        &input.path,
                        &metadata_json,
                        now,
                    ],
                )?;
            }
            Ok(())
        })?;
        self.timeline_artifact(&session_id, &artifact_id)?
            .ok_or_else(|| {
                Error::Message(format!(
                    "timeline artifact not found after upsert: {artifact_id}"
                ))
            })
    }

    pub fn timeline_artifact(
        &self,
        session_id: &str,
        artifact_id: &str,
    ) -> Result<Option<TimelineArtifactRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let row = conn
            .query_row(
                r#"
                SELECT id, session_id, artifact_id, kind, mime_type, title,
                       preview_text, path, metadata_json, created_at_ms
                FROM timeline_artifacts
                WHERE session_id = ?1 AND artifact_id = ?2
                "#,
                params![session_id, artifact_id],
                timeline_artifact_row,
            )
            .optional()?;
        row.map(timeline_artifact_from_row).transpose()
    }

    pub fn load_timeline_artifacts(&self, session_id: &str) -> Result<Vec<TimelineArtifactRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, session_id, artifact_id, kind, mime_type, title,
                   preview_text, path, metadata_json, created_at_ms
            FROM timeline_artifacts
            WHERE session_id = ?1
            ORDER BY id ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id], timeline_artifact_row)?;
        rows.map(|row| timeline_artifact_from_row(row?)).collect()
    }

    pub fn append_timeline_debug_event(&self, input: TimelineDebugEventInput) -> Result<i64> {
        let scope_json = optional_json_string(&input.scope)?;
        let payload_json = optional_json_string(&input.payload)?;
        let now = now_ms();
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO timeline_debug_events (
                    session_id, turn_id, event_type, source, scope_json, status, summary,
                    payload_json, created_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                "#,
                params![
                    &input.session_id,
                    &input.turn_id,
                    &input.event_type,
                    &input.source,
                    &scope_json,
                    &input.status,
                    &input.summary,
                    &payload_json,
                    now,
                ],
            )?;
            let id = conn.last_insert_rowid();
            conn.execute(
                r#"
                DELETE FROM timeline_debug_events
                WHERE session_id = ?1
                  AND id NOT IN (
                    SELECT id
                    FROM timeline_debug_events
                    WHERE session_id = ?1
                    ORDER BY id DESC
                    LIMIT 500
                  )
                "#,
                params![&input.session_id],
            )?;
            Ok(id)
        })
    }

    pub fn load_timeline_debug_events(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<TimelineDebugEventRecord>> {
        let limit = limit.clamp(1, 500) as i64;
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, session_id, turn_id, event_type, source, scope_json, status,
                   summary, payload_json, created_at_ms
            FROM (
                SELECT id, session_id, turn_id, event_type, source, scope_json, status,
                       summary, payload_json, created_at_ms
                FROM timeline_debug_events
                WHERE session_id = ?1
                ORDER BY id DESC
                LIMIT ?2
            )
            ORDER BY id ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id, limit], timeline_debug_event_row)?;
        rows.map(|row| timeline_debug_event_from_row(row?))
            .collect()
    }
}

#[allow(clippy::type_complexity)]
fn timeline_item_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(
    i64,
    String,
    i64,
    String,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    i64,
    i64,
)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
    ))
}

#[allow(clippy::type_complexity)]
fn timeline_item_from_row(
    row: (
        i64,
        String,
        i64,
        String,
        Option<String>,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        i64,
        i64,
    ),
) -> Result<TimelineItemRecord> {
    let (
        id,
        session_id,
        item_seq,
        item_id,
        turn_id,
        kind,
        status,
        source,
        title,
        body_text,
        preview_text,
        detail_text,
        artifact_ids_json,
        metadata_json,
        created_at_ms,
        updated_at_ms,
    ) = row;
    let Some(kind) = TimelineItemKind::parse(&kind) else {
        return Err(Error::Message(format!(
            "unknown timeline item kind: {kind}"
        )));
    };
    let Some(status) = TimelineItemStatus::parse(&status) else {
        return Err(Error::Message(format!(
            "unknown timeline item status: {status}"
        )));
    };
    Ok(TimelineItemRecord {
        id,
        session_id,
        item_seq,
        item_id,
        turn_id,
        kind,
        status,
        source,
        title,
        body_text,
        preview_text,
        detail_text,
        artifact_ids: serde_json::from_str(&artifact_ids_json)?,
        metadata: parse_optional_json(metadata_json)?,
        created_at_ms,
        updated_at_ms,
    })
}

#[allow(clippy::type_complexity)]
fn timeline_artifact_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

#[allow(clippy::type_complexity)]
fn timeline_artifact_from_row(
    row: (
        i64,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
    ),
) -> Result<TimelineArtifactRecord> {
    let (
        id,
        session_id,
        artifact_id,
        kind,
        mime_type,
        title,
        preview_text,
        path,
        metadata_json,
        created_at_ms,
    ) = row;
    Ok(TimelineArtifactRecord {
        id,
        session_id,
        artifact_id,
        kind,
        mime_type,
        title,
        preview_text,
        path,
        metadata: parse_optional_json(metadata_json)?,
        created_at_ms,
    })
}

#[allow(clippy::type_complexity)]
fn timeline_debug_event_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(
    i64,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

#[allow(clippy::type_complexity)]
fn timeline_debug_event_from_row(
    row: (
        i64,
        String,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
    ),
) -> Result<TimelineDebugEventRecord> {
    let (
        id,
        session_id,
        turn_id,
        event_type,
        source,
        scope_json,
        status,
        summary,
        payload_json,
        created_at_ms,
    ) = row;
    Ok(TimelineDebugEventRecord {
        id,
        session_id,
        turn_id,
        event_type,
        source,
        scope: parse_optional_json(scope_json)?,
        status,
        summary,
        payload: parse_optional_json(payload_json)?,
        created_at_ms,
    })
}
