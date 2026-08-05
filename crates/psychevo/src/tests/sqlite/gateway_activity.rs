use crate::application::{FrameworkTurnTerminalOutcome, FrameworkTurnTerminalStatus};
use crate::paths::canonical_cwd;
use crate::state::StateRuntime;
use crate::store::{
    GatewayActivityClaimInput, GatewayActivityKind, GatewayActivityState,
    GatewayActivityTerminalStatus, GatewayChannelOutboxInput, GatewayControlCommandInput,
    GatewayControlCommandKind, GatewayControlCommandStatus, GatewayLiveSnapshotInput,
    GatewayTurnDeliveryInput, GatewayTurnStartReceiptRecord, GatewayTurnTerminalInput,
};
use crate::types::BlockingActionKind;
use psychevo_agent_core::now_ms;
use serde_json::json;
use sqlx::Row;
use tempfile::tempdir;

fn gateway_activity_claim<'a>(
    activity_id: &'a str,
    source_key: &'a str,
    owner_id: &'a str,
    lease_expires_at_ms: i64,
) -> GatewayActivityClaimInput<'a> {
    GatewayActivityClaimInput {
        activity_id,
        thread_id: None,
        source_key: Some(source_key),
        turn_id: Some(activity_id),
        kind: GatewayActivityKind::Turn,
        owner_id,
        owner_surface: Some("test"),
        lease_expires_at_ms,
        queued_turns: 0,
        superseded_activity_id: None,
        intent: Some(json!({"kind": "turn", "input": [{"type": "text", "text": "hello"}]})),
    }
}

fn assert_invalid_persisted_domain_value(
    error: crate::Error,
    table: &str,
    field: &str,
    value: &str,
) {
    assert_eq!(
        error
            .structured_data()
            .expect("structured persisted-domain error"),
        &json!({
            "kind": "invalid_persisted_domain_value",
            "table": table,
            "field": field,
            "value": value,
        })
    );
}

#[tokio::test]
async fn gateway_activity_batch_lookup_deduplicates_ids_and_marks_missing_by_absence() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    store
        .claim_gateway_activity(gateway_activity_claim(
            "activity-1",
            "source:one",
            "owner-a",
            now_ms() + 60_000,
        ))
        .await
        .expect("claim");

    assert!(
        store
            .gateway_activities_by_id(&[])
            .await
            .expect("empty batch")
            .is_empty()
    );
    let activities = store
        .gateway_activities_by_id(&[
            "activity-1".to_string(),
            "missing".to_string(),
            "activity-1".to_string(),
            String::new(),
        ])
        .await
        .expect("batch");
    assert_eq!(activities.len(), 1);
    assert_eq!(
        activities
            .get("activity-1")
            .map(|activity| activity.owner_id.as_str()),
        Some("owner-a")
    );
    assert!(!activities.contains_key("missing"));
}

#[tokio::test]
async fn gateway_activity_claim_rejects_live_foreign_owner_and_reclaims_stale_owner() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let source_key = "source:test";
    let first = store
        .claim_gateway_activity(gateway_activity_claim(
            "activity-1",
            source_key,
            "owner-a",
            now_ms() + 60_000,
        ))
        .await
        .expect("first claim");

    let conflict = store
        .claim_gateway_activity(gateway_activity_claim(
            "activity-2",
            source_key,
            "owner-b",
            now_ms() + 60_000,
        ))
        .await;
    assert!(conflict.is_err());

    assert_eq!(
        store
            .heartbeat_gateway_activities(
                &first.owner_id,
                &[(first.activity_id.clone(), first.generation)],
                now_ms() - 1,
            )
            .await
            .expect("expire first"),
        vec![first.activity_id.clone()]
    );
    let reclaimed = store
        .claim_gateway_activity(gateway_activity_claim(
            "activity-2",
            source_key,
            "owner-b",
            now_ms() + 60_000,
        ))
        .await
        .expect("stale reclaim");

    assert_eq!(reclaimed.generation, first.generation + 1);
    assert_eq!(
        reclaimed.superseded_activity_id.as_deref(),
        Some("activity-1")
    );
    assert_eq!(
        store
            .gateway_activity("activity-1")
            .await
            .expect("old record")
            .expect("activity-1")
            .status,
        GatewayActivityState::Superseded
    );
}

