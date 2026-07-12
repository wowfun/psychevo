#[allow(unused_imports)]
pub(crate) use super::*;
use crate::store::{PromptPrefixRecord, PromptPrefixSlotRecord};
use psychevo_agent_core::now_ms;

#[test]
pub(crate) fn sqlite_schema_v28_rejects_legacy_state_databases_with_reset_guidance() {
    for version in 1..=27 {
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
        assert!(err.to_string().contains("pevo init --reset-state"));
        assert!(err.to_string().contains("PSYCHEVO_DB"));
    }
}

#[test]
pub(crate) fn sqlite_schema_v28_rejects_unknown_state_database() {
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
    assert!(err.to_string().contains("pevo init --reset-state"));
}

#[test]
pub(crate) fn sqlite_schema_v28_creates_current_gateway_coordination_schema() {
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
    assert_eq!(user_version, 28);
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
            .any(|name| name == "draft_profile_ref")
    );
    assert!(
        sqlite_columns(&conn, "gateway_source_bindings")
            .iter()
            .any(|name| name == "draft_control_values_json")
    );
    assert!(
        sqlite_columns(&conn, "gateway_runtime_bindings")
            .iter()
            .any(|name| name == "binding_revision")
    );
    for column in [
        "agent_ref",
        "agent_fingerprint",
        "agent_definition_json",
        "thread_preferences_json",
        "runtime_observed_json",
        "control_revision",
    ] {
        assert!(
            sqlite_columns(&conn, "gateway_runtime_bindings")
                .iter()
                .any(|name| name == column),
            "missing gateway_runtime_bindings.{column}"
        );
    }
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
        sqlite_columns(&conn, "gateway_turn_deliveries")
            .iter()
            .any(|name| name == "input_hash")
    );
    assert!(
        sqlite_columns(&conn, "gateway_channel_outbox")
            .iter()
            .any(|name| name == "payload_hash")
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

fn gateway_runtime_binding_input<'a>(
    thread_id: &'a str,
    cwd: &'a str,
    runtime_ref: &'a str,
    native_session_id: Option<&'a str>,
) -> GatewayRuntimeBindingInput<'a> {
    GatewayRuntimeBindingInput {
        thread_id,
        agent_ref: Some("reviewer"),
        agent_fingerprint: "agent-fingerprint",
        agent_definition_json: r#"{"name":"reviewer"}"#,
        runtime_ref,
        backend_kind: "acp",
        native_kind: "acp",
        native_session_id,
        cwd,
        profile_fingerprint: "profile-fingerprint",
        profile_revision: "profile-revision",
        profile_config_json: "{}",
        adapter_kind: "acp",
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
    assert_eq!(created.agent_ref.as_deref(), Some("reviewer"));
    assert_eq!(
        created.agent_fingerprint.as_deref(),
        Some("agent-fingerprint")
    );
    assert_eq!(created.native_session_id, None);
    let same = store
        .create_gateway_runtime_binding(gateway_runtime_binding_input(
            &thread_id, &cwd_text, "codex", None,
        ))
        .expect("idempotent create");
    assert_eq!(same, created);

    let mut different_agent = gateway_runtime_binding_input(&thread_id, &cwd_text, "codex", None);
    different_agent.agent_ref = Some("other-reviewer");
    different_agent.agent_fingerprint = "other-agent-fingerprint";
    different_agent.agent_definition_json = r#"{"name":"other-reviewer"}"#;
    let agent_conflict = store
        .create_gateway_runtime_binding(different_agent)
        .expect_err("immutable Agent Definition identity");
    assert!(
        agent_conflict
            .to_string()
            .contains("bindings are immutable")
    );

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
pub(crate) fn sqlite_runtime_control_state_separates_preferences_observations_and_cas_revision() {
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
    assert_eq!(created.control_revision, 1);
    assert!(created.thread_preferences.is_empty());
    assert!(created.runtime_observed.is_empty());

    let preferences = BTreeMap::from([
        ("model".to_string(), json!("gpt-5")),
        ("reasoning".to_string(), json!("high")),
    ]);
    let stored = store
        .compare_and_set_gateway_runtime_control_state(
            &thread_id,
            1,
            1,
            GatewayRuntimeControlStatePatch {
                thread_preferences: Some(&preferences),
                runtime_observed: None,
            },
        )
        .expect("store preferences");
    assert_eq!(stored.binding_revision, 1);
    assert_eq!(stored.control_revision, 2);
    assert_eq!(stored.thread_preferences, preferences);
    assert!(stored.runtime_observed.is_empty());

    let idempotent = store
        .compare_and_set_gateway_runtime_control_state(
            &thread_id,
            1,
            2,
            GatewayRuntimeControlStatePatch {
                thread_preferences: Some(&preferences),
                runtime_observed: None,
            },
        )
        .expect("same preference is idempotent");
    assert_eq!(idempotent.control_revision, 2);

    let stale = store
        .compare_and_set_gateway_runtime_control_state(
            &thread_id,
            1,
            1,
            GatewayRuntimeControlStatePatch {
                thread_preferences: Some(&preferences),
                runtime_observed: None,
            },
        )
        .expect_err("stale control revision");
    assert!(stale.to_string().contains("stale runtime control revision"));

    let observed = BTreeMap::from([
        ("model".to_string(), json!("gpt-5")),
        ("reasoning".to_string(), json!("medium")),
    ]);
    let observed_record = store
        .compare_and_set_gateway_runtime_control_state(
            &thread_id,
            1,
            2,
            GatewayRuntimeControlStatePatch {
                thread_preferences: None,
                runtime_observed: Some(&observed),
            },
        )
        .expect("record runtime observation");
    assert_eq!(observed_record.control_revision, 3);
    assert_eq!(observed_record.thread_preferences, preferences);
    assert_eq!(observed_record.runtime_observed, observed);

    let attached = store
        .attach_gateway_runtime_native_session(&thread_id, 1, "native-codex")
        .expect("binding revision advances independently");
    assert_eq!(attached.binding_revision, 2);
    assert_eq!(attached.control_revision, 3);
    let stale_binding = store
        .compare_and_set_gateway_runtime_control_state(
            &thread_id,
            1,
            3,
            GatewayRuntimeControlStatePatch {
                thread_preferences: Some(&BTreeMap::new()),
                runtime_observed: None,
            },
        )
        .expect_err("stale binding revision");
    assert!(
        stale_binding
            .to_string()
            .contains("stale runtime binding revision")
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

    let draft_control_values = BTreeMap::from([
        ("model".to_string(), "gpt-fixture".to_string()),
        ("effort".to_string(), "high".to_string()),
    ]);
    let draft = store
        .upsert_gateway_source_lane(GatewaySourceLaneInput {
            source_key: "im.wechat:user-1",
            source_kind: "im.wechat",
            raw_identity: json!({"connectionId": "wechat", "chatId": "user-1"}),
            visible_name: Some("WeChat user 1"),
            thread_id: None,
            draft_agent_ref: Some("reviewer"),
            draft_profile_ref: Some("codex"),
            draft_control_values: &draft_control_values,
            lineage: None,
        })
        .expect("draft lane");
    assert_eq!(draft.thread_id, None);
    assert_eq!(draft.draft_agent_ref.as_deref(), Some("reviewer"));
    assert_eq!(draft.draft_profile_ref.as_deref(), Some("codex"));
    assert_eq!(draft.draft_control_values, draft_control_values);
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
            draft_agent_ref: None,
            draft_profile_ref: None,
            draft_control_values: &Default::default(),
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
    assert_eq!(cleared.draft_profile_ref, None);
    assert!(cleared.draft_control_values.is_empty());
}

#[test]
pub(crate) fn sqlite_turn_delivery_and_channel_outbox_erase_confirmed_payloads() {
    let temp = tempdir().expect("temp");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let thread_id = store
        .create_session_with_metadata(&cwd, "channel", "pending", "pending", None)
        .expect("session");

    store
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: "turn-1",
            thread_id: Some(&thread_id),
            source_key: Some("agent:test-1"),
            turn_id: Some("turn-1"),
            kind: "turn",
            owner_id: "owner",
            owner_surface: Some("test"),
            lease_expires_at_ms: now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: Some(json!({
                "kind": "turn",
                "threadId": thread_id,
                "runtimeSource": "test",
                "input": [{"type": "text", "text": "private prompt"}],
            })),
        })
        .expect("activity intent");

    let not_delivered = store
        .insert_gateway_turn_delivery(GatewayTurnDeliveryInput {
            turn_id: "turn-1",
            thread_id: &thread_id,
            runtime_ref: "agent-profile",
            input_json: r#"[{"type":"text","text":"private prompt"}]"#,
            input_hash: "input-hash",
        })
        .expect("delivery");
    assert_eq!(not_delivered.status, "not_delivered");
    assert!(
        not_delivered
            .input_json
            .as_deref()
            .is_some_and(|value| value.contains("private prompt"))
    );
    assert!(
        store
            .mark_gateway_turn_delivery_unknown("turn-1")
            .expect("unknown")
    );
    assert!(
        store
            .gateway_turn_delivery("turn-1")
            .expect("read unknown")
            .expect("delivery")
            .input_json
            .as_deref()
            .is_some_and(|value| value.contains("private prompt"))
    );
    assert!(
        store
            .gateway_activity("turn-1")
            .expect("read unknown activity")
            .expect("activity")
            .intent
            .expect("activity intent")
            .get("input")
            .is_some(),
        "unknown delivery must retain the recoverable prompt"
    );
    assert!(
        !store
            .finish_gateway_turn_delivery("turn-1")
            .expect("unknown terminal must not scrub"),
        "a terminal projection cannot erase input while delivery remains unknown"
    );
    assert!(
        store
            .gateway_turn_delivery("turn-1")
            .expect("read terminal unknown")
            .expect("delivery")
            .input_json
            .is_some(),
        "unknown input remains available for explicit recovery"
    );
    assert!(
        store
            .confirm_gateway_turn_delivery("turn-1")
            .expect("confirm")
    );
    let delivered = store
        .gateway_turn_delivery("turn-1")
        .expect("read delivered")
        .expect("delivery");
    assert_eq!(delivered.status, "delivered");
    assert_eq!(delivered.input_json, None);
    assert_eq!(delivered.input_hash, "input-hash");
    assert!(delivered.delivery_confirmed_at_ms.is_some());
    let correlated_activity = store
        .gateway_activity("turn-1")
        .expect("read correlated activity")
        .expect("activity")
        .intent
        .expect("routing metadata remains");
    assert!(
        correlated_activity.get("input").is_none(),
        "the correlation transaction must scrub the generic activity copy"
    );
    assert_eq!(correlated_activity["runtimeSource"], "test");
    assert!(
        store
            .finish_gateway_turn_delivery("turn-1")
            .expect("finish")
    );

    store
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: "turn-2",
            thread_id: None,
            source_key: Some("agent:test-2"),
            turn_id: Some("turn-2"),
            kind: "turn",
            owner_id: "owner",
            owner_surface: Some("test"),
            lease_expires_at_ms: now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: Some(json!({
                "kind": "turn",
                "runtimeSource": "test",
                "input": [{"type": "text", "text": "uncorrelated prompt"}],
            })),
        })
        .expect("uncorrelated activity intent");
    store
        .insert_gateway_turn_delivery(GatewayTurnDeliveryInput {
            turn_id: "turn-2",
            thread_id: &thread_id,
            runtime_ref: "agent-profile",
            input_json: r#"[{"type":"text","text":"uncorrelated prompt"}]"#,
            input_hash: "uncorrelated-hash",
        })
        .expect("uncorrelated intent");
    assert!(
        store
            .finish_gateway_turn_delivery("turn-2")
            .expect("terminal fallback")
    );
    assert_eq!(
        store
            .gateway_turn_delivery("turn-2")
            .expect("read terminal delivery")
            .expect("delivery")
            .input_json,
        None
    );
    assert!(
        store
            .gateway_activity("turn-2")
            .expect("read terminal activity")
            .expect("activity")
            .intent
            .expect("routing metadata remains")
            .get("input")
            .is_none(),
        "terminal must scrub an uncorrelated generic activity copy"
    );

    let pending = store
        .upsert_gateway_channel_outbox(GatewayChannelOutboxInput {
            delivery_id: "delivery-1",
            thread_id: &thread_id,
            turn_id: "turn-1",
            connection_id: "telegram",
            source_key: "im.telegram:user-1",
            payload_text: "final answer",
            payload_hash: "payload-hash",
        })
        .expect("outbox");
    assert_eq!(pending.status, "pending");
    assert_eq!(pending.payload_text.as_deref(), Some("final answer"));
    let retryable = store
        .retryable_gateway_channel_outbox("telegram")
        .expect("pending outbox list");
    assert_eq!(retryable.as_slice(), std::slice::from_ref(&pending));
    assert!(
        store
            .fail_gateway_channel_outbox("delivery-1")
            .expect("mark failed")
    );
    let retryable = store
        .retryable_gateway_channel_outbox("telegram")
        .expect("failed outbox list");
    assert_eq!(retryable.len(), 1);
    assert_eq!(retryable[0].status, "failed");
    assert!(
        store
            .acknowledge_gateway_channel_outbox("delivery-1")
            .expect("ack")
    );
    let acknowledged = store
        .gateway_channel_outbox("delivery-1")
        .expect("read outbox")
        .expect("outbox");
    assert_eq!(acknowledged.status, "acknowledged");
    assert_eq!(acknowledged.payload_text, None);
    assert_eq!(acknowledged.payload_hash, "payload-hash");
    assert!(
        store
            .retryable_gateway_channel_outbox("telegram")
            .expect("acknowledged outbox list")
            .is_empty()
    );
}

