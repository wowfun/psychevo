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
async fn automation_rpc_accepts_one_shot_delay_schedule() {
    let (_temp, state) = web_state();
    let (tx, _rx) = mpsc::unbounded_channel();

    let created = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "automation/write".to_string(),
            params: Some(json!({
                "target": { "kind": "project" },
                "title": "One-shot review",
                "prompt": "Review the repo once soon.",
                "schedule": { "kind": "delay", "afterMinutes": 15 }
            })),
        },
    )
    .await
    .expect("automation/write");

    assert_eq!(
        created["automation"]["schedule"],
        json!({"kind": "delay", "afterMinutes": 15})
    );
    assert!(created["automation"]["nextRunAtMs"].is_number());
}

#[tokio::test]
async fn automation_rpc_pause_resume_are_explicit_lifecycle_mutations() {
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
                "title": "Lifecycle review",
                "prompt": "Review lifecycle behavior.",
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

    let paused = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "automation/pause".to_string(),
            params: Some(json!({ "automationId": automation_id.clone() })),
        },
    )
    .await
    .expect("automation/pause");
    assert_eq!(paused["automation"]["enabled"], false);
    assert!(paused["automation"]["nextRunAtMs"].is_null());

    let updated = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(3)),
            method: "automation/write".to_string(),
            params: Some(json!({
                "automationId": automation_id.clone(),
                "target": { "kind": "project" },
                "title": "Updated lifecycle review",
                "prompt": "Review updated lifecycle behavior.",
                "schedule": { "kind": "interval", "everyMinutes": 45 }
            })),
        },
    )
    .await
    .expect("automation/write update");
    assert_eq!(updated["automation"]["title"], "Updated lifecycle review");
    assert_eq!(updated["automation"]["enabled"], false);
    assert!(updated["automation"]["nextRunAtMs"].is_null());

    let resumed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(4)),
            method: "automation/resume".to_string(),
            params: Some(json!({ "automationId": automation_id.clone() })),
        },
    )
    .await
    .expect("automation/resume");
    assert_eq!(resumed["automation"]["enabled"], true);
    assert!(resumed["automation"]["nextRunAtMs"].is_number());

    let missing = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(5)),
            method: "automation/pause".to_string(),
            params: Some(json!({ "automationId": "missing" })),
        },
    )
    .await;
    assert!(missing.is_err());
}

#[test]
fn gateway_turns_expose_model_facing_automation_tool() {
    let (_temp, state) = web_state();

    let options = state.run_options(state.inner.workdir.clone(), Some("thread-1".to_string()));

    assert!(
        options
            .runtime_tools
            .iter()
            .any(|tool| tool.name() == "automation")
    );
}

#[test]
fn automation_tool_create_defaults_to_current_thread() {
    let (_temp, state) = web_state();
    let thread_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.workdir, "web", "model", "provider", None)
        .expect("session");
    let value = automations::automation_tool_execute_for_test(
        state.clone(),
        state.inner.workdir.clone(),
        Some(thread_id.clone()),
        json!({
            "action": "create",
            "title": "Thread check",
            "prompt": "Continue this thread later.",
            "schedule": { "kind": "interval", "everyMinutes": 30 }
        }),
    )
    .expect("automation tool");

    assert_eq!(value["success"], true);
    assert_eq!(value["action"], "create");
    assert_eq!(value["automation"]["kind"], "threadHeartbeat");
    assert_eq!(value["automation"]["targetThreadId"], thread_id);
}

