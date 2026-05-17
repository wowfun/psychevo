#[test]
fn sqlite_schema_v8_migrates_v3_state_databases() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("v3.db");
    let workdir = temp.path().join("work").to_string_lossy().to_string();
    {
        let conn = Connection::open(&db).expect("db");
        conn.execute_batch(
            r#"
            CREATE TABLE sessions (
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
                message_count INTEGER NOT NULL DEFAULT 0,
                tool_call_count INTEGER NOT NULL DEFAULT 0,
                title TEXT,
                metadata_json TEXT
            );

            CREATE TABLE messages (
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
                UNIQUE(session_id, session_seq)
            );

            CREATE INDEX idx_messages_session_seq
                ON messages(session_id, session_seq);
            "#,
        )
        .expect("schema");
        conn.execute(
            r#"
            INSERT INTO sessions (
                id, source, parent_session_id, workdir, model, provider,
                started_at_ms, updated_at_ms, ended_at_ms, end_reason,
                message_count, tool_call_count, title, metadata_json
            ) VALUES ('session-v3', 'run', NULL, ?1, 'model', 'provider',
                1, 2, NULL, NULL, 0, 0, NULL, NULL)
            "#,
            rusqlite::params![workdir],
        )
        .expect("session");
        conn.pragma_update(None, "user_version", 3)
            .expect("version");
    }

    let store = SqliteStore::open(&db).expect("migrate");
    let conn = Connection::open(&db).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(user_version, 8);
    let archived_at: Option<i64> = conn
        .query_row(
            "SELECT archived_at_ms FROM sessions WHERE id = 'session-v3'",
            [],
            |row| row.get(0),
        )
        .expect("archived");
    assert_eq!(archived_at, None);
    assert!(
        sqlite_columns(&conn, "messages")
            .iter()
            .any(|name| name == "estimated_cost_nanodollars")
    );
    assert!(
        sqlite_columns(&conn, "context_evidence")
            .iter()
            .any(|name| name == "source_kind")
    );
    assert!(
        sqlite_columns(&conn, "agent_edges")
            .iter()
            .any(|name| name == "child_session_id")
    );
    assert!(
        sqlite_columns(&conn, "context_evidence")
            .iter()
            .any(|name| name == "provider_group")
    );
    assert_eq!(
        store
            .latest_run_session_for_workdir(&temp.path().join("work"))
            .expect("latest"),
        Some("session-v3".to_string())
    );
}

