use psychevo::application::{
    GatewayActivityClaimInput, GatewayActivityKind, GatewayActivityTerminalStatus,
    GatewayControlCommandKind,
};
use psychevo_gateway_protocol as wire;
use serde_json::json;
use tokio::sync::mpsc;

use crate::gateway_now_ms;
use crate::server::binding::{AuthContext, PendingInteractionContext};
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;
use crate::server::scope_session::{ResolvedScope, default_resolved_scope};
use crate::server::session_view::thread_snapshot;
use crate::server::tests::helpers::web_state;
use psychevo_gateway_protocol::events_transcript::{
    GatewayActionKind, GatewayEvent, PendingActionView,
};
use psychevo_gateway_protocol::source::{
    GatewaySource, GatewayTurn, GatewayTurnError, GatewayTurnStatus,
};

async fn start_web_thread(state: &crate::server::binding::WebState) -> String {
    let mut request = psychevo::StartThreadRequest::new(&state.inner.cwd);
    request.source = "web".to_string();
    state
        .inner
        .framework
        .start_thread(request)
        .await
        .expect("thread")
        .id()
        .to_string()
}

#[tokio::test]
async fn thread_snapshot_prunes_pending_permission_without_live_activity() {
    let (_temp, state) = web_state().await;
    let session_id = start_web_thread(&state).await;
    state
        .inner
        .pending_actions
        .lock()
        .expect("pending actions")
        .insert(
            "permission-1".to_string(),
            PendingActionView {
                action_id: "permission-1".to_string(),
                kind: GatewayActionKind::Permission,
                title: Some("exec_command".to_string()),
                summary: Some("exec_command".to_string()),
                payload: json!({
                    "toolName": "exec_command",
                    "summary": "exec_command",
                    "reason": "requires approval",
                    "matchedRule": null,
                    "suggestedRule": null,
                    "allowAlways": false,
                    "timeoutSecs": 300,
                }),
                thread_id: Some(session_id.clone()),
                turn_id: Some("turn-1".to_string()),
                activity_id: Some("missing-activity".to_string()),
                source_key: None,
                owner_id: Some("gateway:foreign".to_string()),
                lease_expires_at_ms: Some(gateway_now_ms() + 60_000),
            },
        );
    let scope = ResolvedScope {
        cwd: state.inner.cwd.clone(),
        source: state.inner.source.clone(),
    };

    let snapshot = thread_snapshot(&state, &scope, Some(&session_id))
        .await
        .expect("snapshot");

    assert_eq!(
        snapshot["pendingActions"]
            .as_array()
            .expect("pending actions")
            .len(),
        0
    );
    assert!(
        !state
            .inner
            .pending_actions
            .lock()
            .expect("pending actions")
            .contains_key("permission-1")
    );
}

#[tokio::test]
async fn thread_snapshot_removes_pending_permission_after_activity_finishes() {
    let (_temp, state) = web_state().await;
    let durability = &state.inner.durability;
    let session_id = start_web_thread(&state).await;
    let owner_id = "gateway:foreign";
    let activity = durability
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: "activity-1",
            thread_id: Some(&session_id),
            source_key: None,
            turn_id: Some("turn-1"),
            kind: GatewayActivityKind::Turn,
            owner_id,
            owner_surface: Some("tui"),
            lease_expires_at_ms: gateway_now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .await
        .expect("claim activity");
    state
        .inner
        .pending_actions
        .lock()
        .expect("pending actions")
        .insert(
            "permission-1".to_string(),
            PendingActionView {
                action_id: "permission-1".to_string(),
                kind: GatewayActionKind::Permission,
                title: Some("exec_command".to_string()),
                summary: Some("exec_command".to_string()),
                payload: json!({
                    "toolName": "exec_command",
                    "summary": "exec_command",
                    "reason": "requires approval",
                    "matchedRule": null,
                    "suggestedRule": null,
                    "allowAlways": false,
                    "timeoutSecs": 300,
                }),
                thread_id: Some(session_id.clone()),
                turn_id: Some("turn-1".to_string()),
                activity_id: Some(activity.activity_id.clone()),
                source_key: None,
                owner_id: Some(owner_id.to_string()),
                lease_expires_at_ms: Some(activity.lease_expires_at_ms),
            },
        );
    let scope = ResolvedScope {
        cwd: state.inner.cwd.clone(),
        source: state.inner.source.clone(),
    };

    let running = thread_snapshot(&state, &scope, Some(&session_id))
        .await
        .expect("running snapshot");
    assert_eq!(
        running["pendingActions"]
            .as_array()
            .expect("running actions")
            .len(),
        1
    );

    durability
        .finish_gateway_activity(
            &activity.activity_id,
            owner_id,
            activity.generation,
            GatewayActivityTerminalStatus::Failed,
        )
        .await
        .expect("finish activity");
    let finished = thread_snapshot(&state, &scope, Some(&session_id))
        .await
        .expect("finished snapshot");

    assert_eq!(
        finished["pendingActions"]
            .as_array()
            .expect("finished actions")
            .len(),
        0
    );
    assert!(
        !state
            .inner
            .pending_actions
            .lock()
            .expect("pending actions")
            .contains_key("permission-1")
    );
}

