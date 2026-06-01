#[allow(unused_imports)]
pub(crate) use super::*;
use crate::capabilities::{
    CapabilityCategory, CapabilityContributionRecord, CapabilitySnapshot, CapabilitySnapshotParts,
    source_record,
};

#[test]
pub(crate) fn sqlite_schema_v15_rejects_old_state_databases() {
    for version in 1..=14 {
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
pub(crate) fn sqlite_schema_v15_rejects_unknown_state_database() {
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
pub(crate) fn sqlite_schema_v15_stores_timeline_capability_snapshots_and_gateway_bindings() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("session");
    let message_seq = store
        .append_message_with_undo_snapshot_and_context_evidence(
            &session_id,
            &user_message("show diff", 1),
            None,
            &[],
        )
        .expect("message");

    let item = store
        .upsert_timeline_item(TimelineItemInput {
            session_id: session_id.clone(),
            item_id: "diff:1".to_string(),
            turn_id: Some("turn:1".to_string()),
            kind: TimelineItemKind::Diff,
            status: TimelineItemStatus::Completed,
            source: "slash:/diff".to_string(),
            title: Some("D I F F".to_string()),
            body_text: Some("diff --git a/a b/a\n".to_string()),
            preview_text: Some("a/a".to_string()),
            detail_text: None,
            artifact_ids: vec!["artifact:diff:1".to_string()],
            metadata: Some(json!({
                "message_session_seq": message_seq,
                "truncated": false,
            })),
        })
        .expect("timeline item");
    store
        .upsert_timeline_artifact(TimelineArtifactInput {
            session_id: session_id.clone(),
            artifact_id: "artifact:diff:1".to_string(),
            kind: "diff".to_string(),
            mime_type: Some("text/x-diff".to_string()),
            title: Some("D I F F".to_string()),
            preview_text: Some("diff --git a/a b/a".to_string()),
            path: None,
            metadata: Some(json!({ "truncated": false })),
        })
        .expect("timeline artifact");
    store
        .append_timeline_debug_event(TimelineDebugEventInput {
            session_id: session_id.clone(),
            turn_id: Some("turn:1".to_string()),
            event_type: "hook.completed".to_string(),
            source: "test".to_string(),
            scope: Some(json!({ "tool": "diff" })),
            status: Some("ok".to_string()),
            summary: Some("hook finished".to_string()),
            payload: Some(json!({ "duration_ms": 5 })),
        })
        .expect("debug event");

    assert_eq!(item.item_seq, 1);
    let conn = Connection::open(&db).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(user_version, 15);
    assert!(
        sqlite_columns(&conn, "timeline_items")
            .iter()
            .any(|name| name == "kind")
    );
    assert!(
        sqlite_columns(&conn, "timeline_artifacts")
            .iter()
            .any(|name| name == "preview_text")
    );
    assert!(
        sqlite_columns(&conn, "timeline_debug_events")
            .iter()
            .any(|name| name == "payload_json")
    );
    assert!(
        sqlite_columns(&conn, "capability_snapshots")
            .iter()
            .any(|name| name == "snapshot_json")
    );
    assert!(
        sqlite_columns(&conn, "gateway_source_bindings")
            .iter()
            .any(|name| name == "raw_identity_json")
    );
    let items = store.load_timeline_items(&session_id).expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, TimelineItemKind::Diff);
    assert_eq!(items[0].status, TimelineItemStatus::Completed);
    assert_eq!(items[0].artifact_ids, vec!["artifact:diff:1"]);
    assert_eq!(
        items[0].metadata,
        Some(json!({
            "message_session_seq": message_seq,
            "truncated": false,
        }))
    );
    let artifacts = store
        .load_timeline_artifacts(&session_id)
        .expect("artifacts");
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].artifact_id, "artifact:diff:1");
    let debug = store
        .load_timeline_debug_events(&session_id, 10)
        .expect("debug events");
    assert_eq!(debug.len(), 1);
    assert_eq!(debug[0].event_type, "hook.completed");
    assert_eq!(debug[0].payload, Some(json!({ "duration_ms": 5 })));
}

#[test]
pub(crate) fn sqlite_gateway_source_binding_round_trips_and_rebinds() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let first_session = store
        .create_session_with_metadata(&workdir, "acp", "model", "provider", None)
        .expect("first session");
    let second_session = store
        .create_session_with_metadata(&workdir, "acp", "model", "provider", None)
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
pub(crate) fn sqlite_capability_snapshot_round_trips_by_session_and_prefix_version() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("session");

    let mut parts = CapabilitySnapshotParts::default();
    parts.push_source(source_record(
        "provider:mock",
        "provider_adapter",
        "mock",
        Some("Mock".to_string()),
        "session_snapshot",
        None,
    ));
    parts.push_selected(CapabilityContributionRecord {
        id: "provider:mock:model:fake".to_string(),
        source_id: "provider:mock".to_string(),
        category: CapabilityCategory::Provider,
        raw_name: "mock/fake".to_string(),
        visible_name: Some("mock/fake".to_string()),
        exposure: None,
        status: "selected".to_string(),
        reason: None,
        metadata: Some(json!({ "model": "fake" })),
    });
    let snapshot = CapabilitySnapshot::from_parts(session_id.clone(), 7, 42, parts);
    store
        .upsert_capability_snapshot(&snapshot)
        .expect("snapshot");

    let loaded = store
        .load_capability_snapshot(&session_id, 7)
        .expect("load snapshot")
        .expect("snapshot exists");
    assert_eq!(loaded, snapshot);
    assert_eq!(
        store
            .load_latest_capability_snapshot(&session_id)
            .expect("latest snapshot"),
        Some(snapshot)
    );
}

#[test]
pub(crate) fn sqlite_session_archive_restore_delete_filters_active_lists() {
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
pub(crate) fn sqlite_resume_session_is_non_mutating_and_append_updates_recency() {
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
pub(crate) fn sqlite_archive_restore_preserve_activity_order() {
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
pub(crate) fn sqlite_append_to_archived_session_reopens_and_updates_recency() {
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
pub(crate) fn sqlite_agent_edges_round_trip_and_close_subtree() {
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
pub(crate) fn sqlite_context_evidence_cascades_with_prompt_messages() {
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
