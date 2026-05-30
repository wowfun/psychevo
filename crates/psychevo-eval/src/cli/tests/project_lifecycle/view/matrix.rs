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
        paths: vec![latest.cell_root.parent().expect("task dir").to_path_buf()],
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![ViewInclude::Comparison],
        notes: Vec::new(),
    })
    .expect("view");
    let comparison = view.comparison.as_ref().expect("comparison");
    assert_eq!(comparison.summary.total_trials, 2);
    assert_eq!(comparison.matrix.cells.len(), 1);
    assert_eq!(
        comparison.matrix.cells[0].representative_trial_key,
        trial_key(&latest)
    );
    assert_eq!(comparison.matrix.cells[0].trial_keys.len(), 2);
    assert_eq!(comparison.leaderboard.entries.len(), 1);
    assert_eq!(comparison.leaderboard.entries[0].total_trials, 2);
    assert_eq!(comparison.leaderboard.entries[0].successes, 1);
    assert_eq!(comparison.leaderboard.entries[0].trial_keys.len(), 2);
}

#[test]
pub(crate) fn view_multi_path_exact_cells_use_path_variants() {
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
    let baseline = read_cell_run(&run.cells[0].cell_root).expect("baseline cell");
    let ablation_root = baseline
        .cell_root
        .parent()
        .expect("cell parent")
        .join("1111111111111111");
    copy_dir(&baseline.cell_root, &ablation_root).expect("copy ablation cell");
    let ablation_json_path = ablation_root.join("run.json");
    let mut ablation_json: Value =
        serde_json::from_str(&fs::read_to_string(&ablation_json_path).expect("ablation json"))
            .expect("ablation value");
    ablation_json["cell_key"] = json!("1111111111111111");
    ablation_json["fingerprint"] = json!("1111111111111111");
    ablation_json["case"]["status"] = json!("failed");
    ablation_json["case"]["failure_class"] = json!("ablation_failure");
    ablation_json["case"]["score"]["passed"] = json!(false);
    ablation_json["case"]["score"]["score"] = json!(0.0);
    ablation_json["case"]["score"]["message"] = json!("ablation failed trial");
    fs::write(
        &ablation_json_path,
        serde_json::to_string_pretty(&ablation_json).expect("ablation serialized"),
    )
    .expect("write ablation");

    let view = build_view(ViewRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root),
        paths: vec![baseline.cell_root.clone(), ablation_root],
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![ViewInclude::Comparison],
        notes: Vec::new(),
    })
    .expect("view");

    let comparison = view.comparison.as_ref().expect("comparison");
    assert_eq!(comparison.summary.total_trials, 2);
    assert_eq!(view.path_selections.len(), 2);
    assert_eq!(comparison.matrix.cells.len(), 2);
    assert_eq!(comparison.leaderboard.entries.len(), 2);
    assert!(
        comparison
            .matrix
            .cells
            .iter()
            .all(|cell| cell.variant_id.is_some() && cell.variant_label.is_some())
    );
    assert_eq!(
        comparison
            .matrix
            .cells
            .iter()
            .map(|cell| cell.agent_id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        1
    );
}

#[test]
pub(crate) fn view_multi_path_same_cell_key_paths_get_distinct_trial_keys() {
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
    let base_root = run.cells[0].cell_root.clone();
    let probe_root = store_root
        .join("timing-probes/run-1/runs/test-coding/fake-pass/local_rust-swe-add")
        .join(&run.cells[0].cell_key);
    copy_dir(&base_root, &probe_root).expect("copy probe cell with same cell key");
    let probe_json_path = probe_root.join("run.json");
    let mut probe_json: Value =
        serde_json::from_str(&fs::read_to_string(&probe_json_path).expect("probe json"))
            .expect("probe value");
    probe_json["case"]["metrics"]["duration_ms"] = json!(99_999);
    fs::write(
        &probe_json_path,
        serde_json::to_string_pretty(&probe_json).expect("probe serialized"),
    )
    .expect("write probe");
    fs::write(base_root.join("notes.md"), "base persisted note").expect("base notes");
    fs::write(probe_root.join("notes.md"), "probe persisted note").expect("probe notes");

    let view = build_view(ViewRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root),
        paths: vec![base_root, probe_root],
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![
            ViewInclude::Core,
            ViewInclude::Comparison,
            ViewInclude::Annotations,
        ],
        notes: vec![
            ViewNoteInput {
                index: 1,
                markdown: "base cli note".to_string(),
            },
            ViewNoteInput {
                index: 2,
                markdown: "probe cli note".to_string(),
            },
        ],
    })
    .expect("view");

    let comparison = view.comparison.as_ref().expect("comparison");
    let annotations = view.annotations.as_ref().expect("annotations");
    assert_eq!(comparison.summary.total_trials, 2);
    assert_eq!(comparison.default_metric, "duration");
    assert_eq!(comparison.matrix.cells.len(), 2);
    let trial_keys = view
        .trajectory_meta
        .iter()
        .map(|trial| trial.trial_key.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(trial_keys.len(), 2);
    let representative_keys = view
        .comparison
        .as_ref()
        .expect("comparison")
        .matrix
        .cells
        .iter()
        .map(|cell| cell.representative_trial_key.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(representative_keys, trial_keys);
    let trajectory_ids = view
        .trajectory
        .iter()
        .filter_map(|trajectory| trajectory.trajectory_id.as_deref())
        .collect::<BTreeSet<_>>();
    assert_eq!(trajectory_ids, trial_keys);
    assert_eq!(annotations.notes.len(), 4);
    let note_trial_keys = annotations
        .notes
        .iter()
        .map(|note| note.trial_key.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(note_trial_keys, trial_keys);
    assert_eq!(annotations.notes[0].source, "cell");
    assert_eq!(annotations.notes[1].source, "cli");
    assert!(
        annotations
            .notes
            .iter()
            .any(|note| note.markdown == "probe cli note")
    );
    assert!(annotations.report_notes.is_empty());
}