#[tokio::test]
async fn turn_completed_event_removes_pending_permission_panel() {
    let (_temp, state) = web_state().await;
    let session_id = start_web_thread(&state).await;
    state.record_event_with_context(
        &GatewayEvent::ActionRequested {
            action: PendingActionView {
                action_id: "permission-1".to_string(),
                kind: GatewayActionKind::Permission,
                title: Some("exec_command".to_string()),
                summary: Some("exec_command".to_string()),
                payload: json!({
                    "toolName": "exec_command",
                    "summary": "exec_command",
                    "reason": "requires approval",
                    "matchedRule": null,
                    "suggestedRule": null,
                    "allowAlways": true,
                    "timeoutSecs": 300,
                }),
                thread_id: None,
                turn_id: None,
                activity_id: None,
                source_key: None,
                owner_id: None,
                lease_expires_at_ms: None,
            },
        },
        PendingInteractionContext {
            thread_id: Some(session_id.clone()),
            turn_id: Some("turn-1".to_string()),
            activity_id: Some("turn-1".to_string()),
            source_key: None,
            owner_id: Some(state.inner.gateway.owner_id().to_string()),
            lease_expires_at_ms: Some(gateway_now_ms() + 60_000),
        },
    );

    state.record_event(&GatewayEvent::TurnCompleted {
        thread_id: Some(session_id.clone()),
        turn_id: "turn-1".to_string(),
        turn: GatewayTurn {
            id: "turn-1".to_string(),
            thread_id: Some(session_id.clone()),
            status: GatewayTurnStatus::Failed,
            outcome: Some("failed".to_string()),
            error: Some(GatewayTurnError {
                message: "failed".to_string(),
                code: None,
                stage: None,
                retry_class: None,
                delivery: psychevo_gateway_protocol::source::AgentDeliveryStatusView::Unknown,
                recovery_action: None,
                diagnostic_ref: None,
            }),
            started_at_ms: None,
            completed_at_ms: Some(gateway_now_ms()),
        },
        committed_entries: Vec::new(),
    });
    let scope = ResolvedScope {
        cwd: state.inner.cwd.clone(),
        source: state.inner.source.clone(),
    };
    let snapshot = thread_snapshot(&state, &scope, Some(&session_id))
        .await
        .expect("snapshot");

    assert_eq!(
        snapshot["pendingActions"]
            .as_array()
            .expect("pending actions")
            .len(),
        0
    );
    assert!(
        !state
            .inner
            .pending_actions
            .lock()
            .expect("pending actions")
            .contains_key("permission-1")
    );
}

