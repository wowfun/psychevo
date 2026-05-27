#[allow(unused_imports)]
pub(crate) use super::*;

use pretty_assertions::assert_eq;

#[test]
pub(crate) fn eval_config_resolves_benchmark_and_inline_agents() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let project = EvalProject::load(fixture.join("eval.toml")).expect("eval config load");

    assert_eq!(project.name, "test-coding eval");
    assert_eq!(project.benchmark_id, "test-coding");
    assert_eq!(
        project.agents["fake-fail"].fake.behavior,
        FakeBehavior::Fail
    );

    let cases =
        check_project(&project, Some("rust-swe"), None, None).expect("check selected matrix");
    let ids = cases
        .iter()
        .map(|case| case.case_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "rust-swe__rust-swe-add__fake-fail",
            "rust-swe__rust-swe-add__fake-pass",
        ]
    );
}

#[test]
pub(crate) fn pidx_benchmark_is_benchmark_only_and_templates_select_agents() {
    let manifest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benchmarks/pidx-coding/benchmark.toml");
    let benchmark = BenchmarkManifest::load(&manifest).expect("pidx benchmark");
    assert_eq!(benchmark.id, "pidx-coding");
    assert_eq!(benchmark.task_sets["base"].tasks.len(), 3);

    let template =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/pidx-fake-patch-add.eval.toml");
    let project = EvalProject::load(&template).expect("pidx template eval");
    assert_eq!(project.benchmark_id, "pidx-coding");
    assert_eq!(
        project.agents.keys().collect::<Vec<_>>(),
        vec![&"fake-pass".to_string()]
    );
    let cases = check_project(&project, Some("base"), Some("patch-add"), Some("fake-pass"))
        .expect("check template");
    assert_eq!(cases.len(), 1);
}

#[test]
pub(crate) fn registry_precedence_and_direct_benchmark_selection() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));
    fs::write(
        store_root.join("peval.toml"),
        format!(
            r#"schema_version = 2
kind = "workspace"
name = "test workspace"

[[agents]]
id = "fake-pass"
kind = "fake"
fake = {{ behavior = "fail" }}

[[benchmarks]]
id = "test-coding"
path = "{}"
"#,
            fixture.join("benchmark.toml").display()
        ),
    )
    .expect("workspace registry");

    let one_off =
        load_one_off_benchmark("test-coding", Some(store_root.clone())).expect("one-off benchmark");
    assert_eq!(
        one_off.agents["fake-pass"].fake.behavior,
        FakeBehavior::Fail
    );

    let eval = load_eval_config(&fixture.join("eval.toml"), Some(store_root.clone()))
        .expect("eval config wins registry");
    assert_eq!(eval.agents["fake-pass"].fake.behavior, FakeBehavior::Pass);

    let direct = run_cli_from([
        "peval",
        "check",
        "--root",
        store_root.to_str().expect("root"),
        "--benchmark",
        "test-coding",
        "--agent",
        "fake-pass",
        "--task-set",
        "rust-swe",
        "--json",
    ]);
    assert_eq!(direct.code, 0, "stderr: {}", direct.stderr);
    let payload: Value = serde_json::from_str(&direct.stdout).expect("direct json");
    assert_eq!(payload["benchmark"], "test-coding");
    assert_eq!(payload["cases"], 1);

    let missing_agent = run_cli_from([
        "peval",
        "check",
        "--root",
        store_root.to_str().expect("root"),
        "--benchmark",
        "test-coding",
        "--task-set",
        "rust-swe",
    ]);
    assert_eq!(missing_agent.code, 1);
    assert!(
        missing_agent
            .stderr
            .contains("--benchmark requires an explicit --agent")
    );
}

#[test]
pub(crate) fn duplicate_registry_ids_fail_in_their_own_layer() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    fs::write(
        fixture.join("duplicate-agents.eval.toml"),
        r#"schema_version = 4
id = "duplicate-agents"
name = "duplicate agents"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["fake-pass"]
task_sets = ["rust-swe"]

[[agents]]
id = "fake-pass"
kind = "fake"
fake = { behavior = "pass" }

[[agents]]
id = "fake-pass"
kind = "fake"
fake = { behavior = "fail" }
"#,
    )
    .expect("duplicate eval");
    let err = EvalProject::load(fixture.join("duplicate-agents.eval.toml"))
        .expect_err("duplicate agent ids should fail");
    assert!(format!("{err:#}").contains("duplicate agent id `fake-pass`"));
}

