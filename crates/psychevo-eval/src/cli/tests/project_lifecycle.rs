#[allow(unused_imports)]
pub(crate) use super::*;

use pretty_assertions::assert_eq;

#[allow(unused_imports)]
use anyhow::{Context, Result, bail};
#[allow(unused_imports)]
use clap::{Parser, Subcommand, ValueEnum};
#[allow(unused_imports)]
use serde_json::{Value, json};
#[allow(unused_imports)]
use std::collections::{BTreeMap, BTreeSet};
#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::ffi::OsString;
#[allow(unused_imports)]
use std::fs;
#[allow(unused_imports)]
use std::io::{BufRead, BufReader};
#[allow(unused_imports)]
use std::path::{Component, Path, PathBuf};
#[allow(unused_imports)]
use std::process::{Command, Stdio};
#[allow(unused_imports)]
use std::thread;
#[allow(unused_imports)]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[allow(unused_imports)]
use uuid::Uuid;

#[test]
pub(crate) fn project_discovery_validation_and_matrix_are_deterministic() {
    let project =
        EvalProject::load(fixture_project().join("tasks/rust-swe-add")).expect("project load");
    assert_eq!(project.name, "local-rust-swe");
    let cases = check_project(&project, Some("rust-swe"), None).expect("check");
    let ids = cases
        .iter()
        .map(|case| case.case_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "rust-swe__rust-swe-add__fake-pass",
            "rust-swe__rust-swe-add__fake-fail"
        ]
    );
}

