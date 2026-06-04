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
        let user_version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let has_schema =
            sqlite_table_exists(&conn, "sessions")? || sqlite_table_exists(&conn, "messages")?;
        if user_version != 0 && user_version != SQLITE_SCHEMA_VERSION {
            return Err(Error::Config(format!(
                "state database schema version {user_version} is not supported; run `pevo init --reset-state` or set PSYCHEVO_DB to a new state database"
            )));
        }
        if user_version == 0 && has_schema {
            return Err(Error::Config(
                "state database has an unknown schema version; run `pevo init --reset-state` or set PSYCHEVO_DB to a new state database".to_string(),
            ));
        }
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                parent_session_id TEXT,
                workdir TEXT NOT NULL,
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
                thread_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                backend_kind TEXT NOT NULL,
                backend_native_id TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                lineage_json TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session_seq
                ON messages(session_id, session_seq);
            CREATE INDEX IF NOT EXISTS idx_context_evidence_prompt
                ON context_evidence(session_id, prompt_session_seq, context_seq);
            CREATE INDEX IF NOT EXISTS idx_agent_edges_parent
                ON agent_edges(parent_session_id, status, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_session_prompt_prefixes_latest
                ON session_prompt_prefixes(session_id, version DESC);
            CREATE INDEX IF NOT EXISTS idx_agent_mailbox_parent_pending
                ON agent_mailbox_events(parent_session_id, delivered_at_ms, created_at_ms);
            CREATE INDEX IF NOT EXISTS idx_session_compactions_latest
                ON session_compactions(session_id, created_after_session_seq, created_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_source_bindings_thread
                ON gateway_source_bindings(thread_id, updated_at_ms);
            "#,
        )?;
        if !sqlite_column_exists(&conn, "context_evidence", "provider_group")? {
            conn.execute_batch("ALTER TABLE context_evidence ADD COLUMN provider_group TEXT;")?;
        }
        if !sqlite_column_exists(&conn, "context_evidence", "provider_block_index")? {
            conn.execute_batch(
                "ALTER TABLE context_evidence ADD COLUMN provider_block_index INTEGER;",
            )?;
        }
        if !sqlite_column_exists(&conn, "context_evidence", "context_kind")? {
            conn.execute_batch("ALTER TABLE context_evidence ADD COLUMN context_kind TEXT;")?;
        }
        conn.pragma_update(None, "user_version", SQLITE_SCHEMA_VERSION)?;
        Ok(Self {
            inner: Arc::new(SqliteStoreInner {
                conn: Mutex::new(conn),
                successful_writes: AtomicUsize::new(0),
            }),
        })
    }
}