#[test]
pub(crate) fn init_creates_v2_workspace_without_cache_or_dashboard() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("evals");
    let initialized = init_eval_store(InitStoreRequest {
        root: Some(root.clone()),
        make_default: false,
        force: false,
    })
    .expect("init");

    assert_eq!(initialized.schema_version, WORKSPACE_SCHEMA_VERSION);
    assert_eq!(initialized.root, absolute_path(&root));
    assert!(initialized.root.join("peval.toml").is_file());
    assert!(initialized.root.join("runs").is_dir());
    assert!(initialized.root.join("datasets").is_dir());
    assert!(initialized.root.join("scripts").is_dir());
    assert!(!initialized.root.join(".cache").exists());
    assert!(!initialized.root.join("dashboard.html").exists());

    let workspace = read_workspace_config(&initialized.root).expect("workspace config");
    assert_eq!(workspace.schema_version, WORKSPACE_SCHEMA_VERSION);
    assert!(workspace.agents.is_empty());
    assert!(workspace.benchmarks.is_empty());
}

#[test]
pub(crate) fn cell_runs_execute_reuse_and_overwrite() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));

    let first = run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("rust-swe".to_string()),
        task: None,
        agent: None,
        overwrite: false,
        store_root: Some(store_root.clone()),
        output_root: None,
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
    assert!(!store_root.join("index.json").exists());
    assert!(!store_root.join(".cache").exists());
    assert!(!store_root.join("dashboard.html").exists());

    let second = run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("rust-swe".to_string()),
        task: None,
        agent: None,
        overwrite: false,
        store_root: Some(store_root.clone()),
        output_root: None,
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
        task_set: Some("rust-swe".to_string()),
        task: None,
        agent: Some("fake-pass".to_string()),
        overwrite: true,
        store_root: Some(store_root),
        output_root: None,
    })
    .expect("overwrite");
    assert_eq!(overwrite.status, RunStatus::Passed);
    assert_eq!(overwrite.selected_cells, 1);
    assert_eq!(overwrite.overwritten_cells, 1);
    assert_eq!(overwrite.reused_cells, 0);
}

#[test]
pub(crate) fn view_scopes_filters_formats_and_privacy_boundary() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));
    let run = run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("rust-swe".to_string()),
        task: None,
        agent: None,
        overwrite: false,
        store_root: Some(store_root.clone()),
        output_root: None,
    })
    .expect("run");

    let markdown = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        store_root: Some(store_root.clone()),
        path: Some(PathBuf::from("runs/test-coding/fake-pass")),
        task_set: None,
        agent: None,
        task: None,
        status: Some(CaseStatusFilter::Passed),
        group_by: vec!["agent,task-set".to_string()],
        include: vec!["summary,matrix,usage".to_string()],
        format: Some(ViewFormat::Markdown),
        output: None,
    })
    .expect("markdown view");
    assert!(markdown.stdout.contains("# peval view"));
    assert!(markdown.stdout.contains("fake-pass"));
    assert!(!markdown.stdout.contains("trajectory.jsonl"));
    assert!(!markdown.stdout.contains("evaluator.stdout"));

    let json = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        store_root: Some(store_root.clone()),
        path: None,
        task_set: Some("rust-swe".to_string()),
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
    assert_eq!(payload["summary"]["total_cells"], 1);
    assert!(payload["matrix"][0]["artifact_root"].as_str().is_some());

    let html_path = temp.path().join("view.html");
    let html = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        store_root: Some(store_root),
        path: Some(run.cells[0].cell_root.clone()),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary,matrix".to_string()],
        format: None,
        output: Some(html_path.clone()),
    })
    .expect("html view");
    assert_eq!(html.stdout, format!("wrote {}\n", html_path.display()));
    let html = fs::read_to_string(html_path).expect("html file");
    assert!(html.contains("<!doctype html>"));
    assert!(!html.contains("trajectory.jsonl"));
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
        task_set: Some("rust-swe".to_string()),
        task: None,
        agent: Some("fake-pass".to_string()),
        overwrite: false,
        store_root: Some(store_root.clone()),
        output_root: None,
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
            store_root: None,
            path: None,
            task_set: Some("rust-swe".to_string()),
            agent: Some("fake-pass".to_string()),
            task: None,
            status: None,
            group_by: vec![ViewGroupBy::Agent],
            include: vec![ViewInclude::Summary, ViewInclude::Matrix],
        })
        .expect("read-only view");
    assert_eq!(view.summary.total_cells, 1);
    assert!(!store_root.join(".cache").exists());

    let denied = service.run(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("rust-swe".to_string()),
        task: None,
        agent: Some("fake-pass".to_string()),
        overwrite: false,
        store_root: None,
        output_root: None,
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
        r#"{"schema_version":5,"benchmark":"old","benchmark_slug":"old","cell_key":"old","fingerprint":"old","cell_root":"old","started_at_ms":0,"finished_at_ms":0,"case":{}}"#,
    )
    .expect("legacy run");

    let view = build_view(ViewRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        store_root: Some(store_root),
        path: None,
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![ViewInclude::Summary, ViewInclude::Matrix],
    })
    .expect("view ignores legacy");
    assert_eq!(view.summary.total_cells, 0);
}

