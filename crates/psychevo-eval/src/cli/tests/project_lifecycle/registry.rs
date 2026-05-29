use super::support::*;
#[allow(unused_imports)]
use super::*;
use pretty_assertions::assert_eq;

#[test]
pub(crate) fn eval_config_resolves_benchmark_and_inline_agents() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let project = EvalProject::load(fixture.join("eval.toml")).expect("eval config load");

    assert_eq!(project.name, "test-coding eval");
    assert_eq!(project.benchmark_id, "test-coding");
    assert_eq!(project.agents["fake-fail"].kind, AgentKind::Command);

    let cases =
        check_project(&project, Some("local/rust-swe"), None, None).expect("check selected matrix");
    let ids = cases
        .iter()
        .map(|case| case.case_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "local_rust-swe__local_rust-swe-add__fake-fail",
            "local_rust-swe__local_rust-swe-add__fake-pass",
        ]
    );
}

#[test]
pub(crate) fn pidx_benchmark_is_benchmark_only_and_templates_select_agents() {
    let manifest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benchmarks/pidx-coding/benchmark.toml");
    let benchmark = BenchmarkManifest::load(&manifest).expect("pidx benchmark");
    assert_eq!(benchmark.id, "pidx-coding");
    assert_eq!(benchmark.task_sets["pidx"].tasks.len(), 3);
    assert_eq!(benchmark.task_sets["pidx/smoke"].tasks.len(), 2);

    let template =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/pidx-psychevo-acp.eval.toml");
    let raw_template = fs::read_to_string(&template).expect("template");
    assert!(raw_template.contains("kind = \"psychevo-acp\""));
    assert!(!raw_template.contains("kind = \"psychevo\""));
    assert!(!raw_template.contains("kind = \"command\""));
    assert!(raw_template.contains("id = \"pidx-coding\""));

    let project = EvalProject::load(&template).expect("pidx template eval");
    assert_eq!(project.benchmark_id, "pidx-coding");
    assert_eq!(project.agents["psychevo-acp"].kind, AgentKind::PsychevoAcp);
    assert_eq!(
        project.agents.keys().cloned().collect::<Vec<_>>(),
        vec!["psychevo-acp".to_string()]
    );
    let cases = check_project(
        &project,
        Some("pidx/smoke"),
        Some("pidx/patch-add"),
        Some("psychevo-acp"),
    )
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
kind = "command"

[agents.command]
command = "sh"
args = ["-c", ":"]

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
    assert_eq!(one_off.agents["fake-pass"].kind, AgentKind::Command);

    let eval = load_eval_config(&fixture.join("eval.toml"), Some(store_root.clone()))
        .expect("eval config wins registry");
    assert_eq!(eval.agents["fake-pass"].command.args.len(), 2);

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
        "local/rust-swe",
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
        "local/rust-swe",
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
        r#"schema_version = 5
id = "duplicate-agents"
name = "duplicate agents"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["fake-pass"]
sets = ["local/rust-swe"]

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
pub(crate) fn legacy_task_sets_selection_key_is_rejected() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    fs::write(
        fixture.join("legacy-task-sets.eval.toml"),
        r#"schema_version = 5
id = "legacy-task-sets"
name = "legacy task sets"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["fake-pass"]
task_sets = ["local/rust-swe"]

[[agents]]
id = "fake-pass"
kind = "command"
command = { command = "sh", args = ["-c", ":"] }
"#,
    )
    .expect("legacy eval");

    let err = EvalProject::load(fixture.join("legacy-task-sets.eval.toml"))
        .expect_err("legacy task_sets key should fail");
    assert!(format!("{err:#}").contains("unknown field `task_sets`"));
}

#[test]
pub(crate) fn check_live_flag_is_reported_without_executing_cases() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let config = fixture.join("eval.toml");
    let outcome = run_cli_from([
        "peval",
        "check",
        "--config",
        config.to_str().expect("utf8 path"),
        "--live",
        "--json",
    ]);
    assert_eq!(outcome.code, 0, "stderr: {}", outcome.stderr);
    let payload: Value = serde_json::from_str(&outcome.stdout).expect("json");
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["cases"], 2);
    assert_eq!(payload["live"], true);
}

