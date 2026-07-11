#[tokio::test]
async fn thread_snapshot_prunes_pending_permission_without_live_activity() {
    let (_temp, state) = web_state();
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("session");
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

    let snapshot = thread_snapshot(&state, &scope, Some(&session_id)).expect("snapshot");

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
async fn cross_turn_runtime_permission_survives_event_recording_and_session_snapshot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let runtime_state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = crate::tests::gateway_with_cross_turn_child_interaction(runtime_state);
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home.clone(),
        cwd,
        None,
        BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                home.to_string_lossy().to_string(),
            ),
        ]),
        temp.path().join("static"),
    ));
    let resolved_scope =
        default_resolved_scope(&state, &AuthContext::Bearer).expect("resolved scope");
    let scope = resolved_scope.to_wire_scope();
    let (tx, mut rx) = mpsc::unbounded_channel();

    let first = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "threadId": null,
                "runtimeRef": "codex",
                "runtimeOptions": {},
                "input": [{"type": "text", "text": "observe child"}]
            })),
        },
    )
    .await
    .expect("first turn/start");
    let parent_thread_id = first["threadId"]
        .as_str()
        .expect("first public thread")
        .to_string();
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(message) = rx.recv().await {
            let notification: Value = serde_json::from_str(&message).expect("notification JSON");
            if notification["method"] == "turn/result" {
                return;
            }
        }
        panic!("first turn notification channel closed");
    })
    .await
    .expect("first turn/result");
    let child = state
        .inner
        .state
        .store()
        .gateway_runtime_binding_by_native_session("codex", "codex-native-child-cross-turn")
        .expect("child binding read")
        .expect("first turn child binding");

    let second = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": parent_thread_id.clone(),
                "runtimeRef": "codex",
                "runtimeOptions": {},
                "input": [{"type": "text", "text": "request child command approval"}]
            })),
        },
    )
    .await
    .expect("second turn/start");
    assert_eq!(second["threadId"], parent_thread_id);
    let action_event = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(message) = rx.recv().await {
            let notification: Value = serde_json::from_str(&message).expect("notification JSON");
            if notification["method"] == "gateway/event"
                && notification["params"]["type"] == "actionRequested"
            {
                return notification["params"].clone();
            }
        }
        panic!("second turn notification channel closed");
    })
    .await
    .expect("runtime ActionRequested");
    let action_id = action_event["action"]["actionId"]
        .as_str()
        .expect("public action id")
        .to_string();

    let snapshot =
        thread_snapshot(&state, &resolved_scope, Some(&parent_thread_id)).expect("session snapshot");
    let actions = snapshot["pendingActions"]
        .as_array()
        .expect("pending actions");
    assert_eq!(actions.len(), 1, "{snapshot:#}");
    let action = &actions[0];
    assert_eq!(action["actionId"], action_id);
    assert_eq!(action["threadId"], parent_thread_id);
    assert_eq!(
        action["payload"]["origin"]["parentThreadId"],
        parent_thread_id
    );
    assert_eq!(
        action["payload"]["origin"]["childThreadId"],
        child.thread_id
    );
    assert_eq!(action["payload"]["allowSession"], true);
    assert_eq!(action["payload"]["authorizationLifetime"], "codex_session");
    let public_action = serde_json::to_string(action).expect("public pending action JSON");
    for native_id in [
        "codex-native-parent-cross-turn",
        "codex-native-child-cross-turn",
        "codex-native-request-cross-turn",
    ] {
        assert!(!public_action.contains(native_id), "{public_action}");
    }

    let response = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(3)),
            method: "permission/respond".to_string(),
            params: Some(json!({
                "requestId": action_id,
                "threadId": parent_thread_id,
                "decision": "allowSession"
            })),
        },
    )
    .await
    .expect("permission/respond");
    assert_eq!(response["accepted"], true);
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(message) = rx.recv().await {
            let notification: Value = serde_json::from_str(&message).expect("notification JSON");
            if notification["method"] == "turn/result" {
                return;
            }
        }
        panic!("second turn result channel closed");
    })
    .await
    .expect("second turn/result");
}

