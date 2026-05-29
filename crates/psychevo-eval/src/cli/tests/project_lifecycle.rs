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
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/pidx-fake-patch-add.eval.toml");
    let project = EvalProject::load(&template).expect("pidx template eval");
    assert_eq!(project.benchmark_id, "pidx-coding");
    assert_eq!(
        project.agents.keys().collect::<Vec<_>>(),
        vec![&"local-solver".to_string()]
    );
    let cases = check_project(
        &project,
        Some("pidx/smoke"),
        Some("pidx/patch-add"),
        Some("local-solver"),
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

#[test]
pub(crate) fn acp_agent_executes_stdio_session() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("acp-coding");
    fs::create_dir_all(&root).expect("project root");
    write_local_task(&root, "rust-swe-add", "swe-style");
    fs::write(
        root.join("benchmark.toml"),
        r#"schema_version = 5
id = "acp-coding"
name = "acp-coding"

[[sources.peval_agent]]
id = "local"
path = "tasks"

[[sources.peval_agent.sets]]
id = "rust-swe"
include = ["rust-swe-add"]
"#,
    )
    .expect("benchmark");
    fs::write(
        root.join("tasks").join("rust-swe-add").join("mock-acp.py"),
        r#"import json
import pathlib
import sys

cwd = None

def send(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"protocolVersion": 1, "agentCapabilities": {}, "authMethods": []}})
    elif method == "session/new":
        cwd = pathlib.Path(message["params"]["cwd"])
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"sessionId": "mock-session"}})
    elif method == "session/prompt":
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock-session", "update": {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "inspect status"}}}})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock-session", "update": {"sessionUpdate": "tool_call", "toolCallId": "call-1", "kind": "edit", "title": "Tool: edit", "status": "in_progress", "rawInput": {"path": "status.txt"}}}})
        (cwd / "status.txt").write_text("fixed")
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock-session", "update": {"sessionUpdate": "tool_call_update", "toolCallId": "call-1", "title": "Tool: edit", "status": "completed", "rawOutput": {"diff": "diff --git a/status.txt b/status.txt\n--- a/status.txt\n+++ b/status.txt\n@@ -1 +1 @@\n-pending\n+fixed\n", "success": True}}}})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock-session", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "done"}}}})
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"stopReason": "end_turn"}})
        break
    else:
        send({"jsonrpc": "2.0", "id": message["id"], "result": {}})
"#,
    )
    .expect("mock acp");
    fs::write(
        root.join("eval.toml"),
        r#"schema_version = 5
id = "acp-coding-eval"
name = "acp coding eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["mock-acp"]
sets = ["local/rust-swe"]

[[agents]]
id = "mock-acp"
kind = "acp"

[agents.acp]
command = "python3"
args = ["mock-acp.py"]
timeout_seconds = 10
"#,
    )
    .expect("eval");

    let run = run_evaluation(RunRequest {
        config: Some(root.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: Some("mock-acp".to_string()),
        overwrite: false,
        store_root: Some(init_workspace(temp.path().join("evals"))),
        output_root: None,
        include_artifacts: Vec::new(),
    })
    .expect("acp run");
    assert_eq!(run.status, RunStatus::Passed);
    assert_eq!(run.passed_cells, 1);

    let view = build_view(ViewRequest {
        config: Some(root.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(init_workspace(temp.path().join("view-evals"))),
        path: Some(run.cells[0].cell_root.clone()),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![ViewInclude::Atif, ViewInclude::Diff],
    })
    .expect("acp view");
    let atif = &view.atif[0].trajectory;
    assert_eq!(atif.schema_version, "ATIF-v1.7");
    let agent_step = atif
        .steps
        .iter()
        .find(|step| step.source == "agent")
        .expect("agent step");
    assert_eq!(
        agent_step.reasoning_content.as_deref(),
        Some("inspect status")
    );
    assert_eq!(agent_step.tool_calls[0].function_name, "edit");
    assert!(
        agent_step
            .observation
            .as_ref()
            .expect("observation")
            .results[0]
            .content
            .as_ref()
            .expect("content")
            .to_string()
            .contains("status.txt")
    );
}

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

    let markdown = run_view(ViewArgs {
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
    assert!(
        payload["matrix"]["cells"][0]["matrix_cell_key"]
            .as_str()
            .is_some()
    );
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
    assert!(!html.contains("trajectory.jsonl"));
}

#[test]
pub(crate) fn view_output_optional_path_defaults_to_mirrored_workspace_views() {
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

    let default_html = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        path: None,
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary,matrix".to_string()],
        format: None,
        output: Some(None),
    })
    .expect("default html output");
    let default_html_path = store_root.join("views/test-coding/index.html");
    assert_eq!(
        default_html.stdout,
        format!("wrote {}\n", default_html_path.display())
    );
    assert!(
        fs::read_to_string(default_html_path)
            .expect("default html")
            .contains("<!doctype html>")
    );

    let scoped_json = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        path: Some(PathBuf::from("runs/test-coding/fake-pass")),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary,matrix".to_string()],
        format: Some(ViewFormat::Json),
        output: Some(None),
    })
    .expect("default json output");
    let scoped_json_path = store_root.join("views/test-coding/fake-pass/index.json");
    assert_eq!(
        scoped_json.stdout,
        format!("wrote {}\n", scoped_json_path.display())
    );
    let scoped_payload: Value =
        serde_json::from_str(&fs::read_to_string(scoped_json_path).expect("scoped json"))
            .expect("json payload");
    assert_eq!(scoped_payload["summary"]["total_trials"], 1);

    let explicit_output = temp.path().join("nested").join("explicit.md");
    let explicit = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        path: Some(run.cells[0].cell_root.clone()),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary".to_string()],
        format: None,
        output: Some(Some(explicit_output.clone())),
    })
    .expect("explicit output");
    assert_eq!(
        explicit.stdout,
        format!("wrote {}\n", explicit_output.display())
    );
    assert!(
        fs::read_to_string(explicit_output)
            .expect("explicit markdown")
            .contains("# peval view")
    );

    let external = temp.path().join("external");
    fs::create_dir_all(&external).expect("external");
    let err = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root),
        path: Some(external),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary".to_string()],
        format: None,
        output: Some(None),
    })
    .expect_err("external default output fails");
    assert!(format!("{err:#}").contains("pass -o PATH"));
}

