use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use psychevo::application::Message as RuntimeMessage;
use psychevo::paths::canonicalize_cwd;
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::composition::GatewayApplication;
use crate::server::GatewayWebServerConfig;
use crate::server::auth_input::apply_mentions_to_turn_policy;
use crate::server::bind_gateway_web_server;
use crate::server::binding::{AuthContext, BrowserSession, WebState};
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;
use crate::server::scope_session::{
    bind_source_to_thread, default_resolved_scope, start_empty_source,
};
use crate::server::settings_observability::display_cwd;
use crate::server::tests::helpers::{
    web_state, web_state_with_env, web_state_with_native_test_executor,
};

#[tokio::test]
async fn peer_runtime_rejects_structured_self_agent_mention() {
    let (_temp, state) = web_state().await;
    let (_, mut intent) = state.thread_turn_request(state.inner.cwd.clone(), None, Vec::new());
    intent.policy.runtime_profile_ref = Some("opencode".to_string());
    let err = apply_mentions_to_turn_policy(
        &mut intent.policy,
        &[wire::source::GatewayMention {
            visible_text: "@opencode".to_string(),
            range: wire::source::GatewayMentionRange { start: 0, end: 9 },
            target: wire::source::GatewayMentionTarget::Agent {
                name: "opencode".to_string(),
                source: Some("generated".to_string()),
                entrypoints: vec!["subagent".to_string()],
                backend_ref: Some("opencode".to_string()),
            },
        }],
    )
    .expect_err("self delegation should be rejected");
    assert!(err.to_string().contains("already the current runtime"));
}

#[tokio::test]
async fn peer_runtime_allows_literal_agent_text_without_structured_mention() {
    let (_temp, state) = web_state().await;
    let (_, mut intent) = state.thread_turn_request(state.inner.cwd.clone(), None, Vec::new());
    intent.policy.runtime_profile_ref = Some("opencode".to_string());
    apply_mentions_to_turn_policy(&mut intent.policy, &[]).expect("literal text is not inspected");
    assert!(intent.policy.skill_inputs.is_empty());
}

fn accounted_assistant_executor(
    context_tokens: u64,
    cache_read_tokens: u64,
) -> crate::FrameworkNativeTestExecutor {
    Arc::new(move |invocation| {
        Box::pin(async move {
            invocation.persistence.confirm_delivery().await?;
            invocation
                .persistence
                .append_message_with_metrics(
                    RuntimeMessage::Assistant {
                        content: vec![psychevo::application::AssistantBlock::Text {
                            text: "done".to_string(),
                        }],
                        timestamp_ms: 1,
                        finish_reason: Some("stop".to_string()),
                        outcome: psychevo::application::Outcome::Normal,
                        model: Some("fake-model".to_string()),
                        provider: Some("fake-provider".to_string()),
                    },
                    Some(json!({
                        "input_tokens": context_tokens,
                        "total_tokens": context_tokens,
                        "cached_tokens": cache_read_tokens,
                    })),
                    None,
                )
                .await?;
            Ok(psychevo::TurnResult {
                thread_id: invocation.receipt.thread_id,
                outcome: psychevo::TurnOutcome::Completed,
                final_answer: String::new(),
                provider: "fake-provider".to_string(),
                model: "fake-model".to_string(),
                reasoning_effort: None,
                tool_failures: 0,
                context_limit: None,
                context_snapshot: None,
                warnings: Vec::new(),
                terminal_reason: None,
                terminal_error: None,
                selected_agent: None,
                selected_skills: Vec::new(),
            })
        })
    })
}

async fn start_accounted_thread(state: &WebState, cwd: &Path) -> psychevo::Thread {
    let mut start = psychevo::StartThreadRequest::new(cwd);
    start.source = "web".to_string();
    let thread = state
        .inner
        .framework
        .start_thread(start)
        .await
        .expect("Thread");
    thread
        .start_turn(psychevo::TurnRequest::new("measure this context"))
        .await
        .expect("Turn")
        .wait()
        .await
        .expect("Turn completion");
    thread
}