#[tokio::test]
async fn thread_snapshot_removes_pending_permission_after_activity_finishes() {
    let (_temp, state) = web_state();
    let store = state.inner.state.store();
    let session_id = store
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("session");
    let owner_id = "gateway:foreign";
    let activity = store
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: "activity-1",
            thread_id: Some(&session_id),
            source_key: None,
            turn_id: Some("turn-1"),
            kind: "turn",
            owner_id,
            owner_surface: Some("tui"),
            lease_expires_at_ms: gateway_now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
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

    let running = thread_snapshot(&state, &scope, Some(&session_id)).expect("running snapshot");
    assert_eq!(
        running["pendingActions"]
            .as_array()
            .expect("running actions")
            .len(),
        1
    );

    store
        .finish_gateway_activity(
            &activity.activity_id,
            owner_id,
            activity.generation,
            "failed",
        )
        .expect("finish activity");
    let finished = thread_snapshot(&state, &scope, Some(&session_id)).expect("finished snapshot");

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
    let (_temp, state) = web_state();
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("session");
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
    let snapshot = thread_snapshot(&state, &scope, Some(&session_id)).expect("snapshot");

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
    let (_temp, state) = web_state();
    let draft_source = GatewaySource::new("web", "cwd:test:draft:1").persistent();
    let draft_source_key = draft_source.source_key().0;
    let owner_id = "gateway:foreign";
    let activity = state
        .inner
        .state
        .store()
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: "activity-draft-permission",
            thread_id: None,
            source_key: Some(&draft_source_key),
            turn_id: Some("turn-draft"),
            kind: "turn",
            owner_id,
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
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
    let canonical = thread_snapshot(&state, &canonical_scope, None).expect("canonical snapshot");
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
    let draft = thread_snapshot(&state, &draft_scope, None).expect("draft snapshot");
    assert_eq!(
        draft["pendingActions"][0]["actionId"],
        "permission-draft"
    );
    assert_eq!(
        draft["pendingActions"][0]["sourceKey"],
        draft_source_key
    );
    assert_eq!(draft["pendingActions"][0]["payload"]["allowAlways"], true);
    assert_eq!(draft["pendingActions"][0]["payload"]["matchedRule"], "prompt");
}

#[tokio::test]
async fn source_started_pending_clarify_survives_unbound_canonical_snapshot() {
    let (_temp, state) = web_state();
    let draft_source = GatewaySource::new("web", "cwd:test:draft:clarify").persistent();
    let draft_source_key = draft_source.source_key().0;
    let owner_id = "gateway:foreign";
    let activity = state
        .inner
        .state
        .store()
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: "activity-draft-clarify",
            thread_id: None,
            source_key: Some(&draft_source_key),
            turn_id: Some("turn-draft"),
            kind: "turn",
            owner_id,
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
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
    let canonical = thread_snapshot(&state, &canonical_scope, None).expect("canonical snapshot");
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
    let draft = thread_snapshot(&state, &draft_scope, None).expect("draft snapshot");
    assert_eq!(draft["pendingActions"][0]["actionId"], "clarify-draft");
    assert_eq!(draft["pendingActions"][0]["sourceKey"], draft_source_key);
}

#[tokio::test]
async fn pending_interaction_responses_route_by_activity_context() {
    let (_temp, state) = web_state();
    let draft_source = GatewaySource::new("web", "cwd:test:draft:route").persistent();
    let draft_source_key = draft_source.source_key().0;
    let owner_id = "gateway:foreign";
    state
        .inner
        .state
        .store()
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: "activity-draft-route",
            thread_id: None,
            source_key: Some(&draft_source_key),
            turn_id: Some("turn-draft"),
            kind: "turn",
            owner_id,
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .expect("claim activity");
    let (tx, _rx) = mpsc::unbounded_channel();

    let permission = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "permission/respond".to_string(),
            params: Some(json!({
                "requestId": "permission-draft",
                "activityId": "activity-draft-route",
                "decision": "allowOnce"
            })),
        },
    )
    .await
    .expect("permission/respond");
    assert_eq!(permission["accepted"], true);

    let clarify = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "clarify/respond".to_string(),
            params: Some(json!({
                "requestId": "clarify-draft",
                "activityId": "activity-draft-route",
                "answers": [["Local"], ["Fix"]],
                "cancel": false
            })),
        },
    )
    .await
    .expect("clarify/respond");
    assert_eq!(clarify["accepted"], true);

    let commands = state
        .inner
        .state
        .store()
        .pending_gateway_control_commands(owner_id, 10)
        .expect("pending control commands");
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].activity_id, "activity-draft-route");
    assert_eq!(commands[0].command_kind, "permission");
    assert_eq!(commands[0].payload["requestId"], "permission-draft");
    assert_eq!(commands[0].payload["decision"], "allow_once");
    assert_eq!(commands[1].activity_id, "activity-draft-route");
    assert_eq!(commands[1].command_kind, "clarify");
    assert_eq!(commands[1].payload["requestId"], "clarify-draft");
    assert_eq!(commands[1].payload["answers"][0][0], "Local");
}