#[test]
pub(crate) fn view_output_cli_accepts_short_alias_and_optional_value() {
    let view = Cli::try_parse_from(["peval", "view", "-o"]).expect("-o parses");
    let Commands::View(args) = view.command else {
        panic!("expected view command");
    };
    assert_eq!(args.output, Some(None));

    let view = Cli::try_parse_from(["peval", "view", "-o", "out.html"]).expect("-o path parses");
    let Commands::View(args) = view.command else {
        panic!("expected view command");
    };
    assert_eq!(args.output, Some(Some(PathBuf::from("out.html"))));

    let view = Cli::try_parse_from(["peval", "view", "--output"]).expect("--output parses");
    let Commands::View(args) = view.command else {
        panic!("expected view command");
    };
    assert_eq!(args.output, Some(None));
}

#[test]
pub(crate) fn view_include_all_expands_to_stable_diagnostic_set() {
    let includes = parse_view_includes(&["summary,all".to_string(), "usage".to_string()])
        .expect("all include parses");
    assert_eq!(includes, all_view_includes());

    let view = Cli::try_parse_from(["peval", "view", "-i", "all"]).expect("-i all parses");
    let Commands::View(args) = view.command else {
        panic!("expected view command");
    };
    assert_eq!(args.include, vec!["all".to_string()]);
}

