use std::collections::BTreeMap;

use psychevo::application::{GatewayDurability, GatewaySourceLaneInput};
use psychevo::config::ConfigScope;
use psychevo::session_export::{
    SessionArtifactKind, SessionExportFormat, SessionExportIncludeSet, SessionExportOptions,
};
use psychevo::{
    Application, ConfigurationQuery, RunMode, StartThreadRequest, ThreadListQuery, TurnRequest,
    UsageQuery,
};

#[test]
fn turn_request_is_configured_through_domain_methods() {
    let request = TurnRequest::new("inspect the workspace")
        .with_identity("external-sdk", Some("client-turn-1".to_string()))
        .with_model(Some("provider/model".to_string()), Some("high".to_string()))
        .with_runtime(
            Some("native".to_string()),
            BTreeMap::from([("temperature".to_string(), "0".to_string())]),
        )
        .with_reasoning_output(true)
        .with_execution_policy(RunMode::Plan, None, None)
        .with_agent(None, true, true);

    assert_eq!(request.prompt(), "inspect the workspace");
    assert_eq!(request.source(), "external-sdk");
    assert!(request.image_inputs().is_empty());
}

#[tokio::test]
async fn default_feature_surface_builds_and_owns_threads() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&cwd).expect("workspace");

    let application = Application::builder()
        .home(&home)
        .build()
        .await
        .expect("application");
    let client = application.client();
    let thread = client
        .start_thread(StartThreadRequest::new(&cwd))
        .await
        .expect("thread");

    let snapshot = thread.snapshot().await.expect("snapshot");
    assert_eq!(snapshot.id, thread.id());
    assert_eq!(snapshot.cwd, cwd.to_string_lossy());
    let listed = client
        .list_threads(ThreadListQuery::default())
        .await
        .expect("thread summaries");
    assert_eq!(listed.threads.len(), 1);
    assert_eq!(listed.threads[0].id, thread.id());

    application.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn gateway_durability_uses_the_issuing_applications_owned_pool() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&cwd).expect("workspace");

    let application = Application::builder()
        .home(&home)
        .database_path(":memory:")
        .build()
        .await
        .expect("application");
    let thread = application
        .client()
        .start_thread(StartThreadRequest::new(&cwd))
        .await
        .expect("thread");
    let durability: GatewayDurability = application.gateway_durability();
    let draft_control_values = BTreeMap::new();

    durability
        .upsert_gateway_source_lane(GatewaySourceLaneInput {
            source_key: "test:source",
            source_kind: "test",
            raw_identity: serde_json::json!({ "id": "source" }),
            visible_name: Some("Source"),
            thread_id: Some(thread.id()),
            draft_agent_ref: None,
            draft_profile_ref: None,
            draft_control_values: &draft_control_values,
            lineage: None,
        })
        .await
        .expect("source lane over the Application pool");

    let lane = durability
        .clone()
        .gateway_source_lane("test:source")
        .await
        .expect("source lane read")
        .expect("source lane");
    assert_eq!(lane.thread_id.as_deref(), Some(thread.id()));
    assert_eq!(
        thread.snapshot().await.expect("thread snapshot").id,
        thread.id()
    );
    assert_eq!(format!("{durability:?}"), "GatewayDurability");

    application
        .shutdown()
        .await
        .expect("shutdown")
        .require_clean()
        .expect("clean shutdown");
}

#[tokio::test]
async fn application_bound_administration_never_exposes_state_or_run_options() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&cwd).expect("workspace");
    std::fs::write(
        home.join("config.toml"),
        r#"
model = "mock/default"

[provider.mock]
api = "http://127.0.0.1:9"
no_auth = true

[provider.mock.models.default]
"#,
    )
    .expect("configuration");

    let application = Application::builder()
        .home(&home)
        .build()
        .await
        .expect("application");
    let client = application.client();
    let configuration = client
        .configuration(ConfigurationQuery::new(&cwd))
        .expect("configuration view");
    let models = configuration.configured_models().expect("models");
    assert!(
        models
            .iter()
            .any(|model| model.provider == "mock" && model.model == "default")
    );
    let permissions = configuration
        .permission_rules(ConfigScope::Effective)
        .expect("permission view");
    assert!(permissions.get("permissions").is_some());

    let thread = client
        .start_thread(StartThreadRequest::new(&cwd))
        .await
        .expect("thread");
    assert_eq!(
        thread.set_title("semantic title").await.unwrap(),
        "semantic title"
    );
    assert_eq!(
        thread.snapshot().await.unwrap().title.as_deref(),
        Some("semantic title")
    );
    let export_path = temp.path().join("thread.md");
    let export = thread
        .write_export(
            &export_path,
            SessionExportOptions {
                format: SessionExportFormat::Markdown,
                include: SessionExportIncludeSet::default_for(SessionArtifactKind::Export),
                artifact_kind: SessionArtifactKind::Export,
            },
        )
        .await
        .expect("export");
    assert_eq!(export.path, export_path);
    assert!(export.bytes > 0);
    let usage = client.usage(UsageQuery::new(&cwd)).await.expect("usage");
    assert_eq!(usage["scope"]["all"], false);

    application.shutdown().await.expect("shutdown");
}