#[test]
pub(crate) fn v5_source_sets_are_canonical_filtered_and_strict() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("manifest-v5");
    fs::create_dir_all(&root).expect("root");
    write_local_task(&root, "alpha-add", "coding-patch");
    write_local_task(&root, "beta-add", "coding-patch");
    write_local_task(&root, "tool-state", "stateful-tool-use");
    fs::write(
        root.join("benchmark.toml"),
        r#"schema_version = 5
id = "manifest-v5"
name = "manifest-v5"

[[sources.peval_agent]]
id = "Src"
path = "tasks"

[[sources.peval_agent.sets]]
id = "limited"
include = ["*-add", "tool-*"]
exclude = ["tool-*"]
limit = 1
"#,
    )
    .expect("benchmark");

    let benchmark = BenchmarkManifest::load(root.join("benchmark.toml")).expect("benchmark");
    assert_eq!(
        benchmark.task_sets["src"].tasks,
        vec![
            "src/alpha-add".to_string(),
            "src/beta-add".to_string(),
            "src/tool-state".to_string(),
        ]
    );
    assert_eq!(
        benchmark.task_sets["src/limited"].tasks,
        vec!["src/alpha-add".to_string()]
    );
    assert!(benchmark.tasks.contains_key("src/tool-state"));

    let duplicate_root = temp.path().join("duplicate-v5");
    fs::create_dir_all(&duplicate_root).expect("duplicate root");
    fs::write(
        duplicate_root.join("benchmark.toml"),
        r#"schema_version = 5
id = "duplicate"

[[sources.peval_agent]]
id = "Src"
path = "../manifest-v5/tasks"

[[sources.peval_agent]]
id = "src"
path = "../manifest-v5/tasks"
"#,
    )
    .expect("duplicate benchmark");
    let duplicate = BenchmarkManifest::load(duplicate_root.join("benchmark.toml"))
        .expect_err("duplicate source ids fail");
    assert!(format!("{duplicate:#}").contains("duplicate source id `src`"));

    let v4_root = temp.path().join("v4-benchmark");
    fs::create_dir_all(&v4_root).expect("v4 root");
    fs::write(
        v4_root.join("benchmark.toml"),
        r#"schema_version = 4
id = "old"

[evaluator]
kind = "local-coding"
"#,
    )
    .expect("v4 benchmark");
    let v4 = BenchmarkManifest::load(v4_root.join("benchmark.toml")).expect_err("v4 rejects");
    assert!(format!("{v4:#}").contains("schema_version 4"));
    assert!(format!("{v4:#}").contains("v5 authoring"));
}

#[test]
pub(crate) fn official_source_declarations_are_canonical_and_gated() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("official-sources");
    fs::create_dir_all(&root).expect("root");
    write_local_task(&root.join("harbor"), "case-b", "harbor");
    fs::create_dir_all(root.join("swe-root")).expect("swe root");
    fs::create_dir_all(root.join("tau-root")).expect("tau root");
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

[[sources.swe_bench]]
id = "swe"
root = "swe-root"
dataset = "princeton-nlp/SWE-bench_Lite"
split = "dev"

[[sources.tau2]]
id = "tau"
root = "tau-root"
domain = "airline"
split = "dev"
task_set = "smoke"
"#,
    )
    .expect("benchmark");

    let benchmark = BenchmarkManifest::load(root.join("benchmark.toml")).expect("benchmark");
    assert_eq!(
        benchmark.task_sets["hb/sample"].tasks,
        vec!["hb/case-b".to_string()]
    );
    assert!(
        benchmark
            .tasks
            .contains_key("swe/princeton-nlp_swe-bench_lite_dev")
    );
    assert!(benchmark.tasks.contains_key("tau/airline-dev-smoke"));

    fs::write(
        root.join("eval.toml"),
        r#"schema_version = 5
id = "official-declarations-eval"
name = "official declarations eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["local-agent"]
sets = ["hb/sample"]

[[agents]]
id = "local-agent"
kind = "command"

[agents.command]
command = "sh"
args = ["-c", ":"]
"#,
    )
    .expect("eval");
    let project = EvalProject::load(root.join("eval.toml")).expect("eval");
    let err = check_project(&project, None, None, None)
        .expect_err("local agent should not run official source directly");
    assert!(format!("{err:#}").contains("incompatible_source_agent"));
}