async fn occupied_port_with_free_successor() -> TcpListener {
    for _ in 0..100 {
        let occupied = TcpListener::bind("127.0.0.1:0").await.expect("occupy port");
        let port = occupied.local_addr().expect("occupied addr").port();
        let Some(next_port) = port.checked_add(1) else {
            continue;
        };
        if let Ok(probe) = TcpListener::bind(("127.0.0.1", next_port)).await {
            drop(probe);
            return occupied;
        }
    }
    panic!("could not find adjacent free loopback ports");
}

#[tokio::test]
async fn bind_gateway_web_server_falls_back_from_used_port() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let static_dir = temp.path().join("static");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&static_dir).expect("static dir");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let runtime =
        GatewayApplication::open(home, temp.path().join("state.db"), None, BTreeMap::new())
            .await
            .expect("test composition");
    let occupied = occupied_port_with_free_successor().await;
    let occupied_addr = occupied.local_addr().expect("occupied addr");
    let mut config = GatewayWebServerConfig::with_static(runtime, cwd, static_dir);
    config.bind_addr = occupied_addr;
    config.bind_port_fallbacks = 1;

    let bound = bind_gateway_web_server(config).await.expect("bind");

    assert_eq!(bound.local_addr().ip(), occupied_addr.ip());
    assert_eq!(bound.local_addr().port(), occupied_addr.port() + 1);
}

#[tokio::test]
async fn initialize_reports_current_profile() {
    let mut env = BTreeMap::new();
    env.insert("PSYCHEVO_PROFILE".to_string(), "coder".to_string());
    let (temp, state) = web_state_with_env(env).await;
    let home = temp.path().join("home").display().to_string();
    let expected_display_cwd = display_cwd(&state.inner.cwd);
    let (out_tx, _out_rx) = mpsc::unbounded_channel();

    let value = handle_rpc(
        state,
        AuthContext::Bearer,
        out_tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: None,
        },
    )
    .await
    .expect("initialize");
    let initialize: wire::thread_command_turn::InitializeResult =
        serde_json::from_value(value.clone()).expect("typed initialize result");

    assert_eq!(value["profile"]["name"], "coder");
    assert_eq!(value["profile"]["home"].as_str(), Some(home.as_str()));
    assert_eq!(value["profile"]["default"], false);
    assert_eq!(initialize.display_cwd, expected_display_cwd);
}

#[tokio::test]
async fn observability_read_returns_active_session_usage() {
    let (_temp, state) =
        web_state_with_native_test_executor(accounted_assistant_executor(200, 50)).await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_id = start_accounted_thread(&state, &state.inner.cwd)
        .await
        .id()
        .to_string();
    bind_source_to_thread(&state, &scope, &session_id)
        .await
        .expect("bind");
    let (tx, _rx) = mpsc::unbounded_channel();

    let value = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "observability/read".to_string(),
            params: Some(json!({ "scope": scope.to_wire_scope() })),
        },
    )
    .await
    .expect("observability/read");

    assert_eq!(value["usage"]["available"], true);
    assert_eq!(value["usage"]["sessionId"], session_id);
    assert_eq!(value["usage"]["contextInputTokens"], 200);
    assert_eq!(value["usage"]["cacheReadTokens"], 50);
    assert_eq!(value["usage"]["estimatedCostNanodollars"], 0);
    assert_eq!(value["usage"]["cacheReadPercent"], 25.0);
    let categories = value["context"]["categories"]
        .as_array()
        .expect("context categories");
    assert!(!categories.is_empty());
    assert!(
        categories
            .iter()
            .all(|category| category.get("details").is_some())
    );
    assert!(
        categories
            .iter()
            .all(|category| category.get("id").and_then(Value::as_str) != Some("free_space"))
    );
    let serialized_categories = serde_json::to_string(categories).expect("categories json");
    assert!(!serialized_categories.contains("done"));
    assert!(!serialized_categories.contains("content"));
}

#[tokio::test]
async fn observability_read_returns_explicit_thread_usage_without_active_binding() {
    let (_temp, state) =
        web_state_with_native_test_executor(accounted_assistant_executor(90, 9)).await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_id = start_accounted_thread(&state, &state.inner.cwd)
        .await
        .id()
        .to_string();
    let (tx, _rx) = mpsc::unbounded_channel();

    let value = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "observability/read".to_string(),
            params: Some(json!({ "scope": scope.to_wire_scope(), "threadId": session_id })),
        },
    )
    .await
    .expect("observability/read");

    assert_eq!(value["usage"]["available"], true);
    assert_eq!(value["usage"]["contextInputTokens"], 90);
    assert_eq!(value["usage"]["cacheReadPercent"], 10.0);
}