#[test]
fn sqlite_schema_v8_migrates_v5_state_databases() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("v5.db");
    let workdir = temp.path().join("work").to_string_lossy().to_string();
    {
        let conn = Connection::open(&db).expect("db");
        conn.execute_batch(
            r#"
            CREATE TABLE sessions (
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

            CREATE TABLE messages (
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

            CREATE INDEX idx_messages_session_seq
                ON messages(session_id, session_seq);
            "#,
        )
        .expect("schema");
        conn.execute(
            r#"
            INSERT INTO sessions (
                id, source, parent_session_id, workdir, model, provider,
                started_at_ms, updated_at_ms, ended_at_ms, end_reason,
                archived_at_ms, message_count, tool_call_count, title, metadata_json
            ) VALUES ('session-v5', 'run', NULL, ?1, 'model', 'provider',
                1, 2, NULL, NULL, NULL, 0, 0, NULL, NULL)
            "#,
            rusqlite::params![workdir],
        )
        .expect("session");
        conn.pragma_update(None, "user_version", 5)
            .expect("version");
    }

    let store = SqliteStore::open(&db).expect("migrate");
    let conn = Connection::open(&db).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(user_version, 8);
    assert!(
        sqlite_columns(&conn, "context_evidence")
            .iter()
            .any(|name| name == "content_text")
    );
    assert!(
        sqlite_columns(&conn, "context_evidence")
            .iter()
            .any(|name| name == "provider_block_index")
    );
    assert_eq!(
        store
            .latest_run_session_for_workdir(&temp.path().join("work"))
            .expect("latest"),
        Some("session-v5".to_string())
    );
}

#[test]
fn sqlite_schema_v8_migrates_v6_state_databases() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("v6.db");
    let workdir = temp.path().join("work").to_string_lossy().to_string();
    {
        let conn = Connection::open(&db).expect("db");
        conn.execute_batch(
            r#"
            CREATE TABLE sessions (
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

            CREATE TABLE messages (
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

            CREATE TABLE context_evidence (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                prompt_session_seq INTEGER NOT NULL,
                context_seq INTEGER NOT NULL,
                role TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                source_name TEXT,
                source_path TEXT,
                timestamp_ms INTEGER NOT NULL,
                content_text TEXT NOT NULL,
                metadata_json TEXT,
                UNIQUE(session_id, prompt_session_seq, context_seq),
                FOREIGN KEY (session_id, prompt_session_seq)
                    REFERENCES messages(session_id, session_seq)
                    ON DELETE CASCADE
            );

            CREATE INDEX idx_messages_session_seq
                ON messages(session_id, session_seq);
            CREATE INDEX idx_context_evidence_prompt
                ON context_evidence(session_id, prompt_session_seq, context_seq);
            "#,
        )
        .expect("schema");
        conn.execute(
            r#"
            INSERT INTO sessions (
                id, source, parent_session_id, workdir, model, provider,
                started_at_ms, updated_at_ms, ended_at_ms, end_reason,
                archived_at_ms, message_count, tool_call_count, title, metadata_json
            ) VALUES ('session-v6', 'run', NULL, ?1, 'model', 'provider',
                1, 2, NULL, NULL, NULL, 0, 0, NULL, NULL)
            "#,
            rusqlite::params![workdir],
        )
        .expect("session");
        conn.pragma_update(None, "user_version", 6)
            .expect("version");
    }

    let store = SqliteStore::open(&db).expect("migrate");
    let conn = Connection::open(&db).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(user_version, 8);
    let columns = sqlite_columns(&conn, "context_evidence");
    assert!(columns.iter().any(|name| name == "provider_group"));
    assert!(columns.iter().any(|name| name == "provider_block_index"));
    assert!(columns.iter().any(|name| name == "context_kind"));
    assert_eq!(
        store
            .latest_run_session_for_workdir(&temp.path().join("work"))
            .expect("latest"),
        Some("session-v6".to_string())
    );
}

#[test]
fn sqlite_schema_v8_rejects_old_state_databases() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("old.db");
    {
        let conn = Connection::open(&db).expect("db");
        conn.pragma_update(None, "user_version", 1)
            .expect("version");
        conn.execute_batch("CREATE TABLE sessions (id TEXT);")
            .expect("schema");
    }

    let err = match SqliteStore::open(&db) {
        Ok(_) => panic!("old db opened successfully"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("schema version 1"));
    assert!(err.to_string().contains("--reset-state"));

    let v2_db = temp.path().join("v2.db");
    {
        let conn = Connection::open(&v2_db).expect("db");
        conn.pragma_update(None, "user_version", 2)
            .expect("version");
        conn.execute_batch("CREATE TABLE sessions (id TEXT);")
            .expect("schema");
    }
    let err = match SqliteStore::open(&v2_db) {
        Ok(_) => panic!("v2 db opened successfully"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("schema version 2"));
    assert!(err.to_string().contains("--reset-state"));
}

#[test]
fn sqlite_session_archive_restore_delete_filters_active_lists() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let first = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("second");
    store
        .append_message(&first, &user_message("hello", 1))
        .expect("message");

    store.archive_session(&first).expect("archive");
    assert_eq!(
        store
            .list_sessions_for_workdir_with_sources(&workdir, &["run"])
            .expect("active")
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        vec![second.as_str()]
    );
    assert_eq!(
        store
            .list_archived_sessions_for_workdir_with_sources(&workdir, &["run"])
            .expect("archived")
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        vec![first.as_str()]
    );

    store.archive_session(&second).expect("archive second");
    assert_eq!(
        store
            .latest_run_session_for_workdir(&workdir)
            .expect("latest"),
        None
    );

    store.restore_session(&first).expect("restore");
    assert_eq!(
        store
            .latest_run_session_for_workdir(&workdir)
            .expect("latest"),
        Some(first.clone())
    );

    store.delete_session(&first).expect("delete");
    assert!(store.session_summary(&first).expect("summary").is_none());
    assert!(store.load_messages(&first).expect("messages").is_empty());
}

#[test]
fn sqlite_resume_session_is_non_mutating_and_append_updates_recency() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let first = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("second");
    let conn = Connection::open(&db).expect("db");
    set_session_times(&conn, &first, 1_000, 1_000);
    set_session_times(&conn, &second, 2_000, 2_000);

    store.resume_session(&first).expect("resume");

    assert_eq!(session_updated_at(&store, &first), 1_000);
    assert_eq!(
        store
            .latest_run_session_for_workdir(&workdir)
            .expect("latest"),
        Some(second.clone())
    );

    store
        .append_message(&first, &user_message("new activity", 1))
        .expect("append");

    assert!(session_updated_at(&store, &first) > 2_000);
    assert_eq!(
        store
            .latest_run_session_for_workdir(&workdir)
            .expect("latest"),
        Some(first)
    );
}

#[test]
fn sqlite_archive_restore_preserve_activity_order() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let first = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("second");
    let conn = Connection::open(&db).expect("db");
    set_session_times(&conn, &first, 1_000, 1_000);
    set_session_times(&conn, &second, 2_000, 2_000);

    store.archive_session(&first).expect("archive");
    assert_eq!(session_updated_at(&store, &first), 1_000);
    store.restore_session(&first).expect("restore");

    assert_eq!(session_updated_at(&store, &first), 1_000);
    assert_eq!(
        store
            .list_sessions_for_workdir_with_sources(&workdir, &["run"])
            .expect("active")
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        vec![second.as_str(), first.as_str()]
    );
    assert_eq!(
        store
            .latest_run_session_for_workdir(&workdir)
            .expect("latest"),
        Some(second)
    );
}

