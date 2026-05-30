use super::support::*;
#[allow(unused_imports)]
use super::*;
use pretty_assertions::assert_eq;

#[test]
pub(crate) fn analysis_uses_configured_agent_and_writes_cached_json() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let mut eval = fs::read_to_string(fixture.join("eval.toml")).expect("eval");
    eval.push_str(
        r#"

[analysis]
agent = "fake-pass"

[reports.focused.analysis]
agent = "analyst"

[[agents]]
id = "analyst"
kind = "fake"
"#,
    );
    fs::write(fixture.join("eval.toml"), eval).expect("eval with analysis");
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
    .expect("seed run");
    let service = EvalService::new(ServiceContext {
        cwd: fixture.clone(),
        env: BTreeMap::new(),
        psychevo_home: Some(temp.path().join("psychevo-home")),
        root_override: Some(store_root.clone()),
        capabilities: ServiceCapabilities::all(),
    });
    let view = ViewRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: Some("focused".to_string()),
        store_root: None,
        paths: Vec::new(),
        task_set: Some("local/rust-swe".to_string()),
        agent: Some("fake-pass".to_string()),
        task: None,
        status: None,
        group_by: Vec::new(),
        include: all_view_includes(),
        notes: Vec::new(),
    };
    let status = service.analysis_status(&view).expect("analysis status");
    assert!(status.enabled, "{status:?}");
    assert_eq!(status.agent.as_deref(), Some("analyst"));
    let result = service
        .analyze_trial(AnalysisTrialRequest {
            view,
            trial_key: format!("{}:t001", run.cells[0].cell_key),
            overwrite: false,
        })
        .expect("analysis run");
    assert_eq!(result.status, "ok");
    assert!(result.checks.contains_key("failure_diagnosis"));
    let written =
        fs::read_to_string(run.cells[0].cell_root.join("analysis.json")).expect("analysis json");
    assert!(written.contains("\"input_fingerprint\""));
    assert!(!written.contains("You are analyzing a peval Trial"));
}

#[test]
pub(crate) fn serve_file_access_is_contained_and_bounded() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    fs::write(root.join("ok.txt"), "visible").expect("ok");
    let (bytes, mime) = read_bounded_workspace_file(root, Path::new("ok.txt")).expect("read file");
    assert_eq!(bytes, b"visible");
    assert_eq!(mime, "text/plain");
    let err = read_bounded_workspace_file(root, Path::new("../outside.txt"))
        .expect_err("parent path rejected");
    assert!(format!("{err:#}").contains("relative"));
    fs::write(root.join("big.log"), "a".repeat(1024 * 1024 + 1)).expect("big");
    let err =
        read_bounded_workspace_file(root, Path::new("big.log")).expect_err("large path rejected");
    assert!(format!("{err:#}").contains("1 MiB"));
}

#[test]
pub(crate) fn removed_commands_and_flags_fail_at_cli_boundary() {
    assert_eq!(run_cli_from(["peval", "report"]).code, 2);
    assert_eq!(run_cli_from(["peval", "compare", "a", "b"]).code, 2);
    assert_eq!(
        run_cli_from(["peval", "replay", "--run-root", "latest"]).code,
        2
    );
    assert_eq!(
        run_cli_from(["peval", "run", "--run-id", "old-style"]).code,
        2
    );
    assert_eq!(run_cli_from(["peval", "project", "list"]).code, 1);
    assert_eq!(
        run_cli_from(["peval", "check", "--project", "legacy"]).code,
        2
    );
    assert_eq!(run_cli_from(["peval", "view", "latest"]).code, 2);
    assert_eq!(run_cli_from(["peval", "list", "--kind", "runs"]).code, 2);
    assert_eq!(
        run_cli_from(["peval", "check", "--suite", "legacy"]).code,
        2
    );
    assert_eq!(
        run_cli_from(["peval", "view", "--group-by", "suite"]).code,
        1
    );
    assert_eq!(
        run_cli_from(["peval", "view", "--format", "markdown"]).code,
        2
    );
    assert_eq!(run_cli_from(["peval", "list", "--kind", "suites"]).code, 2);
}

#[test]
pub(crate) fn service_read_only_can_view_but_not_execute() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));
    run_evaluation(RunRequest {
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
    .expect("seed run");

    let service = EvalService::new(ServiceContext {
        cwd: fixture.clone(),
        env: BTreeMap::new(),
        psychevo_home: Some(temp.path().join("psychevo-home")),
        root_override: Some(store_root.clone()),
        capabilities: ServiceCapabilities::read_only(),
    });
    let view = service
        .view(ViewRequest {
            config: Some(fixture.join("eval.toml")),
            benchmark: None,
            report: None,
            store_root: None,
            paths: Vec::new(),
            task_set: Some("local/rust-swe".to_string()),
            agent: Some("fake-pass".to_string()),
            task: None,
            status: None,
            group_by: vec![ViewGroupBy::Agent],
            include: vec![ViewInclude::Comparison],
            notes: Vec::new(),
        })
        .expect("read-only view");
    assert_eq!(
        view.comparison
            .as_ref()
            .expect("comparison")
            .summary
            .total_trials,
        1
    );
    assert!(!store_root.join(".cache").exists());

    let denied = service.run(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: Some("fake-pass".to_string()),
        overwrite: false,
        store_root: None,
        output_root: None,
        include_artifacts: Vec::new(),
    });
    assert_eq!(
        denied.expect_err("execute denied").code,
        "capability_denied"
    );
}

#[test]
pub(crate) fn unsupported_old_artifact_is_ignored_by_view_scan() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));
    fs::create_dir_all(store_root.join("runs/test-coding/legacy-run")).expect("legacy dir");
    fs::write(
        store_root.join("runs/test-coding/legacy-run/run.json"),
        r#"{"schema_version":6,"benchmark":"old","benchmark_slug":"old","cell_key":"old","fingerprint":"old","cell_root":"old","started_at_ms":0,"finished_at_ms":0,"case":{}}"#,
    )
    .expect("legacy run");

    let view = build_view(ViewRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root),
        paths: Vec::new(),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![ViewInclude::Comparison],
        notes: Vec::new(),
    })
    .expect("view ignores legacy");
    assert_eq!(
        view.comparison
            .as_ref()
            .expect("comparison")
            .summary
            .total_trials,
        0
    );
}
