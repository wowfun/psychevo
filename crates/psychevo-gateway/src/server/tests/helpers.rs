use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::Response;
use psychevo::application::Message as RuntimeMessage;
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::composition::GatewayApplication;
use crate::server::GatewayWebServerConfig;
use crate::server::binding::{AuthContext, WebState};
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;

pub(in crate::server::tests) async fn web_state() -> (tempfile::TempDir, WebState) {
    web_state_with_env(BTreeMap::new()).await
}

pub(in crate::server::tests) async fn web_state_with_env(
    inherited_env: BTreeMap<String, String>,
) -> (tempfile::TempDir, WebState) {
    web_state_with_composition(inherited_env, None).await
}

pub(in crate::server::tests) async fn web_state_with_native_test_executor(
    executor: crate::FrameworkNativeTestExecutor,
) -> (tempfile::TempDir, WebState) {
    web_state_with_composition(BTreeMap::new(), Some(executor)).await
}

pub(in crate::server::tests) fn framework_message_fixture_executor(
    messages: Vec<RuntimeMessage>,
) -> crate::FrameworkNativeTestExecutor {
    framework_fixture_executor(messages, false)
}

pub(in crate::server::tests) fn framework_turn_fixture_executor(
    messages: Vec<RuntimeMessage>,
) -> crate::FrameworkNativeTestExecutor {
    framework_fixture_executor(messages, true)
}

fn framework_fixture_executor(
    messages: Vec<RuntimeMessage>,
    persist_turn_input: bool,
) -> crate::FrameworkNativeTestExecutor {
    Arc::new(move |invocation| {
        let messages = messages.clone();
        Box::pin(async move {
            invocation.persistence.confirm_delivery().await?;
            if persist_turn_input {
                let editable_input = invocation
                    .input
                    .prompt_display
                    .as_ref()
                    .and_then(|display| display.editable_input.as_ref())
                    .expect("Gateway Turn fixture must carry canonical editable input");
                let mut metadata = serde_json::Map::new();
                metadata.insert(
                    psychevo::application::EDITABLE_INPUT_METADATA_KEY.to_string(),
                    serde_json::to_value(editable_input)?,
                );
                invocation
                    .persistence
                    .append_message_with_metrics(
                        psychevo::application::user_text_message(invocation.input.prompt.clone()),
                        None,
                        Some(Value::Object(metadata)),
                    )
                    .await?;
            }
            for message in messages {
                invocation.persistence.append_message(message).await?;
            }
            Ok(psychevo::TurnResult {
                thread_id: invocation.receipt.thread_id,
                outcome: psychevo::TurnOutcome::Completed,
                final_answer: String::new(),
                provider: "fixture-provider".to_string(),
                model: "fixture-model".to_string(),
                reasoning_effort: None,
                tool_failures: 0,
                context_limit: None,
                context_snapshot: None,
                warnings: Vec::new(),
                terminal_reason: None,
                terminal_error: None,
                selected_agent: None,
                selected_skills: Vec::new(),
            })
        })
    })
}

async fn web_state_with_composition(
    inherited_env: BTreeMap<String, String>,
    native_test_executor: Option<crate::FrameworkNativeTestExecutor>,
) -> (tempfile::TempDir, WebState) {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let mut env = BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_HOME".to_string(),
            home.to_string_lossy().to_string(),
        ),
    ]);
    env.extend(inherited_env);
    std::fs::create_dir_all(&home).expect("home");
    let database_path = temp.path().join("state.db");
    let runtime = match native_test_executor {
        Some(executor) => {
            GatewayApplication::open_with_native_test_executor(
                home,
                database_path,
                None,
                env,
                executor,
            )
            .await
        }
        None => GatewayApplication::open(home, database_path, None, env).await,
    }
    .expect("test composition");
    let config = GatewayWebServerConfig::with_static(runtime, cwd, temp.path().join("static"));
    (temp, WebState::new(config))
}

pub(in crate::server::tests) async fn web_state_with_static() -> (tempfile::TempDir, WebState) {
    let (temp, state) = web_state().await;
    let static_dir = temp.path().join("static");
    std::fs::create_dir_all(&static_dir).expect("static dir");
    std::fs::write(
        static_dir.join("index.html"),
        "<!doctype html><title>workbench</title>",
    )
    .expect("index");
    (temp, state)
}

pub(in crate::server::tests) fn write_agent_definition(
    dir: &Path,
    name: &str,
    description: &str,
) -> PathBuf {
    std::fs::create_dir_all(dir).expect("agent dir");
    let path = dir.join(format!("{name}.md"));
    std::fs::write(
        &path,
        format!("---\ndescription: {description:?}\n---\n\nUse this agent.\n"),
    )
    .expect("agent definition");
    path
}

pub(in crate::server::tests) async fn response_text(response: Response<Body>) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf8")
}

pub(in crate::server::tests) fn rpc_test_request<'a>(
    state: &'a WebState,
    tx: &'a mpsc::UnboundedSender<String>,
    method: &'a str,
    params: Value,
) -> futures::future::BoxFuture<'a, Value> {
    Box::pin(async move {
        handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
                id: Some(json!(format!("test-{method}"))),
                method: method.to_string(),
                params: Some(params),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{method} failed: {error}"))
    })
}

pub(in crate::server::tests) fn write_project_skill(
    state: &WebState,
    name: &str,
    description: &str,
) {
    let dir = state.inner.cwd.join(".psychevo").join("skills").join(name);
    std::fs::create_dir_all(&dir).expect("skill dir");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description:?}\n---\n\nUse this skill.\n"),
    )
    .expect("skill");
}

pub(in crate::server::tests) fn git<I, S>(cwd: &Path, args: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
