#[tokio::test]
async fn automation_rpc_create_list_and_delete_round_trips() {
    let (_temp, state) = web_state();
    let (tx, _rx) = mpsc::unbounded_channel();

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
                "title": "Daily review",
                "prompt": "Summarize the current repository state.",
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
    assert_eq!(created["automation"]["kind"], "project");
    assert_eq!(
        created["automation"]["execution"]["policy"],
        "autoSandbox"
    );
    assert!(created["automation"]["nextRunAtMs"].is_number());
    assert_eq!(
        created["automation"]["sourceKey"].as_str(),
        Some(format!("automation:{automation_id}").as_str())
    );

    let listed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "automation/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("automation/list");
    let automations = listed["automations"].as_array().expect("automations");
    assert_eq!(automations.len(), 1);
    assert_eq!(automations[0]["id"], automation_id);

    let deleted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(3)),
            method: "automation/delete".to_string(),
            params: Some(json!({ "automationId": automation_id })),
        },
    )
    .await
    .expect("automation/delete");
    assert_eq!(deleted["deleted"], true);

    let listed = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(4)),
            method: "automation/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("automation/list empty");
    assert!(listed["automations"].as_array().expect("automations").is_empty());
}

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
    assert!(listed["automations"].as_array().expect("automations").is_empty());

    let backend_runs = backend.runs.lock().expect("runs").clone();
    assert_eq!(backend_runs.len(), 1);
    assert_eq!(backend_runs[0].runtime_source, "automation-draft");
    assert!(backend_runs[0].prompt.contains("Return only one JSON object"));
    let sandbox = backend_runs[0]
        .sandbox_override
        .as_ref()
        .expect("draft sandbox override");
    assert!(sandbox.enabled);
    assert_eq!(sandbox.mode, psychevo_runtime::RunSandboxMode::ReadOnly);
}

async fn wait_for_automation_status(
    state: &WebState,
    automation_id: &str,
    status: &str,
) -> AutomationTaskRecord {
    for _ in 0..50 {
        let task = state
            .inner
            .state
            .store()
            .automation_task(automation_id)
            .expect("automation task")
            .expect("task");
        if task.last_status.as_deref() == Some(status) {
            return task;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("automation did not reach {status}");
}

#[derive(Debug, Clone)]
struct AutomationFakeRun {
    runtime_source: String,
    prompt: String,
    permission_mode: Option<PermissionMode>,
    sandbox_override: Option<RunSandboxOverride>,
}

#[derive(Default)]
struct AutomationFakeBackend {
    runs: Mutex<Vec<AutomationFakeRun>>,
    notify: tokio::sync::Notify,
}

impl std::fmt::Debug for AutomationFakeBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AutomationFakeBackend")
    }
}

impl crate::GatewayBackend for AutomationFakeBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Psychevo
    }

    fn run_turn(
        &self,
        request: crate::BackendTurnRequest,
    ) -> futures::future::BoxFuture<'static, psychevo_runtime::Result<psychevo_runtime::RunResult>>
    {
        self.runs
            .lock()
            .expect("runs")
            .push(AutomationFakeRun {
                runtime_source: request.runtime_source.clone(),
                prompt: request.options.prompt.clone(),
                permission_mode: request.options.permission_mode,
                sandbox_override: request.options.sandbox_override.clone(),
            });
        self.notify.notify_one();
        Box::pin(async move {
            let session_id = if let Some(session_id) = request.options.session.clone() {
                request.options.state.store().resume_session(&session_id)?;
                session_id
            } else {
                request.options.state.store().create_session_with_metadata(
                    &request.options.workdir,
                    &request.runtime_source,
                    "fake-model",
                    "fake-provider",
                    None,
                )?
            };
            let final_answer = if request.runtime_source == "automation-draft" {
                r#"{
                  "target": {"kind": "project"},
                  "title": "Morning project check",
                  "prompt": "Review the current repository state before standup and summarize risks that need attention.",
                  "schedule": {"kind": "daily", "time": "09:00"},
                  "enabled": true,
                  "execution": {"policy": "autoSandbox"},
                  "model": null,
                  "reasoningEffort": null
                }"#
                .to_string()
            } else {
                "automation done".to_string()
            };
            Ok(psychevo_runtime::RunResult {
                session_id,
                outcome: psychevo_runtime::Outcome::Normal,
                terminal_reason: None,
                final_answer,
                db_path: request.options.state.db_path().to_path_buf(),
                workdir: request.options.workdir,
                provider: "fake-provider".to_string(),
                model: "fake-model".to_string(),
                base_url: String::new(),
                api_key_env: None,
                reasoning_effort: None,
                context_limit: None,
                tool_failures: 0,
                selected_agent: None,
                selected_skills: Vec::new(),
                context_snapshot: None,
                events: Vec::new(),
                warnings: Vec::new(),
            })
        })
    }
}

fn web_state_with_automation_backend(
    backend: Arc<AutomationFakeBackend>,
) -> (tempfile::TempDir, WebState) {
    let temp = tempfile::tempdir().expect("tempdir");
    let workdir = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&workdir).expect("workdir");
    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_HOME".to_string(),
            home.to_string_lossy().to_string(),
        ),
    ]);
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(state, backend);
    let config =
        GatewayWebServerConfig::new(gateway, home, workdir, None, env, temp.path().join("static"));
    (temp, WebState::new(config))
}
