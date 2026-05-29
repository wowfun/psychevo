#[allow(unused_imports)]
use super::*;

#[test]
pub(crate) fn view_matrix_uses_latest_trial_and_leaderboard_keeps_all_trials() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));
    let run = run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: Some("fake-pass".to_string()),
        overwrite: false,
        store_root: Some(store_root.clone()),
        output_root: None,
        include_artifacts: Vec::new(),
    })
    .expect("run");
    let latest = read_cell_run(&run.cells[0].cell_root).expect("latest cell");
    let stale_root = latest
        .cell_root
        .parent()
        .expect("cell parent")
        .join("0000000000000000");
    copy_dir(&latest.cell_root, &stale_root).expect("copy stale cell");
    let stale_json_path = stale_root.join("run.json");
    let mut stale_json: Value =
        serde_json::from_str(&fs::read_to_string(&stale_json_path).expect("stale json"))
            .expect("stale value");
    stale_json["cell_key"] = json!("0000000000000000");
    stale_json["fingerprint"] = json!("0000000000000000");
    stale_json["started_at_ms"] = json!(latest.started_at_ms.saturating_sub(2000));
    stale_json["finished_at_ms"] = json!(latest.finished_at_ms.saturating_sub(1000));
    stale_json["case"]["status"] = json!("failed");
    stale_json["case"]["failure_class"] = json!("stale_failure");
    stale_json["case"]["score"]["passed"] = json!(false);
    stale_json["case"]["score"]["score"] = json!(0.0);
    stale_json["case"]["score"]["message"] = json!("stale failed trial");
    fs::write(
        &stale_json_path,
        serde_json::to_string_pretty(&stale_json).expect("stale serialized"),
    )
    .expect("write stale");

    let view = build_view(ViewRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root),
        path: Some(latest.cell_root.parent().expect("task dir").to_path_buf()),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![ViewInclude::Summary, ViewInclude::Matrix],
    })
    .expect("view");
    assert_eq!(view.summary.total_trials, 2);
    assert_eq!(view.matrix.cells.len(), 1);
    assert_eq!(
        view.matrix.cells[0].representative_trial_key,
        trial_key(&latest)
    );
    assert_eq!(view.matrix.cells[0].trial_keys.len(), 2);
    assert_eq!(view.leaderboard.entries.len(), 1);
    assert_eq!(view.leaderboard.entries[0].total_trials, 2);
    assert_eq!(view.leaderboard.entries[0].successes, 1);
    assert_eq!(view.leaderboard.entries[0].trial_keys.len(), 2);
}
