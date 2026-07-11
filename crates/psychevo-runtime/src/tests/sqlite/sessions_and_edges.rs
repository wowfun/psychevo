#[allow(unused_imports)]
pub(crate) use super::*;
use crate::store::{PromptPrefixRecord, PromptPrefixSlotRecord};

#[test]
pub(crate) fn sqlite_schema_v25_rejects_pre_v24_state_databases() {
    for version in 1..=23 {
        let temp = tempdir().expect("temp");
        let db = temp.path().join(format!("v{version}.db"));
        {
            let conn = Connection::open(&db).expect("db");
            conn.execute_batch("CREATE TABLE sessions (id TEXT);")
                .expect("schema");
            conn.pragma_update(None, "user_version", version)
                .expect("version");
        }

        let err = match SqliteStore::open(&db) {
            Ok(_) => panic!("v{version} db opened successfully"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains(&format!("schema version {version}"))
        );
        assert!(err.to_string().contains("--reset-state"));
        assert!(err.to_string().contains("PSYCHEVO_DB"));
    }
}

#[test]
pub(crate) fn sqlite_schema_v25_rejects_unknown_state_database() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("old.db");
    {
        let conn = Connection::open(&db).expect("db");
        conn.pragma_update(None, "user_version", 99)
            .expect("version");
        conn.execute_batch("CREATE TABLE sessions (id TEXT);")
            .expect("schema");
    }

    let err = match SqliteStore::open(&db) {
        Ok(_) => panic!("old db opened successfully"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("schema version 99"));
    assert!(err.to_string().contains("--reset-state"));
}

#[test]
pub(crate) fn sqlite_schema_v26_stores_gateway_coordination_without_runtime_debug() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("session");
    let message_seq = store
        .append_message_with_undo_snapshot_and_context_evidence(
            &session_id,
            &user_message("show diff", 1),
            None,
            &[],
        )
        .expect("message");

    assert_eq!(message_seq, 1);
    let conn = Connection::open(&db).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(user_version, 26);
    assert!(sqlite_columns(&conn, "timeline_items").is_empty());
    assert!(sqlite_columns(&conn, "timeline_artifacts").is_empty());
    assert!(sqlite_columns(&conn, "timeline_debug_events").is_empty());
    assert!(sqlite_columns(&conn, "runtime_debug_events").is_empty());
    assert!(sqlite_columns(&conn, "capability_snapshots").is_empty());
    assert!(
        sqlite_columns(&conn, "session_prompt_prefixes")
            .iter()
            .any(|name| name == "slots_json")
    );
    assert!(
        sqlite_columns(&conn, "gateway_source_bindings")
            .iter()
            .any(|name| name == "raw_identity_json")
    );
    assert!(
        sqlite_columns(&conn, "gateway_source_bindings")
            .iter()
            .any(|name| name == "draft_runtime_ref")
    );
    assert!(
        sqlite_columns(&conn, "gateway_runtime_bindings")
            .iter()
            .any(|name| name == "binding_revision")
    );
    assert!(
        sqlite_columns(&conn, "gateway_turn_terminals")
            .iter()
            .any(|name| name == "completed_at_ms")
    );
    assert!(
        sqlite_columns(&conn, "gateway_live_snapshots")
            .iter()
            .any(|name| name == "revision")
    );
    assert!(
        sqlite_columns(&conn, "agent_team_runs")
            .iter()
            .any(|name| name == "mission_run_id")
    );
    assert!(
        sqlite_columns(&conn, "agent_mission_runs")
            .iter()
            .any(|name| name == "team_run_id")
    );
}

