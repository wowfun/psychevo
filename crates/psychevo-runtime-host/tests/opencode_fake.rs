#![cfg(unix)]

use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use futures::stream::{self, StreamExt};
use psychevo_runtime_host::{
    ControlState, ExecuteRequest, ExecuteResult, HistoryFidelity, OpenCodeRuntimeModule,
    ReadinessStatus, RuntimeAuthOperation, RuntimeAuthRequest, RuntimeControl,
    RuntimeControlSetRequest, RuntimeErrorStage, RuntimeIntent, RuntimeInteractionExposure,
    RuntimeKind, RuntimeModule, RuntimeObservation, RuntimeObserver, RuntimePlanStepStatus,
    RuntimeProfile, RuntimeSessionOperation, RuntimeSessionRequest, RuntimeStability,
    RuntimeTurnOutcome, RuntimeTurnRequest, SessionOwnership, ShutdownMode, SnapshotMode,
    SnapshotQuery, SnapshotScope,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, oneshot};

#[derive(Debug, Clone, Copy)]
enum TurnBehavior {
    Complete,
    DeferFirst,
    CloseStream,
    ExitProcess,
    WrongAuth,
}

#[tokio::test]
async fn stable_opencode_contract_rejects_unobserved_control_and_auth_mutation_with_guidance() {
    let fake = FakeRuntime::start(TurnBehavior::Complete).await;
    let module = OpenCodeRuntimeModule::new();
    let auth = module
        .execute(
            ExecuteRequest {
                profile: fake.profile.clone(),
                expected_profile_revision: fake.profile.revision,
                expected_capability_revision: None,
                expected_binding_revision: None,
                intent: RuntimeIntent::Auth(RuntimeAuthRequest {
                    operation: RuntimeAuthOperation::Status { refresh: false },
                    cwd: fake.state.cwd.clone(),
                }),
            },
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("OpenCode auth is CLI-owned");
    assert_eq!(auth.code, "opencode_auth_cli_required");
    assert!(auth.message.contains("opencode auth login"));

    let control = module
        .execute(
            ExecuteRequest {
                profile: fake.profile.clone(),
                expected_profile_revision: fake.profile.revision,
                expected_capability_revision: Some(fake.profile.revision),
                expected_binding_revision: Some(1),
                intent: RuntimeIntent::Control(RuntimeControlSetRequest {
                    thread_id: "thread-1".to_string(),
                    native_session_id: "ses_root".to_string(),
                    cwd: fake.state.cwd.clone(),
                    control_id: "agent".to_string(),
                    value: json!("plan"),
                }),
            },
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("OpenCode session state is not a stable control mutation surface");
    assert_eq!(control.code, "opencode_control_mutation_unsupported");
    assert!(control.message.contains("before the next prompt"));
    fake.stop(&module).await;
}

#[derive(Debug)]
struct FakeState {
    cwd: PathBuf,
    password_file: PathBuf,
    exit_file: PathBuf,
    events: broadcast::Sender<String>,
    messages: Mutex<Vec<Value>>,
    prompt_count: AtomicUsize,
    permission_replies: AtomicUsize,
    question_replies: AtomicUsize,
    aborts: AtomicUsize,
    directories: Mutex<Vec<String>>,
    timeline_calls: Mutex<Vec<String>>,
    systems: Mutex<Vec<Option<String>>>,
    behavior: TurnBehavior,
    second_turn: AtomicBool,
    version: String,
}

struct FakeRuntime {
    _temp: TempDir,
    state: Arc<FakeState>,
    profile: RuntimeProfile,
    args_file: PathBuf,
    auth_file: PathBuf,
    spawn_file: PathBuf,
    stop: Option<oneshot::Sender<()>>,
}

impl FakeRuntime {
    async fn start(behavior: TurnBehavior) -> Self {
        Self::start_with_version(behavior, "1.17.17-fixture").await
    }

    async fn start_with_version(behavior: TurnBehavior, version: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("workspace ü");
        fs::create_dir_all(&cwd).expect("workspace");
        let auth_file = temp.path().join("auth.txt");
        let args_file = temp.path().join("args.txt");
        let spawn_file = temp.path().join("spawns.txt");
        let exit_file = temp.path().join("exit.txt");
        let script = temp.path().join("fake-opencode");
        fs::write(
            &script,
            r#"#!/bin/sh
printf 'spawn\n' >> "$FAKE_SPAWN_FILE"
if [ "$FAKE_WRONG_AUTH" = "1" ]; then
  printf 'wrong-password' > "$FAKE_AUTH_FILE"
else
  printf '%s' "$OPENCODE_SERVER_PASSWORD" > "$FAKE_AUTH_FILE"
fi
printf '%s\n' "$@" > "$FAKE_ARGS_FILE"
printf 'opencode server listening on http://127.0.0.1:%s\n' "$FAKE_PORT"
while [ ! -f "$FAKE_EXIT_FILE" ]; do sleep 0.05; done
exit 23
"#,
        )
        .expect("script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).expect("chmod");

        let (events, _) = broadcast::channel(128);
        let state = Arc::new(FakeState {
            cwd: cwd.clone(),
            password_file: auth_file.clone(),
            exit_file: exit_file.clone(),
            events,
            messages: Mutex::new(Vec::new()),
            prompt_count: AtomicUsize::new(0),
            permission_replies: AtomicUsize::new(0),
            question_replies: AtomicUsize::new(0),
            aborts: AtomicUsize::new(0),
            directories: Mutex::new(Vec::new()),
            timeline_calls: Mutex::new(Vec::new()),
            systems: Mutex::new(Vec::new()),
            behavior,
            second_turn: AtomicBool::new(false),
            version: version.to_string(),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("address").port();
        let (stop_tx, stop_rx) = oneshot::channel();
        let app = fake_router(state.clone());
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                })
                .await
                .expect("fake server");
        });

        let profile = RuntimeProfile {
            id: "opencode".to_string(),
            label: "OpenCode".to_string(),
            kind: RuntimeKind::OpenCode,
            enabled: true,
            command: Some(script.display().to_string()),
            args: vec!["serve".to_string()],
            env: BTreeMap::from([
                ("FAKE_PORT".to_string(), port.to_string()),
                (
                    "FAKE_AUTH_FILE".to_string(),
                    auth_file.display().to_string(),
                ),
                (
                    "FAKE_ARGS_FILE".to_string(),
                    args_file.display().to_string(),
                ),
                (
                    "FAKE_SPAWN_FILE".to_string(),
                    spawn_file.display().to_string(),
                ),
                (
                    "FAKE_EXIT_FILE".to_string(),
                    exit_file.display().to_string(),
                ),
                (
                    "FAKE_WRONG_AUTH".to_string(),
                    if matches!(behavior, TurnBehavior::WrongAuth) {
                        "1".to_string()
                    } else {
                        "0".to_string()
                    },
                ),
            ]),
            backend_ref: None,
            default_model: Some("fake/model".to_string()),
            default_mode: Some("build".to_string()),
            default_agent: Some("build".to_string()),
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
            revision: 7,
            fingerprint: "fake-profile".to_string(),
        };
        Self {
            _temp: temp,
            state,
            profile,
            args_file,
            auth_file,
            spawn_file,
            stop: Some(stop_tx),
        }
    }

    fn turn(&self, id: &str, native_session_id: Option<String>) -> ExecuteRequest {
        ExecuteRequest {
            profile: self.profile.clone(),
            expected_profile_revision: self.profile.revision,
            expected_capability_revision: Some(self.profile.revision),
            expected_binding_revision: Some(1),
            intent: RuntimeIntent::Turn(RuntimeTurnRequest {
                turn_id: id.to_string(),
                thread_id: "thread-1".to_string(),
                native_session_id,
                cwd: self.state.cwd.clone(),
                prompt: "hello".to_string(),
                instructions: None,
                model: None,
                mode: None,
                agent: None,
                features: BTreeMap::new(),
                interaction_exposure: RuntimeInteractionExposure::Standard,
                binding_epoch: 1,
            }),
        }
    }

    fn session(
        &self,
        operation: RuntimeSessionOperation,
        native_session_id: Option<String>,
        argument: Option<Value>,
    ) -> ExecuteRequest {
        ExecuteRequest {
            profile: self.profile.clone(),
            expected_profile_revision: self.profile.revision,
            expected_capability_revision: None,
            expected_binding_revision: None,
            intent: RuntimeIntent::Session(RuntimeSessionRequest {
                operation,
                thread_id: Some("thread-1".to_string()),
                native_session_id,
                cwd: self.state.cwd.clone(),
                cursor: None,
                argument,
            }),
        }
    }

    async fn stop(mut self, module: &OpenCodeRuntimeModule) {
        module
            .shutdown(ShutdownMode::Force)
            .await
            .expect("shutdown adapter");
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

fn fake_router(state: Arc<FakeState>) -> Router {
    Router::new()
        .route("/global/health", get(global_health))
        .route("/global/event", get(global_event))
        .route("/session", get(session_list).post(session_create))
        .route("/session/status", get(session_status))
        .route(
            "/session/{session_id}",
            get(session_get).delete(session_delete).patch(session_patch),
        )
        .route("/session/{session_id}/message", get(session_messages))
        .route(
            "/session/{session_id}/message/{message_id}",
            get(session_message),
        )
        .route("/session/{session_id}/children", get(session_children))
        .route("/session/{session_id}/todo", get(session_todo))
        .route("/session/{session_id}/diff", get(session_diff))
        .route("/session/{session_id}/prompt_async", post(prompt_async))
        .route("/session/{session_id}/abort", post(session_abort))
        .route("/session/{session_id}/fork", post(session_fork))
        .route("/session/{session_id}/revert", post(session_revert))
        .route("/session/{session_id}/unrevert", post(session_revert))
        .route("/permission", get(permission_list))
        .route("/permission/{request_id}/reply", post(permission_reply))
        .route("/question", get(question_list))
        .route("/question/{request_id}/reply", post(question_reply))
        .route("/question/{request_id}/reject", post(question_reply))
        .route("/agent", get(agent_list))
        .route("/mcp", get(mcp_status))
        .with_state(state)
}

async fn global_health(State(state): State<Arc<FakeState>>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!({ "healthy": true, "version": state.version.as_str() })).into_response()
}

async fn global_event(State(state): State<Arc<FakeState>>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let connected = json!({
        "payload": { "id": "evt_connected", "type": "server.connected", "properties": {} }
    })
    .to_string();
    let first = stream::once(async move { Ok::<_, Infallible>(Event::default().data(connected)) });
    let receiver = state.events.subscribe();
    let live = stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(item) if item == "__CLOSE__" => return None,
                Ok(item) => {
                    return Some((Ok::<_, Infallible>(Event::default().data(item)), receiver));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(first.chain(live)).into_response()
}

async fn session_list(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(json!([
        session_value("ses_root", None, &state.cwd),
        session_value("ses_child", Some("ses_root"), &state.cwd),
    ]))
    .into_response()
}

async fn session_create(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(session_value("ses_root", None, &state.cwd)).into_response()
}

async fn session_get(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    AxumPath(session_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    let parent = (session_id == "ses_child").then_some("ses_root");
    Json(session_value(&session_id, parent, &state.cwd)).into_response()
}

async fn session_status(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(json!({})).into_response()
}

async fn session_messages(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    AxumPath(session_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    let mut messages = vec![json!({
        "info": {
            "id": format!("msg_history_{suffix}", suffix = session_id.trim_start_matches("ses_")),
            "sessionID": session_id,
            "role": "user",
            "time": { "created": 0 },
        },
        "parts": [{ "type": "text", "text": "history" }],
    })];
    messages.extend(
        state
            .messages
            .lock()
            .expect("messages")
            .iter()
            .filter(|message| message["info"]["sessionID"] == session_id)
            .cloned(),
    );
    Json(messages).into_response()
}

async fn session_message(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    AxumPath((_session_id, message_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    state
        .messages
        .lock()
        .expect("messages")
        .iter()
        .find(|message| message["info"]["id"] == message_id)
        .cloned()
        .map(Json)
        .map(IntoResponse::into_response)
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

async fn session_children(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(json!([session_value(
        "ses_child",
        Some("ses_root"),
        &state.cwd
    )]))
    .into_response()
}

async fn session_todo(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    AxumPath(session_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    state
        .timeline_calls
        .lock()
        .expect("timeline calls")
        .push(format!("todo:{session_id}"));
    let todos = match session_id.as_str() {
        "ses_root" => json!([
            {
                "content": "Hydrated root todo",
                "status": "pending",
                "priority": "high"
            },
            {
                "content": "Hydrated cancelled todo",
                "status": "cancelled",
                "priority": "low"
            }
        ]),
        "ses_child" => json!([{
            "content": "CHILD_TIMELINE_MUST_NOT_PROJECT",
            "status": "pending",
            "priority": "medium"
        }]),
        _ => json!([]),
    };
    Json(todos).into_response()
}

async fn session_diff(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    AxumPath(session_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    state
        .timeline_calls
        .lock()
        .expect("timeline calls")
        .push(format!(
            "diff:{session_id}:{}",
            query.get("messageID").map(String::as_str).unwrap_or("none")
        ));
    let diff = match session_id.as_str() {
        "ses_root" => json!([
            {
                "file": "src/hydrated-root.rs",
                "patch": "--- a/src/hydrated-root.rs\n+++ b/src/hydrated-root.rs\n@@ -0,0 +1 @@\n+hydrated-root",
                "additions": 1,
                "deletions": 0,
                "status": "added"
            },
            {
                "file": "docs/summary.md",
                "additions": 2,
                "deletions": 1,
                "status": "modified"
            }
        ]),
        "ses_child" => json!([{
            "file": "src/child.rs",
            "patch": "CHILD_TIMELINE_MUST_NOT_PROJECT",
            "additions": 1,
            "deletions": 0,
            "status": "added"
        }]),
        _ => json!([]),
    };
    Json(diff).into_response()
}

async fn prompt_async(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    AxumPath(session_id): AxumPath<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    state
        .timeline_calls
        .lock()
        .expect("timeline calls")
        .push(format!("prompt:{session_id}"));
    let message_id = body["messageID"].as_str().expect("message id").to_string();
    state.systems.lock().expect("systems").push(
        body.get("system")
            .and_then(Value::as_str)
            .map(str::to_string),
    );
    let count = state.prompt_count.fetch_add(1, Ordering::SeqCst) + 1;
    state.messages.lock().expect("messages").push(json!({
        "info": {
            "id": message_id,
            "sessionID": session_id,
            "role": "user",
            "time": { "created": 1 },
        },
        "parts": body["parts"],
    }));
    match state.behavior {
        TurnBehavior::DeferFirst if count == 1 => {}
        TurnBehavior::CloseStream => {
            let _ = state.events.send("__CLOSE__".to_string());
        }
        TurnBehavior::ExitProcess => {
            fs::write(&state.exit_file, "exit").expect("request fake process exit");
        }
        TurnBehavior::WrongAuth => unreachable!("authentication fails before prompt"),
        TurnBehavior::Complete | TurnBehavior::DeferFirst => {
            if count > 1 && !state.second_turn.swap(true, Ordering::SeqCst) {
                emit_idle(&state, &session_id, "evt_stale_idle");
            }
            emit_completion(&state, &session_id, &message_id, count);
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn session_abort(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    state.aborts.fetch_add(1, Ordering::SeqCst);
    Json(true).into_response()
}

async fn session_fork(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(session_value("ses_fork", None, &state.cwd)).into_response()
}

async fn session_revert(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(session_value("ses_root", None, &state.cwd)).into_response()
}

async fn session_patch(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    AxumPath(session_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(session_value(&session_id, None, &state.cwd)).into_response()
}

async fn session_delete(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(true).into_response()
}

async fn permission_list(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(json!([
        {
            "id": "per_child",
            "sessionID": "ses_child",
            "permission": "bash",
            "patterns": ["git status"],
            "always": ["git status"],
            "metadata": {},
        },
        {
            "id": "per_once",
            "sessionID": "ses_child",
            "permission": "read",
            "patterns": ["README.md"],
            "always": [],
            "metadata": {},
        }
    ]))
    .into_response()
}

async fn permission_reply(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    state.permission_replies.fetch_add(1, Ordering::SeqCst);
    Json(true).into_response()
}

async fn question_list(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(json!([{
        "id": "que_child",
        "sessionID": "ses_child",
        "questions": [
            {
                "header": "Area",
                "question": "Which area?",
                "options": [
                    { "label": "CLI", "description": "Use CLI" },
                    { "label": "GUI", "description": "Use Workbench" }
                ],
                "multiple": false,
                "custom": true,
            },
            {
                "header": "Checks",
                "question": "Which checks?",
                "options": [
                    { "label": "Tests", "description": "Run focused tests" },
                    { "label": "Clippy", "description": "Run lint checks" }
                ],
                "multiple": true,
                "custom": false,
            }
        ],
    }]))
    .into_response()
}

async fn question_reply(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    state.question_replies.fetch_add(1, Ordering::SeqCst);
    Json(true).into_response()
}

async fn agent_list(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(json!([
        { "name": "build", "mode": "primary", "hidden": false },
        { "name": "secret", "mode": "primary", "hidden": true },
        { "name": "explore", "mode": "subagent", "hidden": false },
    ]))
    .into_response()
}

async fn mcp_status(
    State(state): State<Arc<FakeState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Some(error) = authorize_directory(&state, &headers, &query) {
        return error;
    }
    Json(json!({})).into_response()
}

fn authorize_directory(
    state: &FakeState,
    headers: &HeaderMap,
    query: &HashMap<String, String>,
) -> Option<Response> {
    if !authorized(state, headers) {
        return Some(StatusCode::UNAUTHORIZED.into_response());
    }
    let Some(directory) = query.get("directory") else {
        return Some(StatusCode::BAD_REQUEST.into_response());
    };
    state
        .directories
        .lock()
        .expect("directories")
        .push(directory.clone());
    (Path::new(directory) != state.cwd).then(|| StatusCode::BAD_REQUEST.into_response())
}

fn authorized(state: &FakeState, headers: &HeaderMap) -> bool {
    let Ok(password) = fs::read_to_string(&state.password_file) else {
        return false;
    };
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"))
    );
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        == Some(expected.as_str())
}

fn session_value(id: &str, parent_id: Option<&str>, cwd: &Path) -> Value {
    json!({
        "id": id,
        "parentID": parent_id,
        "title": id,
        "directory": cwd,
        "time": { "created": 1, "updated": 2 },
        "agent": "build",
        "model": { "id": "model", "providerID": "fake" },
    })
}

fn emit_completion(state: &FakeState, session_id: &str, user_message_id: &str, turn: usize) {
    emit_timeline_updates(state, session_id, turn);
    let assistant_id = format!("msg_assistant_{turn}");
    let text = format!("hello from fake {turn}");
    let info = json!({
        "id": assistant_id,
        "sessionID": session_id,
        "role": "assistant",
        "parentID": user_message_id,
        "providerID": "fake",
        "modelID": "model",
        "time": { "created": 2, "completed": 3 },
    });
    state.messages.lock().expect("messages").push(json!({
        "info": info,
        "parts": [{ "id": format!("prt_{turn}"), "type": "text", "text": text }],
    }));
    let direct = json!({
        "directory": state.cwd,
        "payload": {
            "id": format!("evt_assistant_{turn}"),
            "type": "message.updated",
            "properties": { "sessionID": session_id, "info": info },
        }
    });
    let _ = state.events.send(direct.to_string());
    let delta = json!({
        "directory": state.cwd,
        "payload": {
            "id": format!("evt_delta_{turn}"),
            "type": "message.part.delta",
            "properties": {
                "sessionID": session_id,
                "messageID": assistant_id,
                "partID": format!("prt_{turn}"),
                "field": "text",
                "delta": text,
            },
        }
    });
    let _ = state.events.send(delta.to_string());
    let duplicate = json!({
        "directory": state.cwd,
        "payload": {
            "type": "sync",
            "syncEvent": {
                "id": format!("evt_delta_{turn}"),
                "type": "message.part.delta.1",
                "seq": turn,
                "aggregateID": session_id,
                "data": delta["payload"]["properties"],
            },
        }
    });
    let _ = state.events.send(duplicate.to_string());
    emit_idle(state, session_id, &format!("evt_idle_{turn}"));
}

fn emit_timeline_updates(state: &FakeState, session_id: &str, turn: usize) {
    let other_directory = state.cwd.with_file_name("other-workspace");
    let updates = [
        (
            other_directory.clone(),
            "evt_todo_wrong_directory",
            "todo.updated",
            json!({
                "sessionID": session_id,
                "todos": [{
                    "content": "WRONG_DIRECTORY_MUST_NOT_PROJECT",
                    "status": "pending",
                    "priority": "high"
                }]
            }),
        ),
        (
            state.cwd.clone(),
            "evt_todo_foreign_session",
            "todo.updated",
            json!({
                "sessionID": "ses_foreign",
                "todos": [{
                    "content": "FOREIGN_SESSION_MUST_NOT_PROJECT",
                    "status": "pending",
                    "priority": "high"
                }]
            }),
        ),
        (
            state.cwd.clone(),
            "evt_todo_child",
            "todo.updated",
            json!({
                "sessionID": "ses_child",
                "todos": [{
                    "content": "CHILD_TIMELINE_MUST_NOT_PROJECT",
                    "status": "in_progress",
                    "priority": "medium"
                }]
            }),
        ),
        (
            state.cwd.clone(),
            "evt_todo_root",
            "todo.updated",
            json!({
                "sessionID": session_id,
                "todos": [{
                    "content": "SSE root todo",
                    "status": "in_progress",
                    "priority": "high"
                }]
            }),
        ),
        (
            other_directory,
            "evt_diff_wrong_directory",
            "session.diff",
            json!({
                "sessionID": session_id,
                "diff": [{
                    "patch": "WRONG_DIRECTORY_MUST_NOT_PROJECT",
                    "additions": 1,
                    "deletions": 0
                }]
            }),
        ),
        (
            state.cwd.clone(),
            "evt_diff_foreign_session",
            "session.diff",
            json!({
                "sessionID": "ses_foreign",
                "diff": [{
                    "patch": "FOREIGN_SESSION_MUST_NOT_PROJECT",
                    "additions": 1,
                    "deletions": 0
                }]
            }),
        ),
        (
            state.cwd.clone(),
            "evt_diff_child",
            "session.diff",
            json!({
                "sessionID": "ses_child",
                "diff": [{
                    "patch": "CHILD_TIMELINE_MUST_NOT_PROJECT",
                    "additions": 1,
                    "deletions": 0
                }]
            }),
        ),
        (
            state.cwd.clone(),
            "evt_diff_root",
            "session.diff",
            json!({
                "sessionID": session_id,
                "diff": [{
                    "file": "src/sse-root.rs",
                    "patch": "--- a/src/sse-root.rs\n+++ b/src/sse-root.rs\n@@ -0,0 +1 @@\n+sse-root",
                    "additions": 1,
                    "deletions": 0,
                    "status": "added"
                }]
            }),
        ),
    ];
    for (directory, id, event_type, properties) in updates {
        let _ = state.events.send(
            json!({
                "directory": directory,
                "payload": {
                    "id": id,
                    "type": event_type,
                    "properties": properties,
                }
            })
            .to_string(),
        );
    }
    for (id, event_type, data) in [
        (
            "evt_todo_root",
            "todo.updated.1",
            json!({
                "sessionID": session_id,
                "todos": [{
                    "content": "SSE root todo",
                    "status": "in_progress",
                    "priority": "high"
                }]
            }),
        ),
        (
            "evt_diff_root",
            "session.diff.1",
            json!({
                "sessionID": session_id,
                "diff": [{
                    "file": "src/sse-root.rs",
                    "patch": "--- a/src/sse-root.rs\n+++ b/src/sse-root.rs\n@@ -0,0 +1 @@\n+sse-root",
                    "additions": 1,
                    "deletions": 0,
                    "status": "added"
                }]
            }),
        ),
    ] {
        let _ = state.events.send(
            json!({
                "directory": state.cwd,
                "payload": {
                    "type": "sync",
                    "syncEvent": {
                        "id": id,
                        "type": event_type,
                        "seq": turn,
                        "aggregateID": session_id,
                        "data": data,
                    }
                }
            })
            .to_string(),
        );
    }
}

fn emit_idle(state: &FakeState, session_id: &str, id: &str) {
    let _ = state.events.send(
        json!({
            "directory": state.cwd,
            "payload": {
                "id": id,
                "type": "session.status",
                "properties": { "sessionID": session_id, "status": { "type": "idle" } },
            }
        })
        .to_string(),
    );
}

async fn wait_for_count(counter: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while counter.load(Ordering::SeqCst) < expected {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("counter timeout");
}

#[tokio::test]
async fn unsupported_profile_policy_is_never_reported_ready() {
    let fake = FakeRuntime::start(TurnBehavior::Complete).await;
    let module = OpenCodeRuntimeModule::new();
    let mut profile = fake.profile.clone();
    profile.sandbox = Some("workspace-write".to_string());
    let snapshot = module
        .snapshot(SnapshotQuery {
            profile,
            scope: SnapshotScope::Workspace {
                cwd: fake.state.cwd.clone(),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("cached snapshot");

    assert_eq!(
        snapshot
            .readiness
            .iter()
            .find(|stage| stage.id == "policy")
            .map(|stage| stage.status),
        Some(ReadinessStatus::Unsupported)
    );
    assert!(
        !fake.auth_file.exists(),
        "cached policy check must not spawn"
    );
    fake.stop(&module).await;
}

#[tokio::test]
async fn legacy_opencode_version_cannot_promote_the_complete_stable_matrix() {
    let fake = FakeRuntime::start_with_version(TurnBehavior::Complete, "1.16.9").await;
    let module = OpenCodeRuntimeModule::new();
    let result = module
        .execute(
            fake.turn("legacy-turn", None),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("legacy basic turn");
    assert!(
        matches!(result, ExecuteResult::Turn(turn) if turn.outcome == RuntimeTurnOutcome::Completed)
    );
    let snapshot = module
        .snapshot(SnapshotQuery {
            profile: fake.profile.clone(),
            scope: SnapshotScope::Workspace {
                cwd: fake.state.cwd.clone(),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("legacy snapshot");
    for stage_id in ["version", "capabilities"] {
        assert_eq!(
            snapshot
                .readiness
                .iter()
                .find(|stage| stage.id == stage_id)
                .map(|stage| stage.status),
            Some(ReadinessStatus::Unsupported),
            "unsupported stage {stage_id}"
        );
    }
    assert!(
        snapshot
            .capabilities
            .iter()
            .all(|capability| !capability.enabled)
    );
    fake.stop(&module).await;
}

#[tokio::test]
async fn snapshots_are_isolated_by_exact_profile_generation_and_workspace() {
    let old = FakeRuntime::start_with_version(TurnBehavior::Complete, "1.16.9").await;
    let mut current = FakeRuntime::start_with_version(TurnBehavior::Complete, "1.17.17").await;
    current.profile.revision = 8;
    current.profile.fingerprint = "current-profile".to_string();
    let module = OpenCodeRuntimeModule::new();
    module
        .execute(
            old.turn("old-turn", None),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("old turn");
    module
        .execute(
            current.turn("current-turn", None),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("current turn");

    let old_snapshot = module
        .snapshot(SnapshotQuery {
            profile: old.profile.clone(),
            scope: SnapshotScope::Workspace {
                cwd: old.state.cwd.clone(),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("old snapshot");
    let current_snapshot = module
        .snapshot(SnapshotQuery {
            profile: current.profile.clone(),
            scope: SnapshotScope::Workspace {
                cwd: current.state.cwd.clone(),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("current snapshot");
    assert_eq!(
        old_snapshot
            .readiness
            .iter()
            .find(|stage| stage.id == "version")
            .map(|stage| stage.status),
        Some(ReadinessStatus::Unsupported)
    );
    assert_eq!(
        current_snapshot
            .readiness
            .iter()
            .find(|stage| stage.id == "version")
            .map(|stage| stage.status),
        Some(ReadinessStatus::Ready)
    );
    assert_eq!(old_snapshot.runtime_version.as_deref(), Some("1.16.9"));
    assert_eq!(current_snapshot.runtime_version.as_deref(), Some("1.17.17"));
    old.stop(&module).await;
    current.stop(&module).await;
}

#[tokio::test]
async fn bounded_probes_for_two_workspaces_share_one_profile_generation_spawn() {
    let fake = FakeRuntime::start(TurnBehavior::Complete).await;
    let other_cwd = fake.state.cwd.with_file_name("other workspace");
    fs::create_dir_all(&other_cwd).expect("second workspace");
    let module = OpenCodeRuntimeModule::new();

    let first = module
        .snapshot(SnapshotQuery {
            profile: fake.profile.clone(),
            scope: SnapshotScope::Workspace {
                cwd: fake.state.cwd.clone(),
            },
            mode: SnapshotMode::BoundedProbe,
        })
        .await
        .expect("first workspace probe");
    let second = module
        .snapshot(SnapshotQuery {
            profile: fake.profile.clone(),
            scope: SnapshotScope::Workspace { cwd: other_cwd },
            mode: SnapshotMode::BoundedProbe,
        })
        .await
        .expect("second workspace probe");

    assert_eq!(first.process_epoch, second.process_epoch);
    let spawns = fs::read_to_string(&fake.spawn_file).expect("spawn log");
    assert_eq!(
        spawns.lines().collect::<Vec<_>>(),
        ["spawn"],
        "one profile generation should serve both workspace directories"
    );
    fake.stop(&module).await;
}

#[tokio::test]
async fn secure_spawn_reconcile_interactions_and_session_actions() {
    let fake = FakeRuntime::start(TurnBehavior::Complete).await;
    let module = OpenCodeRuntimeModule::new();

    let snapshot = module
        .snapshot(SnapshotQuery {
            profile: fake.profile.clone(),
            scope: SnapshotScope::Workspace {
                cwd: fake.state.cwd.clone(),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("cached snapshot");
    assert!(snapshot.process_epoch.is_none());
    assert!(snapshot.controls.is_empty());
    assert!(!fake.auth_file.exists(), "snapshot must not spawn OpenCode");

    let probed = module
        .snapshot(SnapshotQuery {
            profile: fake.profile.clone(),
            scope: SnapshotScope::Workspace {
                cwd: fake.state.cwd.clone(),
            },
            mode: SnapshotMode::BoundedProbe,
        })
        .await
        .expect("bounded probe");
    assert!(probed.process_epoch.is_some());
    assert!(fake.auth_file.exists(), "probe starts the local adapter");
    assert_eq!(fake.state.prompt_count.load(Ordering::SeqCst), 0);
    assert!(probed.controls.is_empty());
    assert_eq!(
        probed
            .readiness
            .iter()
            .find(|stage| stage.id == "capabilities")
            .map(|stage| stage.status),
        Some(ReadinessStatus::Unchecked)
    );
    assert!(
        fake.state
            .directories
            .lock()
            .expect("directories")
            .is_empty(),
        "probe must not hydrate a directory or contact a provider"
    );

    let catalog = module
        .snapshot(SnapshotQuery {
            profile: fake.profile.clone(),
            scope: SnapshotScope::Workspace {
                cwd: fake.state.cwd.clone(),
            },
            mode: SnapshotMode::CatalogRefresh,
        })
        .await
        .expect("stable agent catalog refresh");
    let agent = catalog
        .controls
        .iter()
        .find(|control| control.id == "agent")
        .expect("catalog refresh hydrates stable agents");
    assert_eq!(agent.state, ControlState::Selectable);
    assert_eq!(agent.choices.len(), 1);
    assert_eq!(agent.choices[0].value, json!("build"));
    assert_eq!(fake.state.prompt_count.load(Ordering::SeqCst), 0);
    assert_eq!(
        fake.state
            .directories
            .lock()
            .expect("directories")
            .as_slice(),
        &[fake.state.cwd.to_string_lossy().to_string()]
    );

    let seen = Arc::new(Mutex::new(Vec::new()));
    let bound_sessions = Arc::new(Mutex::new(Vec::new()));
    let observer = RuntimeObserver::new({
        let seen = seen.clone();
        move |event| seen.lock().expect("observations").push(event)
    })
    .with_session_binder({
        let state = Arc::clone(&fake.state);
        let bound_sessions = Arc::clone(&bound_sessions);
        move |binding| {
            let state = Arc::clone(&state);
            let bound_sessions = Arc::clone(&bound_sessions);
            async move {
                assert_eq!(state.prompt_count.load(Ordering::SeqCst), 0);
                bound_sessions.lock().expect("bound sessions").push(binding);
                Ok(())
            }
        }
    });
    let mut turn = fake.turn("turn-1", None);
    let RuntimeIntent::Turn(turn_request) = &mut turn.intent else {
        panic!("turn request");
    };
    turn_request.instructions = Some("Follow the paired Agent Definition.".to_string());
    let result = module
        .execute(turn, observer, RuntimeControl::default())
        .await
        .expect("turn");
    let ExecuteResult::Turn(result) = result else {
        panic!("turn result");
    };
    assert_eq!(result.outcome, RuntimeTurnOutcome::Completed);
    assert_eq!(result.final_answer, "hello from fake 1");
    assert_eq!(result.native_session_id, "ses_root");
    assert_eq!(result.history_fidelity, HistoryFidelity::Partial);
    {
        let bindings = bound_sessions.lock().expect("bound sessions");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].native_session_id, "ses_root");
    }
    assert_eq!(
        fake.state.systems.lock().expect("systems").as_slice(),
        [Some("Follow the paired Agent Definition.".to_string())]
    );

    let args = fs::read_to_string(&fake.args_file).expect("args");
    assert_eq!(
        args.lines().collect::<Vec<_>>(),
        [
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            "0",
            "--no-mdns"
        ]
    );
    assert!(
        fake.state
            .directories
            .lock()
            .expect("directories")
            .iter()
            .all(|directory| Path::new(directory) == fake.state.cwd)
    );

    let observations = seen.lock().expect("observations").clone();
    let timeline_calls = fake
        .state
        .timeline_calls
        .lock()
        .expect("timeline calls")
        .clone();
    let prompt_index = timeline_calls
        .iter()
        .position(|call| call == "prompt:ses_root")
        .expect("prompt call");
    for expected in [
        "todo:ses_root",
        "diff:ses_root:msg_history_root",
        "todo:ses_child",
        "diff:ses_child:msg_history_child",
    ] {
        let index = timeline_calls
            .iter()
            .position(|call| call == expected)
            .unwrap_or_else(|| panic!("missing timeline hydration call `{expected}`"));
        assert!(index < prompt_index, "`{expected}` must precede prompt");
    }
    let plans = observations
        .iter()
        .filter_map(|event| match event {
            RuntimeObservation::PlanUpdated(update) => Some(update),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        plans.len(),
        2,
        "HTTP snapshot plus one deduplicated SSE update"
    );
    assert_eq!(plans[0].runtime_ref, "opencode");
    assert_eq!(plans[0].thread_id, "thread-1");
    assert_eq!(plans[0].turn_id, "turn-1");
    assert_eq!(plans[0].steps[0].step, "Hydrated root todo");
    assert_eq!(plans[0].steps[0].status, RuntimePlanStepStatus::Pending);
    assert_eq!(plans[0].steps[1].status, RuntimePlanStepStatus::Cancelled);
    assert_eq!(plans[1].steps[0].step, "SSE root todo");
    assert_eq!(plans[1].steps[0].status, RuntimePlanStepStatus::InProgress);
    let diffs = observations
        .iter()
        .filter_map(|event| match event {
            RuntimeObservation::DiffUpdated(update) => Some(update),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        diffs.len(),
        2,
        "HTTP snapshot plus one deduplicated SSE update"
    );
    assert!(diffs[0].diff.contains("hydrated-root"));
    assert!(diffs[0].diff.contains("docs/summary.md (modified): +2 -1"));
    assert!(diffs[1].diff.contains("sse-root"));
    let public_timeline = serde_json::to_string(&(
        plans.into_iter().cloned().collect::<Vec<_>>(),
        diffs.into_iter().cloned().collect::<Vec<_>>(),
    ))
    .expect("serialize typed timeline");
    for forbidden in [
        "ses_root",
        "ses_child",
        "ses_foreign",
        "evt_todo",
        "native",
        "CHILD_TIMELINE_MUST_NOT_PROJECT",
        "WRONG_DIRECTORY_MUST_NOT_PROJECT",
        "FOREIGN_SESSION_MUST_NOT_PROJECT",
    ] {
        assert!(
            !public_timeline.contains(forbidden),
            "typed timeline leaked `{forbidden}`: {public_timeline}"
        );
    }
    let deltas = observations
        .iter()
        .filter(|event| matches!(event, RuntimeObservation::TextDelta { .. }))
        .count();
    assert_eq!(deltas, 1, "direct and sync copies must deduplicate");
    let child = observations.iter().find_map(|event| match event {
        RuntimeObservation::ChildChanged {
            native_session_id,
            read_only,
            ..
        } if native_session_id == "ses_child" => Some(*read_only),
        _ => None,
    });
    assert_eq!(child, Some(true));
    let interactions = observations
        .iter()
        .filter_map(|event| match event {
            RuntimeObservation::Interaction(interaction) => Some(interaction.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(interactions.len(), 3);
    assert!(interactions.iter().all(|interaction| {
        interaction.parent_native_session_id.as_deref() == Some("ses_root")
            && interaction.child_native_session_id.as_deref() == Some("ses_child")
    }));

    let permission = interactions
        .iter()
        .find(|interaction| {
            interaction.kind == "permission" && interaction.authorization_lifetime.is_some()
        })
        .expect("instance-scoped permission");
    assert_eq!(
        permission
            .choices
            .iter()
            .map(|choice| choice.id.as_str())
            .collect::<Vec<_>>(),
        ["once", "always", "reject"]
    );
    assert_eq!(
        permission.authorization_lifetime.as_deref(),
        Some("until_runtime_instance_restarts")
    );
    let once_only = interactions
        .iter()
        .find(|interaction| {
            interaction.kind == "permission" && interaction.authorization_lifetime.is_none()
        })
        .expect("one-shot permission");
    assert_eq!(
        once_only
            .choices
            .iter()
            .map(|choice| choice.id.as_str())
            .collect::<Vec<_>>(),
        ["once", "reject"]
    );
    let question = interactions
        .iter()
        .find(|interaction| interaction.kind == "question")
        .expect("typed question interaction");
    assert!(
        question.choices.is_empty(),
        "questions must not be flattened"
    );
    assert_eq!(question.questions.len(), 2);
    assert_eq!(question.questions[0].header.as_deref(), Some("Area"));
    assert_eq!(question.questions[0].question, "Which area?");
    assert!(question.questions[0].custom);
    assert!(!question.questions[0].multiple);
    assert_eq!(question.questions[0].options[1].label, "GUI");
    assert_eq!(
        question.questions[0].options[1].description,
        "Use Workbench"
    );
    assert_eq!(question.questions[1].question, "Which checks?");
    assert!(question.questions[1].multiple);
    assert!(!question.questions[1].custom);
    let interaction = module
        .execute(
            ExecuteRequest {
                profile: fake.profile.clone(),
                expected_profile_revision: fake.profile.revision,
                expected_capability_revision: None,
                expected_binding_revision: None,
                intent: RuntimeIntent::Interaction(
                    psychevo_runtime_host::RuntimeInteractionResponse {
                        interaction_id: permission.id.clone(),
                        process_epoch: permission.process_epoch,
                        instance_epoch: permission.instance_epoch,
                        response: json!({ "decision": "once" }),
                    },
                ),
            },
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("interaction");
    assert!(matches!(
        interaction,
        ExecuteResult::Interaction(result) if result.accepted && !result.expired
    ));

    let session_snapshot = module
        .snapshot(SnapshotQuery {
            profile: fake.profile.clone(),
            scope: SnapshotScope::Session {
                cwd: fake.state.cwd.clone(),
                thread_id: "thread-1".to_string(),
                native_session_id: Some("ses_root".to_string()),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("session snapshot");
    let agent = session_snapshot
        .controls
        .iter()
        .find(|control| control.id == "agent")
        .expect("observed agent control");
    assert_eq!(agent.state, ControlState::Selectable);
    assert_eq!(agent.current_value, Some(json!("build")));
    assert_eq!(agent.choices.len(), 1);
    let model = session_snapshot
        .controls
        .iter()
        .find(|control| control.id == "model")
        .expect("observed model control");
    assert_eq!(model.state, ControlState::ReadOnlyCurrent);
    assert_eq!(model.current_value, Some(json!("fake/model")));
    assert!(
        session_snapshot
            .capabilities
            .iter()
            .any(|capability| capability.id == "history.partial")
    );
    assert!(
        !session_snapshot
            .capabilities
            .iter()
            .any(|capability| capability.id == "history.full")
    );
    for capability_id in ["timeline.todos", "timeline.diff"] {
        let capability = session_snapshot
            .capabilities
            .iter()
            .find(|capability| capability.id == capability_id)
            .unwrap_or_else(|| panic!("missing stable capability `{capability_id}`"));
        assert!(capability.enabled);
        assert_eq!(capability.stability, RuntimeStability::Stable);
    }
    for stage_id in [
        "command",
        "server",
        "version",
        "authentication",
        "capabilities",
        "policy",
    ] {
        assert_eq!(
            session_snapshot
                .readiness
                .iter()
                .find(|stage| stage.id == stage_id)
                .map(|stage| stage.status),
            Some(ReadinessStatus::Ready),
            "ready stage {stage_id}"
        );
    }

    let list = module
        .execute(
            fake.session(RuntimeSessionOperation::List, None, None),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("list");
    let ExecuteResult::Session(list) = list else {
        panic!("session list result");
    };
    let root = list
        .sessions
        .iter()
        .find(|session| session.native_session_id == "ses_root")
        .expect("root session");
    assert_eq!(root.ownership, SessionOwnership::ReadOnly);
    assert_eq!(root.fidelity, HistoryFidelity::Partial);
    assert_eq!(
        root.actions,
        [
            "read", "fork", "resume", "rename", "revert", "unrevert", "delete", "archive",
        ]
    );
    for action in &root.actions {
        assert!(
            session_snapshot
                .capabilities
                .iter()
                .any(|capability| capability.id == format!("session.{action}")),
            "session action `{action}` must have a capability row"
        );
    }
    let child = list
        .sessions
        .iter()
        .find(|session| session.native_session_id == "ses_child")
        .expect("child session");
    assert_eq!(child.ownership, SessionOwnership::ReadOnly);
    assert_eq!(child.actions, ["read", "fork"]);
    let read = module
        .execute(
            fake.session(
                RuntimeSessionOperation::Read,
                Some("ses_root".to_string()),
                None,
            ),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("read");
    let ExecuteResult::Session(read) = read else {
        panic!("session read result");
    };
    assert!(!read.sessions[0].messages.is_empty());
    assert!(
        read.sessions[0]
            .actions
            .iter()
            .any(|action| action == "resume")
    );

    let resume = module
        .execute(
            fake.session(
                RuntimeSessionOperation::Resume,
                Some("ses_root".to_string()),
                None,
            ),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("resume");
    let ExecuteResult::Session(resume) = resume else {
        panic!("session resume result");
    };
    assert!(resume.changed);
    assert_eq!(resume.sessions[0].ownership, SessionOwnership::ReadWrite);
    assert!(
        !resume.sessions[0]
            .actions
            .iter()
            .any(|action| action == "resume")
    );
    assert!(
        resume.sessions[0]
            .actions
            .iter()
            .any(|action| action == "archive")
    );

    let archive = module
        .execute(
            fake.session(
                RuntimeSessionOperation::Archive,
                Some("ses_root".to_string()),
                None,
            ),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("archive");
    let ExecuteResult::Session(archive) = archive else {
        panic!("session archive result");
    };
    assert!(archive.changed);
    assert_eq!(archive.sessions[0].ownership, SessionOwnership::ReadWrite);
    let fork = module
        .execute(
            fake.session(
                RuntimeSessionOperation::Fork,
                Some("ses_root".to_string()),
                None,
            ),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("fork");
    assert!(
        matches!(fork, ExecuteResult::Session(result) if result.changed && result.sessions[0].native_session_id == "ses_fork")
    );
    let revert = module
        .execute(
            fake.session(
                RuntimeSessionOperation::Revert,
                Some("ses_root".to_string()),
                Some(json!({ "messageID": "msg_assistant_1" })),
            ),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("revert");
    assert!(matches!(revert, ExecuteResult::Session(result) if result.changed));

    fake.stop(&module).await;
}

#[tokio::test]
async fn failed_gateway_binding_ack_prevents_opencode_prompt_delivery() {
    let fake = FakeRuntime::start(TurnBehavior::Complete).await;
    let module = OpenCodeRuntimeModule::new();
    let observer = RuntimeObserver::default().with_session_binder(|_| async {
        Err(psychevo_runtime_host::RuntimeError::new(
            "fake_binding_failed",
            psychevo_runtime_host::RuntimeErrorStage::Binding,
            psychevo_runtime_host::RetryClass::Never,
            "fake Gateway binding failed",
        ))
    });
    let error = module
        .execute(
            fake.turn("turn-binding-fails", None),
            observer,
            RuntimeControl::default(),
        )
        .await
        .expect_err("binding acknowledgement");
    assert_eq!(error.code, "fake_binding_failed");
    assert_eq!(fake.state.prompt_count.load(Ordering::SeqCst), 0);
    fake.stop(&module).await;
}

#[tokio::test]
async fn abort_is_terminal_once_and_stale_idle_cannot_complete_next_turn() {
    let fake = FakeRuntime::start(TurnBehavior::DeferFirst).await;
    let module = OpenCodeRuntimeModule::new();
    let control = RuntimeControl::default();
    let running = module.execute(
        fake.turn("turn-abort", None),
        RuntimeObserver::default(),
        control.clone(),
    );
    tokio::pin!(running);
    tokio::select! {
        _ = wait_for_count(&fake.state.prompt_count, 1) => control.abort(),
        result = &mut running => panic!("turn ended before abort: {result:?}"),
    }
    let result = running.await.expect("abort result");
    let ExecuteResult::Turn(result) = result else {
        panic!("turn result");
    };
    assert_eq!(result.outcome, RuntimeTurnOutcome::Interrupted);
    assert_eq!(fake.state.aborts.load(Ordering::SeqCst), 1);

    let second = module
        .execute(
            fake.turn("turn-2", Some("ses_root".to_string())),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("second turn");
    let ExecuteResult::Turn(second) = second else {
        panic!("turn result");
    };
    assert_eq!(second.outcome, RuntimeTurnOutcome::Completed);
    assert_eq!(second.final_answer, "hello from fake 2");
    assert_eq!(fake.state.prompt_count.load(Ordering::SeqCst), 2);
    fake.stop(&module).await;
}

#[tokio::test]
async fn active_stream_eof_returns_one_failed_terminal_without_native_fallback() {
    let fake = FakeRuntime::start(TurnBehavior::CloseStream).await;
    let module = OpenCodeRuntimeModule::new();
    let result = module
        .execute(
            fake.turn("turn-gap", None),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("failed turn result");
    let ExecuteResult::Turn(result) = result else {
        panic!("turn result");
    };
    assert_eq!(result.outcome, RuntimeTurnOutcome::Failed);
    let terminal_error = result
        .terminal_error
        .as_ref()
        .expect("typed event-gap terminal error");
    assert_eq!(terminal_error.code, "event_gap");
    assert_eq!(terminal_error.stage, RuntimeErrorStage::Transport);
    assert_eq!(
        terminal_error.retry_class,
        psychevo_runtime_host::RetryClass::UnknownDelivery
    );
    assert_eq!(
        terminal_error.message,
        "OpenCode event continuity was lost."
    );
    assert_eq!(
        result
            .metadata
            .as_ref()
            .and_then(|value| value["code"].as_str()),
        Some("event_gap")
    );
    assert_eq!(fake.state.prompt_count.load(Ordering::SeqCst), 1);
    fake.stop(&module).await;
}

#[tokio::test]
async fn helper_process_exit_wakes_the_accepted_turn_once() {
    let fake = FakeRuntime::start(TurnBehavior::ExitProcess).await;
    let module = OpenCodeRuntimeModule::new();
    let result = module
        .execute(
            fake.turn("turn-exit", None),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("failed turn result");
    let ExecuteResult::Turn(result) = result else {
        panic!("turn result");
    };
    assert_eq!(result.outcome, RuntimeTurnOutcome::Failed);
    let terminal_error = result
        .terminal_error
        .as_ref()
        .expect("typed process-exit terminal error");
    assert_eq!(terminal_error.code, "process_exit");
    assert_eq!(terminal_error.stage, RuntimeErrorStage::Transport);
    assert_eq!(
        terminal_error.retry_class,
        psychevo_runtime_host::RetryClass::UnknownDelivery
    );
    assert_eq!(
        terminal_error.message,
        "OpenCode exited before the turn completed."
    );
    assert_eq!(
        result
            .metadata
            .as_ref()
            .and_then(|value| value["code"].as_str()),
        Some("process_exit")
    );
    assert_eq!(fake.state.prompt_count.load(Ordering::SeqCst), 1);
    fake.stop(&module).await;
}

#[tokio::test]
async fn wrong_process_credentials_fail_at_authentication_without_prompt_delivery() {
    let fake = FakeRuntime::start(TurnBehavior::WrongAuth).await;
    let module = OpenCodeRuntimeModule::new();
    let error = module
        .execute(
            fake.turn("turn-auth", None),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("authentication must fail");
    assert_eq!(error.code, "authentication_failed");
    assert_eq!(fake.state.prompt_count.load(Ordering::SeqCst), 0);
    let password = fs::read_to_string(&fake.auth_file).expect("fake auth marker");
    assert_eq!(password, "wrong-password");
    fake.stop(&module).await;
}