#[test]
fn sqlite_append_to_archived_session_reopens_and_updates_recency() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let first = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("second");
    let conn = Connection::open(&db).expect("db");
    set_session_times(&conn, &first, 1_000, 1_000);
    set_session_times(&conn, &second, 2_000, 2_000);
    store
        .finish_session(&first, Outcome::Normal, None)
        .expect("finish");
    set_session_times(&conn, &first, 1_000, 1_000);
    store.archive_session(&first).expect("archive");

    store
        .append_message(&first, &user_message("reopen", 1))
        .expect("append");

    let summary = store
        .session_summary(&first)
        .expect("summary")
        .expect("session");
    assert!(summary.updated_at_ms > 2_000);
    assert_eq!(summary.ended_at_ms, None);
    assert_eq!(summary.end_reason, None);
    assert_eq!(summary.archived_at_ms, None);
    assert_eq!(
        store
            .latest_run_session_for_workdir(&workdir)
            .expect("latest"),
        Some(first)
    );
}

fn set_session_times(
    conn: &Connection,
    session_id: &str,
    started_at_ms: i64,
    updated_at_ms: i64,
) {
    conn.execute(
        "UPDATE sessions SET started_at_ms = ?2, updated_at_ms = ?3 WHERE id = ?1",
        rusqlite::params![session_id, started_at_ms, updated_at_ms],
    )
    .expect("set session times");
}

fn session_updated_at(store: &SqliteStore, session_id: &str) -> i64 {
    store
        .session_summary(session_id)
        .expect("summary")
        .expect("session")
        .updated_at_ms
}

