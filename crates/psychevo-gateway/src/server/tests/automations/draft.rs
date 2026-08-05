use std::collections::BTreeSet;
use std::sync::Arc;

use psychevo_gateway_protocol as wire;
use serde_json::json;
use tokio::sync::mpsc;

use super::helpers::{AutomationTurnProbe, web_state_with_automation_framework_provider};
use crate::server::binding::AuthContext;
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;

#[tokio::test]
async fn automation_draft_returns_model_draft_without_persisting_task() {
    let backend = Arc::new(AutomationTurnProbe::default());
    let (_temp, state, provider_requests) =
        web_state_with_automation_framework_provider(backend.clone()).await;
    let (tx, _rx) = mpsc::unbounded_channel();

    let drafted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "automation/draft".to_string(),
            params: Some(json!({
                "request": "Check this project every morning before standup."
            })),
        },
    )
    .await
    .expect("automation/draft");
    assert_eq!(drafted["draft"]["target"]["kind"], "project");
    assert_eq!(drafted["draft"]["title"], "Morning project check");
    assert_eq!(
        drafted["draft"]["schedule"],
        json!({"kind": "daily", "time": "09:00"})
    );
    assert_eq!(drafted["draft"]["execution"]["policy"], "autoSandbox");

    let listed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "automation/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("automation/list");
    assert!(
        listed["automations"]
            .as_array()
            .expect("automations")
            .is_empty()
    );

    assert!(
        backend.runs.lock().expect("runs").is_empty(),
        "automation/draft must not bypass Framework through a legacy Gateway executor"
    );
    let provider_requests = provider_requests.lock().expect("provider requests");
    assert_eq!(provider_requests.len(), 2);
    assert!(
        provider_requests[0]
            .to_string()
            .contains("Return only one JSON object")
    );
    let tool_names = provider_requests[0]["tools"]
        .as_array()
        .expect("provider tools")
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(!tool_names.contains("automation"));
    assert!(!tool_names.contains("spawn_agent"));
    assert!(!tool_names.contains("clarify"));
    assert!(
        provider_requests[1].to_string().contains("read-only"),
        "the Framework runtime must reject the model's write in the read-only sandbox"
    );
    assert!(!state.inner.cwd.join("draft-write.txt").exists());
}