#[test]
pub(crate) fn sqlite_schema_v24_migrates_source_evidence_without_guessing_profile_snapshot() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("v24.db");
    {
        let conn = Connection::open(&db).expect("db");
        conn.execute_batch(
            r#"
            CREATE TABLE sessions (
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
            CREATE TABLE gateway_source_bindings (
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
            CREATE INDEX idx_gateway_source_bindings_thread
                ON gateway_source_bindings(thread_id, updated_at_ms);
            INSERT INTO sessions (
                id, source, cwd, model, provider, started_at_ms, updated_at_ms,
                metadata_json
            ) VALUES
                ('thread-native', 'web', '/work', 'model', 'provider', 1, 1, NULL),
                (
                    'thread-acp', 'peer_agent', '/work', 'opencode', 'acp:opencode', 2, 2,
                    '{"peer_agent":{"backendKind":"acp","backendId":"opencode","nativeSessionId":"native-acp"}}'
                ),
                ('thread-ambiguous', 'peer_agent', '/work', 'unknown', 'acp:unknown', 3, 3, NULL);
            INSERT INTO gateway_source_bindings (
                source_key, source_kind, raw_identity_json, visible_name,
                thread_id, backend_kind, backend_native_id, created_at_ms,
                updated_at_ms, lineage_json
            ) VALUES
                (
                    'web:native', 'web', '{}', 'Native', 'thread-native',
                    'psychevo', 'thread-native', 1, 1, '{"runtimeRef":"native"}'
                ),
                (
                    'web:acp', 'web', '{}', 'ACP', 'thread-acp',
                    'peer_agent', 'native-acp', 2, 2, '{"runtimeRef":"opencode"}'
                ),
                (
                    'web:ambiguous', 'web', '{}', 'Ambiguous', 'thread-ambiguous',
                    'peer_agent', 'native-unknown', 3, 3, '{"runtimeRef":"unknown"}'
                );
            PRAGMA user_version = 24;
            "#,
        )
        .expect("v24 schema");
    }

    let store = SqliteStore::open(&db).expect("migrate v24");
    let native = store
        .gateway_runtime_binding("thread-native")
        .expect("native binding")
        .expect("native binding exists");
    assert_eq!(native.status, GatewayRuntimeBindingStatus::Unresolved);
    assert_eq!(native.runtime_ref.as_deref(), Some("native"));
    assert_eq!(
        native.unresolved_reason.as_deref(),
        Some("legacy_v24_profile_snapshot_required")
    );

    let acp = store
        .gateway_runtime_binding("thread-acp")
        .expect("ACP binding")
        .expect("ACP binding exists");
    assert_eq!(acp.status, GatewayRuntimeBindingStatus::Unresolved);
    assert_eq!(acp.runtime_ref.as_deref(), Some("acp:opencode"));
    assert_eq!(acp.native_kind.as_deref(), Some("acp"));
    assert_eq!(acp.native_session_id.as_deref(), Some("native-acp"));
    assert_eq!(
        acp.unresolved_reason.as_deref(),
        Some("legacy_v24_profile_snapshot_required")
    );

    let ambiguous = store
        .gateway_runtime_binding("thread-ambiguous")
        .expect("ambiguous binding")
        .expect("ambiguous binding exists");
    assert_eq!(ambiguous.status, GatewayRuntimeBindingStatus::Unresolved);
    assert_eq!(ambiguous.runtime_ref, None);
    assert_eq!(ambiguous.native_session_id, None);
    assert_eq!(
        ambiguous.unresolved_reason.as_deref(),
        Some("legacy_v24_backend_ambiguous")
    );

    let lane = store
        .gateway_source_lane("web:acp")
        .expect("source lane")
        .expect("source lane exists");
    assert_eq!(lane.thread_id.as_deref(), Some("thread-acp"));
    assert_eq!(lane.draft_runtime_ref, None);

    let conn = Connection::open(&db).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("version");
    assert_eq!(user_version, 26);
}

#[test]
pub(crate) fn sqlite_schema_v25_marks_snapshotless_resolved_bindings_unresolved() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("v25.db");
    {
        let conn = Connection::open(&db).expect("db");
        conn.execute_batch(
            r#"
            CREATE TABLE sessions (
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
            INSERT INTO sessions (
                id, source, cwd, model, provider, started_at_ms, updated_at_ms
            ) VALUES ('thread-codex', 'web', '/work', 'pending', 'codex', 1, 1);
            INSERT INTO gateway_runtime_bindings (
                thread_id, resolution_status, runtime_ref, backend_kind,
                native_kind, native_session_id, cwd, profile_fingerprint,
                profile_revision, adapter_kind, adapter_revision, ownership,
                parent_thread_id, binding_revision, unresolved_reason,
                created_at_ms, updated_at_ms
            ) VALUES (
                'thread-codex', 'resolved', 'codex', 'runtime', 'codex',
                'native-secret', '/work', 'legacy-fingerprint', '1', 'codex',
                'legacy-adapter', 'read_write', NULL, 1, NULL, 1, 1
            );
            PRAGMA user_version = 25;
            "#,
        )
        .expect("v25 schema");
    }

    let store = SqliteStore::open(&db).expect("migrate v25");
    let binding = store
        .gateway_runtime_binding("thread-codex")
        .expect("binding")
        .expect("binding exists");
    assert_eq!(binding.status, GatewayRuntimeBindingStatus::Unresolved);
    assert_eq!(binding.runtime_ref.as_deref(), Some("codex"));
    assert_eq!(binding.profile_config_json, None);
    assert_eq!(
        binding.unresolved_reason.as_deref(),
        Some("legacy_v25_profile_snapshot_required")
    );
    let conn = Connection::open(&db).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("version");
    assert_eq!(user_version, 26);
}

fn gateway_runtime_binding_input<'a>(
    thread_id: &'a str,
    cwd: &'a str,
    runtime_ref: &'a str,
    native_session_id: Option<&'a str>,
) -> GatewayRuntimeBindingInput<'a> {
    GatewayRuntimeBindingInput {
        thread_id,
        runtime_ref,
        backend_kind: "runtime",
        native_kind: "codex",
        native_session_id,
        cwd,
        profile_fingerprint: "profile-fingerprint",
        profile_revision: "profile-revision",
        profile_config_json: "{}",
        adapter_kind: "codex",
        adapter_revision: "adapter-revision",
        ownership: GatewayRuntimeBindingOwnership::ReadWrite,
        parent_thread_id: None,
    }
}

