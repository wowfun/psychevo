use crate::{init_tui_home, pevo_cmd};
use tempfile::tempdir;
#[tokio::test]
pub(crate) async fn cli_agent_inspect_unknown_id_reports_not_found() {
    let temp = tempdir().expect("temp");
    let psychevo_home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");

    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
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

#[tokio::test]
pub(crate) async fn cli_agent_validate_json_reports_effective_empty_tools_policy() {
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

#[tokio::test]
pub(crate) async fn cli_agent_wait_zero_timeout_reports_timeout_without_pending_mail() {
    let temp = tempdir().expect("temp");
    let psychevo_home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let application = psychevo::Application::builder()
        .home(&psychevo_home)
        .database_path(&db)
        .build()
        .await
        .expect("Application");
    let mut request = psychevo::StartThreadRequest::new(&cwd);
    request.source = "run".to_string();
    application
        .client()
        .start_thread(request)
        .await
        .expect("run Thread");
    application
        .shutdown()
        .await
        .expect("shutdown")
        .require_clean()
        .expect("clean shutdown");

    let output = pevo_cmd(temp.path())
        .current_dir(&cwd)
        .env("PSYCHEVO_HOME", &psychevo_home)
        .env("PSYCHEVO_DB", &db)
        .args(["agent", "wait", "--timeout-ms", "0", "--json"])
        .output()
        .expect("pevo agent wait");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(
        body,
        serde_json::json!({"message": "Wait timed out.", "timed_out": true})
    );
}
