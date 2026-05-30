use super::support::*;
#[allow(unused_imports)]
use super::*;
use pretty_assertions::assert_eq;

#[test]
pub(crate) fn task_env_cli_args_parse() {
    let create = Cli::try_parse_from([
        "peval",
        "env",
        "create",
        "--config",
        "eval.toml",
        "--task",
        "local/rust-swe-add",
        "--json",
    ])
    .expect("create parses");
    let Commands::Env(TaskEnvCommands::Create(args)) = create.command else {
        panic!("expected env create");
    };
    assert_eq!(args.config, Some(PathBuf::from("eval.toml")));
    assert_eq!(args.task.as_deref(), Some("local/rust-swe-add"));
    assert!(args.json);

    let verify = Cli::try_parse_from([
        "peval",
        "env",
        "verify",
        "--env",
        "runs/x",
        "--duration-seconds",
        "42",
        "--json",
    ])
    .expect("verify parses");
    let Commands::Env(TaskEnvCommands::Verify(args)) = verify.command else {
        panic!("expected env verify");
    };
    assert_eq!(args.env_root, PathBuf::from("runs/x"));
    assert_eq!(args.duration_seconds, 42);
    assert!(args.json);
}

#[test]
pub(crate) fn task_env_create_verify_and_view_human_in_loop_trial() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));
    let service = EvalService::new(ServiceContext {
        cwd: fixture.clone(),
        env: BTreeMap::new(),
        psychevo_home: Some(temp.path().join("psychevo-home")),
        root_override: Some(store_root.clone()),
        capabilities: ServiceCapabilities::all(),
    });

    let created = service
        .create_task_env(TaskEnvCreateRequest {
            config: Some(fixture.join("eval.toml")),
            benchmark: None,
            task_set: Some("local/rust-swe".to_string()),
            task: Some("local/rust-swe-add".to_string()),
            store_root: None,
        })
        .expect("create task env");
    assert_eq!(created.schema_version, TASK_ENV_SCHEMA_VERSION);
    assert_eq!(created.benchmark, "test-coding");
    assert_eq!(created.task_id, "local/rust-swe-add");
    assert!(
        created
            .env_root
            .starts_with(store_root.join("runs/test-coding/human-in-loop"))
    );
    assert!(created.workspace.join("status.txt").is_file());
    assert!(created.prompt.is_file());
    assert!(created.metadata.is_file());
    assert!(created.readme.is_file());
    assert!(!created.env_root.join("run.json").exists());

    let empty_view = build_view(ViewRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        paths: Vec::new(),
        task_set: Some("local/rust-swe".to_string()),
        agent: Some("human-in-loop".to_string()),
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![ViewInclude::Comparison],
        notes: Vec::new(),
    })
    .expect("empty view");
    assert_eq!(
        empty_view
            .comparison
            .as_ref()
            .expect("comparison")
            .summary
            .total_trials,
        0
    );

    fs::write(created.workspace.join("status.txt"), "fixed").expect("fix workspace");
    let verified = service
        .verify_task_env(TaskEnvVerifyRequest {
            env_root: created.env_root.clone(),
            duration_seconds: 42,
        })
        .expect("verify task env");
    assert_eq!(verified.status, CaseStatus::Passed);
    assert!(verified.passed);
    assert_eq!(verified.score, Some(1.0));
    assert_eq!(verified.duration_ms, 42_000);
    assert!(verified.run_json.is_file());
    assert!(created.env_root.join("trajectory.jsonl").is_file());
    assert!(created.env_root.join("evaluator.stdout").is_file());
    assert!(created.env_root.join("evaluator.stderr").is_file());

    let cell = read_cell_run(&created.env_root).expect("cell run");
    assert_eq!(cell.case.agent_id, "human-in-loop");
    assert_eq!(cell.case.candidate.agent_id, "human-in-loop");
    assert_eq!(cell.case.candidate.adapter, AgentKind::HumanInLoop);
    assert_eq!(cell.case.duration_ms, 42_000);
    assert_eq!(cell.case.metrics.duration_ms, 42_000);
    assert_eq!(cell.case.score.message, "env checked");

    let view = build_view(ViewRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        paths: Vec::new(),
        task_set: Some("local/rust-swe".to_string()),
        agent: Some("human-in-loop".to_string()),
        task: None,
        status: None,
        group_by: Vec::new(),
        include: all_view_includes(),
        notes: Vec::new(),
    })
    .expect("view");
    let comparison = view.comparison.as_ref().expect("comparison");
    assert_eq!(comparison.summary.total_trials, 1);
    assert_eq!(comparison.leaderboard.entries[0].agent_id, "human-in-loop");
    assert_eq!(comparison.matrix.cells[0].agent_id, "human-in-loop");
    assert_eq!(view.trajectory[0].agent.name, "human-in-loop");

    let verified_again = service
        .verify_task_env(TaskEnvVerifyRequest {
            env_root: created.env_root.clone(),
            duration_seconds: 7,
        })
        .expect("verify again");
    assert_eq!(verified_again.duration_ms, 7_000);
    let overwritten = read_cell_run(&created.env_root).expect("overwritten cell");
    assert_eq!(overwritten.case.duration_ms, 7_000);
}