#[test]
pub(crate) fn view_all_outputs_full_diagnostics_with_bounded_previews() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let solver = fixture.join("solver.sh");
    fs::write(
        &solver,
        r#"printf '%s\n' '{"type":"user_message","message":"complete task"}'
printf '%s\n' '{"type":"assistant_message","message":"I will write status","usage":{"input_tokens":3,"output_tokens":4,"cache_read_tokens":1,"cost_usd":0.01}}'
printf '%s\n' '{"type":"tool_call","tool_call_id":"call-1","function_name":"write_file","arguments":{"path":"status.txt"}}'
printf '%s\n' '{"type":"tool_result","tool_call_id":"call-1","result":{"diff":"diff --git a/status.txt b/status.txt\n--- a/status.txt\n+++ b/status.txt\n@@ -1 +1 @@\n-pending\n+fixed\n","ok":true}}'
printf fixed > status.txt
"#,
    )
    .expect("solver");
    fs::write(
        fixture.join("eval.toml"),
        format!(
            r#"schema_version = 5
id = "test-coding-eval"
name = "test-coding eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["jsonl-solver"]
sets = ["local/rust-swe"]

[[agents]]
id = "jsonl-solver"
kind = "command"
command = {{ command = "sh", args = ["{}"] }}
"#,
            solver.display()
        ),
    )
    .expect("eval");
    let store_root = init_workspace(temp.path().join("evals"));
    let run = run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: Some("jsonl-solver".to_string()),
        overwrite: false,
        store_root: Some(store_root.clone()),
        output_root: None,
        include_artifacts: vec!["workspace".to_string()],
    })
    .expect("run");
    let cell_root = &run.cells[0].cell_root;
    fs::create_dir_all(cell_root.join("logs")).expect("logs");
    let big_log = format!(
        "visible-start\n{}secret-tail\n",
        "a".repeat(2 * 1024 * 1024)
    );
    fs::write(cell_root.join("logs/big.log"), big_log).expect("big log");

    let json = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        path: Some(cell_root.clone()),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["all".to_string()],
        format: Some(ViewFormat::Json),
        output: None,
    })
    .expect("json view");
    let payload: Value = serde_json::from_str(&json.stdout).expect("json payload");
    assert_eq!(payload["schema_version"], VIEW_SCHEMA_VERSION);
    assert_eq!(
        payload["includes"],
        json!([
            "summary",
            "matrix",
            "usage",
            "warnings",
            "artifacts",
            "trajectory",
            "atif",
            "logs",
            "analysis",
            "diff"
        ])
    );
    assert!(json.stdout.contains("\"trial_key\""));
    assert!(json.stdout.contains("\"matrix_cell_key\""));
    assert!(!json.stdout.contains("\"cell_key\""));
    assert!(
        payload["trials"][0]["trial_key"]
            .as_str()
            .unwrap()
            .ends_with(":t001")
    );
    assert!(
        payload["matrix"]["cells"][0]["trial_keys"]
            .as_array()
            .unwrap()
            .len()
            == 1
    );
    assert!(payload["artifacts"][0]["files"].as_array().unwrap().len() > 3);
    assert!(
        !payload["trajectory"][0]["steps"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        payload["atif"][0]["trajectory"]["schema_version"],
        "ATIF-v1.7"
    );
    assert_eq!(
        payload["atif"][0]["trajectory"]["steps"][0]["source"],
        "user"
    );
    assert_eq!(
        payload["analysis"][0]["status"], "missing",
        "analysis should not run providers"
    );
    assert_eq!(payload["diff"][0]["source"], "trajectory");
    assert!(
        payload["diff"][0]["preview"]
            .as_str()
            .expect("diff preview")
            .contains("status.txt")
    );

    let markdown = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        path: Some(cell_root.clone()),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["all".to_string()],
        format: Some(ViewFormat::Markdown),
        output: None,
    })
    .expect("markdown view");
    for heading in [
        "## Artifacts",
        "## Trajectory",
        "## ATIF",
        "## Logs",
        "## Analysis",
        "## Diff",
    ] {
        assert!(markdown.stdout.contains(heading), "missing {heading}");
    }
    assert!(!markdown.stdout.contains("secret-tail"));

    let html = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        path: Some(cell_root.clone()),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["all".to_string()],
        format: Some(ViewFormat::Html),
        output: None,
    })
    .expect("html view");
    assert!(html.stdout.contains("<h2>Trajectory</h2>"));
    assert!(html.stdout.contains("<h2>Diff</h2>"));
    assert!(!html.stdout.contains("secret-tail"));

    let escape = cell_root.parent().expect("cell parent").join("escape.txt");
    fs::write(&escape, "outside").expect("escape file");
    let err = safe_artifact_path(cell_root, Path::new("../escape.txt"))
        .expect_err("outside path rejected");
    assert!(format!("{err:#}").contains("escapes cell root"));

    fs::write(
        cell_root.join("artifact.patch"),
        "diff --git a/artifact b/artifact\n+artifact\n",
    )
    .expect("patch");
    let patch_view = build_view(ViewRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root),
        path: Some(cell_root.clone()),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![ViewInclude::Diff],
    })
    .expect("patch view");
    assert_eq!(patch_view.diff[0].source, "artifact");
    assert_eq!(
        patch_view.diff[0]
            .data_ref
            .as_ref()
            .map(|data_ref| data_ref.relative_path.as_path()),
        Some(Path::new("artifact.patch"))
    );
}