#[tokio::test]
async fn turn_start_receipts_are_persisted_and_bounded_per_thread() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let thread_id = store.create_session(temp.path()).await.expect("thread");

    for index in 0..34 {
        store
            .record_gateway_turn_start_receipt(
                &thread_id,
                &format!("client-{index}"),
                &format!("turn-{index}"),
            )
            .await
            .expect("record receipt");
    }
    store
        .record_gateway_turn_start_receipt(&thread_id, "client-10", "turn-10-replaced")
        .await
        .expect("replace receipt");

    let receipts = store
        .gateway_turn_start_receipts(&thread_id)
        .await
        .expect("read receipts");
    assert_eq!(receipts.len(), 32);
    assert_eq!(
        receipts
            .first()
            .map(|receipt| receipt.client_turn_id.as_str()),
        Some("client-2")
    );
    assert_eq!(
        receipts.last(),
        Some(&GatewayTurnStartReceiptRecord {
            client_turn_id: "client-10".to_string(),
            turn_id: "turn-10-replaced".to_string(),
        })
    );
}

#[tokio::test]
async fn gateway_activity_terminal_is_generation_guarded_and_immutable() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let record = store
        .claim_gateway_activity(gateway_activity_claim(
            "activity-1",
            "source:test",
            "owner-a",
            now_ms() + 60_000,
        ))
        .await
        .expect("claim");

    assert!(
        !store
            .finish_gateway_activity(
                &record.activity_id,
                &record.owner_id,
                record.generation + 1,
                GatewayActivityTerminalStatus::Completed,
            )
            .await
            .expect("wrong generation ignored")
    );
    assert_eq!(
        store
            .gateway_activity(&record.activity_id)
            .await
            .expect("record")
            .expect("activity")
            .status,
        GatewayActivityState::Running
    );
    assert!(
        store
            .finish_gateway_activity(
                &record.activity_id,
                &record.owner_id,
                record.generation,
                GatewayActivityTerminalStatus::Completed,
            )
            .await
            .expect("finish")
    );
    assert_eq!(
        store
            .gateway_activity(&record.activity_id)
            .await
            .expect("record")
            .expect("activity")
            .status,
        GatewayActivityState::Completed
    );
    assert!(
        !store
            .finish_gateway_activity(
                &record.activity_id,
                &record.owner_id,
                record.generation,
                GatewayActivityTerminalStatus::Failed,
            )
            .await
            .expect("terminal status cannot be rewritten")
    );
    assert_eq!(
        store
            .gateway_activity(&record.activity_id)
            .await
            .expect("record after duplicate terminal")
            .expect("activity after duplicate terminal")
            .status,
        GatewayActivityState::Completed
    );
}

#[tokio::test]
async fn gateway_activity_heartbeat_batch_reports_only_owned_generations() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let first = store
        .claim_gateway_activity(gateway_activity_claim(
            "activity-1",
            "source:first",
            "owner-a",
            now_ms() + 60_000,
        ))
        .await
        .expect("first claim");
    let second = store
        .claim_gateway_activity(gateway_activity_claim(
            "activity-2",
            "source:second",
            "owner-a",
            now_ms() + 60_000,
        ))
        .await
        .expect("second claim");
    let refreshed_until = now_ms() + 120_000;

    let refreshed = store
        .heartbeat_gateway_activities(
            "owner-a",
            &[
                (first.activity_id.clone(), first.generation),
                (second.activity_id.clone(), second.generation + 1),
            ],
            refreshed_until,
        )
        .await
        .expect("batch heartbeat");

    assert_eq!(refreshed, vec![first.activity_id.clone()]);
    assert_eq!(
        store
            .gateway_activity(&first.activity_id)
            .await
            .expect("first record")
            .expect("first activity")
            .lease_expires_at_ms,
        refreshed_until
    );
    assert_ne!(
        store
            .gateway_activity(&second.activity_id)
            .await
            .expect("second record")
            .expect("second activity")
            .lease_expires_at_ms,
        refreshed_until
    );
}