#[tokio::test]
async fn source_started_pending_permission_survives_unbound_canonical_snapshot() {
    let (_temp, state) = web_state().await;
    let draft_source = GatewaySource::new("web", "cwd:test:draft:1").persistent();
    let draft_source_key = draft_source.source_key().0;
    let owner_id = "gateway:foreign";
    let activity = state
        .inner
        .durability
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: "activity-draft-permission",
            thread_id: None,
            source_key: Some(&draft_source_key),
            turn_id: Some("turn-draft"),
            kind: GatewayActivityKind::Turn,
            owner_id,
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .await
        .expect("claim activity");
    state
        .inner
        .pending_actions
        .lock()
        .expect("pending actions")
        .insert(
            "permission-draft".to_string(),
            PendingActionView {
                action_id: "permission-draft".to_string(),
                kind: GatewayActionKind::Permission,
                title: Some("exec_command".to_string()),
                summary: Some("exec_command".to_string()),
                payload: json!({
                    "toolName": "exec_command",
                    "summary": "exec_command",
                    "reason": "requires approval",
                    "matchedRule": "prompt",
                    "suggestedRule": "exec:python",
                    "allowAlways": true,
                    "timeoutSecs": 300,
                }),
                thread_id: None,
                turn_id: Some("turn-draft".to_string()),
                activity_id: Some(activity.activity_id.clone()),
                source_key: Some(draft_source_key.clone()),
                owner_id: Some(owner_id.to_string()),
                lease_expires_at_ms: Some(activity.lease_expires_at_ms),
            },
        );

    let canonical_scope = ResolvedScope {
        cwd: state.inner.cwd.clone(),
        source: state.inner.source.clone(),
    };
    let canonical = thread_snapshot(&state, &canonical_scope, None)
        .await
        .expect("canonical snapshot");
    assert_eq!(
        canonical["pendingActions"]
            .as_array()
            .expect("canonical actions")
            .len(),
        0
    );
    assert!(
        state
            .inner
            .pending_actions
            .lock()
            .expect("pending actions")
            .contains_key("permission-draft")
    );

    let draft_scope = ResolvedScope {
        cwd: state.inner.cwd.clone(),
        source: draft_source,
    };
    let draft = thread_snapshot(&state, &draft_scope, None)
        .await
        .expect("draft snapshot");
    assert_eq!(draft["pendingActions"][0]["actionId"], "permission-draft");
    assert_eq!(draft["pendingActions"][0]["sourceKey"], draft_source_key);
    assert_eq!(draft["pendingActions"][0]["payload"]["allowAlways"], true);
    assert_eq!(
        draft["pendingActions"][0]["payload"]["matchedRule"],
        "prompt"
    );
}

#[tokio::test]
async fn source_started_pending_clarify_survives_unbound_canonical_snapshot() {
    let (_temp, state) = web_state().await;
    let draft_source = GatewaySource::new("web", "cwd:test:draft:clarify").persistent();
    let draft_source_key = draft_source.source_key().0;
    let owner_id = "gateway:foreign";
    let activity = state
        .inner
        .durability
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: "activity-draft-clarify",
            thread_id: None,
            source_key: Some(&draft_source_key),
            turn_id: Some("turn-draft"),
            kind: GatewayActivityKind::Turn,
            owner_id,
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .await
        .expect("claim activity");
    state
        .inner
        .pending_actions
        .lock()
        .expect("pending actions")
        .insert(
            "clarify-draft".to_string(),
            PendingActionView {
                action_id: "clarify-draft".to_string(),
                kind: GatewayActionKind::Clarify,
                title: Some("Clarify".to_string()),
                summary: Some("Choose mode".to_string()),
                payload: json!({
                    "raw": {
                        "questions": [{
                            "question": "Choose mode",
                            "options": [
                                {"label": "Fast", "description": "Short answer"},
                                {"label": "Deep", "description": "More detail"}
                            ]
                        }]
                    }
                }),
                thread_id: None,
                turn_id: Some("turn-draft".to_string()),
                activity_id: Some(activity.activity_id.clone()),
                source_key: Some(draft_source_key.clone()),
                owner_id: Some(owner_id.to_string()),
                lease_expires_at_ms: Some(activity.lease_expires_at_ms),
            },
        );

    let canonical_scope = ResolvedScope {
        cwd: state.inner.cwd.clone(),
        source: state.inner.source.clone(),
    };
    let canonical = thread_snapshot(&state, &canonical_scope, None)
        .await
        .expect("canonical snapshot");
    assert_eq!(
        canonical["pendingActions"]
            .as_array()
            .expect("canonical actions")
            .len(),
        0
    );
    assert!(
        state
            .inner
            .pending_actions
            .lock()
            .expect("pending actions")
            .contains_key("clarify-draft")
    );

    let draft_scope = ResolvedScope {
        cwd: state.inner.cwd.clone(),
        source: draft_source,
    };
    let draft = thread_snapshot(&state, &draft_scope, None)
        .await
        .expect("draft snapshot");
    assert_eq!(draft["pendingActions"][0]["actionId"], "clarify-draft");
    assert_eq!(draft["pendingActions"][0]["sourceKey"], draft_source_key);
}