#[test]
pub(crate) fn view_timeline_include_is_removed() {
    let err = parse_view_includes(&["timeline".to_string()]).expect_err("timeline fails");
    assert!(format!("{err:#}").contains("invalid view include `timeline`"));
}

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
        path: None,
        task_set: Some("local/rust-swe".to_string()),
        agent: Some("fake-pass".to_string()),
        task: None,
        status: None,
        group_by: Vec::new(),
        include: all_view_includes(),
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
            path: None,
            task_set: Some("local/rust-swe".to_string()),
            agent: Some("fake-pass".to_string()),
            task: None,
            status: None,
            group_by: vec![ViewGroupBy::Agent],
            include: vec![ViewInclude::Summary, ViewInclude::Matrix],
        })
        .expect("read-only view");
    assert_eq!(view.summary.total_trials, 1);
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
        path: None,
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![ViewInclude::Summary, ViewInclude::Matrix],
    })
    .expect("view ignores legacy");
    assert_eq!(view.summary.total_trials, 0);
}

#[test]
pub(crate) fn official_sources_require_compatible_bridges() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("tau2-eval");
    fs::create_dir_all(&root).expect("project root");
    fs::write(
        root.join("benchmark.toml"),
        r#"schema_version = 5
id = "tau2-declaration"
name = "tau2-declaration"

[[sources.tau2]]
id = "tau"
root = "."
domain = "airline"
"#,
    )
    .expect("benchmark");
    fs::write(
        root.join("eval.toml"),
        r#"schema_version = 5
id = "tau2-declaration-eval"
name = "tau2 declaration eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["fake-pass"]
sets = ["tau"]

[[agents]]
id = "fake-pass"
kind = "command"

[agents.command]
command = "sh"
args = ["-c", ":"]
"#,
    )
    .expect("eval");
    let project = EvalProject::load(root.join("eval.toml")).expect("external project");
    let err = check_project(&project, None, None, None).expect_err("official bridge required");
    assert!(
        format!("{err:#}").contains("incompatible_source_agent"),
        "{err:#}"
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
        include_artifacts: Vec::new(),
    })
    .expect_err("official source run should fail");
    assert_eq!(denied.code, "incompatible_source_agent");
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
        r#"schema_version = 5
id = "test-coding"
name = "test-coding"

[[sources.peval_agent]]
id = "local"
path = "tasks"
verifier_timeout_seconds = 600

[[sources.peval_agent.sets]]
id = "rust-swe"
include = ["rust-swe-add"]
"#,
    )
    .expect("benchmark");
    fs::write(
        root.join("eval.toml"),
        r#"schema_version = 5
id = "test-coding-eval"
name = "test-coding eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["fake-pass", "fake-fail"]
sets = ["local/rust-swe"]

[[agents]]
id = "fake-pass"
kind = "command"
command = { command = "sh", args = ["-c", "printf fixed > status.txt"] }

[[agents]]
id = "fake-fail"
kind = "command"
command = { command = "sh", args = ["-c", ":"] }
"#,
    )
    .expect("eval");
    write_local_task(root, "rust-swe-add", "swe-style");
    root.to_path_buf()
}

pub(crate) fn write_local_task(root: &Path, id: &str, kind: &str) {
    let dir = root.join("tasks").join(id);
    fs::create_dir_all(dir.join("environment")).expect("environment");
    fs::create_dir_all(dir.join("tests")).expect("tests");
    fs::write(dir.join("environment/status.txt"), "pending").expect("status");
    fs::write(
        dir.join("task.toml"),
        format!("name = \"complete {id}\"\nkind = \"{kind}\"\n"),
    )
    .expect("task toml");
    fs::write(dir.join("instruction.md"), format!("complete {id}\n")).expect("instruction");
    fs::write(
        dir.join("tests/test.sh"),
        format!(
            r#"set -e
test "$PEVAL_TASK_ID" = "local/{id}"
test "$PEVAL_NATIVE_TASK_ID" = "{id}"
test "$PEVAL_SOURCE_ID" = "local"
test -d "$PEVAL_WORKSPACE"
test -d "$PEVAL_TASK_DIR"
test -d "$PEVAL_LOGS"
test "$(cat status.txt 2>/dev/null || true)" = fixed
mkdir -p "$PEVAL_LOGS/verifier"
printf '{{"message":"env checked","score":1.0}}' > "$PEVAL_LOGS/verifier/result.json"
"#
        ),
    )
    .expect("verifier");
}