#[test]
pub(crate) fn sqlite_unknown_delivery_reconciliation_is_atomic_and_idempotent() {
    let temp = tempdir().expect("temp");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let thread_id = store
        .create_session_with_metadata(&cwd, "channel", "pending", "pending", None)
        .expect("session");

    store
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: "turn-unknown",
            thread_id: Some(&thread_id),
            source_key: Some("agent:test"),
            turn_id: Some("turn-unknown"),
            kind: "turn",
            owner_id: "owner",
            owner_surface: Some("test"),
            lease_expires_at_ms: now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: Some(json!({
                "kind": "turn",
                "runtimeSource": "acp",
                "input": [{"type": "text", "text": "do not replay me"}],
            })),
        })
        .expect("activity intent");
    store
        .insert_gateway_turn_delivery(GatewayTurnDeliveryInput {
            turn_id: "turn-unknown",
            thread_id: &thread_id,
            runtime_ref: "acp-profile",
            input_json: r#"[{"type":"text","text":"do not replay me"}]"#,
            input_hash: "unknown-input-hash",
        })
        .expect("delivery");
    assert!(
        store
            .mark_gateway_turn_delivery_unknown("turn-unknown")
            .expect("mark unknown")
    );
    store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "turn-unknown",
            thread_id: &thread_id,
            status: "failed",
            outcome: Some("failed"),
            error_message: Some("ACP response boundary became uncertain"),
            started_at_ms: Some(10),
            completed_at_ms: 20,
            metadata: Some(json!({"source": "transport_failure"})),
        })
        .expect("provisional failed terminal");

    let unknown = store
        .unknown_gateway_turn_deliveries_for_thread(&thread_id, "turn-new")
        .expect("unknown deliveries");
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].turn_id, "turn-unknown");
    assert!(
        unknown[0]
            .input_json
            .as_deref()
            .is_some_and(|value| value.contains("do not replay me"))
    );

    let reconciliation_metadata = json!({
        "reconciledFrom": "agent_history",
        "messageIds": ["assistant-message-1"],
    });
    assert!(
        store
            .reconcile_unknown_gateway_turn_delivery(
                "turn-unknown",
                &thread_id,
                Some(&reconciliation_metadata),
            )
            .expect("reconcile unknown delivery")
    );

    let delivery = store
        .gateway_turn_delivery("turn-unknown")
        .expect("read delivery")
        .expect("delivery");
    assert_eq!(delivery.status, "terminal");
    assert_eq!(delivery.input_json, None);
    assert_eq!(delivery.input_hash, "unknown-input-hash");
    assert!(delivery.delivery_confirmed_at_ms.is_some());
    assert!(delivery.terminal_at_ms.is_some());
    let activity_intent = store
        .gateway_activity("turn-unknown")
        .expect("read activity")
        .expect("activity")
        .intent
        .expect("routing intent remains");
    assert!(activity_intent.get("input").is_none());
    assert_eq!(activity_intent["runtimeSource"], "acp");

    let terminal = store
        .gateway_turn_terminal("turn-unknown")
        .expect("read terminal")
        .expect("terminal");
    assert_eq!(terminal.status, "completed");
    assert_eq!(terminal.outcome.as_deref(), Some("normal"));
    assert_eq!(terminal.error_message, None);
    assert_eq!(terminal.started_at_ms, Some(10));
    assert_eq!(terminal.metadata.as_ref(), Some(&reconciliation_metadata));

    assert!(
        store
            .unknown_gateway_turn_deliveries_for_thread(&thread_id, "turn-new")
            .expect("no unknown deliveries")
            .is_empty()
    );
    assert!(
        !store
            .reconcile_unknown_gateway_turn_delivery(
                "turn-unknown",
                &thread_id,
                Some(&json!({"mustNotOverwrite": true})),
            )
            .expect("idempotent reconciliation")
    );
    assert_eq!(
        store
            .gateway_turn_terminal("turn-unknown")
            .expect("read terminal after retry")
            .expect("terminal")
            .metadata
            .as_ref(),
        Some(&reconciliation_metadata),
        "a repeated reconciliation must not rewrite the durable terminal"
    );
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