#[tokio::test]
async fn turn_start_first_prompt_materializes_current_thread_for_automation_tool() {
    let backend = Arc::new(AutomationFakeBackend::default());
    *backend.model_tool_args.lock().expect("tool args") = Some(json!({
        "action": "create",
        "target": "currentThread",
        "title": "First turn thread tip",
        "prompt": "Send one useful software engineering tip.",
        "schedule": { "kind": "interval", "everyMinutes": 1 }
    }));
    let (_temp, state) = web_state_with_automation_backend(backend.clone());
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    assert!(
        state
            .inner
            .gateway
            .resolve_source_thread(&state.inner.source)
            .expect("source")
            .is_none()
    );
    let (tx, mut rx) = mpsc::unbounded_channel();

    let accepted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": null,
                "text": "每分钟发一条你认为最有价值的软件工程 tip"
            })),
        },
    )
    .await
    .expect("turn/start");
    assert_eq!(accepted["accepted"], true);

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if !backend
                .model_tool_results
                .lock()
                .expect("tool results")
                .is_empty()
                || !backend
                    .model_tool_errors
                    .lock()
                    .expect("tool errors")
                    .is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("model-facing automation tool call");

    let errors = backend.model_tool_errors.lock().expect("tool errors").clone();
    assert_eq!(errors, Vec::<String>::new());
    let tool_results = backend
        .model_tool_results
        .lock()
        .expect("tool results")
        .clone();
    assert_eq!(tool_results.len(), 1);
    assert_eq!(tool_results[0]["automation"]["kind"], "threadHeartbeat");
    let target_thread_id = tool_results[0]["automation"]["targetThreadId"]
        .as_str()
        .expect("target thread")
        .to_string();

    let runs = backend.runs.lock().expect("runs").clone();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].session.as_deref(), Some(target_thread_id.as_str()));
    assert!(runs[0].runtime_tools.iter().any(|name| name == "automation"));
    assert_eq!(
        state
            .inner
            .gateway
            .resolve_source_thread(&state.inner.source)
            .expect("source binding")
            .as_deref(),
        Some(target_thread_id.as_str())
    );

    let mut saw_result = false;
    while let Ok(message) = rx.try_recv() {
        saw_result |= message.contains("\"method\":\"turn/result\"");
        assert!(!message.contains("current thread is not available"));
    }
    assert!(saw_result);
}

#[tokio::test]
async fn thread_start_remains_empty_draft_without_creating_session() {
    let backend = Arc::new(AutomationFakeBackend::default());
    let (_temp, state) = web_state_with_automation_backend(backend);
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let snapshot = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "thread/start".to_string(),
            params: Some(json!({ "scope": scope })),
        },
    )
    .await
    .expect("thread/start");

    assert!(snapshot.get("thread").is_some_and(Value::is_null));
    assert_eq!(
        state
            .inner
            .state
            .store()
            .list_sessions_for_workdir_with_sources(&state.inner.workdir, &[])
            .expect("sessions")
            .len(),
        0
    );
    assert!(
        state
            .inner
            .gateway
            .resolve_source_thread(&state.inner.source)
            .expect("source lookup")
            .is_none()
    );
}

