use psychevo::{Application, StartThreadRequest, ThreadListQuery};

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