#[test]
pub(crate) fn task_env_create_requires_single_local_task() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    write_local_task(&fixture, "rust-swe-sub", "swe-style");
    fs::write(
        fixture.join("benchmark.toml"),
        r#"schema_version = 5
id = "test-coding"
name = "test-coding"

[[sources.peval_agent]]
id = "local"
path = "tasks"

[[sources.peval_agent.sets]]
id = "rust-swe"
include = ["rust-swe-add", "rust-swe-sub"]
"#,
    )
    .expect("benchmark");
    let store_root = init_workspace(temp.path().join("evals"));
    let service = EvalService::new(ServiceContext {
        cwd: fixture.clone(),
        env: BTreeMap::new(),
        psychevo_home: Some(temp.path().join("psychevo-home")),
        root_override: Some(store_root),
        capabilities: ServiceCapabilities::all(),
    });
    let err = service
        .create_task_env(TaskEnvCreateRequest {
            config: Some(fixture.join("eval.toml")),
            benchmark: None,
            task_set: Some("local/rust-swe".to_string()),
            task: None,
            store_root: None,
        })
        .expect_err("multi-task selection should fail");
    assert!(err.message.contains("requires exactly one selected task"));
}

#[test]
pub(crate) fn task_env_rejects_container_backed_tasks_and_configured_human_agent() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("official-sources");
    fs::create_dir_all(&root).expect("root");
    write_local_task(&root.join("harbor"), "case-b", "harbor");
    fs::write(
        root.join("benchmark.toml"),
        r#"schema_version = 5
id = "official-declarations"
name = "official declarations"

[[sources.harbor]]
id = "hb"
root = "harbor"
path = "tasks"

[[sources.harbor.sets]]
id = "sample"
include = ["case-*"]
"#,
    )
    .expect("benchmark");
    let store_root = init_workspace(temp.path().join("evals"));
    let service = EvalService::new(ServiceContext {
        cwd: root.clone(),
        env: BTreeMap::new(),
        psychevo_home: Some(temp.path().join("psychevo-home")),
        root_override: Some(store_root),
        capabilities: ServiceCapabilities::all(),
    });
    let err = service
        .create_task_env(TaskEnvCreateRequest {
            config: None,
            benchmark: Some(root.join("benchmark.toml").display().to_string()),
            task_set: Some("hb/sample".to_string()),
            task: Some("hb/case-b".to_string()),
            store_root: None,
        })
        .expect_err("container task should fail");
    assert!(
        err.message
            .contains("supports only local-directory task environments")
    );

    fs::write(
        root.join("eval.toml"),
        r#"schema_version = 5
id = "human-agent-eval"
name = "human agent eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["human-in-loop"]
sets = ["hb/sample"]

[[agents]]
id = "human-in-loop"
kind = "human-in-loop"
"#,
    )
    .expect("eval");
    let project = EvalProject::load(root.join("eval.toml")).expect("project");
    let err = check_project(&project, None, None, None).expect_err("human agent manifest denied");
    assert!(format!("{err:#}").contains("reserved for `peval env verify`"));
}
