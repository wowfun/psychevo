use std::collections::BTreeMap;

use psychevo::{Application, RunMode, StartThreadRequest, ThreadListQuery, TurnRequest};

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
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, thread.id());

    application.shutdown().await.expect("shutdown");
}
