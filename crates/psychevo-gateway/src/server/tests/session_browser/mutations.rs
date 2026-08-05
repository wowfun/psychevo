use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::server::binding::AuthContext;
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;
use crate::server::scope_session::{
    bind_source_to_thread, default_resolved_scope, reset_source_to_empty,
};
use crate::server::tests::helpers::web_state;
use crate::server::workspace::workspace_dir_name;

#[tokio::test]
async fn workspace_dir_name_rejects_path_components() {
    assert_eq!(workspace_dir_name(" notes ").expect("trimmed"), "notes");
    let err = workspace_dir_name("../notes").expect_err("parent path rejected");
    assert!(
        err.to_string()
            .contains("workspace name must be a single directory name")
    );
}

#[tokio::test]
async fn reset_source_to_empty_archives_previous_binding_without_replacement() {
    let (_temp, state) = web_state().await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let mut start = psychevo::StartThreadRequest::new(&state.inner.cwd);
    start.source = "web".to_string();
    let first_id = state
        .inner
        .framework
        .start_thread(start)
        .await
        .expect("thread")
        .id()
        .to_string();
    bind_source_to_thread(&state, &scope, &first_id)
        .await
        .expect("bind");

    let snapshot = reset_source_to_empty(&state, &scope).await.expect("reset");

    assert!(snapshot.get("thread").is_some_and(Value::is_null));
    assert!(
        state
            .inner
            .gateway
            .resolve_source_thread(&state.inner.source)
            .await
            .expect("source lookup")
            .is_none()
    );
    let first_summary = state
        .inner
        .framework
        .resume_thread(&first_id)
        .await
        .expect("first thread")
        .snapshot()
        .await
        .expect("first snapshot")
        .summary;
    assert_eq!(first_summary.end_reason.as_deref(), Some("gateway_reset"));
    assert!(first_summary.archived_at_ms.is_some());
    assert_eq!(
        state
            .inner
            .framework
            .list_threads(psychevo::ThreadListQuery {
                cwd: Some(state.inner.cwd.clone()),
                ..psychevo::ThreadListQuery::default()
            })
            .await
            .expect("active threads")
            .threads
            .len(),
        0
    );
}

#[tokio::test]
async fn bind_source_to_thread_rebinds_existing_session() {
    let (_temp, state) = web_state().await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let mut start = psychevo::StartThreadRequest::new(&state.inner.cwd);
    start.source = "web".to_string();
    let session_id = state
        .inner
        .framework
        .start_thread(start)
        .await
        .expect("thread")
        .id()
        .to_string();

    bind_source_to_thread(&state, &scope, &session_id)
        .await
        .expect("bind");

    assert_eq!(
        state
            .inner
            .gateway
            .resolve_source_thread(&state.inner.source)
            .await
            .expect("source lookup")
            .as_deref(),
        Some(session_id.as_str())
    );
}

#[tokio::test]
async fn bind_source_to_thread_does_not_restore_an_archived_session() {
    let (_temp, state) = web_state().await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let mut start = psychevo::StartThreadRequest::new(&state.inner.cwd);
    start.source = "web".to_string();
    let thread = state
        .inner
        .framework
        .start_thread(start)
        .await
        .expect("thread");
    let thread_id = thread.id().to_string();
    thread.archive().await.expect("archive");

    bind_source_to_thread(&state, &scope, &thread_id)
        .await
        .expect("bind");

    assert!(
        thread.summary().await.expect("summary").archived,
        "source binding must not perform the explicit Thread restore lifecycle"
    );
}

#[tokio::test]
async fn deleting_the_idle_current_thread_clears_its_source_binding() {
    let (_temp, state) = web_state().await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let mut start = psychevo::StartThreadRequest::new(&state.inner.cwd);
    start.source = "web".to_string();
    let session_id = state
        .inner
        .framework
        .start_thread(start)
        .await
        .expect("thread")
        .id()
        .to_string();
    bind_source_to_thread(&state, &scope, &session_id)
        .await
        .expect("bind");
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "thread/delete".to_string(),
            params: Some(json!({ "threadId": session_id })),
        },
    )
    .await
    .expect("delete idle current Thread");

    assert_eq!(result["deleted"], true);
    assert!(
        state
            .inner
            .framework
            .resume_thread(&session_id)
            .await
            .is_err()
    );
    assert!(
        state
            .inner
            .gateway
            .resolve_source_thread(&scope.source)
            .await
            .expect("source lookup")
            .is_none()
    );
}
