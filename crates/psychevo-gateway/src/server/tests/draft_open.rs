use psychevo_gateway_protocol as wire;
use serde_json::json;
use tokio::sync::mpsc;

use crate::server::binding::{AuthContext, BrowserSession};
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;
use crate::server::runtime_profiles::thread_context_read_result_live;
use crate::server::scope_session::{
    ResolvedScope, canonical_source_mutation_key, default_resolved_scope, detached_draft_scope,
};
use crate::server::tests::helpers::web_state;

#[tokio::test]
async fn default_draft_open_returns_one_exact_authoritative_context() {
    let (_temp, state) = web_state().await;
    let browser_session_id = "draft-open-browser".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("browser sessions")
        .insert(
            browser_session_id.clone(),
            BrowserSession::with_external_action_grant(
                state.inner.cwd.clone(),
                state.inner.source.clone(),
            ),
        );
    let origin = ResolvedScope {
        cwd: state.inner.cwd.clone(),
        source: state.inner.source.clone(),
    }
    .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state,
        AuthContext::Browser {
            session_id: browser_session_id,
        },
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "thread/draft/open".to_string(),
            params: Some(json!({
                "origin": origin,
                "targetIntent": { "kind": "default" }
            })),
        },
    )
    .await
    .expect("default draft open");

    assert!(result["snapshot"]["thread"].is_null());
    assert!(result["context"]["selectedTargetId"].is_string());
    assert!(result["context"]["suggestedTargetId"].is_null());
    assert_ne!(
        result["context"]["sendability"]["reason"],
        "Select an Agent target before starting a turn.",
        "{result:#}"
    );
    assert!(result["context"].get("targetId").is_none());
    assert!(result["problem"].is_null());
}

#[tokio::test]
async fn unavailable_exact_target_returns_atomic_context_and_problem() {
    let (_temp, state) = web_state().await;
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[agents.backends.broken]
kind = "acp"
label = "Broken"
command = "definitely-missing-psychevo-agent"
entrypoints = ["peer"]
"#,
    )
    .expect("backend config");
    let agents = state.inner.cwd.join(".psychevo/agents");
    std::fs::create_dir_all(&agents).expect("agents");
    std::fs::write(
        agents.join("broken.md"),
        "---\ndescription: Broken peer\nbackend:\n  ref: broken\nentrypoints: [peer]\n---\nBroken fixture.\n",
    )
    .expect("Agent Definition");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let discovery = thread_context_read_result_live(
        &state,
        &scope,
        wire::agents_backend_rpc::ThreadContextReadParams {
            thread_id: None,
            target: None,
            scope: Some(scope.to_wire_scope()),
        },
    )
    .await
    .expect("discovery");
    let target_id = discovery
        .compatible_targets
        .iter()
        .find(|target| target.agent_ref.as_deref() == Some("broken"))
        .expect("broken target")
        .target_id
        .clone();
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "thread/draft/open".to_string(),
            params: Some(json!({
                "origin": scope.to_wire_scope(),
                "targetIntent": { "kind": "exact", "targetId": target_id }
            })),
        },
    )
    .await
    .expect("blocked draft open remains a result");

    assert_eq!(result["context"]["selectedTargetId"], target_id);
    assert_eq!(result["problem"]["code"], "runtime_unavailable");
    assert_eq!(result["problem"]["retryClass"], "userAction");
    assert_eq!(result["context"]["sendability"]["allowed"], false);
}
#[tokio::test]
async fn detached_draft_keeps_the_canonical_source_mutation_lane() {
    let (_temp, state) = web_state().await;
    let origin = default_resolved_scope(&state, &AuthContext::Bearer).expect("origin");
    let detached = detached_draft_scope(
        &origin,
        &AuthContext::Browser {
            session_id: "draft-lock-browser".to_string(),
        },
    );
    let nested = detached_draft_scope(
        &detached,
        &AuthContext::Browser {
            session_id: "draft-lock-browser".to_string(),
        },
    );

    assert_eq!(
        canonical_source_mutation_key(&origin.source),
        canonical_source_mutation_key(&detached.source)
    );
    assert_eq!(
        canonical_source_mutation_key(&origin.source),
        canonical_source_mutation_key(&nested.source)
    );
}
