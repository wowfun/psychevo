#[allow(unused_imports)]
pub(crate) use super::*;
#[test]
pub(crate) fn cli_agent_inspect_unknown_id_reports_not_found() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");

    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_DB", &db)
        .args(["agent", "inspect", "missing-agent"])
        .output()
        .expect("pevo agent inspect");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(
        stderr.contains("agent not found: missing-agent"),
        "{stderr}"
    );
}

#[test]
pub(crate) fn cli_agent_inspect_json_includes_identity_and_depth() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let store = psychevo_runtime::state::StateRuntime::open(&db).expect("store");
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let parent = store
        .create_session_with_metadata(&cwd, "tui", "mock-model", "mock", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(&parent, &cwd, "agent", "mock-model", "mock", None)
        .expect("child");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::state::AgentEdgeStatus::Open,
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

#[test]
pub(crate) fn cli_agent_validate_json_reports_effective_empty_tools_policy() {
    let temp = tempdir().expect("temp");
    let psychevo_home = init_tui_home(temp.path());
    let agents_dir = psychevo_home.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("agents dir");
    std::fs::write(
        agents_dir.join("translate.md"),
        "---\nname: translate\ndescription: Translate only\ntools: []\n---\nTranslate.\n",
    )
    .expect("agent");

    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .args(["agent", "validate", "translate", "--json"])
        .output()
        .expect("pevo agent validate");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(body["valid"], true);
    assert_eq!(body["agent"]["tool_policy"]["tools"], serde_json::json!([]));
    assert_eq!(
        body["agent"]["effective_policy"]["tools"]["mode"],
        "explicit_empty"
    );
    assert_eq!(
        body["agent"]["effective_policy"]["agent_catalog"]["visible"],
        false
    );
    assert_eq!(
        body["agent"]["effective_policy"]["skill_catalog"]["visible"],
        false
    );
    assert_eq!(
        body["agent"]["effective_policy"]["project_instructions"]["visible"],
        true
    );
}