#[tokio::test]
async fn gateway_live_events_are_ordered_and_control_commands_track_status() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let first_seq = store
        .append_gateway_live_event(
            Some("activity-1"),
            Some("owner-a"),
            None,
            Some("turn-1"),
            None,
            &json!({"type": "activityChanged"}),
        )
        .await
        .expect("first event")
        .seq;
    let second_seq = store
        .append_gateway_live_event(
            Some("activity-1"),
            Some("owner-a"),
            None,
            Some("turn-1"),
            None,
            &json!({"type": "titleChanged"}),
        )
        .await
        .expect("second event")
        .seq;

    assert!(second_seq > first_seq);
    let events = store
        .list_gateway_live_events_after(first_seq - 1, 10)
        .await
        .expect("events");
    assert_eq!(
        events.iter().map(|event| event.seq).collect::<Vec<_>>(),
        vec![first_seq, second_seq]
    );

    let command_id = store
        .enqueue_gateway_control_command(GatewayControlCommandInput {
            activity_id: "activity-1",
            owner_id: "owner-a",
            command_kind: GatewayControlCommandKind::Interrupt,
            payload: json!({"reason": "test"}),
        })
        .await
        .expect("command");
    let pending = store
        .pending_gateway_control_commands("owner-a", 10)
        .await
        .expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, command_id);
    let claimed = store
        .claim_pending_gateway_control_commands("owner-a", 10)
        .await
        .expect("claim command before application");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, command_id);
    assert!(
        store
            .mark_gateway_control_command_applied(command_id)
            .await
            .expect("applied")
    );
    assert!(
        !store
            .mark_gateway_control_command_failed(command_id, "late rewrite")
            .await
            .expect("terminal command status cannot be rewritten")
    );
    assert_eq!(
        store
            .gateway_control_command(command_id)
            .await
            .expect("query terminal command")
            .expect("terminal command")
            .status,
        GatewayControlCommandStatus::Applied
    );
    assert!(
        store
            .pending_gateway_control_commands("owner-a", 10)
            .await
            .expect("no pending")
            .is_empty()
    );
}

#[tokio::test]
async fn gateway_live_event_idempotency_replay_returns_original_commit_without_a_second_row() {
    let store = StateRuntime::open(":memory:").await.expect("store");
    let event = json!({"type": "warning", "message": "once"});

    let first = store
        .append_gateway_live_event(
            Some("activity-1"),
            Some("owner-a"),
            None,
            Some("turn-1"),
            Some("gateway-ingress:v1:owner-a:activity-1:1:7"),
            &event,
        )
        .await
        .expect("first append");
    let replay = store
        .append_gateway_live_event(
            Some("activity-1"),
            Some("owner-a"),
            None,
            Some("turn-1"),
            Some("gateway-ingress:v1:owner-a:activity-1:1:7"),
            &event,
        )
        .await
        .expect("exact replay");

    assert!(first.inserted);
    assert!(!replay.inserted);
    assert_eq!(replay.seq, first.seq);
    assert_eq!(replay.idempotency_key, first.idempotency_key);
    let events = store
        .list_gateway_live_events_after(0, 10)
        .await
        .expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].idempotency_key, first.idempotency_key);
}

#[tokio::test]
async fn gateway_live_event_idempotency_key_rejects_a_different_envelope() {
    let store = StateRuntime::open(":memory:").await.expect("store");
    let key = "gateway-ingress:v1:owner-a:activity-1:1:8";
    store
        .append_gateway_live_event(
            Some("activity-1"),
            Some("owner-a"),
            None,
            Some("turn-1"),
            Some(key),
            &json!({"type": "warning", "message": "first"}),
        )
        .await
        .expect("first append");

    let error = store
        .append_gateway_live_event(
            Some("activity-1"),
            Some("owner-a"),
            None,
            Some("turn-1"),
            Some(key),
            &json!({"type": "warning", "message": "different"}),
        )
        .await
        .expect_err("key collision must not alias a different envelope");
    assert!(
        error
            .to_string()
            .contains("reused for a different envelope")
    );
    assert_eq!(
        store
            .list_gateway_live_events_after(0, 10)
            .await
            .expect("events")
            .len(),
        1
    );
}