#[test]
pub(crate) fn unsupported_schema_is_rejected() {
    let temp = tempfile::tempdir().expect("temp");
    write_minimal_project(temp.path(), 99, r#"agents = ["fake-pass"]"#);
    let err = EvalProject::load(temp.path()).expect_err("unsupported schema");
    assert!(
        err.to_string().contains("unsupported schema_version 99"),
        "{err:#}"
    );
}

#[test]
pub(crate) fn psychevo_live_agent_requires_manifest_opt_in() {
    let temp = tempfile::tempdir().expect("temp");
    write_minimal_project(temp.path(), 1, r#"agents = ["psychevo-live"]"#);
    fs::write(
            temp.path().join("agents/psychevo-live.toml"),
            "schema_version = 1\nid = \"psychevo-live\"\nkind = \"psychevo\"\n[psychevo]\ncommand = \"pevo\"\n",
        )
        .expect("psychevo agent");
    let project = EvalProject::load(temp.path()).expect("project");
    let err = check_project(&project, Some("suite"), Some("psychevo-live"))
        .expect_err("live agent should be gated");
    assert!(err.to_string().contains("allow_live = false"), "{err:#}");
}

#[test]
pub(crate) fn fake_agents_write_artifacts_reports_compare_and_replay() {
    let temp = tempfile::tempdir().expect("temp");
    let run_one = run_evaluation(RunRequest {
        config: Some(fixture_project().join("eval.toml")),
        suite: Some("rust-swe".to_string()),
        agent: None,
        run_id: Some("fixture-one".to_string()),
        store_root: None,
        output_root: Some(temp.path().to_path_buf()),
    })
    .expect("run");
    assert_eq!(run_one.status, RunStatus::Failed);
    assert_eq!(run_one.passed_cases, 1);
    assert_eq!(run_one.failed_cases, 1);
    let root_one = temp.path().join("fixture-one");
    assert!(root_one.join("summary.json").is_file());
    assert!(
        root_one
            .join("cases/rust-swe__rust-swe-add__fake-pass/result.json")
            .is_file()
    );
    assert!(
        !root_one
            .join("cases/rust-swe__rust-swe-add__fake-pass/workspace")
            .exists(),
        "case workspaces should not be retained as artifacts"
    );
    let markdown = render_report(ReportRequest {
        run_root: root_one.clone(),
        format: ReportFormat::Markdown,
    })
    .expect("markdown");
    assert!(markdown.contains("fake-pass"));
    let html = render_report(ReportRequest {
        run_root: root_one.clone(),
        format: ReportFormat::Html,
    })
    .expect("html");
    assert!(html.contains("id=\"caseTable\""));
    let json_report = render_report(ReportRequest {
        run_root: root_one.clone(),
        format: ReportFormat::Json,
    })
    .expect("json");
    assert!(json_report.contains("\"schema_version\": 1"));

    let run_two = run_evaluation(RunRequest {
        config: Some(fixture_project().join("eval.toml")),
        suite: Some("rust-swe".to_string()),
        agent: Some("fake-pass".to_string()),
        run_id: Some("fixture-two".to_string()),
        store_root: None,
        output_root: Some(temp.path().to_path_buf()),
    })
    .expect("run two");
    assert_eq!(run_two.status, RunStatus::Passed);
    let compare = compare_runs(CompareRequest {
        run_roots: vec![root_one.clone(), temp.path().join("fixture-two")],
    })
    .expect("compare");
    assert_eq!(compare.runs.len(), 2);
    assert!(
        compare
            .cases
            .iter()
            .any(|case| case.key == "rust-swe/rust-swe-add/fake-pass")
    );
    let replay = replay_run(ReplayRequest {
        run_root: root_one,
        case_id: Some("rust-swe__rust-swe-add__fake-pass".to_string()),
    })
    .expect("replay");
    assert!(
        replay
            .events
            .iter()
            .any(|event| event.kind == "scorer_finished")
    );
}

#[test]
pub(crate) fn eval_store_init_default_root_env_root_output_bypass_and_manifest_namespace() {
    let _lock = ENV_LOCK.lock().expect("env lock");
    let _env = set_env_var("PEVAL_ROOT", None);
    let temp = tempfile::tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let user_home = temp.path().join("user-home");
    fs::create_dir_all(&user_home).expect("user home");
    let _psychevo_home = set_env_var("PSYCHEVO_HOME", Some(&psychevo_home));
    let _home = set_env_var("HOME", Some(&user_home));

    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("project dir");
    write_minimal_project(&project_root, 1, r#"agents = ["fake-pass"]"#);

    let uninitialized = run_evaluation(RunRequest {
        config: Some(project_root.join("eval.toml")),
        suite: Some("suite".to_string()),
        agent: Some("fake-pass".to_string()),
        run_id: Some("uninitialized".to_string()),
        store_root: None,
        output_root: None,
    })
    .expect_err("uninitialized store should fail");
    assert!(uninitialized.to_string().contains("peval init"));

    let initialized = init_eval_store(InitStoreRequest {
        root: None,
        force: false,
    })
    .expect("init default root");
    assert_eq!(initialized.root, user_home.join(".local/evals"));
    assert!(psychevo_home.join("peval.toml").is_file());
    assert!(initialized.root.join("runs").is_dir());
    assert!(initialized.root.join("datasets").is_dir());
    assert!(initialized.root.join("index.json").is_file());
    assert!(initialized.root.join("dashboard.html").is_file());

    let same_init = init_eval_store(InitStoreRequest {
        root: None,
        force: false,
    })
    .expect("idempotent init");
    assert_eq!(same_init.root, initialized.root);

    let replacement_root = temp.path().join("replacement-root");
    let replace_without_force = init_eval_store(InitStoreRequest {
        root: Some(replacement_root),
        force: false,
    })
    .expect_err("changing root requires force");
    assert!(replace_without_force.to_string().contains("--force"));

    let default = run_evaluation(RunRequest {
        config: Some(project_root.join("eval.toml")),
        suite: Some("suite".to_string()),
        agent: Some("fake-pass".to_string()),
        run_id: Some("default-root".to_string()),
        store_root: None,
        output_root: None,
    })
    .expect("default root");
    assert_eq!(
        default.artifact_root,
        initialized.root.join("runs/bad").join("default-root")
    );
    assert!(initialized.root.join("index.json").is_file());
    assert!(default.artifact_root.join("report.html").is_file());
    assert!(default.artifact_root.join("report.md").is_file());

    let flag_root = temp.path().join("flag-root");
    let by_flag = run_evaluation(RunRequest {
        config: Some(project_root.join("eval.toml")),
        suite: Some("suite".to_string()),
        agent: Some("fake-pass".to_string()),
        run_id: Some("flag-root".to_string()),
        store_root: Some(flag_root.clone()),
        output_root: None,
    })
    .expect("flag root");
    assert_eq!(
        by_flag.artifact_root,
        flag_root.join("runs/bad").join("flag-root")
    );

    let env_root = temp.path().join("env-root");
    {
        let _env = set_env_var("PEVAL_ROOT", Some(&env_root));
        let by_env = run_evaluation(RunRequest {
            config: Some(project_root.join("eval.toml")),
            suite: Some("suite".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("env-root".to_string()),
            store_root: None,
            output_root: None,
        })
        .expect("env root");
        assert_eq!(by_env.artifact_root, env_root.join("runs/bad/env-root"));
    }

    let bypass_root = temp.path().join("bypass-store");
    let external = temp.path().join("external");
    let bypass = run_evaluation(RunRequest {
        config: Some(project_root.join("eval.toml")),
        suite: Some("suite".to_string()),
        agent: Some("fake-pass".to_string()),
        run_id: Some("external-run".to_string()),
        store_root: Some(bypass_root.clone()),
        output_root: Some(external.clone()),
    })
    .expect("external run");
    assert_eq!(bypass.artifact_root, external.join("external-run"));
    assert!(
        !bypass_root.join("index.json").exists(),
        "explicit output-root should not register in EvalStore"
    );

    let legacy_root = temp.path().join("legacy-project");
    fs::create_dir_all(&legacy_root).expect("legacy dir");
    write_minimal_project(&legacy_root, 1, r#"agents = ["fake-pass"]"#);
    fs::write(
        legacy_root.join("eval.toml"),
        "schema_version = 1\nname = \"legacy\"\noutput_root = \"legacy-runs\"\n",
    )
    .expect("legacy manifest");
    let namespace_store = temp.path().join("namespace-store");
    let legacy = run_evaluation(RunRequest {
        config: Some(legacy_root.join("eval.toml")),
        suite: Some("suite".to_string()),
        agent: Some("fake-pass".to_string()),
        run_id: Some("legacy-run".to_string()),
        store_root: Some(namespace_store.clone()),
        output_root: None,
    })
    .expect("legacy run");
    assert_eq!(
        legacy.artifact_root,
        namespace_store.join("legacy-runs/legacy-run")
    );
    assert!(namespace_store.join("legacy-runs/latest.json").is_file());

    fs::write(
        legacy_root.join("eval.toml"),
        "schema_version = 1\nname = \"legacy\"\noutput_root = \"../outside\"\n",
    )
    .expect("invalid namespace manifest");
    let invalid = run_evaluation(RunRequest {
        config: Some(legacy_root.join("eval.toml")),
        suite: Some("suite".to_string()),
        agent: Some("fake-pass".to_string()),
        run_id: Some("invalid".to_string()),
        store_root: Some(namespace_store),
        output_root: None,
    })
    .expect_err("invalid namespace");
    assert!(invalid.to_string().contains("output_root"));
}

#[test]
pub(crate) fn eval_store_index_fallback_latest_dataset_import_and_dashboard() {
    let _lock = ENV_LOCK.lock().expect("env lock");
    let _env = set_env_var("PEVAL_ROOT", None);
    let temp = tempfile::tempdir().expect("temp");
    let store_root = temp.path().join("store");
    let project = fixture_project();

    let failed = run_evaluation(RunRequest {
        config: Some(project.join("eval.toml")),
        suite: Some("rust-swe".to_string()),
        agent: None,
        run_id: Some("store-failed".to_string()),
        store_root: Some(store_root.clone()),
        output_root: None,
    })
    .expect("failed run");
    assert_eq!(failed.status, RunStatus::Failed);
    let passed = run_evaluation(RunRequest {
        config: Some(project.join("eval.toml")),
        suite: Some("rust-swe".to_string()),
        agent: Some("fake-pass".to_string()),
        run_id: Some("store-passed".to_string()),
        store_root: Some(store_root.clone()),
        output_root: None,
    })
    .expect("passed run");
    assert_eq!(passed.status, RunStatus::Passed);

    let loaded = EvalProject::load(&project).expect("project");
    let store = EvalStore::new(store_root.clone());
    let namespace = loaded.namespace().expect("namespace");
    let failed_latest = store
        .resolve_run_selector(
            Some(&namespace),
            Path::new("latest"),
            &RunSelectorFilters {
                suite: Some("rust-swe".to_string()),
                agent: None,
                status: Some(RunStatusFilter::Failed),
            },
        )
        .expect("latest failed");
    assert_eq!(failed_latest, failed.artifact_root);

    fs::write(store_root.join("index.json"), "{not-json").expect("corrupt index");
    let fallback_runs = store.list_runs().expect("fallback scan");
    assert!(fallback_runs.iter().any(|run| run.run_id == "store-failed"));
    assert!(fallback_runs.iter().any(|run| run.run_id == "store-passed"));

    let payload = temp.path().join("tasks.jsonl");
    fs::write(&payload, "{\"prompt\":\"x\"}\n").expect("payload");
    let dataset = import_dataset(DatasetImportRequest {
        store_root: Some(store_root.clone()),
        path: payload.clone(),
        id: Some("GDPVal Mini".to_string()),
        name: None,
        kind: Some("gdpval".to_string()),
        loader: Some("jsonl".to_string()),
        split: Some("mini".to_string()),
        sample_limit: Some(1),
        cache_key: Some("gdpval-mini".to_string()),
        license: Some("local".to_string()),
        tags: vec!["fixture".to_string()],
        notes: Some("local reference".to_string()),
    })
    .expect("dataset import");
    assert_eq!(dataset.id, "gdpval_mini");
    assert!(dataset.payload_exists);
    assert!(
        store_root
            .join("datasets/gdpval_mini/dataset.toml")
            .is_file()
    );

    let dashboard = fs::read_to_string(store_root.join("dashboard.html")).expect("dashboard");
    assert!(dashboard.contains("Evaluation results center"));
    assert!(dashboard.contains("gdpval_mini"));
    assert!(!dashboard.contains("scorer_finished"));

    let report = fs::read_to_string(passed.artifact_root.join("report.html")).expect("report");
    assert!(report.contains("Psychevo evaluation report"));
    assert!(report.contains("trajectory"));
    assert!(!report.contains("case execution started"));
}

#[test]
pub(crate) fn scorer_failure_malformed_json_and_timeout_are_classified() {
    let temp = tempfile::tempdir().expect("temp");
    let project = create_scorer_project(temp.path());
    let summary = run_evaluation(RunRequest {
        config: Some(project.join("eval.toml")),
        suite: Some("scorer-suite".to_string()),
        agent: Some("fake-pass".to_string()),
        run_id: Some("scorer-cases".to_string()),
        store_root: None,
        output_root: Some(temp.path().join("runs")),
    })
    .expect("run");
    let statuses = summary
        .cases
        .iter()
        .map(|case| (case.task_id.as_str(), case.status))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(statuses["scorer-success"], CaseStatus::Passed);
    assert_eq!(statuses["scorer-failure"], CaseStatus::ScorerFailed);
    assert_eq!(statuses["scorer-malformed"], CaseStatus::ScorerFailed);
    assert_eq!(statuses["scorer-timeout"], CaseStatus::Timeout);
}

#[test]
pub(crate) fn psychevo_adapter_preserves_runtime_observation_events_in_trajectory() {
    let temp = tempfile::tempdir().expect("temp");
    let project = create_psychevo_trace_project(temp.path());
    let summary = run_evaluation(RunRequest {
        config: Some(project.join("eval.toml")),
        suite: Some("trace-suite".to_string()),
        agent: Some("psychevo-trace".to_string()),
        run_id: Some("trace-run".to_string()),
        store_root: Some(temp.path().join("store")),
        output_root: None,
    })
    .expect("run");
    assert_eq!(summary.status, RunStatus::Passed);
    let trajectory = fs::read_to_string(
        summary
            .artifact_root
            .join("cases/trace-suite__trace-task__psychevo-trace/trajectory.jsonl"),
    )
    .expect("trajectory");
    assert!(trajectory.contains("\"kind\":\"psychevo_run_start\""));
    assert!(trajectory.contains("\"kind\":\"psychevo_tool_execution_start\""));
    assert!(trajectory.contains("\"raw_event\":{\"session_id\":\"trace-session\""));
    assert!(trajectory.contains("agent stderr line"));
}

#[test]
pub(crate) fn cli_smoke_covers_all_commands() {
    let _lock = ENV_LOCK.lock().expect("env lock");
    let _env = set_env_var("PEVAL_ROOT", None);
    let temp = tempfile::tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let user_home = temp.path().join("user-home");
    fs::create_dir_all(&user_home).expect("user home");
    let _psychevo_home = set_env_var("PSYCHEVO_HOME", Some(&psychevo_home));
    let _home = set_env_var("HOME", Some(&user_home));

    let project = fixture_project();
    let config = project.join("eval.toml");
    let store_root = temp.path().join("cli-store");

    let init = run_cli_from([
        "peval",
        "init",
        "--root",
        store_root.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(init.code, 0, "{}", init.stderr);
    assert!(init.stdout.contains("cli-store"));

    let removed_project = run_cli_from(["peval", "check", "--project", project.to_str().unwrap()]);
    assert_eq!(removed_project.code, 2);

    let doctor = run_cli_from(["peval", "doctor", "-c", config.to_str().unwrap(), "--json"]);
    assert_eq!(doctor.code, 0, "{}", doctor.stderr);
    let list = run_cli_from(["peval", "list", "-c", config.to_str().unwrap(), "--json"]);
    assert_eq!(list.code, 0, "{}", list.stderr);
    assert!(!list.stdout.to_ascii_lowercase().contains("csv"));
    let check = run_cli_from([
        "peval",
        "check",
        "-c",
        config.to_str().unwrap(),
        "--suite",
        "rust-swe",
        "--json",
    ]);
    assert_eq!(check.code, 0, "{}", check.stderr);
    let run = run_cli_from([
        "peval",
        "run",
        "-c",
        config.to_str().unwrap(),
        "--suite",
        "rust-swe",
        "--agent",
        "fake-pass",
        "--run-id",
        "cli-smoke",
        "--output-root",
        temp.path().to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let run_root = temp.path().join("cli-smoke");
    let failing_run = run_cli_from([
        "peval",
        "run",
        "-c",
        config.to_str().unwrap(),
        "--suite",
        "rust-swe",
        "--run-id",
        "cli-smoke-failing-suite",
        "--output-root",
        temp.path().to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(failing_run.code, 1);
    assert!(failing_run.stdout.contains("\"failed_cases\": 1"));
    let report = run_cli_from([
        "peval",
        "report",
        "--run-root",
        run_root.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(report.code, 0, "{}", report.stderr);
    let compare = run_cli_from([
        "peval",
        "compare",
        run_root.to_str().unwrap(),
        run_root.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(compare.code, 0, "{}", compare.stderr);
    let replay = run_cli_from([
        "peval",
        "replay",
        "--run-root",
        run_root.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(replay.code, 0, "{}", replay.stderr);

    let payload = temp.path().join("dataset.jsonl");
    fs::write(&payload, "{\"prompt\":\"hello\"}\n").expect("dataset payload");
    let dataset = run_cli_from([
        "peval",
        "dataset",
        "import",
        payload.to_str().unwrap(),
        "--id",
        "cli-data",
        "--kind",
        "jsonl",
        "--json",
    ]);
    assert_eq!(dataset.code, 0, "{}", dataset.stderr);
    assert!(dataset.stdout.contains("\"id\": \"cli-data\""));

    let store_run = run_cli_from([
        "peval",
        "run",
        "-c",
        config.to_str().unwrap(),
        "--suite",
        "rust-swe",
        "--agent",
        "fake-pass",
        "--run-id",
        "cli-store-run",
        "--json",
    ]);
    assert_eq!(store_run.code, 0, "{}", store_run.stderr);
    assert!(
        store_root
            .join("runs/local-rust-swe/cli-store-run/report.html")
            .is_file()
    );
    assert!(store_root.join("dashboard.html").is_file());

    let list_runs = run_cli_from(["peval", "list", "--kind", "runs", "--json"]);
    assert_eq!(list_runs.code, 0, "{}", list_runs.stderr);
    assert!(list_runs.stdout.contains("cli-store-run"));

    let list_datasets = run_cli_from(["peval", "list", "--kind", "datasets", "--json"]);
    assert_eq!(list_datasets.code, 0, "{}", list_datasets.stderr);
    assert!(list_datasets.stdout.contains("cli-data"));

    let latest_report = run_cli_from([
        "peval",
        "report",
        "-c",
        config.to_str().unwrap(),
        "--run-root",
        "latest",
        "--agent",
        "fake-pass",
        "--status",
        "passed",
        "--format",
        "json",
    ]);
    assert_eq!(latest_report.code, 0, "{}", latest_report.stderr);
    assert!(
        latest_report
            .stdout
            .contains("\"run_id\": \"cli-store-run\"")
    );

    let latest_compare = run_cli_from([
        "peval",
        "compare",
        "latest",
        "local-rust-swe/cli-store-run",
        "-c",
        config.to_str().unwrap(),
        "--agent",
        "fake-pass",
        "--status",
        "passed",
        "--json",
    ]);
    assert_eq!(latest_compare.code, 0, "{}", latest_compare.stderr);

    let latest_replay = run_cli_from([
        "peval",
        "replay",
        "--run-root",
        "latest",
        "--agent",
        "fake-pass",
        "--status",
        "passed",
        "--json",
    ]);
    assert_eq!(latest_replay.code, 0, "{}", latest_replay.stderr);
}

pub(crate) fn write_minimal_project(root: &Path, schema_version: u32, suite_extra: &str) {
    fs::create_dir_all(root.join("agents")).expect("agents");
    fs::create_dir_all(root.join("suites")).expect("suites");
    fs::create_dir_all(root.join("tasks/task/workspace")).expect("workspace");
    fs::write(
        root.join("eval.toml"),
        format!("schema_version = {schema_version}\nname = \"bad\"\n"),
    )
    .expect("project");
    fs::write(
        root.join("agents/fake-pass.toml"),
        "schema_version = 1\nid = \"fake-pass\"\nkind = \"fake\"\n[fake]\nbehavior = \"pass\"\n",
    )
    .expect("agent");
    fs::write(
        root.join("suites/suite.toml"),
        format!(
            "schema_version = 1\nid = \"suite\"\n{}\ntasks = [\"../tasks/task/task.toml\"]\n",
            suite_extra
        ),
    )
    .expect("suite");
    fs::write(
            root.join("tasks/task/task.toml"),
            "schema_version = 1\nid = \"task\"\n[prompt]\ntext = \"fix\"\n[workspace]\nsource = \"workspace\"\n[scorer]\ncommand = [\"sh\", \"score.sh\"]\n",
        )
        .expect("task");
    fs::write(
        root.join("tasks/task/score.sh"),
        "echo '{\"schema_version\":1,\"passed\":true,\"score\":1.0,\"message\":\"ok\"}'\n",
    )
    .expect("score");
}

pub(crate) fn create_scorer_project(root: &Path) -> PathBuf {
    fs::create_dir_all(root.join("agents")).expect("agents");
    fs::create_dir_all(root.join("suites")).expect("suites");
    fs::write(
        root.join("eval.toml"),
        "schema_version = 1\nname = \"scorer-project\"\n",
    )
    .expect("project");
    fs::write(
        root.join("agents/fake-pass.toml"),
        "schema_version = 1\nid = \"fake-pass\"\nkind = \"fake\"\n[fake]\nbehavior = \"pass\"\n",
    )
    .expect("agent");
    let tasks = [
        (
            "scorer-success",
            "echo '{\"schema_version\":1,\"passed\":true,\"score\":1.0,\"message\":\"ok\"}'\n",
        ),
        ("scorer-failure", "echo scorer failed >&2\nexit 7\n"),
        ("scorer-malformed", "echo not-json\n"),
        ("scorer-timeout", "sleep 2\n"),
    ];
    let mut task_paths = Vec::new();
    for (id, script) in tasks {
        let dir = root.join("tasks").join(id);
        fs::create_dir_all(dir.join("workspace")).expect("workspace");
        fs::write(dir.join("workspace/README.md"), id).expect("readme");
        fs::write(dir.join("score.sh"), script).expect("score");
        let timeout = if id == "scorer-timeout" {
            "timeout_seconds = 1\n"
        } else {
            ""
        };
        fs::write(
                dir.join("task.toml"),
                format!(
                    "schema_version = 1\nid = \"{id}\"\n[prompt]\ntext = \"score\"\n[workspace]\nsource = \"workspace\"\n[scorer]\ncommand = [\"sh\", \"score.sh\"]\n{timeout}"
                ),
            )
            .expect("task");
        task_paths.push(format!("\"../tasks/{id}/task.toml\""));
    }
    fs::write(
        root.join("suites/scorer.toml"),
        format!(
            "schema_version = 1\nid = \"scorer-suite\"\nagents = [\"fake-pass\"]\ntasks = [{}]\n",
            task_paths.join(", ")
        ),
    )
    .expect("suite");
    root.to_path_buf()
}

pub(crate) fn create_psychevo_trace_project(root: &Path) -> PathBuf {
    fs::create_dir_all(root.join("agents")).expect("agents");
    fs::create_dir_all(root.join("suites")).expect("suites");
    let task_dir = root.join("tasks/trace-task");
    fs::create_dir_all(task_dir.join("workspace")).expect("workspace");
    fs::write(
        root.join("eval.toml"),
        "schema_version = 1\nname = \"trace-project\"\nallow_live = true\n",
    )
    .expect("project");
    fs::write(
            root.join("agents/psychevo-trace.toml"),
            "schema_version = 1\nid = \"psychevo-trace\"\nkind = \"psychevo\"\n[psychevo]\ncommand = \"sh\"\nargs = [\"agent.sh\", \"{workspace}\", \"{prompt}\"]\n",
        )
        .expect("agent");
    fs::write(
            root.join("suites/trace.toml"),
            "schema_version = 1\nid = \"trace-suite\"\nagents = [\"psychevo-trace\"]\ntasks = [\"../tasks/trace-task/task.toml\"]\n",
        )
        .expect("suite");
    fs::write(
            task_dir.join("task.toml"),
            "schema_version = 1\nid = \"trace-task\"\n[prompt]\ntext = \"trace\"\n[workspace]\nsource = \"workspace\"\n[scorer]\ncommand = [\"sh\", \"score.sh\"]\n",
        )
        .expect("task");
    fs::write(
        task_dir.join("score.sh"),
        "echo '{\"schema_version\":1,\"passed\":true,\"score\":1.0,\"message\":\"ok\"}'\n",
    )
    .expect("score");
    fs::write(
        task_dir.join("agent.sh"),
        r#"#!/bin/sh
printf '%s\n' '{"type":"run_start","session_id":"trace-session","workdir":"workspace"}'
printf '%s\n' '{"type":"message_update","role":"assistant","delta":"editing"}'
printf '%s\n' '{"type":"tool_execution_start","tool_name":"write","tool_call_id":"call-1"}'
printf '%s\n' '{"type":"agent_end","outcome":"normal","final_answer":"done"}'
echo "agent stderr line" >&2
"#,
    )
    .expect("agent script");
    root.to_path_buf()
}
