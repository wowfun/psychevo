use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::Response;
use axum::response::Json;
use axum::routing::post;
use psychevo::ConfigurationQuery;
use psychevo::application::{AutomationRunStatus, AutomationTaskRecord};
use psychevo::{Error, PermissionMode, RunMode, RunSandboxOverride};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use crate::composition::GatewayApplication;
use crate::server::GatewayWebServerConfig;
use crate::server::automations;
use crate::server::binding::WebState;

pub(in crate::server::tests) async fn wait_for_automation_status(
    state: &WebState,
    automation_id: &str,
    status: AutomationRunStatus,
) -> AutomationTaskRecord {
    for _ in 0..50 {
        let task = state
            .inner
            .durability
            .automation_task(automation_id)
            .await
            .expect("automation task")
            .expect("task");
        if task.last_status == Some(status) {
            return task;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("automation did not reach {}", status.as_str());
}

pub(in crate::server::tests) async fn wait_for_automation_status_with_timeout(
    state: &WebState,
    automation_id: &str,
    status: AutomationRunStatus,
    timeout: Duration,
) -> AutomationTaskRecord {
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        let task = state
            .inner
            .durability
            .automation_task(automation_id)
            .await
            .expect("automation task")
            .expect("task");
        if task.last_status == Some(status) {
            return task;
        }
        if task.last_status == Some(AutomationRunStatus::Failed) {
            panic!(
                "automation failed: {}",
                task.last_error
                    .unwrap_or_else(|| "unknown error".to_string())
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!(
        "automation did not reach {} within {timeout:?}",
        status.as_str()
    );
}

pub(in crate::server::tests) async fn live_xiaomi_token_plan_web_state()
-> (tempfile::TempDir, WebState) {
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
    let runtime = GatewayApplication::open(home, temp.path().join("state.db"), None, env)
        .await
        .expect("test composition");
    let config = GatewayWebServerConfig::with_static(runtime, cwd, temp.path().join("static"));
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

pub(in crate::server::tests) fn live_xiaomi_token_plan_unavailable(state: &WebState) -> bool {
    let configuration = state
        .inner
        .framework
        .configuration(ConfigurationQuery::new(&state.inner.cwd));
    match configuration
        .and_then(|configuration| configuration.model_catalog_provider("xiaomi-token-plan"))
    {
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
pub(in crate::server::tests) struct AutomationTurnObservation {
    pub(in crate::server::tests) prompt: String,
    pub(in crate::server::tests) session: Option<String>,
    pub(in crate::server::tests) runtime_tools: Vec<String>,
    pub(in crate::server::tests) mode: RunMode,
    pub(in crate::server::tests) permission_mode: Option<PermissionMode>,
    pub(in crate::server::tests) sandbox_override: Option<RunSandboxOverride>,
}

const AUTOMATION_DRAFT_JSON: &str = r#"{
  "target": {"kind": "project"},
  "title": "Morning project check",
  "prompt": "Review the current repository state before standup and summarize risks that need attention.",
  "schedule": {"kind": "daily", "time": "09:00"},
  "execution": {"policy": "autoSandbox"},
  "model": null,
  "reasoningEffort": null
}"#;

#[derive(Default)]
pub(in crate::server::tests) struct AutomationTurnProbe {
    pub(in crate::server::tests) runs: Mutex<Vec<AutomationTurnObservation>>,
    pub(in crate::server::tests) dispatch_times: Mutex<Vec<std::time::Instant>>,
    pub(in crate::server::tests) model_tool_args: Mutex<Option<Value>>,
    pub(in crate::server::tests) model_tool_results: Arc<Mutex<Vec<Value>>>,
    pub(in crate::server::tests) model_tool_errors: Arc<Mutex<Vec<String>>>,
    pub(in crate::server::tests) outcomes: Mutex<VecDeque<psychevo::TurnOutcome>>,
    web_state: Mutex<Option<WebState>>,
    pub(in crate::server::tests) notify: tokio::sync::Notify,
}

impl std::fmt::Debug for AutomationTurnProbe {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AutomationTurnProbe")
    }
}

impl AutomationTurnProbe {
    pub(in crate::server::tests) fn executor(
        self: &Arc<Self>,
    ) -> crate::FrameworkNativeTestExecutor {
        let backend = Arc::clone(self);
        Arc::new(move |invocation| {
            let backend = Arc::clone(&backend);
            Box::pin(async move { backend.execute(invocation).await })
        })
    }

    async fn execute(
        &self,
        invocation: psychevo::AgentTurnInvocation,
    ) -> psychevo::Result<psychevo::TurnResult> {
        invocation.persistence.confirm_delivery().await?;
        self.dispatch_times
            .lock()
            .expect("dispatch times")
            .push(std::time::Instant::now());
        let runtime_tools = invocation
            .capabilities
            .tools
            .iter()
            .map(|tool| tool.name().to_string())
            .collect();
        let session = Some(invocation.receipt.thread_id.clone());
        let cwd = PathBuf::from(&invocation.thread.cwd);
        let model_tool_args = self.model_tool_args.lock().expect("tool args").clone();
        let model_tool_state = self.web_state.lock().expect("web state").clone();
        self.runs
            .lock()
            .expect("runs")
            .push(AutomationTurnObservation {
                prompt: invocation.input.prompt.clone(),
                session: session.clone(),
                runtime_tools,
                mode: invocation.execution.mode,
                permission_mode: invocation.execution.permission_mode,
                sandbox_override: invocation.execution.sandbox.clone(),
            });
        self.notify.notify_one();
        if let Some(args) = model_tool_args {
            let result = match model_tool_state {
                Some(state) => {
                    automations::automation_tool_execute_for_test(state, cwd, session, args).await
                }
                None => Err(Error::Message(
                    "test web state was not installed".to_string(),
                )),
            };
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
        let outcome = self
            .outcomes
            .lock()
            .expect("automation outcomes")
            .pop_front()
            .unwrap_or(psychevo::TurnOutcome::Completed);
        let terminal_error = (outcome == psychevo::TurnOutcome::Failed).then(|| {
            psychevo::application::RunTerminalError {
                code: "fake_automation_failure".to_string(),
                stage: "turn".to_string(),
                retry_class: "never".to_string(),
                message: "fake automation terminal failure".to_string(),
                diagnostic_ref: "diag-fake-automation".to_string(),
            }
        });
        Ok(psychevo::TurnResult {
            thread_id: invocation.receipt.thread_id,
            outcome,
            terminal_reason: None,
            final_answer: "automation done".to_string(),
            provider: "fake-provider".to_string(),
            model: "fake-model".to_string(),
            reasoning_effort: None,
            context_limit: None,
            tool_failures: 0,
            selected_agent: None,
            selected_skills: Vec::new(),
            context_snapshot: None,
            terminal_error,
            warnings: Vec::new(),
        })
    }
}

pub(in crate::server::tests) async fn web_state_with_automation_turn_probe(
    backend: Arc<AutomationTurnProbe>,
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
    std::fs::create_dir_all(&home).expect("home");
    let runtime = GatewayApplication::open_with_native_test_executor(
        home,
        temp.path().join("state.db"),
        None,
        env,
        backend.executor(),
    )
    .await
    .expect("test composition");
    let config = GatewayWebServerConfig::with_static(runtime, cwd, temp.path().join("static"));
    let web_state = WebState::new(config);
    *backend.web_state.lock().expect("web state") = Some(web_state.clone());
    (temp, web_state)
}

pub(in crate::server::tests) async fn web_state_with_automation_framework_provider(
    backend: Arc<AutomationTurnProbe>,
) -> (tempfile::TempDir, WebState, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind automation provider");
    let provider_addr = listener.local_addr().expect("automation provider addr");
    let provider_requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured_requests = Arc::clone(&provider_requests);
    let responses = Arc::new([
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"draft-write\",\"function\":{{\"name\":\"write\",\"arguments\":{}}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n",
            serde_json::to_string(
                &json!({"path": "draft-write.txt", "content": "must not exist"}).to_string()
            )
            .expect("tool arguments")
        ),
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n",
            serde_json::to_string(AUTOMATION_DRAFT_JSON).expect("draft response")
        ),
    ]);
    let provider = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(request): Json<Value>| {
            let requests = Arc::clone(&captured_requests);
            let responses = Arc::clone(&responses);
            async move {
                let index = {
                    let mut requests = requests.lock().expect("provider requests");
                    let index = requests.len();
                    requests.push(request);
                    index
                };
                Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from(
                        responses
                            .get(index)
                            .cloned()
                            .unwrap_or_else(|| "data: [DONE]\n\n".to_string()),
                    ))
                    .expect("provider response")
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, provider)
            .await
            .expect("automation provider");
    });

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&home).expect("home");
    let config_path = home.join("config.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"model = "mock/mock-model"

[provider.mock]
api = "http://{provider_addr}/v1"
no_auth = true

[provider.mock.models."mock-model"]
"#
        ),
    )
    .expect("automation provider config");
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
    let runtime =
        GatewayApplication::open(home, temp.path().join("state.db"), Some(config_path), env)
            .await
            .expect("test composition");
    let config = GatewayWebServerConfig::with_static(runtime, cwd, temp.path().join("static"));
    let web_state = WebState::new(config);
    *backend.web_state.lock().expect("web state") = Some(web_state.clone());
    (temp, web_state, provider_requests)
}