#[test]
fn sqlite_context_evidence_is_prompt_scoped_and_hidden_from_messages() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("session");

    let prompt_seq = store
        .append_message_with_undo_snapshot_and_context_evidence(
            &session_id,
            &user_message("$reviewer check", 1),
            None,
            &[
                ContextEvidenceInput {
                    role: "system".to_string(),
                    source_kind: "system_instruction".to_string(),
                    source_name: Some("mode".to_string()),
                    source_path: None,
                    provider_group: Some("system_instructions".to_string()),
                    provider_block_index: Some(0),
                    context_kind: Some("system_instruction".to_string()),
                    content_text: "mode instruction".to_string(),
                    metadata: Some(json!({ "instruction_index": 0 })),
                },
                ContextEvidenceInput {
                    role: "user".to_string(),
                    source_kind: "selected_skill".to_string(),
                    source_name: Some("reviewer".to_string()),
                    source_path: Some("/tmp/reviewer/SKILL.md".to_string()),
                    provider_group: Some("selected_skill:0:reviewer".to_string()),
                    provider_block_index: Some(0),
                    context_kind: Some("selected_skill".to_string()),
                    content_text: "<skill>body</skill>".to_string(),
                    metadata: Some(json!({ "base_dir": "/tmp/reviewer" })),
                },
            ],
        )
        .expect("append");

    let messages = store.load_messages(&session_id).expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(store.session_summary(&session_id).unwrap().unwrap().message_count, 1);

    let evidence = store
        .load_context_evidence(&session_id, prompt_seq)
        .expect("evidence");
    assert_eq!(evidence.len(), 2);
    assert_eq!(evidence[0].source_name.as_deref(), Some("mode"));
    assert_eq!(
        evidence[0].provider_group.as_deref(),
        Some("system_instructions")
    );
    assert_eq!(evidence[0].provider_block_index, Some(0));
    assert_eq!(
        evidence[0].context_kind.as_deref(),
        Some("system_instruction")
    );
    assert_eq!(evidence[1].source_kind, "selected_skill");
    assert_eq!(evidence[1].source_path.as_deref(), Some("/tmp/reviewer/SKILL.md"));
    assert_eq!(
        evidence[1].provider_group.as_deref(),
        Some("selected_skill:0:reviewer")
    );
    assert_eq!(evidence[1].provider_block_index, Some(0));
    assert_eq!(evidence[1].context_kind.as_deref(), Some("selected_skill"));
    assert_eq!(evidence[1].content_text, "<skill>body</skill>");
}

#[test]
fn sqlite_user_message_display_metadata_overrides_content_text() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let display_text = "[Image #1] describe it";
    let metadata = json!({
        "undo": { "pre_snapshot": "snapshot-id" },
        "tui_display": {
            "content_text": display_text,
            "attachments": [
                {
                    "kind": "image",
                    "placeholder": "[Image #1]",
                    "source": "image.png"
                }
            ]
        }
    });

    store
        .append_message_with_undo_snapshot_metadata_and_context_evidence(
            &session_id,
            &user_message("describe it", 1),
            Some(metadata),
            Some(display_text.to_string()),
            &[],
        )
        .expect("append");

    let conn = Connection::open(&db).expect("db");
    let (content_text, metadata_json): (String, String) = conn
        .query_row(
            "SELECT content_text, metadata_json FROM messages WHERE session_id = ?1",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("message row");
    assert_eq!(content_text, display_text);
    let metadata: Value = serde_json::from_str(&metadata_json).expect("metadata");
    assert_eq!(
        metadata[crate::types::TUI_DISPLAY_METADATA_KEY]["attachments"][0]["source"],
        "image.png"
    );
}

#[test]
fn sqlite_agent_edges_round_trip_and_close_subtree() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let parent = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(&parent, &workdir, "agent", "model", "provider", None)
        .expect("child");
    let grandchild = store
        .create_child_session_with_metadata(&child, &workdir, "agent", "model", "provider", None)
        .expect("grandchild");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            AgentEdgeStatus::Open,
            Some(json!({"agent": {"id": "agent-1", "task_name": "review"}})),
        )
        .expect("edge");
    store
        .upsert_agent_edge(&child, &grandchild, AgentEdgeStatus::Open, None)
        .expect("grandchild edge");

    let found = store
        .find_agent_edge("review")
        .expect("find")
        .expect("edge found");
    assert_eq!(found.child_session_id, child);
    assert_eq!(found.status, AgentEdgeStatus::Open);

    store.close_agent_edge_subtree(&child).expect("close");
    let closed = store
        .find_agent_edge("agent-1")
        .expect("find")
        .expect("closed edge");
    assert_eq!(closed.status, AgentEdgeStatus::Closed);
    let descendants = store.list_agent_edges_for_parent(&child).expect("children");
    assert_eq!(descendants[0].status, AgentEdgeStatus::Closed);
}

