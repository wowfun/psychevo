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

async fn wait_for_automation_status_with_timeout(
    state: &WebState,
    automation_id: &str,
    status: &str,
    timeout: Duration,
) -> AutomationTaskRecord {
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
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
        if task.last_status.as_deref() == Some("failed") {
            panic!(
                "automation failed: {}",
                task.last_error
                    .unwrap_or_else(|| "unknown error".to_string())
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("automation did not reach {status} within {timeout:?}");
}

fn live_xiaomi_token_plan_web_state() -> (tempfile::TempDir, WebState) {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&home).expect("home");
    let mut env = std::env::vars().collect::<BTreeMap<_, _>>();
    import_xiaomi_live_env(&mut env);
    env.remove("PSYCHEVO_CONFIG");
    env.insert(
        "PSYCHEVO_HOME".to_string(),
        home.to_string_lossy().to_string(),
    );
    let api_key_env = [
        "XIAOMI_TOKEN_PLAN_API_KEY",
        "XIAOMI_TOKEN_PLAN_CN_API_KEY",
        "XIAOMI_API_KEY",
    ]
    .into_iter()
    .find(|key| env.get(*key).is_some_and(|value| !value.trim().is_empty()))
    .unwrap_or("XIAOMI_TOKEN_PLAN_API_KEY");
    if let Some(api_key) = env.get(api_key_env).cloned() {
        env.insert("XIAOMI_TOKEN_PLAN_API_KEY".to_string(), api_key);
    }
    let base_url = env
        .get("XIAOMI_TOKEN_PLAN_BASE_URL")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("https://token-plan-cn.xiaomimimo.com/v1");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"
[provider.xiaomi-token-plan]
api = "{base_url}"

[provider.xiaomi-token-plan.models."mimo-v2.5-pro"]
"#
        ),
    )
    .expect("live automation config");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::new(state);
    let config = GatewayWebServerConfig::new(
        gateway,
        home,
        cwd,
        None,
        env,
        temp.path().join("static"),
    );
    (temp, WebState::new(config))
}

fn import_xiaomi_live_env(env: &mut BTreeMap<String, String>) {
    let mut candidates = Vec::new();
    if let Some(config) = env.get("PSYCHEVO_CONFIG").map(PathBuf::from)
        && let Some(parent) = config.parent()
    {
        candidates.push(parent.join(".env"));
    }
    if let Some(home) = env.get("PSYCHEVO_HOME").map(PathBuf::from).or_else(|| {
        env.get("HOME")
            .map(|home| PathBuf::from(home).join(".psychevo"))
    }) {
        candidates.push(home.join(".env"));
    }
    for path in candidates {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if ![
                "XIAOMI_TOKEN_PLAN_API_KEY",
                "XIAOMI_TOKEN_PLAN_CN_API_KEY",
                "XIAOMI_API_KEY",
                "XIAOMI_TOKEN_PLAN_BASE_URL",
            ]
            .contains(&key)
            {
                continue;
            }
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            env.entry(key.to_string()).or_insert(value);
        }
    }
}

fn live_xiaomi_token_plan_unavailable(state: &WebState) -> bool {
    let options = state.run_options(state.inner.cwd.clone(), None);
    match model_catalog_provider(&options, "xiaomi-token-plan") {
        Ok(Some(provider)) if provider.fetchable() => false,
        Ok(Some(provider)) => {
            eprintln!(
                "skipping live automation: {}",
                provider
                    .unavailable_reason
                    .or(provider.missing_credentials)
                    .unwrap_or_else(|| "xiaomi-token-plan is not fetchable".to_string())
            );
            true
        }
        Ok(None) => {
            eprintln!("skipping live automation: xiaomi-token-plan provider is unavailable");
            true
        }
        Err(err) => {
            eprintln!("skipping live automation: {err}");
            true
        }
    }
}

#[derive(Debug, Clone)]
struct AutomationFakeRun {
    runtime_source: String,
    prompt: String,
    session: Option<String>,
    runtime_tools: Vec<String>,
    mode: RunMode,
    permission_mode: Option<PermissionMode>,
    sandbox_override: Option<RunSandboxOverride>,
}

#[derive(Default)]
struct AutomationFakeBackend {
    runs: Mutex<Vec<AutomationFakeRun>>,
    dispatch_times: Mutex<Vec<std::time::Instant>>,
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
        BackendKind::Native
    }

    fn run_turn(
        &self,
        request: crate::BackendTurnRequest,
    ) -> futures::future::BoxFuture<'static, psychevo_runtime::Result<psychevo_runtime::RunResult>>
    {
        self.dispatch_times
            .lock()
            .expect("dispatch times")
            .push(std::time::Instant::now());
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
                        request.options.cwd.clone(),
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
        self.runs.lock().expect("runs").push(AutomationFakeRun {
            runtime_source: request.runtime_source.clone(),
            prompt: request.options.prompt.clone(),
            session,
            runtime_tools,
            mode: request.options.mode,
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
                    &request.options.cwd,
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
                cwd: request.options.cwd,
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
                terminal_error: None,
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
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
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
    let config = GatewayWebServerConfig::new(
        gateway,
        home,
        cwd,
        None,
        env,
        temp.path().join("static"),
    );
    let web_state = WebState::new(config);
    *backend.web_state.lock().expect("web state") = Some(web_state.clone());
    (temp, web_state)
}