#[tokio::test]
async fn exact_gateway_live_snapshot_replay_keeps_its_revision() {
    let store = StateRuntime::open(":memory:").await.expect("store");
    let input = || GatewayLiveSnapshotInput {
        snapshot_key: "activity-1:turn-1:entry-1",
        activity_id: Some("activity-1"),
        owner_id: Some("owner-a"),
        thread_id: None,
        turn_id: Some("turn-1"),
        event_kind: "entryUpdated",
        event: json!({"type": "entryUpdated", "entry": {"id": "entry-1"}}),
    };

    let first_revision = store
        .upsert_gateway_live_snapshot(input())
        .await
        .expect("first snapshot");
    let replay_revision = store
        .upsert_gateway_live_snapshot(input())
        .await
        .expect("snapshot replay");

    assert_eq!(first_revision, 1);
    assert_eq!(replay_revision, first_revision);
    assert_eq!(
        store
            .list_gateway_live_snapshots(10)
            .await
            .expect("snapshots")[0]
            .revision,
        first_revision
    );
}

#[tokio::test]
async fn gateway_live_snapshot_batch_rolls_back_every_key_when_a_later_write_fails() {
    let store = StateRuntime::open(":memory:").await.expect("store");
    let inputs = [
        GatewayLiveSnapshotInput {
            snapshot_key: "activity-1:turn-1:entry-1",
            activity_id: Some("activity-1"),
            owner_id: Some("owner-a"),
            thread_id: None,
            turn_id: Some("turn-1"),
            event_kind: "entryUpdated",
            event: json!({"type": "entryUpdated", "entry": {"id": "entry-1"}}),
        },
        GatewayLiveSnapshotInput {
            snapshot_key: "activity-1:turn-1:entry-2",
            activity_id: Some("activity-1"),
            owner_id: Some("owner-a"),
            thread_id: Some("missing-thread"),
            turn_id: Some("turn-1"),
            event_kind: "entryUpdated",
            event: json!({"type": "entryUpdated", "entry": {"id": "entry-2"}}),
        },
    ];

    store
        .upsert_gateway_live_snapshots(&inputs)
        .await
        .expect_err("foreign-key failure must roll back the whole snapshot batch");
    assert!(
        store
            .list_gateway_live_snapshots(10)
            .await
            .expect("snapshots after rollback")
            .is_empty()
    );
}

#[tokio::test]
async fn gateway_control_command_is_claimed_by_only_one_dispatcher_before_application() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let command_id = store
        .enqueue_gateway_control_command(GatewayControlCommandInput {
            activity_id: "activity-1",
            owner_id: "owner-a",
            command_kind: GatewayControlCommandKind::Steer,
            payload: json!({"message": "once"}),
        })
        .await
        .expect("command");

    let first_store = store.clone();
    let second_store = store.clone();
    let (first, second) = tokio::join!(
        first_store.claim_pending_gateway_control_commands("owner-a", 10),
        second_store.claim_pending_gateway_control_commands("owner-a", 10),
    );
    let claimed = first
        .expect("first dispatcher")
        .into_iter()
        .chain(second.expect("second dispatcher"))
        .collect::<Vec<_>>();

    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, command_id);
    assert_eq!(claimed[0].status, GatewayControlCommandStatus::Applying);
    assert!(
        store
            .pending_gateway_control_commands("owner-a", 10)
            .await
            .expect("no replayable commands")
            .is_empty()
    );
}

#[tokio::test]
async fn claimed_gateway_control_can_be_requeued_only_while_applying() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let command_id = store
        .enqueue_gateway_control_command(GatewayControlCommandInput {
            activity_id: "activity-1",
            owner_id: "owner-a",
            command_kind: GatewayControlCommandKind::Steer,
            payload: json!({"message": "retry"}),
        })
        .await
        .expect("command");
    assert_eq!(
        store
            .claim_pending_gateway_control_commands("owner-a", 10)
            .await
            .expect("claim")
            .len(),
        1
    );

    assert!(
        store
            .retry_gateway_control_command(command_id)
            .await
            .expect("requeue applying command")
    );
    assert_eq!(
        store
            .pending_gateway_control_commands("owner-a", 10)
            .await
            .expect("pending after retry")
            .len(),
        1
    );
    assert!(
        !store
            .retry_gateway_control_command(command_id)
            .await
            .expect("pending command cannot be requeued again")
    );
}