#[test]
pub(crate) fn external_evaluators_check_but_do_not_run() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("tau2-eval");
    fs::create_dir_all(&root).expect("project root");
    fs::write(
        root.join("benchmark.toml"),
        r#"schema_version = 4
id = "tau2-declaration"
name = "tau2-declaration"

[evaluator]
kind = "tau2"

[evaluator.args]
domain = "airline"

[[task_sources]]
path = "tasks.jsonl"
format = "jsonl"

[[task_sets]]
id = "base"
tasks = ["placeholder"]
"#,
    )
    .expect("benchmark");
    fs::write(
        root.join("tasks.jsonl"),
        r#"{"schema_version":4,"task_id":"placeholder","kind":"external","problem_statement":"placeholder","workspace":{"source":"."},"test_spec":{"checks":[]}}"#,
    )
    .expect("tasks");
    fs::write(
        root.join("eval.toml"),
        r#"schema_version = 4
id = "tau2-declaration-eval"
name = "tau2 declaration eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["fake-pass"]
task_sets = ["base"]

[[agents]]
id = "fake-pass"
kind = "fake"
fake = { behavior = "pass" }
"#,
    )
    .expect("eval");
    let project = EvalProject::load(root.join("eval.toml")).expect("external project");
    assert_eq!(project.evaluator.kind, EvaluatorKind::Tau2);
    assert!(!project.evaluator.run_supported());
    assert_eq!(
        check_project(&project, None, None, None)
            .expect("declaration-only check")
            .len(),
        0
    );

    let denied = EvalService::new(ServiceContext {
        cwd: root.clone(),
        env: BTreeMap::new(),
        psychevo_home: Some(temp.path().join("psychevo-home")),
        root_override: Some(init_workspace(temp.path().join("evals"))),
        capabilities: ServiceCapabilities::all(),
    })
    .run(RunRequest {
        config: Some(root.join("eval.toml")),
        benchmark: None,
        task_set: None,
        task: None,
        agent: None,
        overwrite: false,
        store_root: None,
        output_root: None,
    })
    .expect_err("external evaluator run should fail");
    assert_eq!(denied.code, "unsupported_evaluator");
}

pub(crate) fn init_workspace(root: PathBuf) -> PathBuf {
    init_eval_store(InitStoreRequest {
        root: Some(root.clone()),
        make_default: false,
        force: false,
    })
    .expect("init")
    .root
}

pub(crate) fn create_local_coding_eval(root: &Path) -> PathBuf {
    fs::create_dir_all(root).expect("project root");
    fs::write(
        root.join("benchmark.toml"),
        r#"schema_version = 4
id = "test-coding"
name = "test-coding"

[evaluator]
kind = "local-coding"

[[task_sources]]
path = "tasks.jsonl"
format = "jsonl"

[[task_sets]]
id = "rust-swe"
tasks = ["rust-swe-add"]
"#,
    )
    .expect("benchmark");
    fs::write(
        root.join("eval.toml"),
        r#"schema_version = 4
id = "test-coding-eval"
name = "test-coding eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["fake-pass", "fake-fail"]
task_sets = ["rust-swe"]

[[agents]]
id = "fake-pass"
kind = "fake"
fake = { behavior = "pass" }

[[agents]]
id = "fake-fail"
kind = "fake"
fake = { behavior = "fail" }
"#,
    )
    .expect("eval");
    write_local_task(root, "rust-swe-add", "swe-style");
    root.to_path_buf()
}

pub(crate) fn write_local_task(root: &Path, id: &str, kind: &str) {
    let dir = root.join("tasks").join(id);
    fs::create_dir_all(dir.join("workspace")).expect("workspace");
    fs::write(dir.join("workspace/status.txt"), "pending").expect("status");
    fs::write(
        root.join("tasks.jsonl"),
        format!(
            "{{\"schema_version\":4,\"task_id\":\"{id}\",\"kind\":\"{kind}\",\"dir\":\"tasks/{id}\",\"problem_statement\":\"complete {id}\",\"workspace\":{{\"source\":\"workspace\"}},\"test_spec\":{{\"checks\":[{{\"kind\":\"exact_file\",\"path\":\"status.txt\",\"expected\":\"fixed\"}}]}}}}\n"
        ),
    )
    .expect("tasks jsonl");
}