#[test]
fn sqlite_context_evidence_cascades_with_prompt_messages() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("session");
    let prompt_seq = store
        .append_message_with_undo_snapshot_and_context_evidence(
            &session_id,
            &user_message("hello", 1),
            None,
            &[ContextEvidenceInput {
                role: "system".to_string(),
                source_kind: "system_instruction".to_string(),
                source_name: Some("mode".to_string()),
                source_path: None,
                provider_group: Some("system_instructions".to_string()),
                provider_block_index: Some(0),
                context_kind: Some("system_instruction".to_string()),
                content_text: "mode instruction".to_string(),
                metadata: None,
            }],
        )
        .expect("append");
    store
        .append_message(&session_id, &assistant_message("hi", 2))
        .expect("assistant");

    store
        .set_session_revert_state(
            &session_id,
            crate::store::SessionRevertState {
                start_seq: prompt_seq,
                original_snapshot: "snapshot".to_string(),
            },
        )
        .expect("revert");
    assert_eq!(
        store
            .load_context_evidence(&session_id, prompt_seq)
            .expect("before cleanup")
            .len(),
        1
    );

    store
        .cleanup_reverted_messages(&session_id)
        .expect("cleanup");
    assert!(
        store
            .load_context_evidence(&session_id, prompt_seq)
            .expect("after cleanup")
            .is_empty()
    );

    let next_seq = store
        .append_message_with_undo_snapshot_and_context_evidence(
            &session_id,
            &user_message("again", 3),
            None,
            &[ContextEvidenceInput {
                role: "system".to_string(),
                source_kind: "system_instruction".to_string(),
                source_name: Some("mode".to_string()),
                source_path: None,
                provider_group: Some("system_instructions".to_string()),
                provider_block_index: Some(0),
                context_kind: Some("system_instruction".to_string()),
                content_text: "mode instruction".to_string(),
                metadata: None,
            }],
        )
        .expect("append again");
    store.delete_session(&session_id).expect("delete");
    assert!(
        store
            .load_context_evidence(&session_id, next_seq)
            .expect("after delete")
            .is_empty()
    );
}

#[test]
fn sqlite_schema_v8_stores_reasoning_only_in_message_json_and_metrics_separately() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("session");
    store
        .append_message_with_metrics(
            &session_id,
            &Message::Assistant {
                content: vec![
                    AssistantBlock::Reasoning {
                        text: "folded".to_string(),
                        provider_evidence: Some(json!({
                            "reasoning_details": [{ "type": "thinking", "text": "opaque" }]
                        })),
                    },
                    AssistantBlock::Text {
                        text: "visible".to_string(),
                    },
                ],
                timestamp_ms: 1,
                finish_reason: Some("stop".to_string()),
                outcome: Outcome::Normal,
                model: Some("model".to_string()),
                provider: Some("provider".to_string()),
            },
            Some(json!({"total_tokens": 12, "input_tokens": 5, "output_tokens": 7})),
            Some(json!({"provider_response_id": "resp_1", "model": "model"})),
        )
        .expect("append");

    let conn = Connection::open(&db).expect("db");
    let columns = conn
        .prepare("PRAGMA table_info(messages)")
        .expect("schema stmt")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("schema rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("columns");
    assert!(!columns.iter().any(|name| name == "reasoning_json"));
    assert!(!columns.iter().any(|name| name == "reasoning_content"));
    assert!(!columns.iter().any(|name| name == "reasoning_details_json"));

    let (message_json, usage_json, metadata_json): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT message_json, usage_json, metadata_json FROM messages WHERE session_id = ?1",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("message row");
    let message: Value = serde_json::from_str(&message_json).expect("message");
    assert_eq!(message["content"][0]["type"], "reasoning");
    assert_eq!(message["content"][0]["text"], "folded");
    assert_eq!(
        message["content"][0]["provider_evidence"]["reasoning_details"][0]["type"],
        "thinking"
    );
    assert!(message.get("reasoning_content").is_none());
    assert!(message.get("reasoning_details").is_none());
    assert!(message.get("usage").is_none());
    assert!(message.get("metadata").is_none());

    let usage: Value = serde_json::from_str(&usage_json.expect("usage")).expect("usage json");
    let metadata: Value =
        serde_json::from_str(&metadata_json.expect("metadata")).expect("metadata json");
    assert_eq!(usage["total_tokens"], 12);
    assert_eq!(metadata["provider_response_id"], "resp_1");

    let summaries = store
        .load_sanitized_message_summaries(&session_id)
        .expect("summaries");
    assert_eq!(summaries[0].usage.as_ref().unwrap()["total_tokens"], 12);
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["provider_response_id"],
        "resp_1"
    );
    let sanitized = serde_json::to_string(&summaries[0].message).expect("sanitized");
    assert!(!sanitized.contains("folded"));

    let tui_summaries = store
        .load_tui_message_summaries(&session_id)
        .expect("tui summaries");
    let tui_message = serde_json::to_value(&tui_summaries[0].message).expect("tui message");
    assert_eq!(tui_message["content"][0]["type"], "reasoning");
    assert_eq!(tui_message["content"][0]["text"], "folded");
    assert!(tui_message["content"][0].get("provider_evidence").is_none());
}