#[test]
pub(crate) fn sqlite_runtime_binding_create_is_immutable_and_native_attach_is_cas() {
    let temp = tempdir().expect("temp");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let cwd_text = cwd.display().to_string();
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let thread_id = store
        .create_session_with_metadata(&cwd, "web", "pending", "pending", None)
        .expect("session");

    let created = store
        .create_gateway_runtime_binding(gateway_runtime_binding_input(
            &thread_id, &cwd_text, "codex", None,
        ))
        .expect("create binding");
    assert_eq!(created.binding_revision, 1);
    assert_eq!(created.native_session_id, None);
    let same = store
        .create_gateway_runtime_binding(gateway_runtime_binding_input(
            &thread_id, &cwd_text, "codex", None,
        ))
        .expect("idempotent create");
    assert_eq!(same, created);

    let conflict = store
        .create_gateway_runtime_binding(gateway_runtime_binding_input(
            &thread_id, &cwd_text, "opencode", None,
        ))
        .expect_err("immutable runtime identity");
    assert!(conflict.to_string().contains("bindings are immutable"));

    let attached = store
        .attach_gateway_runtime_native_session(&thread_id, 1, "native-codex")
        .expect("attach native session");
    assert_eq!(attached.binding_revision, 2);
    assert_eq!(attached.native_session_id.as_deref(), Some("native-codex"));
    let idempotent_from_pre_attach_revision = store
        .attach_gateway_runtime_native_session(&thread_id, 1, "native-codex")
        .expect("same native identity is idempotent across the attach revision");
    assert_eq!(idempotent_from_pre_attach_revision, attached);
    let idempotent = store
        .attach_gateway_runtime_native_session(&thread_id, 2, "native-codex")
        .expect("idempotent attach");
    assert_eq!(idempotent.binding_revision, 2);
    let native_conflict = store
        .attach_gateway_runtime_native_session(&thread_id, 2, "different-native")
        .expect_err("different native identity stays immutable");
    assert!(
        native_conflict
            .to_string()
            .contains("native session identity is immutable")
    );
    assert_eq!(
        store
            .gateway_runtime_binding_by_native_session("codex", "native-codex")
            .expect("native lookup")
            .expect("native binding"),
        attached
    );
}

