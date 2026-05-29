#[allow(unused_imports)]
use super::*;

#[test]
pub(crate) fn view_scopes_filters_formats_and_privacy_boundary() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));
    let run = run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: None,
        overwrite: false,
        store_root: Some(store_root.clone()),
        output_root: None,
        include_artifacts: Vec::new(),
    })
    .expect("run");
    assert!(
        run.cells
            .iter()
            .all(|cell| cell.cell_root.join("prompt.md").is_file())
    );

    let html_stdout = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        path: Some(PathBuf::from("runs/test-coding/fake-pass")),
        task_set: None,
        agent: None,
        task: None,
        status: Some(CaseStatusFilter::Passed),
        group_by: vec!["agent,task-set".to_string()],
        include: vec!["summary,matrix,usage".to_string()],
        format: None,
        output: None,
    })
    .expect("html stdout view");
    assert!(html_stdout.stdout.contains("<!doctype html>"));
    assert!(html_stdout.stdout.contains("Visible Trial Heatmap"));
    assert!(html_stdout.stdout.contains("fake-pass"));
    assert!(!html_stdout.stdout.contains("evaluator stdout body"));

    let json = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        path: None,
        task_set: Some("local/rust-swe".to_string()),
        agent: Some("fake-pass".to_string()),
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary,matrix".to_string()],
        format: Some(ViewFormat::Json),
        output: None,
    })
    .expect("json view");
    let payload: Value = serde_json::from_str(&json.stdout).expect("view json");
    assert_eq!(payload["schema_version"], VIEW_SCHEMA_VERSION);
    assert_eq!(payload["summary"]["total_trials"], 1);
    assert!(payload["leaderboard"]["entries"].as_array().is_some());
    assert!(
        payload["matrix"]["cells"][0]["matrix_cell_key"]
            .as_str()
            .is_some()
    );
    assert!(
        payload["trials"][0]["cell_root_relative"]
            .as_str()
            .expect("cell root relative")
            .starts_with("runs/")
    );
    assert!(payload["trials"][0]["score_passed"].as_bool().is_some());
    assert!(payload["trials"][0]["score_message"].as_str().is_some());
    assert!(payload["trials"][0]["artifact_refs"].as_array().is_some());

    let task_scope_json = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        path: Some(PathBuf::from(
            "runs/test-coding/fake-pass/local_rust-swe-add",
        )),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary,matrix".to_string()],
        format: Some(ViewFormat::Json),
        output: None,
    })
    .expect("task scope json view");
    let task_scope_payload: Value =
        serde_json::from_str(&task_scope_json.stdout).expect("task scope view json");
    assert_eq!(task_scope_payload["summary"]["total_trials"], 1);

    let html_path = temp.path().join("view.html");
    let html = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root),
        path: Some(run.cells[0].cell_root.clone()),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary,matrix".to_string()],
        format: None,
        output: Some(Some(html_path.clone())),
    })
    .expect("html view");
    assert_eq!(html.stdout, format!("wrote {}\n", html_path.display()));
    let html = fs::read_to_string(html_path).expect("html file");
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("Visible Trial Heatmap"));
    assert!(!html.contains("evaluator stdout body"));
}