#[tokio::test]
async fn applying_gateway_control_without_a_live_owner_becomes_outcome_indeterminate() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let command_id = store
        .enqueue_gateway_control_command(GatewayControlCommandInput {
            activity_id: "lost-activity",
            owner_id: "lost-owner",
            command_kind: GatewayControlCommandKind::Steer,
            payload: json!({"message": "ambiguous"}),
        })
        .await
        .expect("command");
    store
        .claim_pending_gateway_control_commands("lost-owner", 10)
        .await
        .expect("claim before process loss");

    let recovered = store
        .recover_indeterminate_gateway_control_commands(now_ms())
        .await
        .expect("restart recovery");

    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].id, command_id);
    assert_eq!(
        recovered[0].status,
        GatewayControlCommandStatus::OutcomeIndeterminate
    );
    assert_eq!(
        recovered[0].error.as_deref(),
        Some("control side effect outcome is indeterminate after owner loss")
    );
    assert_eq!(
        store
            .gateway_control_command(command_id)
            .await
            .expect("query command")
            .expect("retained command")
            .status,
        GatewayControlCommandStatus::OutcomeIndeterminate
    );
    assert!(
        store
            .recover_indeterminate_gateway_control_commands(now_ms())
            .await
            .expect("idempotent recovery")
            .is_empty()
    );
}

#[tokio::test]
async fn gateway_live_snapshots_upsert_latest_revision_and_delete_by_activity() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let session_id = store.create_session(temp.path()).await.expect("session");

    let first_revision = store
        .upsert_gateway_live_snapshot(GatewayLiveSnapshotInput {
            snapshot_key: "activity-1:turn-1:entry-1",
            activity_id: Some("activity-1"),
            owner_id: Some("owner-a"),
            thread_id: Some(&session_id),
            turn_id: Some("turn-1"),
            event_kind: "entryUpdated",
            event: json!({"type": "entryUpdated", "value": "first"}),
        })
        .await
        .expect("first snapshot");
    let second_revision = store
        .upsert_gateway_live_snapshot(GatewayLiveSnapshotInput {
            snapshot_key: "activity-1:turn-1:entry-1",
            activity_id: Some("activity-1"),
            owner_id: Some("owner-a"),
            thread_id: Some(&session_id),
            turn_id: Some("turn-1"),
            event_kind: "entryUpdated",
            event: json!({"type": "entryUpdated", "value": "second"}),
        })
        .await
        .expect("second snapshot");

    assert_eq!(first_revision, 1);
    assert_eq!(second_revision, 2);
    let snapshots = store
        .list_gateway_live_snapshots_for_thread(&session_id, Some("turn-1"), 10)
        .await
        .expect("snapshots");
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].revision, 2);
    assert_eq!(snapshots[0].event["value"], "second");

    assert_eq!(
        store
            .delete_gateway_live_snapshots_for_activity("activity-1")
            .await
            .expect("delete snapshots"),
        1
    );
    assert!(
        store
            .list_gateway_live_snapshots(10)
            .await
            .expect("no snapshots")
            .is_empty()
    );
}