#[test]
pub(crate) fn sqlite_runtime_binding_native_identity_is_unique_per_profile() {
    let temp = tempdir().expect("temp");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let cwd_text = cwd.display().to_string();
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let first = store
        .create_session_with_metadata(&cwd, "web", "pending", "pending", None)
        .expect("first session");
    let second = store
        .create_session_with_metadata(&cwd, "web", "pending", "pending", None)
        .expect("second session");

    store
        .create_gateway_runtime_binding(gateway_runtime_binding_input(
            &first,
            &cwd_text,
            "codex",
            Some("native-shared"),
        ))
        .expect("first binding");
    let error = store
        .create_gateway_runtime_binding(gateway_runtime_binding_input(
            &second,
            &cwd_text,
            "codex",
            Some("native-shared"),
        ))
        .expect_err("duplicate native identity");
    assert!(error.to_string().contains("UNIQUE constraint failed"));
}

#[test]
pub(crate) fn sqlite_source_lane_persists_draft_without_thread() {
    let temp = tempdir().expect("temp");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");

    let draft = store
        .upsert_gateway_source_lane(GatewaySourceLaneInput {
            source_key: "im.wechat:user-1",
            source_kind: "im.wechat",
            raw_identity: json!({"connectionId": "wechat", "chatId": "user-1"}),
            visible_name: Some("WeChat user 1"),
            thread_id: None,
            draft_runtime_ref: Some("codex"),
            lineage: None,
        })
        .expect("draft lane");
    assert_eq!(draft.thread_id, None);
    assert_eq!(draft.draft_runtime_ref.as_deref(), Some("codex"));
    assert!(
        store
            .gateway_source_binding("im.wechat:user-1")
            .expect("compat binding")
            .is_none()
    );

    let thread_id = store
        .create_session_with_metadata(&cwd, "channel", "pending", "pending", None)
        .expect("session");
    store
        .upsert_gateway_source_lane(GatewaySourceLaneInput {
            source_key: "im.wechat:user-1",
            source_kind: "im.wechat",
            raw_identity: json!({"connectionId": "wechat", "chatId": "user-1"}),
            visible_name: Some("WeChat user 1"),
            thread_id: Some(&thread_id),
            draft_runtime_ref: Some("codex"),
            lineage: None,
        })
        .expect("bound lane");
    assert!(
        store
            .clear_gateway_source_lane_thread("im.wechat:user-1")
            .expect("clear thread")
    );
    let cleared = store
        .gateway_source_lane("im.wechat:user-1")
        .expect("lane")
        .expect("lane exists");
    assert_eq!(cleared.thread_id, None);
    assert_eq!(cleared.draft_runtime_ref.as_deref(), Some("codex"));
}

#[test]
pub(crate) fn sqlite_agent_team_and_mission_runs_round_trip() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("session");
    let members = json!([
        {"id": "researcher", "agent": "general", "role": "research"},
        {"id": "tester", "agent": "general", "role": "verify", "maxTurns": 2}
    ]);

    let team = store
        .create_agent_team_run(AgentTeamRunInput {
            id: "team-run-1",
            parent_session_id: &session_id,
            mission_run_id: Some("mission-run-1"),
            team_name: "ship",
            description: Some("Ship a feature"),
            source_path: Some("/repo/.psychevo/teams/ship.md"),
            leader_agent_name: "general",
            members: members.clone(),
            max_parallel_agents: 4,
            status: "running",
            metadata: Some(json!({"source": "test"})),
        })
        .expect("team run");
    let mission = store
        .create_agent_mission_run(AgentMissionRunInput {
            id: "mission-run-1",
            parent_session_id: &session_id,
            team_run_id: Some(&team.id),
            team_name: Some("ship"),
            goal: "finish the feature",
            lead_agent_name: "general",
            status: "running",
            metadata: Some(json!({"source": "test"})),
        })
        .expect("mission run");

    assert_eq!(team.members, members);
    assert_eq!(mission.team_run_id.as_deref(), Some("team-run-1"));
    let active_team = store
        .find_active_agent_team_run(&session_id)
        .expect("active team")
        .expect("team");
    let active_mission = store
        .find_active_agent_mission_run(&session_id)
        .expect("active mission")
        .expect("mission");
    assert_eq!(active_team.id, "team-run-1");
    assert_eq!(active_mission.id, "mission-run-1");
}

