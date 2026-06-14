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
