use std::collections::BTreeMap;
use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::composition::GatewayApplication;
use crate::server::{GatewayWebServerConfig, bind_gateway_web_server};

async fn managed_test_config(
    temp: &tempfile::TempDir,
    instance_id: Option<&str>,
) -> GatewayWebServerConfig {
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(home.join("config.toml"), "\n").expect("config");
    let runtime =
        GatewayApplication::open(home, temp.path().join("state.db"), None, BTreeMap::new())
            .await
            .expect("test composition");
    let mut config = GatewayWebServerConfig::headless(runtime, cwd, "managed-secret".to_string());
    config.managed_instance_id = instance_id.map(str::to_string);
    config
}

#[tokio::test]
async fn managed_identity_and_shutdown_require_auth_and_matching_instance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bound = bind_gateway_web_server(managed_test_config(&temp, Some("instance-a")).await)
        .await
        .expect("bind");
    let base_url = bound.url();
    let task = tokio::spawn(bound.run_with_shutdown_signal(std::future::pending()));
    let client = reqwest::Client::new();

    let unauthorized = client
        .get(format!("{base_url}/_gateway/managed/identity"))
        .send()
        .await
        .expect("unauthorized identity");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let identity = client
        .get(format!("{base_url}/_gateway/managed/identity"))
        .bearer_auth("managed-secret")
        .send()
        .await
        .expect("identity");
    assert_eq!(identity.status(), StatusCode::OK);
    let identity: Value = identity.json().await.expect("identity json");
    assert_eq!(identity["ok"], true);
    assert_eq!(identity["instanceId"], "instance-a");
    assert_eq!(identity["pid"], std::process::id());

    let mismatch = client
        .post(format!("{base_url}/_gateway/managed/shutdown"))
        .bearer_auth("managed-secret")
        .json(&json!({"instanceId": "instance-b"}))
        .send()
        .await
        .expect("mismatched shutdown");
    assert_eq!(mismatch.status(), StatusCode::CONFLICT);
    assert!(!task.is_finished());

    let shutdown = client
        .post(format!("{base_url}/_gateway/managed/shutdown"))
        .bearer_auth("managed-secret")
        .json(&json!({"instanceId": "instance-a"}))
        .send()
        .await
        .expect("shutdown");
    assert_eq!(shutdown.status(), StatusCode::OK);
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("server stopped")
        .expect("server task")
        .expect("server result");
}

#[tokio::test]
async fn non_managed_server_does_not_expose_managed_routes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bound = bind_gateway_web_server(managed_test_config(&temp, None).await)
        .await
        .expect("bind");
    let base_url = bound.url();
    let task = tokio::spawn(bound.run_with_shutdown_signal(std::future::pending()));

    let response = reqwest::Client::new()
        .get(format!("{base_url}/_gateway/managed/identity"))
        .bearer_auth("managed-secret")
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    task.abort();
}
