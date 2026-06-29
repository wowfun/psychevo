#[tokio::test]
async fn automation_draft_returns_model_draft_without_persisting_task() {
    let backend = Arc::new(AutomationFakeBackend::default());
    let (_temp, state) = web_state_with_automation_backend(backend.clone());
    let (tx, _rx) = mpsc::unbounded_channel();

    let drafted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
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
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
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

    let backend_runs = backend.runs.lock().expect("runs").clone();
    assert_eq!(backend_runs.len(), 1);
    assert_eq!(backend_runs[0].runtime_source, "automation-draft");
    assert!(backend_runs[0].runtime_tools.is_empty());
    assert!(
        backend_runs[0]
            .prompt
            .contains("Return only one JSON object")
    );
    let sandbox = backend_runs[0]
        .sandbox_override
        .as_ref()
        .expect("draft sandbox override");
    assert!(sandbox.enabled);
    assert_eq!(sandbox.mode, psychevo_runtime::RunSandboxMode::ReadOnly);
}
