#[tokio::test]
#[ignore = "live provider opt-in"]
async fn live_xiaomi_token_plan_automation_manual_run_completes() {
    const PROVIDER: &str = "xiaomi-token-plan";
    const MODEL: &str = "xiaomi-token-plan/mimo-v2.5-pro";
    let (_temp, state) = live_xiaomi_token_plan_web_state();
    if live_xiaomi_token_plan_unavailable(&state) {
        return;
    }
    let (tx, _rx) = mpsc::unbounded_channel();
    let created = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
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
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
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
        "completed",
        Duration::from_secs(180),
    )
    .await;
    assert_eq!(task.model.as_deref(), Some(MODEL));
    let runs = state
        .inner
        .state

        .automation_runs_for_task(&automation_id, 5)
        .expect("automation runs");
    assert_eq!(runs[0].status, "completed");
    assert!(runs[0].thread_id.is_some());
    let summary = state
        .inner
        .state

        .session_summary(runs[0].thread_id.as_deref().expect("thread id"))
        .expect("session summary")
        .expect("session");
    assert_eq!(summary.provider, PROVIDER);
}