#[tokio::test]
async fn observability_read_clears_usage_when_no_active_session() {
    let (_temp, state) = web_state().await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let (tx, _rx) = mpsc::unbounded_channel();

    let value = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "observability/read".to_string(),
            params: Some(json!({ "scope": scope.to_wire_scope() })),
        },
    )
    .await
    .expect("observability/read");

    assert_eq!(value["usage"]["available"], false);
    assert_eq!(value["usage"]["reportedTotalTokens"], 0);
    assert_eq!(value["context"]["available"], false);
}

#[tokio::test]
async fn browser_observability_read_authorizes_cross_cwd_thread() {
    let (temp, state) =
        web_state_with_native_test_executor(accounted_assistant_executor(300, 150)).await;
    let other_cwd = temp.path().join("other-work");
    std::fs::create_dir_all(&other_cwd).expect("other cwd");
    let other_cwd = canonicalize_cwd(&other_cwd).expect("other canonical");
    let session_id = start_accounted_thread(&state, &other_cwd)
        .await
        .id()
        .to_string();
    let browser_session_id = "browser-session".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("sessions")
        .insert(
            browser_session_id.clone(),
            BrowserSession::with_external_action_grant(
                state.inner.cwd.clone(),
                state.inner.source.clone(),
            ),
        );
    let auth = AuthContext::Browser {
        session_id: browser_session_id,
    };
    let current_scope = default_resolved_scope(&state, &auth).expect("scope");
    let (tx, _rx) = mpsc::unbounded_channel();

    let value = handle_rpc(
        state,
        auth,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "observability/read".to_string(),
            params: Some(json!({
                "scope": current_scope.to_wire_scope(),
                "threadId": session_id
            })),
        },
    )
    .await
    .expect("observability/read");

    assert_eq!(value["usage"]["available"], true);
    assert_eq!(value["usage"]["sessionId"], session_id);
    assert_eq!(value["usage"]["contextInputTokens"], 300);
    assert_eq!(value["usage"]["cacheReadPercent"], 50.0);
}

#[tokio::test]
async fn start_empty_source_returns_null_thread_and_creates_no_session() {
    let (_temp, state) = web_state().await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

    let snapshot = start_empty_source(&state, &scope).await.expect("snapshot");

    assert!(snapshot.get("thread").is_some_and(Value::is_null));
    assert!(
        state
            .inner
            .framework
            .list_human_threads(psychevo::application::HumanThreadListQuery {
                cwd: Some(state.inner.cwd.clone()),
                ..psychevo::application::HumanThreadListQuery::default()
            })
            .await
            .expect("Threads")
            .threads
            .is_empty()
    );
    assert_eq!(
        state
            .inner
            .gateway
            .resolve_source_thread(&state.inner.source)
            .await
            .expect("source lookup")
            .as_deref(),
        None
    );
}

#[tokio::test]
async fn start_empty_source_clears_binding_without_archiving_previous_history() {
    let (_temp, state) = web_state().await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let mut request = psychevo::StartThreadRequest::new(&state.inner.cwd);
    request.source = "web".to_string();
    let session_id = state
        .inner
        .framework
        .start_thread(request)
        .await
        .expect("Thread")
        .id()
        .to_string();
    bind_source_to_thread(&state, &scope, &session_id)
        .await
        .expect("bind");

    let snapshot = start_empty_source(&state, &scope).await.expect("snapshot");

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
    let active_ids = state
        .inner
        .framework
        .list_human_threads(psychevo::application::HumanThreadListQuery {
            cwd: Some(state.inner.cwd.clone()),
            ..psychevo::application::HumanThreadListQuery::default()
        })
        .await
        .expect("active Threads")
        .threads
        .into_iter()
        .map(|thread| thread.summary.id)
        .collect::<Vec<_>>();

    assert_eq!(active_ids, vec![session_id]);
}
