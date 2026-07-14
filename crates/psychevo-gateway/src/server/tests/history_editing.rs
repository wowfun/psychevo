#[tokio::test]
async fn native_history_draft_edit_restore_and_point_fork_share_one_typed_contract() {
    let (_temp, state) = web_state();
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake", None)
        .expect("session");
    let profile = generated_runtime_profiles()
        .into_iter()
        .find(|profile| profile.id == "native")
        .expect("Native profile");
    let profile_json = serde_json::to_string(&profile).expect("profile snapshot");
    let profile_fingerprint = crate::runtime_profile_config_fingerprint(&profile);
    let profile_revision = crate::runtime_profile_config_revision(&profile_fingerprint).to_string();
    let agent_fingerprint = crate::gateway_agent_definition_fingerprint("null");
    let cwd = state.inner.cwd.display().to_string();
    state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &session_id,
            agent_ref: None,
            agent_fingerprint: &agent_fingerprint,
            agent_definition_json: "null",
            runtime_ref: "native",
            backend_kind: "native",
            native_kind: "native",
            native_session_id: Some(&session_id),
            cwd: &cwd,
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_json,
            adapter_kind: "native",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("binding");
    let message = RuntimeMessage::User {
        content: vec![
            UserContentBlock::text("visible plus injected context"),
            UserContentBlock::image_url("https://example.test/image.png"),
        ],
        timestamp_ms: 1,
    };
    let metadata = json!({
        psychevo_runtime::EDITABLE_INPUT_METADATA_KEY: {
            "version": 1,
            "parts": [
                {"type": "text", "text": "visible"},
                {"type": "image", "imageBlockIndex": 0}
            ]
        }
    });
    let message_seq = state
        .inner
        .state
        .store()
        .append_message_with_undo_snapshot_metadata_and_context_evidence(
            &session_id,
            &message,
            Some(metadata),
            Some("visible".to_string()),
            &[],
        )
        .expect("message");
    let message_id = format!("message:{message_seq}");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let listed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("list-native-fork")),
            method: "thread/list".to_string(),
            params: Some(json!({"cwd": state.inner.cwd, "archived": false})),
        },
    )
    .await
    .expect("list native fork");
    let listed_session = listed["sessions"]
        .as_array()
        .and_then(|sessions| sessions.iter().find(|session| session["id"] == session_id))
        .expect("listed session");
    assert!(
        listed_session["lifecycle"]["actions"]
            .as_array()
            .is_some_and(|actions| actions.iter().any(|action| {
                action["id"] == "fork" && action["enabled"] == true
            }))
    );

    let read = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("draft")),
            method: "thread/history/draft/read".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session_id,
                "messageId": message_id,
            })),
        },
    )
    .await
    .expect("draft read");
    assert_eq!(read["fidelity"], "exact", "{read:#}");
    assert_eq!(
        read["parts"],
        json!([
            {"type": "text", "text": "visible"},
            {"type": "image", "input": {"kind": "url", "url": "https://example.test/image.png"}}
        ])
    );

    let legacy_seq = message_seq + 1;
    state
        .inner
        .state
        .store()
        .append_message(
            &session_id,
            &RuntimeMessage::User {
                content: vec![UserContentBlock::text("legacy visible plus flattened context")],
                timestamp_ms: 2,
            },
        )
        .expect("legacy message");
    let legacy = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("legacy-draft")),
            method: "thread/history/draft/read".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session_id,
                "messageId": format!("message:{legacy_seq}"),
            })),
        },
    )
    .await
    .expect("legacy draft");
    assert_eq!(legacy["fidelity"], "bestEffort", "{legacy:#}");
    assert!(
        legacy["warning"]
            .as_str()
            .is_some_and(|warning| warning.contains("reconstructed"))
    );

    let synthetic_seq = legacy_seq + 1;
    state
        .inner
        .state
        .store()
        .append_message_with_undo_snapshot_metadata_and_context_evidence(
            &session_id,
            &RuntimeMessage::User {
                content: vec![UserContentBlock::text("synthetic resource payload")],
                timestamp_ms: 3,
            },
            Some(json!({
                psychevo_runtime::EDITABLE_INPUT_METADATA_KEY: {
                    "version": 1,
                    "parts": []
                }
            })),
            None,
            &[],
        )
        .expect("synthetic-only message");
    let synthetic = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("synthetic-draft")),
            method: "thread/history/draft/read".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session_id,
                "messageId": format!("message:{synthetic_seq}"),
            })),
        },
    )
    .await
    .expect("synthetic draft");
    assert_eq!(
        synthetic["unavailableReason"],
        "This message has no editable text or image input."
    );

    let no_op = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("noop")),
            method: "thread/action/run".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session_id,
                "action": {"kind": "revertConversation", "messageId": message_id, "draft": {"parts": read["parts"]}}
            })),
        },
    )
    .await
    .expect("no-op edit");
    assert_eq!(no_op["noOp"], true, "{no_op:#}");
    assert!(
        state
            .inner
            .state
            .store()
            .session_revert_state(&session_id)
            .expect("revert state")
            .is_none()
    );

    let replacement = json!([{"type": "text", "text": "edited"}]);
    let staged = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("stage")),
            method: "thread/action/run".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session_id,
                "action": {"kind": "revertConversation", "messageId": message_id, "draft": {"parts": replacement}}
            })),
        },
    )
    .await
    .expect("stage edit");
    assert_eq!(staged["staged"], true, "{staged:#}");
    assert_eq!(
        staged["snapshot"]["historyEditing"]["kind"],
        "conversationEdit"
    );
    assert_eq!(staged["snapshot"]["historyEditing"]["hiddenEntryCount"], 3);
    assert_eq!(staged["snapshot"]["entries"], json!([]));

    let retried = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("retry-stage")),
            method: "thread/action/run".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session_id,
                "action": {"kind": "revertConversation", "messageId": message_id, "draft": {"parts": replacement}}
            })),
        },
    )
    .await
    .expect("idempotent stage retry");
    assert_eq!(retried["staged"], true, "{retried:#}");
    assert_eq!(retried["noOp"], false, "{retried:#}");

    let restored = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("restore")),
            method: "thread/action/run".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session_id,
                "action": {"kind": "unrevertConversation"}
            })),
        },
    )
    .await
    .expect("restore history");
    assert_eq!(restored["draft"]["parts"], replacement);
    assert!(restored["snapshot"]["historyEditing"].is_null());
    assert_eq!(restored["snapshot"]["entries"].as_array().map(Vec::len), Some(3));

    let forked = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("fork")),
            method: "thread/action/run".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session_id,
                "action": {"kind": "forkBefore", "messageId": message_id}
            })),
        },
    )
    .await
    .expect("point fork");
    assert_eq!(forked["kind"], "forkBefore", "{forked:#}");
    assert_eq!(forked["sourceThreadId"], session_id);
    assert_eq!(forked["snapshot"]["entries"], json!([]));
    assert_eq!(
        forked["snapshot"]["thread"]["forkedFromThreadId"],
        session_id
    );
}
