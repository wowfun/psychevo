use super::support::*;
#[allow(unused_imports)]
use super::*;
use pretty_assertions::assert_eq;

#[test]
pub(crate) fn cell_runs_execute_reuse_and_overwrite() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));

    let first = run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: None,
        overwrite: false,
        store_root: Some(store_root.clone()),
        output_root: None,
        include_artifacts: vec!["workspace".to_string()],
    })
    .expect("first run");
    assert_eq!(first.schema_version, ARTIFACT_SCHEMA_VERSION);
    assert_eq!(first.benchmark, "test-coding");
    assert_eq!(first.status, RunStatus::Failed);
    assert_eq!(first.selected_cells, 2);
    assert_eq!(first.executed_cells, 2);
    assert_eq!(first.reused_cells, 0);
    assert_eq!(first.passed_cells, 1);
    assert_eq!(first.failed_cells, 1);
    assert!(
        first
            .cells
            .iter()
            .all(|cell| cell.cell_root.join("run.json").is_file())
    );
    assert!(
        first
            .cells
            .iter()
            .all(|cell| cell.cell_root.join("trajectory.jsonl").is_file())
    );
    assert!(
        first
            .cells
            .iter()
            .all(|cell| cell.cell_root.join("prompt.md").is_file())
    );
    assert!(
        first
            .cells
            .iter()
            .all(|cell| cell.cell_root.join("workspace/status.txt").is_file())
    );
    for cell in &first.cells {
        assert_eq!(cell.cell_key.len(), 16);
        assert!(cell.cell_key.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_eq!(
            cell.cell_key,
            cell.fingerprint.chars().take(16).collect::<String>()
        );
        assert_eq!(
            cell.cell_root,
            store_root
                .join("runs/test-coding")
                .join(sanitize_id(&cell.agent_id))
                .join(sanitize_id(&cell.task_id))
                .join(&cell.cell_key)
        );
        assert!(!cell.cell_key.contains("__"));
    }
    let passed_cell = first
        .cells
        .iter()
        .find(|cell| cell.agent_id == "fake-pass")
        .expect("passed command cell");
    let passed_run = read_cell_run(&passed_cell.cell_root).expect("passed cell run");
    assert_eq!(passed_run.case.score.message, "env checked");
    assert!(!store_root.join("index.json").exists());
    assert!(!store_root.join(".cache").exists());
    assert!(!store_root.join("dashboard.html").exists());

    let second = run_evaluation(RunRequest {
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
    .expect("second run");
    assert_eq!(second.executed_cells, 0);
    assert_eq!(second.reused_cells, 2);
    assert!(
        second
            .cells
            .iter()
            .all(|cell| cell.action == CellRunAction::Reused)
    );

    let overwrite = run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: Some("fake-pass".to_string()),
        overwrite: true,
        store_root: Some(store_root),
        output_root: None,
        include_artifacts: Vec::new(),
    })
    .expect("overwrite");
    assert_eq!(overwrite.status, RunStatus::Passed);
    assert_eq!(overwrite.selected_cells, 1);
    assert_eq!(overwrite.overwritten_cells, 1);
    assert_eq!(overwrite.reused_cells, 0);
}

#[test]
pub(crate) fn old_v6_cell_at_short_path_is_not_reused() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));
    let project = EvalProject::load(fixture.join("eval.toml")).expect("project");
    let case = expand_matrix(
        &project,
        Some("local/rust-swe"),
        Some("local/rust-swe-add"),
        Some("fake-pass"),
    )
    .expect("matrix")
    .into_iter()
    .next()
    .expect("case");
    let fingerprint = cell_fingerprint(&project, &case).expect("fingerprint");
    let cell_key = cell_key(&fingerprint);
    let stale_cell_root = EvalStore::new(store_root.clone()).cell_root(&project, &case, &cell_key);
    fs::create_dir_all(&stale_cell_root).expect("stale cell root");
    fs::write(
        stale_cell_root.join("run.json"),
        r#"{"schema_version":6,"benchmark":"test-coding","benchmark_slug":"test-coding","cell_key":"old","fingerprint":"old","cell_root":"old","started_at_ms":0,"finished_at_ms":0,"case":{}}"#,
    )
    .expect("stale v6 run");

    let run = run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: Some("fake-pass".to_string()),
        overwrite: false,
        store_root: Some(store_root),
        output_root: None,
        include_artifacts: Vec::new(),
    })
    .expect("run");

    assert_eq!(run.status, RunStatus::Passed);
    assert_eq!(run.selected_cells, 1);
    assert_eq!(run.reused_cells, 0);
    assert_eq!(run.retried_cells, 1);
    assert_eq!(run.cells[0].cell_key, cell_key);
    assert_eq!(
        read_cell_run(&run.cells[0].cell_root)
            .expect("fresh cell")
            .schema_version,
        ARTIFACT_SCHEMA_VERSION
    );
}
