#[tokio::test]
async fn automation_manual_run_uses_auto_sandbox_and_updates_status() {
    let backend = Arc::new(AutomationFakeBackend::default());
    let (_temp, state) = web_state_with_automation_backend(backend.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let created = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "automation/write".to_string(),
            params: Some(json!({
                "target": { "kind": "project" },
                "title": "Repo check",
                "prompt": "Check the repo.",
                "schedule": { "kind": "interval", "everyMinutes": 30 }
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
            id: Some(json!(2)),
            method: "automation/run".to_string(),
            params: Some(json!({ "automationId": automation_id })),
        },
    )
    .await
    .expect("automation/run");
    assert_eq!(run["accepted"], true);
    assert!(run["run"]["id"].is_string());

    tokio::time::timeout(Duration::from_secs(2), backend.notify.notified())
        .await
        .expect("fake backend run");
    let task = wait_for_automation_status(&state, &automation_id, "completed").await;
    assert!(task.next_run_at_ms.is_some());
    let runs = state
        .inner
        .state
        .store()
        .automation_runs_for_task(&automation_id, 10)
        .expect("automation runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, "completed");
    assert!(runs[0].thread_id.is_some());

    let backend_runs = backend.runs.lock().expect("runs").clone();
    assert_eq!(backend_runs.len(), 1);
    assert_eq!(backend_runs[0].prompt, "Check the repo.");
    assert_eq!(
        backend_runs[0].permission_mode,
        Some(PermissionMode::BypassPermissions)
    );
    let sandbox = backend_runs[0]
        .sandbox_override
        .as_ref()
        .expect("sandbox override");
    assert!(sandbox.enabled);
    assert_eq!(
        sandbox.mode,
        psychevo_runtime::RunSandboxMode::WorkspaceWrite
    );

    let mut saw_result = false;
    while let Ok(message) = rx.try_recv() {
        saw_result |= message.contains("\"method\":\"turn/result\"");
    }
    assert!(saw_result);
}

#[tokio::test]
async fn automation_stale_running_run_recovers_and_preserves_history_thread() {
    let backend = Arc::new(AutomationFakeBackend::default());
    let (_temp, state) = web_state_with_automation_backend(backend.clone());
    let cwd = state.inner.cwd.to_string_lossy().to_string();
    let historical_thread = state
        .inner
        .state
        .store()
        .create_session_with_metadata(
            &state.inner.cwd,
            "automation",
            "model",
            "provider",
            None,
        )
        .expect("historical session");
    let automation_id = "stale-gateway-run";
    let source_key = "automation:stale-gateway-run";
    state
        .inner
        .state
        .store()
        .upsert_automation_task(AutomationTaskInput {
            id: Some(automation_id.to_string()),
            cwd,
            kind: "project".to_string(),
            target_thread_id: None,
            title: "Recover stale run".to_string(),
            prompt: "Recover this stale run.".to_string(),
            schedule: json!({"kind": "interval", "everyMinutes": 30}),
            enabled: true,
            execution: json!({"policy": "autoSandbox"}),
            model: None,
            reasoning_effort: None,
            source_key: Some(source_key.to_string()),
            next_run_at_ms: Some(gateway_now_ms().saturating_sub(1_000)),
        })
        .expect("automation task");
    let stale_run = state
        .inner
        .state
        .store()
        .claim_automation_run(automation_id, "scheduler")
        .expect("claim")
        .expect("running claim");
    let stale_started_at = gateway_now_ms().saturating_sub(10 * 60 * 1_000);
    let conn = rusqlite::Connection::open(state.inner.state.db_path()).expect("db");
    conn.execute(
        "UPDATE automation_runs SET started_at_ms = ?2, thread_id = ?3, source_key = ?4 WHERE id = ?1",
        rusqlite::params![stale_run.id, stale_started_at, historical_thread, source_key],
    )
    .expect("age stale run");

    let accepted = automations::run_due_automations_once(state.clone())
        .await
        .expect("scheduler pass");
    assert_eq!(accepted, 0);
    let recovered = state
        .inner
        .state
        .store()
        .automation_task(automation_id)
        .expect("automation task")
        .expect("task");
    assert_eq!(recovered.last_status.as_deref(), Some("failed"));
    assert!(
        recovered
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("stale running claim expired"))
    );
    assert!(
        recovered
            .next_run_at_ms
            .is_some_and(|next| next > gateway_now_ms())
    );

    let (tx, _rx) = mpsc::unbounded_channel();
    let manual = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("manual")),
            method: "automation/run".to_string(),
            params: Some(json!({ "automationId": automation_id })),
        },
    )
    .await
    .expect("automation/run");
    assert_eq!(manual["accepted"], true);
    tokio::time::timeout(Duration::from_secs(2), backend.notify.notified())
        .await
        .expect("manual backend run");
    wait_for_automation_status(&state, automation_id, "completed").await;

    let listed = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("list")),
            method: "automation/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("automation/list");
    let runs = listed["automations"][0]["runs"].as_array().expect("runs");
    assert!(runs.iter().any(|run| {
        run["status"].as_str() == Some("failed")
            && run["threadId"].as_str() == Some(historical_thread.as_str())
    }));
}
