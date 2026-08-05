use std::time::Duration;

use psychevo::application::AutomationRunStatus;
use psychevo_gateway_protocol as wire;
use serde_json::json;
use tokio::sync::mpsc;

use super::helpers::{
    live_xiaomi_token_plan_unavailable, live_xiaomi_token_plan_web_state,
    wait_for_automation_status_with_timeout,
};
use crate::server::binding::AuthContext;
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;

#[tokio::test]
#[ignore = "live provider opt-in"]
async fn live_xiaomi_token_plan_automation_manual_run_completes() {
    const PROVIDER: &str = "xiaomi-token-plan";
    const MODEL: &str = "xiaomi-token-plan/mimo-v2.5-pro";
    let (_temp, state) = live_xiaomi_token_plan_web_state().await;
    if live_xiaomi_token_plan_unavailable(&state) {
        return;
    }
    let (tx, _rx) = mpsc::unbounded_channel();
    let created = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("create-live-automation")),
            method: "automation/write".to_string(),
            params: Some(json!({
                "target": { "kind": "project" },
                "title": "Live Xiaomi automation smoke",
                "prompt": "Reply with exactly: automation live ok",
                "schedule": { "kind": "delay", "afterMinutes": 60 },
                "execution": { "policy": "autoSandbox" },
                "model": MODEL
            })),
        },
    )
    .await
    .expect("automation/write");
    let automation_id = created["automation"]["id"]
        .as_str()
        .expect("automation id")
        .to_string();

    let run = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("run-live-automation")),
            method: "automation/run".to_string(),
            params: Some(json!({ "automationId": automation_id })),
        },
    )
    .await
    .expect("automation/run");
    assert_eq!(run["accepted"], true);

    let task = wait_for_automation_status_with_timeout(
        &state,
        &automation_id,
        AutomationRunStatus::Completed,
        Duration::from_secs(180),
    )
    .await;
    assert_eq!(task.model.as_deref(), Some(MODEL));
    let runs = state
        .inner
        .durability
        .automation_runs_for_task(&automation_id, 5)
        .await
        .expect("automation runs");
    assert_eq!(runs[0].status, AutomationRunStatus::Completed);
    assert!(runs[0].thread_id.is_some());
    let summary = state
        .inner
        .framework
        .thread_summary(runs[0].thread_id.as_deref().expect("thread id"))
        .await
        .expect("session summary")
        .expect("session");
    assert_eq!(summary.provider, PROVIDER);
}