#[tokio::test]
async fn gateway_turn_terminals_round_trip_and_order_by_thread() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let thread_id = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .await
        .expect("session");

    let failed = store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "turn-failed",
            thread_id: &thread_id,
            status: FrameworkTurnTerminalStatus::Failed,
            outcome: Some(FrameworkTurnTerminalOutcome::Failed),
            error_message: Some("model service failed"),
            started_at_ms: Some(10),
            completed_at_ms: 20,
            boundary_session_seq: Some(0),
            metadata: Some(json!({"source": "test"})),
        })
        .await
        .expect("failed terminal");
    assert_eq!(failed.status, FrameworkTurnTerminalStatus::Failed);
    assert_eq!(failed.started_at_ms, Some(10));
    assert_eq!(
        failed.error_message.as_deref(),
        Some("model service failed")
    );

    let updated = store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "turn-failed",
            thread_id: &thread_id,
            status: FrameworkTurnTerminalStatus::Interrupted,
            outcome: Some(FrameworkTurnTerminalOutcome::Aborted),
            error_message: None,
            started_at_ms: None,
            completed_at_ms: 30,
            boundary_session_seq: Some(0),
            metadata: None,
        })
        .await
        .expect("updated terminal");
    assert_eq!(updated.status, FrameworkTurnTerminalStatus::Interrupted);
    assert_eq!(updated.outcome, Some(FrameworkTurnTerminalOutcome::Aborted));
    assert_eq!(updated.started_at_ms, Some(10));
    assert_eq!(updated.completed_at_ms, 30);

    store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "turn-ok",
            thread_id: &thread_id,
            status: FrameworkTurnTerminalStatus::Completed,
            outcome: Some(FrameworkTurnTerminalOutcome::Normal),
            error_message: None,
            started_at_ms: Some(1),
            completed_at_ms: 2,
            boundary_session_seq: Some(0),
            metadata: None,
        })
        .await
        .expect("completed terminal");

    let records = store
        .list_gateway_turn_terminals_for_thread(&thread_id)
        .await
        .expect("list terminals");
    assert_eq!(
        records
            .iter()
            .map(|record| record.turn_id.as_str())
            .collect::<Vec<_>>(),
        vec!["turn-ok", "turn-failed"]
    );
}

#[tokio::test]
async fn gateway_turn_terminal_existence_is_indexed_and_does_not_decode_large_history() {
    let store = StateRuntime::open(":memory:").await.expect("store");
    let cwd = canonical_cwd(std::path::Path::new(".")).expect("cwd");
    let thread_id = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .await
        .expect("session");
    let empty_thread_id = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .await
        .expect("empty session");
    let mut connection = store.acquire_sqlx().await.expect("connection");
    sqlx::query(
        r#"
        WITH digits(value) AS (
            VALUES (0), (1), (2), (3), (4), (5), (6), (7), (8), (9)
        ), terminal_numbers(value) AS (
            SELECT thousands.value * 1000
                 + hundreds.value * 100
                 + tens.value * 10
                 + ones.value
            FROM digits AS thousands
            CROSS JOIN digits AS hundreds
            CROSS JOIN digits AS tens
            CROSS JOIN digits AS ones
        )
        INSERT INTO gateway_turn_terminals (
            turn_id, thread_id, status, completed_at_ms, metadata_json
        )
        SELECT printf('turn-%04d', value), ?1, 'completed', value, 'not-json'
        FROM terminal_numbers
        WHERE value < 4096
        "#,
    )
    .bind(&thread_id)
    .execute(&mut *connection)
    .await
    .expect("large terminal history");
    sqlx::query(
        r#"
        INSERT INTO gateway_turn_terminals (
            turn_id, thread_id, status, completed_at_ms,
            boundary_session_seq, metadata_json
        ) VALUES ('visible-failure', ?1, 'failed', 5000, 5000, NULL)
        "#,
    )
    .bind(&thread_id)
    .execute(&mut *connection)
    .await
    .expect("visible terminal");
    let plan = sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT 1
        FROM gateway_turn_terminals
        WHERE thread_id = ?1
        LIMIT 1
        "#,
    )
    .bind(&thread_id)
    .fetch_all(&mut *connection)
    .await
    .expect("query plan")
    .into_iter()
    .map(|row| row.get::<String, _>(3))
    .collect::<Vec<_>>();
    let visible_plan = sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT turn_id
        FROM gateway_turn_terminals
        WHERE thread_id = ?1
          AND status IN ('failed', 'interrupted')
          AND boundary_session_seq >= 0
          AND boundary_session_seq < 6000
        ORDER BY boundary_session_seq DESC, completed_at_ms DESC, turn_id DESC
        LIMIT 1
        "#,
    )
    .bind(&thread_id)
    .fetch_all(&mut *connection)
    .await
    .expect("visible history query plan")
    .into_iter()
    .map(|row| row.get::<String, _>(3))
    .collect::<Vec<_>>();
    drop(connection);

    assert!(
        store
            .gateway_turn_terminal_exists_for_thread(&thread_id)
            .await
            .expect("terminal existence")
    );
    assert!(
        !store
            .gateway_turn_terminal_exists_for_thread(&empty_thread_id)
            .await
            .expect("empty terminal existence")
    );
    assert!(
        plan.iter().any(|detail| {
            detail.contains("SEARCH") && detail.contains("idx_gateway_turn_terminals_thread")
        }),
        "terminal existence query did not use the thread index: {plan:?}"
    );
    assert!(
        visible_plan.iter().any(|detail| {
            detail.contains("SEARCH")
                && detail.contains("idx_gateway_turn_terminals_visible_history")
        }),
        "visible terminal history query did not use the partial index: {visible_plan:?}"
    );
    let visible = store
        .list_valid_gateway_turn_terminals_for_thread_window(&thread_id, 0, Some(6000), None, 1)
        .await
        .expect("visible terminal window");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].turn_id, "visible-failure");
    assert!(
        store
            .list_gateway_turn_terminals_for_thread(&thread_id)
            .await
            .is_err(),
        "fixture must prove the existence probe does not decode terminal payloads"
    );
}

