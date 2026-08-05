use psychevo::model_state::ModelState;
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::server::binding::AuthContext;
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;
use crate::server::settings_observability::{display_cwd, display_relative_to_home};
use crate::server::tests::helpers::web_state;

#[tokio::test]
async fn settings_read_returns_workbench_project_and_controls() {
    let (_temp, state) = web_state().await;
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "settings/read".to_string(),
            params: None,
        },
    )
    .await
    .expect("settings/read");

    let cwd = state.inner.cwd.display().to_string();
    assert_eq!(result["project"]["path"].as_str(), Some(cwd.as_str()));
    assert!(
        result["project"]["displayPath"]
            .as_str()
            .is_some_and(|path| path.ends_with("/work") || path == "work"),
        "{result:#}"
    );
    assert_eq!(result["controls"]["permissionMode"], "default");
    assert_eq!(result["controls"]["mode"], "default");
    assert_eq!(result["controls"]["agent"], Value::Null);
    assert_eq!(result["controls"]["model"], Value::Null);
    assert_eq!(
        result["controls"]["modelStatus"], "unconfigured",
        "{result:#}"
    );
    assert_eq!(result["controls"]["modelError"], Value::Null);
    assert_eq!(result["controls"]["variant"], "none");
    assert!(
        result["controls"]["variantOptions"]
            .as_array()
            .expect("variant options")
            .iter()
            .any(|value| value.as_str() == Some("medium"))
    );
}

#[test]
fn settings_workbench_display_cwd_strips_windows_verbatim_prefixes() {
    assert_eq!(
        display_cwd(std::path::Path::new(r"\\?\C:\Users\Ada\project")),
        "/c/Users/Ada/project"
    );
    assert_eq!(
        display_cwd(std::path::Path::new("//?/C:/Users/Ada/project")),
        "/c/Users/Ada/project"
    );
    assert_eq!(
        display_cwd(std::path::Path::new(r"\\?\UNC\server\share\project")),
        "//server/share/project"
    );
}

#[test]
fn settings_workbench_display_cwd_preserves_home_relative_display_after_normalization() {
    assert_eq!(
        display_relative_to_home("/c/Users/Ada/project", "/c/Users/Ada"),
        Some("~/project".to_string())
    );
    assert_eq!(
        display_relative_to_home("/c/Users/Ada", "/c/Users/Ada/"),
        Some("~".to_string())
    );
}

#[tokio::test]
async fn model_state_rpc_saves_cwd_selection_and_controls_recent_models() {
    let (_temp, state) = web_state().await;
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let (tx, _rx) = mpsc::unbounded_channel();
    let cwd = state.inner.cwd.display().to_string();

    let saved = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("model-state-set")),
            method: "model/state/set".to_string(),
            params: Some(json!({
                "cwd": cwd,
                "model": "mock/model-a",
                "reasoningEffort": "high"
            })),
        },
    )
    .await
    .expect("model/state/set");

    assert_eq!(saved["model"], "mock/model-a");
    assert_eq!(saved["reasoningEffort"], "high");
    assert_eq!(saved["recentModels"], json!(["mock/model-a"]));

    let read = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("model-state-read")),
            method: "model/state/read".to_string(),
            params: Some(json!({ "cwd": state.inner.cwd.display().to_string() })),
        },
    )
    .await
    .expect("model/state/read");
    assert_eq!(read["model"], "mock/model-a");
    assert_eq!(read["reasoningEffort"], "high");

    let settings = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("settings-read")),
            method: "settings/read".to_string(),
            params: Some(json!({ "cwd": state.inner.cwd.display().to_string() })),
        },
    )
    .await
    .expect("settings/read");
    assert_eq!(settings["controls"]["model"], "mock/model-a");
    assert_eq!(settings["controls"]["variant"], "high");
    assert_eq!(
        settings["controls"]["recentModels"],
        json!(["mock/model-a"])
    );

    let model_state =
        ModelState::load(&ModelState::path_for_home(&state.inner.home)).expect("model state");
    assert_eq!(
        model_state
            .model_for(state.inner.cwd.to_string_lossy().as_ref())
            .as_deref(),
        Some("mock/model-a")
    );
}

#[tokio::test]
async fn model_state_rpc_with_thread_updates_session_model_metadata() {
    let (_temp, state) = web_state().await;
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let mut start = psychevo::StartThreadRequest::new(&state.inner.cwd);
    start.source = "web".to_string();
    let thread = state
        .inner
        .framework
        .start_thread(start)
        .await
        .expect("thread");
    let session_id = thread.id().to_string();
    let (tx, _rx) = mpsc::unbounded_channel();

    let saved = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("model-state-thread-set")),
            method: "model/state/set".to_string(),
            params: Some(json!({
                "threadId": session_id,
                "model": "mock/model-b",
                "reasoningEffort": "low"
            })),
        },
    )
    .await
    .expect("model/state/set");
    assert_eq!(saved["threadId"], session_id);
    assert_eq!(saved["model"], "mock/model-b");
    assert_eq!(saved["reasoningEffort"], "low");

    let summary = thread.snapshot().await.expect("snapshot").summary;
    assert_eq!(summary.provider, "mock");
    assert_eq!(summary.model, "model-b");
    let selection = thread
        .model_selection()
        .await
        .expect("model selection")
        .expect("selected model");
    assert_eq!(selection.provider, "mock");
    assert_eq!(selection.model, "model-b");
    assert_eq!(selection.reasoning_effort.as_deref(), Some("low"));

    let settings = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("settings-read-thread")),
            method: "settings/read".to_string(),
            params: Some(json!({ "threadId": session_id })),
        },
    )
    .await
    .expect("settings/read");
    assert_eq!(settings["controls"]["model"], "mock/model-b");
    assert_eq!(settings["controls"]["variant"], "low");
}
