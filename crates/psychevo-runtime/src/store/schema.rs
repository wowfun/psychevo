#[allow(unused_imports)]
pub(crate) use super::*;
impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self> {
        if path != Path::new(":memory:")
            && let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_millis(250))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut user_version: i64 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let has_schema =
            sqlite_table_exists(&conn, "sessions")? || sqlite_table_exists(&conn, "messages")?;
        if user_version != 0
            && !(MIN_SUPPORTED_SQLITE_SCHEMA_VERSION..=SQLITE_SCHEMA_VERSION)
                .contains(&user_version)
        {
            return Err(Error::Config(format!(
                "state database schema version {user_version} is not supported; run `pevo init --reset-state` or set PSYCHEVO_DB to a new state database"
            )));
        }
        if user_version == 0 && has_schema {
            return Err(Error::Config(
                "state database has an unknown schema version; run `pevo init --reset-state` or set PSYCHEVO_DB to a new state database".to_string(),
            ));
        }
        if user_version == 24 {
            migrate_sqlite_schema_v24_to_v25(&conn)?;
            user_version = 25;
        }
        if user_version == 25 {
            migrate_sqlite_schema_v25_to_v26(&conn)?;
        }
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                parent_session_id TEXT,
                cwd TEXT NOT NULL,
                model TEXT NOT NULL,
                provider TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER,
                end_reason TEXT,
                archived_at_ms INTEGER,
                message_count INTEGER NOT NULL DEFAULT 0,
                tool_call_count INTEGER NOT NULL DEFAULT 0,
                title TEXT,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                session_seq INTEGER NOT NULL,
                role TEXT NOT NULL,
                timestamp_ms INTEGER NOT NULL,
                message_json TEXT NOT NULL,
                content_text TEXT,
                tool_call_id TEXT,
                tool_name TEXT,
                tool_calls_json TEXT,
                finish_reason TEXT,
                outcome TEXT,
                model TEXT,
                provider TEXT,
                usage_json TEXT,
                metadata_json TEXT,
                context_input_tokens INTEGER,
                billable_input_tokens INTEGER,
                billable_output_tokens INTEGER,
                reasoning_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                reported_total_tokens INTEGER,
                estimated_cost_nanodollars INTEGER,
                pricing_source TEXT,
                pricing_tier TEXT,
                cost_status TEXT,
                pricing_missing_reason TEXT,
                pricing_version TEXT,
                UNIQUE(session_id, session_seq)
            );

            CREATE TABLE IF NOT EXISTS context_evidence (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                prompt_session_seq INTEGER NOT NULL,
                context_seq INTEGER NOT NULL,
                role TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                source_name TEXT,
                source_path TEXT,
                provider_group TEXT,
                provider_block_index INTEGER,
                context_kind TEXT,
                timestamp_ms INTEGER NOT NULL,
                content_text TEXT NOT NULL,
                metadata_json TEXT,
                UNIQUE(session_id, prompt_session_seq, context_seq),
                FOREIGN KEY (session_id, prompt_session_seq)
                    REFERENCES messages(session_id, session_seq)
                    ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS agent_edges (
                parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                child_session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
                status TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS agent_team_runs (
                id TEXT PRIMARY KEY,
                parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                mission_run_id TEXT,
                team_name TEXT NOT NULL,
                description TEXT,
                source_path TEXT,
                leader_agent_name TEXT NOT NULL,
                members_json TEXT NOT NULL,
                max_parallel_agents INTEGER NOT NULL,
                status TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER,
                final_summary TEXT,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS agent_mission_runs (
                id TEXT PRIMARY KEY,
                parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                team_run_id TEXT REFERENCES agent_team_runs(id) ON DELETE SET NULL,
                team_name TEXT,
                goal TEXT NOT NULL,
                lead_agent_name TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER,
                final_summary TEXT,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS session_prompt_prefixes (
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                version INTEGER NOT NULL,
                created_at_ms INTEGER NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                prefix_hash TEXT NOT NULL,
                tool_declarations_hash TEXT NOT NULL,
                invalidation_reason TEXT,
                slots_json TEXT NOT NULL,
                metadata_json TEXT,
                PRIMARY KEY (session_id, version)
            );

            CREATE TABLE IF NOT EXISTS agent_mailbox_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                child_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
                agent_id TEXT NOT NULL,
                task_name TEXT,
                agent_name TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                delivered_at_ms INTEGER,
                delivered_prompt_session_seq INTEGER,
                delivered_after_session_seq INTEGER,
                delivered_tool_call_id TEXT,
                content_text TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS session_compactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                created_at_ms INTEGER NOT NULL,
                reason TEXT NOT NULL,
                summary_text TEXT NOT NULL,
                first_kept_session_seq INTEGER NOT NULL,
                created_after_session_seq INTEGER NOT NULL,
                tokens_before INTEGER,
                tokens_after INTEGER,
                summary_provider TEXT NOT NULL,
                summary_model TEXT NOT NULL,
                instructions TEXT,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS gateway_source_bindings (
                source_key TEXT PRIMARY KEY,
                source_kind TEXT NOT NULL,
                raw_identity_json TEXT NOT NULL,
                visible_name TEXT,
                thread_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
                backend_kind TEXT,
                backend_native_id TEXT,
                draft_runtime_ref TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                lineage_json TEXT
            );

            CREATE TABLE IF NOT EXISTS gateway_runtime_bindings (
                thread_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
                resolution_status TEXT NOT NULL CHECK (resolution_status IN ('resolved', 'unresolved')),
                runtime_ref TEXT,
                backend_kind TEXT,
                native_kind TEXT,
                native_session_id TEXT,
                cwd TEXT NOT NULL,
                profile_fingerprint TEXT,
                profile_revision TEXT,
                profile_config_json TEXT,
                adapter_kind TEXT,
                adapter_revision TEXT,
                ownership TEXT NOT NULL CHECK (ownership IN ('read_write', 'read_only')),
                parent_thread_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
                binding_revision INTEGER NOT NULL CHECK (binding_revision > 0),
                unresolved_reason TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                CHECK (
                    (resolution_status = 'resolved'
                        AND runtime_ref IS NOT NULL
                        AND backend_kind IS NOT NULL
                        AND native_kind IS NOT NULL
                        AND profile_fingerprint IS NOT NULL
                        AND profile_revision IS NOT NULL
                        AND adapter_kind IS NOT NULL
                        AND adapter_revision IS NOT NULL
                        AND unresolved_reason IS NULL)
                    OR
                    (resolution_status = 'unresolved' AND unresolved_reason IS NOT NULL)
                )
            );

            CREATE TABLE IF NOT EXISTS gateway_activities (
                activity_id TEXT PRIMARY KEY,
                thread_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
                source_key TEXT,
                turn_id TEXT,
                kind TEXT NOT NULL,
                status TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                owner_surface TEXT,
                generation INTEGER NOT NULL,
                started_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                lease_expires_at_ms INTEGER NOT NULL,
                queued_turns INTEGER NOT NULL DEFAULT 0,
                superseded_activity_id TEXT,
                intent_json TEXT
            );

            CREATE TABLE IF NOT EXISTS gateway_live_events (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                activity_id TEXT,
                owner_id TEXT,
                thread_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
                turn_id TEXT,
                event_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS gateway_live_snapshots (
                snapshot_key TEXT PRIMARY KEY,
                activity_id TEXT,
                owner_id TEXT,
                thread_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
                turn_id TEXT,
                event_kind TEXT NOT NULL,
                event_json TEXT NOT NULL,
                revision INTEGER NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS gateway_control_commands (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                activity_id TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                command_kind TEXT NOT NULL,
                status TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                error TEXT
            );

            CREATE TABLE IF NOT EXISTS gateway_turn_terminals (
                turn_id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                status TEXT NOT NULL,
                outcome TEXT,
                error_message TEXT,
                started_at_ms INTEGER,
                completed_at_ms INTEGER NOT NULL,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS automations (
                id TEXT PRIMARY KEY,
                cwd TEXT NOT NULL,
                kind TEXT NOT NULL,
                target_thread_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
                title TEXT NOT NULL,
                prompt TEXT NOT NULL,
                schedule_json TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                execution_json TEXT NOT NULL,
                model TEXT,
                reasoning_effort TEXT,
                source_key TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                last_run_at_ms INTEGER,
                next_run_at_ms INTEGER,
                last_status TEXT,
                last_error TEXT
            );

            CREATE TABLE IF NOT EXISTS automation_runs (
                id TEXT PRIMARY KEY,
                automation_id TEXT NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
                trigger TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                completed_at_ms INTEGER,
                thread_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
                source_key TEXT,
                error TEXT,
                metadata_json TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_messages_session_seq
                ON messages(session_id, session_seq);
            CREATE INDEX IF NOT EXISTS idx_context_evidence_prompt
                ON context_evidence(session_id, prompt_session_seq, context_seq);
            CREATE INDEX IF NOT EXISTS idx_agent_edges_parent
                ON agent_edges(parent_session_id, status, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_agent_team_runs_parent
                ON agent_team_runs(parent_session_id, status, started_at_ms);
            CREATE INDEX IF NOT EXISTS idx_agent_mission_runs_parent
                ON agent_mission_runs(parent_session_id, status, started_at_ms);
            CREATE INDEX IF NOT EXISTS idx_session_prompt_prefixes_latest
                ON session_prompt_prefixes(session_id, version DESC);
            CREATE INDEX IF NOT EXISTS idx_agent_mailbox_parent_pending
                ON agent_mailbox_events(parent_session_id, delivered_at_ms, created_at_ms);
            CREATE INDEX IF NOT EXISTS idx_session_compactions_latest
                ON session_compactions(session_id, created_after_session_seq, created_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_source_bindings_thread
                ON gateway_source_bindings(thread_id, updated_at_ms);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_gateway_runtime_bindings_native_session
                ON gateway_runtime_bindings(runtime_ref, native_session_id)
                WHERE native_session_id IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_gateway_runtime_bindings_parent
                ON gateway_runtime_bindings(parent_thread_id, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_activities_thread
                ON gateway_activities(thread_id, status, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_activities_source
                ON gateway_activities(source_key, status, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_activities_owner
                ON gateway_activities(owner_id, status, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_live_events_seq
                ON gateway_live_events(seq);
            CREATE INDEX IF NOT EXISTS idx_gateway_live_events_thread
                ON gateway_live_events(thread_id, seq);
            CREATE INDEX IF NOT EXISTS idx_gateway_live_snapshots_thread
                ON gateway_live_snapshots(thread_id, turn_id, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_live_snapshots_activity
                ON gateway_live_snapshots(activity_id, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_live_snapshots_owner
                ON gateway_live_snapshots(owner_id, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_control_commands_owner
                ON gateway_control_commands(owner_id, status, id);
            CREATE INDEX IF NOT EXISTS idx_gateway_turn_terminals_thread
                ON gateway_turn_terminals(thread_id, completed_at_ms);
            CREATE INDEX IF NOT EXISTS idx_automations_cwd_enabled_next
                ON automations(cwd, enabled, next_run_at_ms);
            CREATE INDEX IF NOT EXISTS idx_automation_runs_task
                ON automation_runs(automation_id, started_at_ms);
            CREATE INDEX IF NOT EXISTS idx_automation_runs_status
                ON automation_runs(automation_id, status, started_at_ms);
            "#,
        )?;
        conn.pragma_update(None, "user_version", SQLITE_SCHEMA_VERSION)?;
        Ok(Self {
            inner: Arc::new(SqliteStoreInner {
                conn: Mutex::new(conn),
                successful_writes: AtomicUsize::new(0),
            }),
        })
    }
}

#[derive(Debug)]
struct LegacyGatewayBindingEvidence {
    thread_id: String,
    backend_kind: String,
    backend_native_id: Option<String>,
    lineage: Option<Value>,
    metadata: Option<Value>,
    malformed_json: bool,
    cwd: String,
    parent_thread_id: Option<String>,
}

#[derive(Debug)]
struct LegacyGatewayBindingIdentity {
    runtime_ref: Option<String>,
    backend_kind: Option<String>,
    native_kind: Option<String>,
    native_session_id: Option<String>,
    unresolved_reason: &'static str,
}

fn migrate_sqlite_schema_v25_to_v26(conn: &Connection) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = conn.execute_batch(
        r#"
        ALTER TABLE gateway_runtime_bindings ADD COLUMN profile_config_json TEXT;
        UPDATE gateway_runtime_bindings
        SET resolution_status = 'unresolved',
            unresolved_reason = 'legacy_v25_profile_snapshot_required'
        WHERE resolution_status = 'resolved';
        PRAGMA user_version = 26;
        "#,
    );
    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error.into())
        }
    }
}

fn migrate_sqlite_schema_v24_to_v25(conn: &Connection) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = migrate_sqlite_schema_v24_to_v25_inner(conn);
    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn migrate_sqlite_schema_v24_to_v25_inner(conn: &Connection) -> Result<()> {
    let had_source_bindings = sqlite_table_exists(conn, "gateway_source_bindings")?;
    if had_source_bindings {
        conn.execute_batch(
            r#"
            ALTER TABLE gateway_source_bindings RENAME TO gateway_source_bindings_v24;
            "#,
        )?;
    }

    conn.execute_batch(
        r#"
        CREATE TABLE gateway_source_bindings (
            source_key TEXT PRIMARY KEY,
            source_kind TEXT NOT NULL,
            raw_identity_json TEXT NOT NULL,
            visible_name TEXT,
            thread_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
            backend_kind TEXT,
            backend_native_id TEXT,
            draft_runtime_ref TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            lineage_json TEXT
        );

        CREATE TABLE gateway_runtime_bindings (
            thread_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
            resolution_status TEXT NOT NULL CHECK (resolution_status IN ('resolved', 'unresolved')),
            runtime_ref TEXT,
            backend_kind TEXT,
            native_kind TEXT,
            native_session_id TEXT,
            cwd TEXT NOT NULL,
            profile_fingerprint TEXT,
            profile_revision TEXT,
            adapter_kind TEXT,
            adapter_revision TEXT,
            ownership TEXT NOT NULL CHECK (ownership IN ('read_write', 'read_only')),
            parent_thread_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
            binding_revision INTEGER NOT NULL CHECK (binding_revision > 0),
            unresolved_reason TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            CHECK (
                (resolution_status = 'resolved'
                    AND runtime_ref IS NOT NULL
                    AND backend_kind IS NOT NULL
                    AND native_kind IS NOT NULL
                    AND profile_fingerprint IS NOT NULL
                    AND profile_revision IS NOT NULL
                    AND adapter_kind IS NOT NULL
                    AND adapter_revision IS NOT NULL
                    AND unresolved_reason IS NULL)
                OR
                (resolution_status = 'unresolved' AND unresolved_reason IS NOT NULL)
            )
        );
        "#,
    )?;

    if had_source_bindings {
        let evidence = legacy_gateway_binding_evidence(conn)?;
        conn.execute_batch(
            r#"
            INSERT INTO gateway_source_bindings (
                source_key, source_kind, raw_identity_json, visible_name,
                thread_id, backend_kind, backend_native_id, draft_runtime_ref,
                created_at_ms, updated_at_ms, lineage_json
            )
            SELECT source_key, source_kind, raw_identity_json, visible_name,
                   thread_id, backend_kind, backend_native_id, NULL,
                   created_at_ms, updated_at_ms, lineage_json
            FROM gateway_source_bindings_v24;
            "#,
        )?;
        insert_legacy_runtime_bindings(conn, evidence)?;
        conn.execute_batch("DROP TABLE gateway_source_bindings_v24;")?;
    }

    conn.execute_batch(
        r#"
        CREATE INDEX idx_gateway_source_bindings_thread
            ON gateway_source_bindings(thread_id, updated_at_ms);
        CREATE UNIQUE INDEX idx_gateway_runtime_bindings_native_session
            ON gateway_runtime_bindings(runtime_ref, native_session_id)
            WHERE native_session_id IS NOT NULL;
        CREATE INDEX idx_gateway_runtime_bindings_parent
            ON gateway_runtime_bindings(parent_thread_id, updated_at_ms);
        PRAGMA user_version = 25;
        "#,
    )?;
    Ok(())
}

fn legacy_gateway_binding_evidence(conn: &Connection) -> Result<Vec<LegacyGatewayBindingEvidence>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT binding.thread_id, binding.backend_kind, binding.backend_native_id,
               binding.lineage_json, sessions.metadata_json, sessions.cwd,
               sessions.parent_session_id
        FROM gateway_source_bindings_v24 AS binding
        JOIN sessions ON sessions.id = binding.thread_id
        ORDER BY binding.thread_id, binding.source_key
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
        ))
    })?;
    let mut evidence = Vec::new();
    for row in rows {
        let (
            thread_id,
            backend_kind,
            backend_native_id,
            lineage_json,
            metadata_json,
            cwd,
            parent_thread_id,
        ) = row?;
        let (lineage, malformed_lineage) = parse_optional_legacy_json(lineage_json.as_deref());
        let (metadata, malformed_metadata) = parse_optional_legacy_json(metadata_json.as_deref());
        evidence.push(LegacyGatewayBindingEvidence {
            thread_id,
            backend_kind,
            backend_native_id,
            lineage,
            metadata,
            malformed_json: malformed_lineage || malformed_metadata,
            cwd,
            parent_thread_id,
        });
    }
    Ok(evidence)
}

fn parse_optional_legacy_json(value: Option<&str>) -> (Option<Value>, bool) {
    match value {
        Some(value) => match serde_json::from_str(value) {
            Ok(value) => (Some(value), false),
            Err(_) => (None, true),
        },
        None => (None, false),
    }
}

fn insert_legacy_runtime_bindings(
    conn: &Connection,
    evidence: Vec<LegacyGatewayBindingEvidence>,
) -> Result<()> {
    let mut by_thread = std::collections::BTreeMap::<String, Vec<_>>::new();
    for row in evidence {
        by_thread
            .entry(row.thread_id.clone())
            .or_default()
            .push(row);
    }
    let now = now_ms();
    let mut migrated = by_thread
        .into_iter()
        .map(|(thread_id, rows)| {
            let identity = legacy_gateway_binding_identity(&rows);
            let cwd = rows.first().map(|row| row.cwd.clone()).unwrap_or_default();
            let parent_thread_id =
                consistent_optional_value(rows.iter().map(|row| row.parent_thread_id.as_deref()));
            (thread_id, cwd, parent_thread_id, identity)
        })
        .collect::<Vec<_>>();
    let mut native_identity_counts = std::collections::BTreeMap::new();
    for (_, _, _, identity) in &migrated {
        if let (Some(runtime_ref), Some(native_session_id)) =
            (&identity.runtime_ref, &identity.native_session_id)
        {
            *native_identity_counts
                .entry((runtime_ref.clone(), native_session_id.clone()))
                .or_insert(0usize) += 1;
        }
    }
    for (_, _, _, identity) in &mut migrated {
        if let (Some(runtime_ref), Some(native_session_id)) =
            (&identity.runtime_ref, &identity.native_session_id)
            && native_identity_counts
                .get(&(runtime_ref.clone(), native_session_id.clone()))
                .copied()
                .unwrap_or_default()
                > 1
        {
            *identity = ambiguous_legacy_binding();
        }
    }
    for (thread_id, cwd, parent_thread_id, identity) in migrated {
        conn.execute(
            r#"
            INSERT INTO gateway_runtime_bindings (
                thread_id, resolution_status, runtime_ref, backend_kind,
                native_kind, native_session_id, cwd, profile_fingerprint,
                profile_revision, adapter_kind, adapter_revision, ownership,
                parent_thread_id, binding_revision, unresolved_reason,
                created_at_ms, updated_at_ms
            ) VALUES (
                ?1, 'unresolved', ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL,
                NULL, 'read_write', ?7, 1, ?8, ?9, ?9
            )
            "#,
            params![
                thread_id,
                identity.runtime_ref,
                identity.backend_kind,
                identity.native_kind,
                identity.native_session_id,
                cwd,
                parent_thread_id,
                identity.unresolved_reason,
                now,
            ],
        )?;
    }
    Ok(())
}

fn legacy_gateway_binding_identity(
    rows: &[LegacyGatewayBindingEvidence],
) -> LegacyGatewayBindingIdentity {
    if rows.is_empty()
        || rows.iter().any(|row| row.malformed_json)
        || !all_equal(rows.iter().map(|row| row.cwd.as_str()))
    {
        return ambiguous_legacy_binding();
    }
    let backend_kinds = rows
        .iter()
        .map(|row| row.backend_kind.as_str())
        .collect::<BTreeSet<_>>();
    if backend_kinds == BTreeSet::from(["psychevo"]) {
        let runtime_refs = rows
            .iter()
            .filter_map(|row| legacy_lineage_runtime_ref(row.lineage.as_ref()))
            .collect::<BTreeSet<_>>();
        if runtime_refs.iter().any(|value| *value != "native") {
            return ambiguous_legacy_binding();
        }
        let native_ids = rows
            .iter()
            .filter_map(|row| row.backend_native_id.as_deref())
            .collect::<BTreeSet<_>>();
        if native_ids.len() > 1 {
            return ambiguous_legacy_binding();
        }
        let native_session_id = native_ids.into_iter().next().map(str::to_string);
        return LegacyGatewayBindingIdentity {
            runtime_ref: Some("native".to_string()),
            backend_kind: Some("psychevo".to_string()),
            native_kind: Some("native".to_string()),
            native_session_id,
            unresolved_reason: "legacy_v24_profile_snapshot_required",
        };
    }
    if backend_kinds != BTreeSet::from(["peer_agent"]) {
        return ambiguous_legacy_binding();
    }

    let peer_metadata = rows
        .iter()
        .filter_map(|row| row.metadata.as_ref()?.get("peer_agent"))
        .collect::<Vec<_>>();
    let backend_ids = peer_metadata
        .iter()
        .filter(|peer| peer.get("backendKind").and_then(Value::as_str) == Some("acp"))
        .filter_map(|peer| peer.get("backendId").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .collect::<BTreeSet<_>>();
    if backend_ids.len() != 1 {
        return ambiguous_legacy_binding();
    }
    let backend_id = *backend_ids.iter().next().expect("one backend id");
    let lineage_refs = rows
        .iter()
        .filter_map(|row| legacy_lineage_runtime_ref(row.lineage.as_ref()))
        .collect::<BTreeSet<_>>();
    if lineage_refs
        .iter()
        .any(|value| *value != backend_id && *value != format!("acp:{backend_id}"))
    {
        return ambiguous_legacy_binding();
    }
    let native_ids = rows
        .iter()
        .filter_map(|row| row.backend_native_id.as_deref())
        .chain(
            peer_metadata
                .iter()
                .filter_map(|peer| peer.get("nativeSessionId").and_then(Value::as_str)),
        )
        .filter(|value| !value.trim().is_empty())
        .collect::<BTreeSet<_>>();
    if native_ids.len() > 1 {
        return ambiguous_legacy_binding();
    }
    LegacyGatewayBindingIdentity {
        runtime_ref: Some(format!("acp:{backend_id}")),
        backend_kind: Some("peer_agent".to_string()),
        native_kind: Some("acp".to_string()),
        native_session_id: native_ids.into_iter().next().map(str::to_string),
        unresolved_reason: "legacy_v24_profile_snapshot_required",
    }
}

fn legacy_lineage_runtime_ref(lineage: Option<&Value>) -> Option<&str> {
    lineage?
        .get("runtimeRef")
        .or_else(|| lineage?.get("runtime_ref"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn consistent_optional_value<'a>(values: impl Iterator<Item = Option<&'a str>>) -> Option<String> {
    let values = values.flatten().collect::<BTreeSet<_>>();
    (values.len() == 1).then(|| (*values.iter().next().expect("one value")).to_string())
}

fn all_equal<'a>(values: impl Iterator<Item = &'a str>) -> bool {
    let mut values = values;
    let Some(first) = values.next() else {
        return true;
    };
    values.all(|value| value == first)
}

fn ambiguous_legacy_binding() -> LegacyGatewayBindingIdentity {
    LegacyGatewayBindingIdentity {
        runtime_ref: None,
        backend_kind: None,
        native_kind: None,
        native_session_id: None,
        unresolved_reason: "legacy_v24_backend_ambiguous",
    }
}