#[tokio::test]
async fn public_pending_interaction_responses_are_typed_and_accepted_once() {
    let (_temp, state) = web_state().await;
    let thread_id = start_web_thread(&state).await;
    let source_key = state.inner.source.source_key().0;
    let owner_id = "gateway:foreign";
    let activity = state
        .inner
        .durability
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: "activity-draft-route",
            thread_id: Some(&thread_id),
            source_key: Some(&source_key),
            turn_id: Some("turn-draft"),
            kind: GatewayActivityKind::Turn,
            owner_id,
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .await
        .expect("claim activity");
    for (action_id, kind) in [
        ("permission-draft", GatewayActionKind::Permission),
        ("clarify-draft", GatewayActionKind::Clarify),
    ] {
        state
            .inner
            .pending_actions
            .lock()
            .expect("pending actions")
            .insert(
                action_id.to_string(),
                PendingActionView {
                    action_id: action_id.to_string(),
                    kind,
                    title: None,
                    summary: None,
                    payload: json!({}),
                    thread_id: Some(thread_id.clone()),
                    turn_id: Some("turn-draft".to_string()),
                    activity_id: Some(activity.activity_id.clone()),
                    source_key: Some(source_key.clone()),
                    owner_id: Some(owner_id.to_string()),
                    lease_expires_at_ms: Some(activity.lease_expires_at_ms),
                },
            );
    }
    let (tx, _rx) = mpsc::unbounded_channel();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let permission = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "thread/interaction/respond".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": thread_id,
                "interactionId": "permission-draft",
                "response": { "kind": "permission", "decision": "allowOnce" }
            })),
        },
    )
    .await
    .expect("thread/interaction/respond permission");
    assert_eq!(permission["accepted"], true);
    assert_eq!(permission["outcome"], "accepted");

    let repeated = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(11)),
            method: "thread/interaction/respond".to_string(),
            params: Some(json!({
                "scope": default_resolved_scope(&state, &AuthContext::Bearer)
                    .expect("scope")
                    .to_wire_scope(),
                "threadId": thread_id,
                "interactionId": "permission-draft",
                "response": { "kind": "permission", "decision": "allowOnce" }
            })),
        },
    )
    .await
    .expect_err("accepted interaction is consumed exactly once");
    assert!(
        repeated.to_string().contains("already resolved"),
        "{repeated}"
    );

    let clarify = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "thread/interaction/respond".to_string(),
            params: Some(json!({
                "scope": default_resolved_scope(&state, &AuthContext::Bearer)
                    .expect("scope")
                    .to_wire_scope(),
                "threadId": thread_id,
                "interactionId": "clarify-draft",
                "response": {
                    "kind": "clarify",
                    "answers": [["Local"], ["Fix"]]
                }
            })),
        },
    )
    .await
    .expect("thread/interaction/respond clarify");
    assert_eq!(clarify["accepted"], true);

    let commands = state
        .inner
        .durability
        .claim_pending_gateway_control_commands(owner_id, 10)
        .await
        .expect("pending control commands");
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].activity_id, "activity-draft-route");
    assert_eq!(
        commands[0].command_kind,
        GatewayControlCommandKind::Permission
    );
    assert_eq!(commands[0].payload["requestId"], "permission-draft");
    assert_eq!(commands[0].payload["decision"], "allow_once");
    assert_eq!(commands[1].activity_id, "activity-draft-route");
    assert_eq!(commands[1].command_kind, GatewayControlCommandKind::Clarify);
    assert_eq!(commands[1].payload["requestId"], "clarify-draft");
    assert_eq!(commands[1].payload["answers"][0][0], "Local");
}
