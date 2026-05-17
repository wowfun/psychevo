#[test]
fn cli_agent_inspect_unknown_id_reports_not_found() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");

    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_DB", &db)
        .args(["agent", "inspect", "missing-agent"])
        .output()
        .expect("pevo agent inspect");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("agent not found: missing-agent"), "{stderr}");
}

#[test]
fn cli_agent_inspect_json_includes_identity_and_depth() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let store = psychevo_runtime::SqliteStore::open(&db).expect("store");
    let workdir = temp.path().join("repo");
    std::fs::create_dir_all(&workdir).expect("workdir");
    let parent = store
        .create_session_with_metadata(&workdir, "tui", "mock-model", "mock", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(&parent, &workdir, "agent", "mock-model", "mock", None)
        .expect("child");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            Some(serde_json::json!({
                "agent": {
                    "id": "agent-run-1",
                    "task_name": "translate-1",
                    "name": "translate",
                    "task": "translate hello",
                    "effective_max_spawn_depth": 1
                }
            })),
        )
        .expect("edge");

    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_DB", &db)
        .args(["agent", "inspect", "translate-1", "--json"])
        .output()
        .expect("pevo agent inspect");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(body["agent"]["agent_name"], "translate");
    assert_eq!(body["agent"]["task_name"], "translate-1");
    assert_eq!(body["agent"]["effective_max_spawn_depth"], 1);
    assert_eq!(body["parent_session"]["id"], parent);
    assert_eq!(body["child_session"]["id"], child);
}