#[test]
pub(crate) fn sqlite_gateway_source_binding_round_trips_and_rebinds() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let first_session = store
        .create_session_with_metadata(&cwd, "acp", "model", "provider", None)
        .expect("first session");
    let second_session = store
        .create_session_with_metadata(&cwd, "acp", "model", "provider", None)
        .expect("second session");

    let first = store
        .upsert_gateway_source_binding(GatewaySourceBindingInput {
            source_key: "acp:client-session",
            source_kind: "acp",
            raw_identity: json!({
                "kind": "acp",
                "session_id": "client-session",
            }),
            visible_name: Some("ACP client-session"),
            thread_id: &first_session,
            backend_kind: "psychevo",
            backend_native_id: Some(&first_session),
            lineage: Some(json!({ "source": "test" })),
        })
        .expect("insert binding");

    assert_eq!(first.source_key, "acp:client-session");
    assert_eq!(first.source_kind, "acp");
    assert_eq!(first.visible_name.as_deref(), Some("ACP client-session"));
    assert_eq!(first.thread_id, first_session);
    assert_eq!(first.backend_kind, "psychevo");
    assert_eq!(
        first.backend_native_id.as_deref(),
        Some(first_session.as_str())
    );
    assert_eq!(first.lineage, Some(json!({ "source": "test" })));

    let rebound = store
        .upsert_gateway_source_binding(GatewaySourceBindingInput {
            source_key: "acp:client-session",
            source_kind: "acp",
            raw_identity: json!({
                "kind": "acp",
                "session_id": "client-session",
                "reset": true,
            }),
            visible_name: Some("ACP client-session"),
            thread_id: &second_session,
            backend_kind: "psychevo",
            backend_native_id: Some(&second_session),
            lineage: Some(json!({ "reason": "gateway_reset" })),
        })
        .expect("rebind");

    assert_eq!(rebound.thread_id, second_session);
    assert_eq!(
        store
            .gateway_source_binding("acp:client-session")
            .expect("load binding")
            .expect("binding")
            .thread_id,
        second_session
    );
    assert!(
        store
            .delete_gateway_source_binding("acp:client-session")
            .expect("delete binding")
    );
    assert!(
        store
            .gateway_source_binding("acp:client-session")
            .expect("load deleted binding")
            .is_none()
    );
    assert!(
        !store
            .delete_gateway_source_binding("acp:client-session")
            .expect("delete missing binding")
    );

    store
        .mark_session_ended_with_reason(&first_session, "gateway_reset")
        .expect("end first session");
    store
        .archive_session(&first_session)
        .expect("archive first");
    let first_summary = store
        .session_summary(&first_session)
        .expect("summary")
        .expect("first session");
    assert_eq!(first_summary.end_reason.as_deref(), Some("gateway_reset"));
    assert!(first_summary.ended_at_ms.is_some());
    assert!(first_summary.archived_at_ms.is_some());
}

