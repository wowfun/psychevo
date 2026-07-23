use super::*;
use psychevo_agent_core::now_ms;

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
        kind: "turn",
        owner_id,
        owner_surface: Some("test"),
        lease_expires_at_ms,
        queued_turns: 0,
        superseded_activity_id: None,
        intent: Some(json!({"kind": "turn", "input": [{"type": "text", "text": "hello"}]})),
    }
}

#[test]
fn gateway_activity_claim_rejects_live_foreign_owner_and_reclaims_stale_owner() {
    let temp = tempdir().expect("tempdir");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let source_key = "source:test";
    let first = store
        .claim_gateway_activity(gateway_activity_claim(
            "activity-1",
            source_key,
            "owner-a",
            now_ms() + 60_000,
        ))
        .expect("first claim");

    let conflict = store.claim_gateway_activity(gateway_activity_claim(
        "activity-2",
        source_key,
        "owner-b",
        now_ms() + 60_000,
    ));
    assert!(conflict.is_err());

    assert!(
        store
            .heartbeat_gateway_activity(
                &first.activity_id,
                &first.owner_id,
                first.generation,
                now_ms() - 1,
            )
            .expect("expire first")
    );
    let reclaimed = store
        .claim_gateway_activity(gateway_activity_claim(
            "activity-2",
            source_key,
            "owner-b",
            now_ms() + 60_000,
        ))
        .expect("stale reclaim");

    assert_eq!(reclaimed.generation, first.generation + 1);
    assert_eq!(
        reclaimed.superseded_activity_id.as_deref(),
        Some("activity-1")
    );
    assert_eq!(
        store
            .gateway_activity("activity-1")
            .expect("old record")
            .expect("activity-1")
            .status,
        "superseded"
    );
}

#[test]
fn turn_start_receipts_are_persisted_and_bounded_per_thread() {
    let temp = tempdir().expect("tempdir");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let thread_id = store.create_session(temp.path()).expect("thread");

    for index in 0..34 {
        store
            .record_gateway_turn_start_receipt(
                &thread_id,
                &format!("client-{index}"),
                &format!("turn-{index}"),
            )
            .expect("record receipt");
    }
    store
        .record_gateway_turn_start_receipt(&thread_id, "client-10", "turn-10-replaced")
        .expect("replace receipt");

    let receipts = store
        .gateway_turn_start_receipts(&thread_id)
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

#[test]
fn gateway_activity_release_is_generation_guarded() {
    let temp = tempdir().expect("tempdir");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let record = store
        .claim_gateway_activity(gateway_activity_claim(
            "activity-1",
            "source:test",
            "owner-a",
            now_ms() + 60_000,
        ))
        .expect("claim");

    assert!(
        !store
            .finish_gateway_activity(
                &record.activity_id,
                &record.owner_id,
                record.generation + 1,
                "completed",
            )
            .expect("wrong generation ignored")
    );
    assert_eq!(
        store
            .gateway_activity(&record.activity_id)
            .expect("record")
            .expect("activity")
            .status,
        "running"
    );
    assert!(
        store
            .finish_gateway_activity(
                &record.activity_id,
                &record.owner_id,
                record.generation,
                "completed",
            )
            .expect("release")
    );
    assert_eq!(
        store
            .gateway_activity(&record.activity_id)
            .expect("record")
            .expect("activity")
            .status,
        "completed"
    );
}

#[test]
fn gateway_live_events_are_ordered_and_control_commands_track_status() {
    let temp = tempdir().expect("tempdir");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let first_seq = store
        .append_gateway_live_event(
            Some("activity-1"),
            Some("owner-a"),
            None,
            Some("turn-1"),
            &json!({"type": "activityChanged"}),
        )
        .expect("first event");
    let second_seq = store
        .append_gateway_live_event(
            Some("activity-1"),
            Some("owner-a"),
            None,
            Some("turn-1"),
            &json!({"type": "titleChanged"}),
        )
        .expect("second event");

    assert!(second_seq > first_seq);
    let events = store
        .list_gateway_live_events_after(first_seq - 1, 10)
        .expect("events");
    assert_eq!(
        events.iter().map(|event| event.seq).collect::<Vec<_>>(),
        vec![first_seq, second_seq]
    );

    let command_id = store
        .enqueue_gateway_control_command(GatewayControlCommandInput {
            activity_id: "activity-1",
            owner_id: "owner-a",
            command_kind: "interrupt",
            payload: json!({"reason": "test"}),
        })
        .expect("command");
    let pending = store
        .pending_gateway_control_commands("owner-a", 10)
        .expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, command_id);
    store
        .mark_gateway_control_command_applied(command_id)
        .expect("applied");
    assert!(
        store
            .pending_gateway_control_commands("owner-a", 10)
            .expect("no pending")
            .is_empty()
    );
}

#[test]
fn gateway_live_snapshots_upsert_latest_revision_and_delete_by_activity() {
    let temp = tempdir().expect("tempdir");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let session_id = store.create_session(temp.path()).expect("session");

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
        .expect("second snapshot");

    assert_eq!(first_revision, 1);
    assert_eq!(second_revision, 2);
    let snapshots = store
        .list_gateway_live_snapshots_for_thread(&session_id, Some("turn-1"), 10)
        .expect("snapshots");
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].revision, 2);
    assert_eq!(snapshots[0].event["value"], "second");

    assert_eq!(
        store
            .delete_gateway_live_snapshots_for_activity("activity-1")
            .expect("delete snapshots"),
        1
    );
    assert!(
        store
            .list_gateway_live_snapshots(10)
            .expect("no snapshots")
            .is_empty()
    );
}

#[test]
fn gateway_turn_terminals_round_trip_and_order_by_thread() {
    let temp = tempdir().expect("tempdir");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let thread_id = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("session");

    let failed = store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "turn-failed",
            thread_id: &thread_id,
            status: "failed",
            outcome: Some("failed"),
            error_message: Some("model service failed"),
            started_at_ms: Some(10),
            completed_at_ms: 20,
            metadata: Some(json!({"source": "test"})),
        })
        .expect("failed terminal");
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.started_at_ms, Some(10));
    assert_eq!(
        failed.error_message.as_deref(),
        Some("model service failed")
    );

    let updated = store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "turn-failed",
            thread_id: &thread_id,
            status: "interrupted",
            outcome: Some("aborted"),
            error_message: None,
            started_at_ms: None,
            completed_at_ms: 30,
            metadata: None,
        })
        .expect("updated terminal");
    assert_eq!(updated.status, "interrupted");
    assert_eq!(updated.outcome.as_deref(), Some("aborted"));
    assert_eq!(updated.started_at_ms, Some(10));
    assert_eq!(updated.completed_at_ms, 30);

    store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "turn-ok",
            thread_id: &thread_id,
            status: "completed",
            outcome: Some("normal"),
            error_message: None,
            started_at_ms: Some(1),
            completed_at_ms: 2,
            metadata: None,
        })
        .expect("completed terminal");

    let records = store
        .list_gateway_turn_terminals_for_thread(&thread_id)
        .expect("list terminals");
    assert_eq!(
        records
            .iter()
            .map(|record| record.turn_id.as_str())
            .collect::<Vec<_>>(),
        vec!["turn-ok", "turn-failed"]
    );
}
