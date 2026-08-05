use std::time::Duration;

use psychevo_gateway_protocol as wire;
use serde_json::json;
use tokio::sync::mpsc;

use crate::server::binding::AuthContext;
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;
use crate::server::scope_session::default_resolved_scope;
use crate::server::tests::helpers::{
    framework_turn_fixture_executor, web_state_with_native_test_executor,
};

#[tokio::test]
async fn native_history_draft_edit_restore_and_point_fork_share_one_typed_contract() {
    Box::pin(native_history_draft_edit_restore_and_point_fork_contract()).await;
}

async fn native_history_draft_edit_restore_and_point_fork_contract() {
    let (_temp, state) =
        web_state_with_native_test_executor(framework_turn_fixture_executor(Vec::new())).await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("seed-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": scope,
                "target": {"agentRef": null, "runtimeProfileRef": "native"}
            })),
        },
    )
    .await
    .expect("seed context");
    let accepted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("seed-turn")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "clientTurnId": "history-edit-seed",
                "scope": scope,
                "threadId": null,
                "target": {"agentRef": null, "runtimeProfileRef": "native"},
                "input": [{"type": "text", "text": "visible"}],
                "turnOverrides": {"model": "fake-model"},
                "expectedContextRevision": context["contextRevision"],
                "expectedControlRevision": context["controlRevision"]
            })),
        },
    )
    .await
    .expect("fixture Turn");
    let session_id = accepted["threadId"]
        .as_str()
        .expect("accepted Thread id")
        .to_string();
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(message) = rx.recv().await {
            if message.contains("\"type\":\"turnCompleted\"") {
                return;
            }
        }
        panic!("fixture Turn notification channel closed")
    })
    .await
    .expect("fixture Turn completion");
    let thread = state
        .inner
        .framework
        .resume_thread(&session_id)
        .await
        .expect("fixture Thread");
    let message_seq = thread
        .history()
        .latest(Some(200))
        .await
        .expect("history")
        .items
        .into_iter()
        .find(|item| matches!(item.message, psychevo::application::Message::User { .. }))
        .expect("editable user input")
        .session_seq;
    let message_id = format!("message:{message_seq}");
    let listed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
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
            .is_some_and(|actions| actions
                .iter()
                .any(|action| { action["id"] == "fork" && action["enabled"] == true }))
    );

    let read = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
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
    assert_eq!(read["parts"], json!([{"type": "text", "text": "visible"}]));

    let available = state
        .inner
        .gateway
        .native_history_actions(
            &session_id,
            crate::history_editing::HistoryEditingSurface::Workbench,
        )
        .await
        .expect("history availability");
    assert!(available.unavailable_reason.is_none());
    assert!(available.staged.is_none());

    let no_op = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
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
        thread
            .history_editing_state()
            .await
            .expect("history editing state")
            .staged
            .is_none()
    );

    let replacement = json!([{"type": "text", "text": "edited"}]);
    let staged = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
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
    assert_eq!(staged["snapshot"]["historyEditing"]["hiddenEntryCount"], 1);
    assert_eq!(staged["snapshot"]["entries"], json!([]));
    let staged_status = state
        .inner
        .gateway
        .native_history_actions(
            &session_id,
            crate::history_editing::HistoryEditingSurface::Workbench,
        )
        .await
        .expect("staged history status");
    assert_eq!(
        staged_status.staged.as_ref().map(|staged| staged.kind),
        Some(wire::events_transcript::ThreadHistoryEditingKind::ConversationEdit)
    );
    assert!(staged_status.unavailable_reason.is_some());
    let staged_draft_read = state
        .inner
        .gateway
        .read_native_editable_draft(
            &session_id,
            &message_id,
            crate::history_editing::HistoryEditingSurface::Workbench,
        )
        .await
        .expect("staged draft read");
    assert!(staged_draft_read.unavailable_reason.is_some());

    let retried = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
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
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
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
    assert_eq!(
        restored["snapshot"]["entries"].as_array().map(Vec::len),
        Some(1)
    );

    let forked = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
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