#[test]
pub(crate) fn sqlite_gateway_source_bindings_filter_channel_connection_id() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let wechat_session = store
        .create_session_with_metadata(&cwd, "channel", "model", "provider", None)
        .expect("wechat session");
    let telegram_session = store
        .create_session_with_metadata(&cwd, "channel", "model", "provider", None)
        .expect("telegram session");
    let web_session = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .expect("web session");

    store
        .upsert_gateway_source_binding(GatewaySourceBindingInput {
            source_key: "im.wechat:user-1",
            source_kind: "im.wechat",
            raw_identity: json!({
                "connectionId": "wechat",
                "chatId": "user-1",
            }),
            visible_name: Some("WeChat user 1"),
            thread_id: &wechat_session,
            backend_kind: "psychevo",
            backend_native_id: Some(&wechat_session),
            lineage: None,
        })
        .expect("wechat binding");
    store
        .upsert_gateway_source_binding(GatewaySourceBindingInput {
            source_key: "im.telegram:user-1",
            source_kind: "im.telegram",
            raw_identity: json!({
                "connectionId": "telegram",
                "chatId": "user-1",
            }),
            visible_name: Some("Telegram user 1"),
            thread_id: &telegram_session,
            backend_kind: "psychevo",
            backend_native_id: Some(&telegram_session),
            lineage: None,
        })
        .expect("telegram binding");
    store
        .upsert_gateway_source_binding(GatewaySourceBindingInput {
            source_key: "web:user-1",
            source_kind: "web",
            raw_identity: json!({
                "connectionId": "wechat",
                "chatId": "user-1",
            }),
            visible_name: Some("Workbench user 1"),
            thread_id: &web_session,
            backend_kind: "psychevo",
            backend_native_id: Some(&web_session),
            lineage: None,
        })
        .expect("web binding");

    let bindings = store
        .gateway_source_bindings_for_connection_id("wechat")
        .expect("bindings");
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].source_key, "im.wechat:user-1");
    assert_eq!(bindings[0].thread_id, wechat_session);
}

#[test]
pub(crate) fn sqlite_prompt_prefixes_round_trip_by_session_and_version() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("session");

    let first = store
        .upsert_session_prompt_prefix(PromptPrefixRecord {
            session_id: session_id.clone(),
            version: 0,
            created_at_ms: 10,
            provider: "provider".to_string(),
            model: "model".to_string(),
            prefix_hash: "prefix-a".to_string(),
            tool_declarations_hash: "tools-a".to_string(),
            invalidation_reason: Some("new_session".to_string()),
            slots: vec![PromptPrefixSlotRecord {
                slot: "base/mode".to_string(),
                tier: "base".to_string(),
                semantic_role: "instruction".to_string(),
                provider_role: "system".to_string(),
                order: 0,
                content: "mode a".to_string(),
                content_hash: "hash-a".to_string(),
                source_kind: Some("runtime".to_string()),
                source_name: Some("mode".to_string()),
                source_path: None,
            }],
            metadata: Some(json!({ "effective_tools": ["read"] })),
        })
        .expect("first prefix");
    let second = store
        .upsert_session_prompt_prefix(PromptPrefixRecord {
            session_id: session_id.clone(),
            version: 0,
            created_at_ms: 20,
            provider: "provider".to_string(),
            model: "model".to_string(),
            prefix_hash: "prefix-b".to_string(),
            tool_declarations_hash: "tools-b".to_string(),
            invalidation_reason: Some("runtime_context_changed".to_string()),
            slots: vec![PromptPrefixSlotRecord {
                slot: "base/mode".to_string(),
                tier: "base".to_string(),
                semantic_role: "instruction".to_string(),
                provider_role: "system".to_string(),
                order: 0,
                content: "mode b".to_string(),
                content_hash: "hash-b".to_string(),
                source_kind: Some("runtime".to_string()),
                source_name: Some("mode".to_string()),
                source_path: None,
            }],
            metadata: Some(json!({ "effective_tools": ["read", "write"] })),
        })
        .expect("second prefix");

    assert_eq!(first.version, 1);
    assert_eq!(second.version, 2);
    assert_eq!(
        store
            .load_session_prompt_prefix_version(&session_id, 1)
            .expect("load v1")
            .expect("v1")
            .prefix_hash,
        "prefix-a"
    );
    assert_eq!(
        store
            .load_session_prompt_prefix(&session_id)
            .expect("load latest")
            .expect("latest")
            .prefix_hash,
        "prefix-b"
    );
}