#[tokio::test]
async fn automation_tool_manages_project_lifecycle_actions() {
    let backend = Arc::new(AutomationFakeBackend::default());
    let (_temp, state) = web_state_with_automation_backend(backend.clone());
    let workdir = state.inner.workdir.clone();

    let created = automations::automation_tool_execute_for_test(
        state.clone(),
        workdir.clone(),
        None,
        json!({
            "action": "create",
            "target": "project",
            "title": "Repo lifecycle",
            "prompt": "Check the repository.",
            "schedule": { "kind": "interval", "everyMinutes": 30 }
        }),
    )
    .expect("create");
    let automation_id = created["automation"]["id"]
        .as_str()
        .expect("automation id")
        .to_string();
    assert_eq!(created["action"], "create");
    assert_eq!(created["automation"]["kind"], "project");

    let listed = automations::automation_tool_execute_for_test(
        state.clone(),
        workdir.clone(),
        None,
        json!({ "action": "list" }),
    )
    .expect("list");
    assert_eq!(listed["action"], "list");
    assert_eq!(listed["automations"][0]["id"], automation_id);

    let updated = automations::automation_tool_execute_for_test(
        state.clone(),
        workdir.clone(),
        None,
        json!({
            "action": "update",
            "automationId": automation_id,
            "title": "Updated lifecycle"
        }),
    )
    .expect("update");
    assert_eq!(updated["action"], "update");
    assert_eq!(updated["automation"]["title"], "Updated lifecycle");

    let paused = automations::automation_tool_execute_for_test(
        state.clone(),
        workdir.clone(),
        None,
        json!({ "action": "pause", "automationId": automation_id }),
    )
    .expect("pause");
    assert_eq!(paused["action"], "pause");
    assert_eq!(paused["automation"]["enabled"], false);
    assert!(paused["automation"]["nextRunAtMs"].is_null());

    let resumed = automations::automation_tool_execute_for_test(
        state.clone(),
        workdir.clone(),
        None,
        json!({ "action": "resume", "automationId": automation_id }),
    )
    .expect("resume");
    assert_eq!(resumed["action"], "resume");
    assert_eq!(resumed["automation"]["enabled"], true);
    assert!(resumed["automation"]["nextRunAtMs"].is_number());

    let run = automations::automation_tool_execute_for_test(
        state.clone(),
        workdir.clone(),
        None,
        json!({ "action": "run", "automationId": automation_id }),
    )
    .expect("run");
    assert_eq!(run["action"], "run");
    assert_eq!(run["accepted"], true);

    tokio::time::timeout(Duration::from_secs(2), backend.notify.notified())
        .await
        .expect("fake backend run");
    let backend_runs = backend.runs.lock().expect("runs").clone();
    assert_eq!(backend_runs.len(), 1);
    assert!(backend_runs[0].runtime_tools.is_empty());

    let removed = automations::automation_tool_execute_for_test(
        state.clone(),
        workdir.clone(),
        None,
        json!({ "action": "remove", "automationId": automation_id }),
    )
    .expect("remove");
    assert_eq!(removed["action"], "remove");
    assert_eq!(removed["deleted"], true);

    let listed = automations::automation_tool_execute_for_test(
        state,
        workdir,
        None,
        json!({ "action": "list" }),
    )
    .expect("list empty");
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
    assert!(backend_runs[0].runtime_tools.is_empty());
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
    session: Option<String>,
    runtime_tools: Vec<String>,
    permission_mode: Option<PermissionMode>,
    sandbox_override: Option<RunSandboxOverride>,
}

#[derive(Default)]
struct AutomationFakeBackend {
    runs: Mutex<Vec<AutomationFakeRun>>,
    model_tool_args: Mutex<Option<Value>>,
    model_tool_results: Mutex<Vec<Value>>,
    model_tool_errors: Mutex<Vec<String>>,
    web_state: Mutex<Option<WebState>>,
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
        let runtime_tools = request
            .options
            .runtime_tools
            .iter()
            .map(|tool| tool.name().to_string())
            .collect();
        let session = request.options.session.clone();
        if let Some(args) = self.model_tool_args.lock().expect("tool args").clone() {
            let result = self
                .web_state
                .lock()
                .expect("web state")
                .clone()
                .ok_or_else(|| Error::Message("test web state was not installed".to_string()))
                .and_then(|state| {
                    automations::automation_tool_execute_for_test(
                        state,
                        request.options.workdir.clone(),
                        session.clone(),
                        args,
                    )
                });
            match result {
                Ok(value) => self
                    .model_tool_results
                    .lock()
                    .expect("tool results")
                    .push(value),
                Err(err) => self
                    .model_tool_errors
                    .lock()
                    .expect("tool errors")
                    .push(err.to_string()),
            }
        }
        self.runs
            .lock()
            .expect("runs")
            .push(AutomationFakeRun {
                runtime_source: request.runtime_source.clone(),
                prompt: request.options.prompt.clone(),
                session,
                runtime_tools,
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
    let gateway = Gateway::with_backend(state, backend.clone());
    let config =
        GatewayWebServerConfig::new(gateway, home, workdir, None, env, temp.path().join("static"));
    let web_state = WebState::new(config);
    *backend.web_state.lock().expect("web state") = Some(web_state.clone());
    (temp, web_state)
}