#[test]
fn sqlite_stats_aggregate_accounting_columns() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "run", "mimo-v2.5-pro", "xiaomi", None)
        .expect("session");
    store
        .append_message_with_metrics_and_accounting(
            &session_id,
            &Message::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "done".to_string(),
                }],
                timestamp_ms: 1,
                finish_reason: Some("stop".to_string()),
                outcome: Outcome::Normal,
                model: Some("mimo-v2.5-pro".to_string()),
                provider: Some("xiaomi".to_string()),
            },
            Some(json!({
                "input_tokens": 120,
                "output_tokens": 30,
                "total_tokens": 150
            })),
            None,
            Some(MessageAccounting {
                context_input_tokens: Some(120),
                billable_input_tokens: Some(100),
                billable_output_tokens: Some(25),
                reasoning_tokens: Some(5),
                cache_read_tokens: Some(10),
                cache_write_tokens: Some(10),
                reported_total_tokens: Some(150),
                estimated_cost_nanodollars: Some(42),
                pricing_source: Some("test".to_string()),
                pricing_tier: Some("standard".to_string()),
            }),
        )
        .expect("append");

    let report = usage_stats(StatsOptions {
        db_path: db,
        workdir,
        all: false,
        days: None,
        limit: 5,
    })
    .expect("stats");
    assert_eq!(report["totals"]["estimated_cost_nanodollars"], 42);
    assert_eq!(report["totals"]["cache_write_tokens"], 10);
    assert_eq!(report["provider_models"][0]["model"], "mimo-v2.5-pro");
}

#[test]
fn accounting_uses_cache_reasoning_and_over_200k_pricing() {
    let metadata = ModelMetadata {
        cost: Some(ModelCost {
            input: Some(1.0),
            output: Some(2.0),
            cache_read: Some(0.1),
            cache_write: Some(0.2),
            context_over_200k: Some(ModelCostTier {
                input: Some(3.0),
                output: Some(4.0),
                cache_read: Some(0.3),
                cache_write: Some(0.4),
            }),
            source: Some("test-pricing".to_string()),
        }),
        ..Default::default()
    };
    let accounting = crate::accounting::account_usage(
        Some(&json!({
            "input_tokens": 250_020,
            "output_tokens": 30,
            "total_tokens": 250_050,
            "reasoning_tokens": 5,
            "cached_tokens": 10,
            "cache_write_tokens": 10
        })),
        &metadata,
    )
    .expect("accounting");
    assert_eq!(accounting.billable_input_tokens, Some(250_000));
    assert_eq!(accounting.billable_output_tokens, Some(25));
    assert_eq!(accounting.pricing_tier.as_deref(), Some("context_over_200k"));
    assert_eq!(accounting.pricing_source.as_deref(), Some("test-pricing"));
    assert_eq!(
        accounting.estimated_cost_nanodollars,
        Some(250_000 * 3_000 + 25 * 4_000 + 5 * 4_000 + 10 * 300 + 10 * 400)
    );
}

fn sqlite_columns(conn: &Connection, table: &str) -> Vec<String> {
    conn.prepare(&format!("PRAGMA table_info({table})"))
        .expect("schema stmt")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("schema rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("columns")
}