#[test]
pub(crate) fn sqlite_session_archive_restore_delete_filters_active_lists() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let first = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("second");
    store
        .append_message(&first, &user_message("hello", 1))
        .expect("message");

    store.archive_session(&first).expect("archive");
    assert_eq!(
        store
            .list_sessions_for_cwd_with_sources(&cwd, &["run"])
            .expect("active")
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        vec![second.as_str()]
    );
    assert_eq!(
        store
            .list_archived_sessions_for_cwd_with_sources(&cwd, &["run"])
            .expect("archived")
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        vec![first.as_str()]
    );

    store.archive_session(&second).expect("archive second");
    assert_eq!(
        store.latest_run_session_for_cwd(&cwd).expect("latest"),
        None
    );

    store.restore_session(&first).expect("restore");
    assert_eq!(
        store.latest_run_session_for_cwd(&cwd).expect("latest"),
        Some(first.clone())
    );

    store.delete_session(&first).expect("delete");
    assert!(store.session_summary(&first).expect("summary").is_none());
    assert!(store.load_messages(&first).expect("messages").is_empty());
}

#[test]
pub(crate) fn sqlite_resume_session_is_non_mutating_and_append_updates_recency() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let first = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("second");
    let conn = Connection::open(&db).expect("db");
    set_session_times(&conn, &first, 1_000, 1_000);
    set_session_times(&conn, &second, 2_000, 2_000);

    store.resume_session(&first).expect("resume");

    assert_eq!(session_updated_at(&store, &first), 1_000);
    assert_eq!(
        store.latest_run_session_for_cwd(&cwd).expect("latest"),
        Some(second.clone())
    );

    store
        .append_message(&first, &user_message("new activity", 1))
        .expect("append");

    assert!(session_updated_at(&store, &first) > 2_000);
    assert_eq!(
        store.latest_run_session_for_cwd(&cwd).expect("latest"),
        Some(first)
    );
}

#[test]
pub(crate) fn sqlite_archive_restore_preserve_activity_order() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let first = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
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
            .list_sessions_for_cwd_with_sources(&cwd, &["run"])
            .expect("active")
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        vec![second.as_str(), first.as_str()]
    );
    assert_eq!(
        store.latest_run_session_for_cwd(&cwd).expect("latest"),
        Some(second)
    );
}

#[test]
pub(crate) fn sqlite_append_to_archived_session_reopens_and_updates_recency() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let first = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
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
        store.latest_run_session_for_cwd(&cwd).expect("latest"),
        Some(first)
    );
}

pub(crate) fn set_session_times(
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

pub(crate) fn session_updated_at(store: &SqliteStore, session_id: &str) -> i64 {
    store
        .session_summary(session_id)
        .expect("summary")
        .expect("session")
        .updated_at_ms
}

#[test]
pub(crate) fn sqlite_context_evidence_is_prompt_scoped_and_hidden_from_messages() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
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
    assert_eq!(
        store
            .session_summary(&session_id)
            .unwrap()
            .unwrap()
            .message_count,
        1
    );

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
    assert_eq!(
        evidence[1].source_path.as_deref(),
        Some("/tmp/reviewer/SKILL.md")
    );
    assert_eq!(
        evidence[1].provider_group.as_deref(),
        Some("selected_skill:0:reviewer")
    );
    assert_eq!(evidence[1].provider_block_index, Some(0));
    assert_eq!(evidence[1].context_kind.as_deref(), Some("selected_skill"));
    assert_eq!(evidence[1].content_text, "<skill>body</skill>");
}

#[test]
pub(crate) fn sqlite_user_message_display_metadata_overrides_content_text() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
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
pub(crate) fn sqlite_agent_edges_round_trip_and_close_subtree() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let parent = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(&parent, &cwd, "agent", "model", "provider", None)
        .expect("child");
    let grandchild = store
        .create_child_session_with_metadata(&child, &cwd, "agent", "model", "provider", None)
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
pub(crate) fn sqlite_context_evidence_cascades_with_prompt_messages() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
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