#[tokio::test]
async fn closed_gateway_domains_reject_unknown_persisted_values_at_decode() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let thread_id = store
        .create_session_with_metadata(&cwd, "test", "model", "provider", None)
        .await
        .expect("Thread");
    store
        .claim_gateway_activity(gateway_activity_claim(
            "typed-activity",
            "typed-source",
            "typed-owner",
            now_ms() + 30_000,
        ))
        .await
        .expect("activity");
    let command_id = store
        .enqueue_gateway_control_command(GatewayControlCommandInput {
            activity_id: "typed-activity",
            owner_id: "typed-owner",
            command_kind: GatewayControlCommandKind::Interrupt,
            payload: json!({}),
        })
        .await
        .expect("control command");
    store
        .insert_gateway_turn_delivery(GatewayTurnDeliveryInput {
            turn_id: "typed-turn",
            thread_id: &thread_id,
            runtime_ref: "native",
            input_json: "[]",
            input_hash: "typed-input",
        })
        .await
        .expect("delivery");
    store
        .upsert_gateway_channel_outbox(GatewayChannelOutboxInput {
            delivery_id: "typed-outbox",
            thread_id: &thread_id,
            turn_id: "typed-turn",
            connection_id: "typed-connection",
            source_key: "typed-source",
            payload_text: "answer",
            payload_hash: "typed-payload",
        })
        .await
        .expect("outbox");
    store
        .request_framework_interaction(
            "typed-interaction",
            &thread_id,
            "typed-turn",
            BlockingActionKind::Clarify,
            json!([]),
        )
        .await
        .expect("interaction");
    for (interaction_id, kind) in [
        ("typed-permission", BlockingActionKind::Permission),
        ("typed-custom-tool", BlockingActionKind::CustomTool),
        ("typed-user-input", BlockingActionKind::UserInput),
    ] {
        store
            .request_framework_interaction(
                interaction_id,
                &thread_id,
                "typed-turn",
                kind,
                json!([]),
            )
            .await
            .expect("typed interaction kind");
    }
    let interaction_kinds = store
        .framework_interactions_for_thread(&thread_id, false)
        .await
        .expect("typed interactions")
        .into_iter()
        .map(|interaction| interaction.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        interaction_kinds,
        vec![
            BlockingActionKind::Clarify,
            BlockingActionKind::Permission,
            BlockingActionKind::CustomTool,
            BlockingActionKind::UserInput,
        ]
    );
    store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "typed-terminal",
            thread_id: &thread_id,
            status: FrameworkTurnTerminalStatus::Failed,
            outcome: Some(FrameworkTurnTerminalOutcome::Failed),
            error_message: Some("failed"),
            started_at_ms: None,
            completed_at_ms: now_ms(),
            boundary_session_seq: None,
            metadata: None,
        })
        .await
        .expect("terminal");

    let mut conn = store.acquire_sqlx().await.expect("corruption connection");
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(&mut *conn)
        .await
        .expect("ignore checks for corruption fixtures");

    sqlx::query(
        "UPDATE gateway_activities SET kind = 'future' WHERE activity_id = 'typed-activity'",
    )
    .execute(&mut *conn)
    .await
    .expect("corrupt activity kind");
    assert_invalid_persisted_domain_value(
        store
            .gateway_activity("typed-activity")
            .await
            .expect_err("unknown activity kind"),
        "gateway_activities",
        "kind",
        "future",
    );
    sqlx::query(
        "UPDATE gateway_activities SET kind = 'turn', status = 'future' WHERE activity_id = 'typed-activity'",
    )
    .execute(&mut *conn)
    .await
    .expect("corrupt activity status");
    assert_invalid_persisted_domain_value(
        store
            .gateway_activity("typed-activity")
            .await
            .expect_err("unknown activity status"),
        "gateway_activities",
        "status",
        "future",
    );

    sqlx::query("UPDATE gateway_control_commands SET command_kind = 'future' WHERE id = ?1")
        .bind(command_id)
        .execute(&mut *conn)
        .await
        .expect("corrupt command kind");
    assert_invalid_persisted_domain_value(
        store
            .gateway_control_command(command_id)
            .await
            .expect_err("unknown command kind"),
        "gateway_control_commands",
        "command_kind",
        "future",
    );
    sqlx::query(
        "UPDATE gateway_control_commands SET command_kind = 'interrupt', status = 'future' WHERE id = ?1",
    )
    .bind(command_id)
    .execute(&mut *conn)
    .await
    .expect("corrupt command status");
    assert_invalid_persisted_domain_value(
        store
            .gateway_control_command(command_id)
            .await
            .expect_err("unknown command status"),
        "gateway_control_commands",
        "status",
        "future",
    );

    sqlx::query(
        "UPDATE gateway_turn_deliveries SET status = 'future' WHERE turn_id = 'typed-turn'",
    )
    .execute(&mut *conn)
    .await
    .expect("corrupt delivery status");
    assert_invalid_persisted_domain_value(
        store
            .gateway_turn_delivery("typed-turn")
            .await
            .expect_err("unknown delivery status"),
        "gateway_turn_deliveries",
        "status",
        "future",
    );

    sqlx::query(
        "UPDATE gateway_channel_outbox SET status = 'future' WHERE delivery_id = 'typed-outbox'",
    )
    .execute(&mut *conn)
    .await
    .expect("corrupt outbox status");
    assert_invalid_persisted_domain_value(
        store
            .gateway_channel_outbox("typed-outbox")
            .await
            .expect_err("unknown outbox status"),
        "gateway_channel_outbox",
        "status",
        "future",
    );

    sqlx::query(
        "UPDATE framework_interactions SET kind = 'future' WHERE interaction_id = 'typed-interaction'",
    )
    .execute(&mut *conn)
    .await
    .expect("corrupt interaction kind");
    assert_invalid_persisted_domain_value(
        store
            .framework_interactions_for_thread(&thread_id, false)
            .await
            .expect_err("unknown interaction kind"),
        "framework_interactions",
        "kind",
        "future",
    );
    sqlx::query(
        "UPDATE framework_interactions SET kind = 'clarify', status = 'future' WHERE interaction_id = 'typed-interaction'",
    )
    .execute(&mut *conn)
    .await
    .expect("corrupt interaction status");
    assert_invalid_persisted_domain_value(
        store
            .framework_interactions_for_thread(&thread_id, false)
            .await
            .expect_err("unknown interaction status"),
        "framework_interactions",
        "status",
        "future",
    );

    sqlx::query(
        "UPDATE gateway_turn_terminals SET status = 'future' WHERE turn_id = 'typed-terminal'",
    )
    .execute(&mut *conn)
    .await
    .expect("corrupt terminal status");
    assert_invalid_persisted_domain_value(
        store
            .gateway_turn_terminal("typed-terminal")
            .await
            .expect_err("unknown terminal status"),
        "gateway_turn_terminals",
        "status",
        "future",
    );
    sqlx::query(
        "UPDATE gateway_turn_terminals SET status = 'failed', outcome = 'future' WHERE turn_id = 'typed-terminal'",
    )
    .execute(&mut *conn)
    .await
    .expect("corrupt terminal outcome");
    assert_invalid_persisted_domain_value(
        store
            .gateway_turn_terminal("typed-terminal")
            .await
            .expect_err("unknown terminal outcome"),
        "gateway_turn_terminals",
        "outcome",
        "future",
    );
}
