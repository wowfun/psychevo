#[allow(unused_imports)]
pub(crate) use super::*;

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

pub(crate) fn expand_matrix(
    project: &EvalProject,
    task_set_filter: Option<&str>,
    task_filter: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<Vec<CasePlan>> {
    let mut plans = Vec::new();
    let task_sets = selected_task_sets(project, task_set_filter)?;
    for task_set in task_sets {
        let agent_ids = selected_agent_ids(project, agent_filter)?;
        let tasks = match load_task_set_tasks(project, &task_set, task_filter) {
            Ok(tasks) => tasks,
            Err(err) if task_filter.is_some() && task_set_filter.is_none() => {
                let message = format!("{err:#}");
                if message.contains("does not include selected task") {
                    continue;
                }
                return Err(err);
            }
            Err(err) => return Err(err),
        };
        for task in tasks {
            for agent_id in &agent_ids {
                let agent = project
                    .agents
                    .get(agent_id)
                    .with_context(|| {
                        format!("unknown agent `{agent_id}` in task set `{}`", task_set.id)
                    })?
                    .clone();
                let case_id = sanitize_id(&format!("{}__{}__{}", task_set.id, task.id, agent.id));
                plans.push(CasePlan {
                    case_id,
                    task_set: task_set.clone(),
                    task: task.clone(),
                    agent,
                });
            }
        }
    }
    if plans.is_empty() {
        bail!("no cases selected");
    }
    Ok(plans)
}

pub(crate) fn run_evaluation(request: RunRequest) -> Result<RunExecutionSummary> {
    validate_direct_benchmark_selection(
        request.benchmark.as_deref(),
        request.agent.as_deref(),
        request.task_set.as_deref(),
        request.task.as_deref(),
    )?;
    let project = load_project_from_selection(
        request.config.as_deref(),
        request.benchmark.as_deref(),
        request.store_root.clone(),
    )?;
    if !project.evaluator.run_supported() {
        bail!(
            "unsupported_evaluator: evaluator kind `{:?}` is not executable yet",
            project.evaluator.kind
        );
    }
    let cases = expand_matrix(
        &project,
        request.task_set.as_deref(),
        request.task.as_deref(),
        request.agent.as_deref(),
    )?;
    if cases.is_empty() {
        bail!("no cases selected");
    }
    for case in &cases {
        validate_case(case)?;
    }

    let explicit_output = request.output_root.is_some();
    let store = if explicit_output {
        None
    } else {
        Some(EvalStore::resolve(request.store_root)?)
    };
    let output_base = if let Some(path) = request.output_root {
        resolve_cli_path(&path)?.join("runs").join(project.slug())
    } else if let Some(store) = &store {
        store.cell_runs_root(&project)
    } else {
        unreachable!("explicit output-root is the only non-store run path")
    };
    fs::create_dir_all(&output_base)
        .with_context(|| format!("failed to create {}", output_base.display()))?;

    let mut cells = Vec::new();
    for case in cases {
        let fingerprint = cell_fingerprint(&project, &case)?;
        let cell_key = cell_key(&case, &fingerprint);
        let cell_root = output_base
            .join(sanitize_id(&case.agent.id))
            .join(sanitize_id(&case.task.id))
            .join(&cell_key);
        let mut action = CellRunAction::Executed;
        if !explicit_output
            && !request.overwrite
            && let Ok(existing) = read_cell_run(&cell_root)
            && existing.fingerprint == fingerprint
            && existing.case.status.is_terminal_reusable()
        {
            cells.push(RunExecutionCell {
                cell_key: existing.cell_key,
                fingerprint: existing.fingerprint,
                cell_root: existing.cell_root,
                task_set_id: existing.case.task_set_id,
                task_id: existing.case.task_id,
                agent_id: existing.case.agent_id,
                status: existing.case.status,
                action: CellRunAction::Reused,
            });
            continue;
        }
        if !explicit_output && request.overwrite && cell_root.exists() {
            action = CellRunAction::Overwritten;
        } else if !explicit_output && cell_root.exists() {
            action = CellRunAction::Retried;
        }
        let cell = execute_cell(&project, case, &cell_root, &cell_key, &fingerprint)?;
        cells.push(RunExecutionCell {
            cell_key: cell.cell_key,
            fingerprint: cell.fingerprint,
            cell_root: cell.cell_root,
            task_set_id: cell.case.task_set_id,
            task_id: cell.case.task_id,
            agent_id: cell.case.agent_id,
            status: cell.case.status,
            action,
        });
    }

    let passed_cells = cells
        .iter()
        .filter(|cell| cell.status == CaseStatus::Passed)
        .count();
    let failed_cells = cells.len().saturating_sub(passed_cells);
    let status = if failed_cells == 0 {
        RunStatus::Passed
    } else {
        RunStatus::Failed
    };
    let benchmark_slug = project.slug();
    Ok(RunExecutionSummary {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        project: project.name.clone(),
        benchmark: project.benchmark_id,
        benchmark_slug,
        selected_cells: cells.len(),
        executed_cells: cells
            .iter()
            .filter(|cell| cell.action == CellRunAction::Executed)
            .count(),
        reused_cells: cells
            .iter()
            .filter(|cell| cell.action == CellRunAction::Reused)
            .count(),
        overwritten_cells: cells
            .iter()
            .filter(|cell| cell.action == CellRunAction::Overwritten)
            .count(),
        retried_cells: cells
            .iter()
            .filter(|cell| cell.action == CellRunAction::Retried)
            .count(),
        failed_cells,
        passed_cells,
        status,
        cells,
    })
}

pub(crate) fn execute_cell(
    project: &EvalProject,
    case: CasePlan,
    cell_root: &Path,
    cell_key: &str,
    fingerprint: &str,
) -> Result<CellRun> {
    let parent = cell_root
        .parent()
        .with_context(|| format!("cell root has no parent: {}", cell_root.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let temp = parent.join(format!(".tmp-{}-{}", cell_key, Uuid::now_v7()));
    if temp.exists() {
        fs::remove_dir_all(&temp)
            .with_context(|| format!("failed to remove {}", temp.display()))?;
    }
    fs::create_dir_all(&temp).with_context(|| format!("failed to create {}", temp.display()))?;
    let started_at_ms = now_ms();
    let result = run_case(&temp, case)?;
    let finished_at_ms = now_ms();
    let cell = CellRun {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        benchmark: project.benchmark_id.clone(),
        benchmark_slug: project.slug(),
        cell_key: cell_key.to_string(),
        fingerprint: fingerprint.to_string(),
        cell_root: cell_root.to_path_buf(),
        started_at_ms,
        finished_at_ms,
        case: result,
    };
    write_json_pretty(&temp.join("run.json"), &cell)?;
    if cell_root.exists() {
        fs::remove_dir_all(cell_root)
            .with_context(|| format!("failed to replace {}", cell_root.display()))?;
    }
    fs::rename(&temp, cell_root).with_context(|| {
        format!(
            "failed to move {} to {}",
            temp.display(),
            cell_root.display()
        )
    })?;
    read_cell_run(cell_root)
}

pub(crate) fn cell_key(case: &CasePlan, fingerprint: &str) -> String {
    let short = fingerprint.chars().take(16).collect::<String>();
    sanitize_id(&format!(
        "{}__{}__{}__{}",
        case.task_set.id, case.task.id, case.agent.id, short
    ))
}

pub(crate) fn cell_fingerprint(project: &EvalProject, case: &CasePlan) -> Result<String> {
    let workspace_source = resolve_relative(&case.task.dir, &case.task.workspace.source);
    let payload = json!({
        "artifact_schema_version": ARTIFACT_SCHEMA_VERSION,
        "runner": "psychevo-eval-cell-v6",
        "benchmark": {
            "id": &project.benchmark_id,
            "name": &project.benchmark_name,
            "slug": project.slug(),
        },
        "evaluator": &project.evaluator,
        "task_set": {
            "id": &case.task_set.id,
        },
        "task": {
            "id": &case.task.id,
            "kind": &case.task.kind,
            "definition": serde_json::to_value(&case.task)?,
            "prompt": task_prompt(&case.task)?,
            "workspace": workspace_tree_hash(&workspace_source)?,
        },
        "agent": {
            "id": &case.agent.id,
            "kind": case.agent.kind,
            "model": agent_model(&case.agent),
            "fake": &case.agent.fake,
            "psychevo": &case.agent.psychevo,
            "opencode": &case.agent.opencode,
            "hermes": &case.agent.hermes,
        },
        "factors": {},
    });
    Ok(stable_hash_hex(&serde_json::to_string(&payload)?))
}

pub(crate) fn workspace_tree_hash(root: &Path) -> Result<String> {
    if !root.exists() {
        bail!("workspace source does not exist: {}", root.display());
    }
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hash = Fnv64::new();
    for (relative, path) in files {
        hash.add(relative.to_string_lossy().as_bytes());
        hash.add(&[0]);
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        hash.add(&bytes);
        hash.add(&[0]);
    }
    Ok(format!("{:016x}", hash.finish()))
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            files.push((relative, path));
        }
    }
    Ok(())
}

pub(crate) fn stable_hash_hex(value: &str) -> String {
    stable_hash_bytes(value.as_bytes())
}

pub(crate) fn stable_hash_bytes(value: &[u8]) -> String {
    let mut hash = Fnv64::new();
    hash.add(value);
    format!("{:016x}", hash.finish())
}

struct Fnv64(u64);

impl Fnv64 {
    fn new() -> Self {
        Self(0xcbf29ce484222325)
    }

    fn add(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}

pub(crate) fn import_dataset(request: DatasetImportRequest) -> Result<DatasetEntry> {
    let store = EvalStore::resolve(request.store_root)?;
    let input = resolve_cli_path(&request.path)?;
    let source = fs::canonicalize(&input)
        .with_context(|| format!("failed to resolve dataset path {}", input.display()))?;
    if !source.exists() {
        bail!("dataset path does not exist: {}", source.display());
    }

    let id = request.id.unwrap_or_else(|| {
        source
            .file_stem()
            .or_else(|| source.file_name())
            .and_then(|value| value.to_str())
            .map(slugify)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "dataset".to_string())
    });
    let id = slugify(&id);
    let dataset_dir = store.root.join("datasets").join(&id);
    fs::create_dir_all(&dataset_dir)
        .with_context(|| format!("failed to create {}", dataset_dir.display()))?;

    let payload_link = dataset_dir.join("payload");
    let payload = if link_dataset_payload(&source, &payload_link)? {
        PathBuf::from("payload")
    } else {
        source.clone()
    };
    let manifest = DatasetManifest {
        schema_version: INDEX_SCHEMA_VERSION,
        id: id.clone(),
        name: request.name.unwrap_or_else(|| id.clone()),
        kind: request.kind.unwrap_or_else(|| "local".to_string()),
        source: source.display().to_string(),
        payload,
        loader: request.loader,
        split: request.split,
        sample_limit: request.sample_limit,
        cache_key: request.cache_key,
        license: request.license,
        tags: request.tags,
        notes: request.notes,
    };
    write_toml_pretty(&dataset_dir.join("dataset.toml"), &manifest)?;
    store.refresh_after_dataset_change()?;
    read_dataset_entry(&dataset_dir.join("dataset.toml"))
}

pub(crate) fn run_case(artifact_root: &Path, case: CasePlan) -> Result<CaseResult> {
    let started = Instant::now();
    fs::create_dir_all(artifact_root)
        .with_context(|| format!("failed to create {}", artifact_root.display()))?;
    let result_rel = PathBuf::from("run.json");
    let trajectory_rel = PathBuf::from("trajectory.jsonl");
    let stdout_rel = PathBuf::from("evaluator.stdout");
    let stderr_rel = PathBuf::from("evaluator.stderr");

    let mut events = Vec::new();
    push_event(
        &mut events,
        &case.case_id,
        "case_started",
        "case execution started",
        json!({
            "task_set": case.task_set.id,
            "task": case.task.id,
            "agent": case.agent.id,
        }),
    );

    let workspace_source = resolve_relative(&case.task.dir, &case.task.workspace.source);
    let workspace_temp = tempfile::Builder::new()
        .prefix("peval-workspace-")
        .tempdir()
        .context("failed to create temporary case workspace")?;
    copy_dir(&workspace_source, workspace_temp.path()).with_context(|| {
        format!(
            "failed to copy task workspace {}",
            workspace_source.display()
        )
    })?;
    push_event(
        &mut events,
        &case.case_id,
        "workspace_prepared",
        "temporary workspace prepared",
        json!({ "workspace_source": workspace_source }),
    );

    let (status, score) = if let Err(err) = run_agent(&case, workspace_temp.path(), &mut events) {
        let score = ScoreResult {
            schema_version: EVALUATOR_RESULT_SCHEMA_VERSION,
            passed: false,
            score: None,
            message: format!("{err:#}"),
            details: Value::Null,
        };
        push_event(
            &mut events,
            &case.case_id,
            "agent_failed",
            &score.message,
            Value::Null,
        );
        fs::write(artifact_root.join(&stdout_rel), "").with_context(|| {
            format!(
                "failed to write {}",
                artifact_root.join(&stdout_rel).display()
            )
        })?;
        fs::write(artifact_root.join(&stderr_rel), score.message.as_bytes()).with_context(
            || {
                format!(
                    "failed to write {}",
                    artifact_root.join(&stderr_rel).display()
                )
            },
        )?;
        (CaseStatus::RuntimeFailed, score)
    } else {
        let (case_status, evaluator_score, evaluator_stdout, evaluator_stderr) =
            run_evaluator(&case, workspace_temp.path()).context("failed to run evaluator")?;
        fs::write(artifact_root.join(&stdout_rel), evaluator_stdout.as_bytes()).with_context(
            || {
                format!(
                    "failed to write {}",
                    artifact_root.join(&stdout_rel).display()
                )
            },
        )?;
        fs::write(artifact_root.join(&stderr_rel), evaluator_stderr.as_bytes()).with_context(
            || {
                format!(
                    "failed to write {}",
                    artifact_root.join(&stderr_rel).display()
                )
            },
        )?;
        push_event(
            &mut events,
            &case.case_id,
            "evaluator_finished",
            &evaluator_score.message,
            json!({
                "status": case_status,
                "passed": evaluator_score.passed,
            }),
        );
        (case_status, evaluator_score)
    };

    push_event(
        &mut events,
        &case.case_id,
        "case_finished",
        "case execution finished",
        json!({ "status": status }),
    );
    let duration_ms = started.elapsed().as_millis();
    let metrics = collect_case_metrics(&events, duration_ms);
    write_jsonl(&artifact_root.join(&trajectory_rel), &events)?;

    let result = CaseResult {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        identity: CaseIdentity {
            case_id: case.case_id.clone(),
            task_set_id: case.task_set.id.clone(),
            task_id: case.task.id.clone(),
            task_family: case.task.kind.clone(),
        },
        candidate: CandidateIdentity {
            agent_id: case.agent.id.clone(),
            adapter: case.agent.kind,
            model: agent_model(&case.agent),
        },
        factors: CaseFactors::default(),
        case_id: case.case_id,
        task_set_id: case.task_set.id,
        task_id: case.task.id,
        task_family: case.task.kind,
        agent_id: case.agent.id,
        status,
        failure_class: failure_class(status, &score),
        score,
        duration_ms,
        metrics,
        artifacts: CaseArtifacts {
            result: result_rel.clone(),
            trajectory: trajectory_rel,
            evaluator_stdout: stdout_rel,
            evaluator_stderr: stderr_rel,
        },
    };
    Ok(result)
}

pub(crate) fn run_evaluator(
    case: &CasePlan,
    workspace: &Path,
) -> Result<(CaseStatus, ScoreResult, String, String)> {
    let mut stderr = String::new();
    let mut details = Vec::new();
    for check in &case.task.test_spec.checks {
        let outcome = run_local_coding_check(check, workspace)?;
        if !outcome.stderr.is_empty() {
            stderr.push_str(&outcome.stderr);
            if !stderr.ends_with('\n') {
                stderr.push('\n');
            }
        }
        details.push(outcome.detail);
        if outcome.status != CaseStatus::Passed {
            let score = evaluator_score(
                outcome.status,
                outcome.message,
                json!({ "checks": details }),
            );
            let stdout = serde_json::to_string(&score)?;
            return Ok((outcome.status, score, stdout, stderr));
        }
    }
    let score = evaluator_score(
        CaseStatus::Passed,
        "all evaluator checks passed".to_string(),
        json!({ "checks": details }),
    );
    let stdout = serde_json::to_string(&score)?;
    Ok((CaseStatus::Passed, score, stdout, stderr))
}

#[derive(Debug)]
pub(crate) struct LocalCheckOutcome {
    pub status: CaseStatus,
    pub message: String,
    pub stderr: String,
    pub detail: Value,
}

pub(crate) fn run_local_coding_check(
    check: &LocalCodingCheck,
    workspace: &Path,
) -> Result<LocalCheckOutcome> {
    match check {
        LocalCodingCheck::ExactFile { path, expected } => {
            let actual_path = resolve_relative(workspace, path);
            let actual = fs::read_to_string(&actual_path).unwrap_or_default();
            let passed = actual == *expected;
            Ok(LocalCheckOutcome {
                status: if passed {
                    CaseStatus::Passed
                } else {
                    CaseStatus::Failed
                },
                message: if passed {
                    format!("exact file check passed for {}", path.display())
                } else {
                    format!("exact file check failed for {}", path.display())
                },
                stderr: String::new(),
                detail: json!({
                    "kind": "exact_file",
                    "path": path,
                    "passed": passed,
                }),
            })
        }
        LocalCodingCheck::CargoTest { timeout_seconds } => {
            let outcome = run_process(
                &CommandManifest {
                    command: vec![
                        "cargo".to_string(),
                        "test".to_string(),
                        "--quiet".to_string(),
                    ],
                    timeout_seconds: *timeout_seconds,
                },
                workspace,
                workspace,
            )?;
            let status = if outcome.timed_out {
                CaseStatus::Timeout
            } else if outcome.success {
                CaseStatus::Passed
            } else {
                CaseStatus::Failed
            };
            Ok(LocalCheckOutcome {
                status,
                message: match status {
                    CaseStatus::Passed => "cargo test passed".to_string(),
                    CaseStatus::Timeout => "cargo test timed out".to_string(),
                    _ => "cargo test failed".to_string(),
                },
                stderr: outcome.stderr,
                detail: json!({
                    "kind": "cargo_test",
                    "passed": status == CaseStatus::Passed,
                    "exit_code": outcome.code,
                    "timed_out": outcome.timed_out,
                }),
            })
        }
        LocalCodingCheck::PythonFunctionCases {
            module,
            function,
            cases,
            timeout_seconds,
        } => run_python_function_cases(workspace, module, function, cases, *timeout_seconds),
    }
}

pub(crate) fn run_python_function_cases(
    workspace: &Path,
    module: &Path,
    function: &str,
    cases: &[PythonFunctionCase],
    timeout_seconds: Option<u64>,
) -> Result<LocalCheckOutcome> {
    let payload = json!({
        "module": module,
        "function": function,
        "cases": cases,
    });
    let script = r#"
import importlib.util
import json
import os
import sys

payload = json.loads(os.environ["PEVAL_PYTHON_FUNCTION_CASES"])
spec = importlib.util.spec_from_file_location("peval_target", payload["module"])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
func = getattr(module, payload["function"])
for index, case in enumerate(payload["cases"]):
    actual = func(*case.get("args", []), **case.get("kwargs", {}))
    expected = case["expected"]
    if actual != expected:
        print(
            f"case {index} failed: expected {expected!r}, got {actual!r}",
            file=sys.stderr,
        )
        sys.exit(1)
"#;
    let mut command = Command::new("python3");
    command
        .arg("-c")
        .arg(script)
        .current_dir(workspace)
        .env(
            "PEVAL_PYTHON_FUNCTION_CASES",
            serde_json::to_string(&payload)?,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let outcome = wait_for_command(command, timeout_seconds.map(Duration::from_secs), workspace)?;
    let status = if outcome.timed_out {
        CaseStatus::Timeout
    } else if outcome.success {
        CaseStatus::Passed
    } else {
        CaseStatus::Failed
    };
    Ok(LocalCheckOutcome {
        status,
        message: match status {
            CaseStatus::Passed => format!("python function cases passed for {function}"),
            CaseStatus::Timeout => format!("python function cases timed out for {function}"),
            _ => format!("python function cases failed for {function}"),
        },
        stderr: outcome.stderr,
        detail: json!({
            "kind": "python_function_cases",
            "module": module,
            "function": function,
            "cases": cases.len(),
            "passed": status == CaseStatus::Passed,
            "timed_out": outcome.timed_out,
        }),
    })
}

pub(crate) fn evaluator_score(status: CaseStatus, message: String, details: Value) -> ScoreResult {
    ScoreResult {
        schema_version: EVALUATOR_RESULT_SCHEMA_VERSION,
        passed: status == CaseStatus::Passed,
        score: Some(if status == CaseStatus::Passed {
            1.0
        } else {
            0.0
        }),
        message,
        details,
    }
}

pub(crate) fn failure_class(status: CaseStatus, score: &ScoreResult) -> Option<String> {
    match status {
        CaseStatus::Passed => None,
        CaseStatus::Failed => Some("oracle_failed".to_string()),
        CaseStatus::SetupFailed => Some("setup_failed".to_string()),
        CaseStatus::RuntimeFailed => Some("runtime_failed".to_string()),
        CaseStatus::EvaluatorFailed if score.message.contains("malformed evaluator JSON") => {
            Some("evaluator_malformed_json".to_string())
        }
        CaseStatus::EvaluatorFailed if score.message.contains("evaluator exited") => {
            Some("evaluator_exit".to_string())
        }
        CaseStatus::EvaluatorFailed => Some("evaluator_failed".to_string()),
        CaseStatus::Timeout => Some("timeout".to_string()),
    }
}

pub(crate) fn agent_model(agent: &AgentManifest) -> Option<String> {
    match agent.kind {
        AgentKind::Psychevo => agent.psychevo.model.clone(),
        AgentKind::Opencode => agent.opencode.model.clone(),
        AgentKind::Hermes => agent.hermes.model.clone(),
        AgentKind::Fake => None,
    }
}

pub(crate) fn collect_case_metrics(events: &[TrajectoryEvent], duration_ms: u128) -> CaseMetrics {
    let mut metrics = CaseMetrics {
        duration_ms,
        ..CaseMetrics::default()
    };
    let mut turns = 0_u64;
    let mut usage = UsageAccumulator::default();
    for event in events {
        if event.kind.ends_with("turn_start") {
            turns += 1;
        }
        if event.kind.ends_with("tool_execution_start") {
            metrics.tool_calls += 1;
        }
        if event.kind.ends_with("tool_execution_end") && event_indicates_tool_error(event) {
            metrics.tool_errors += 1;
        }
        if let Some(raw) = event.data.get("raw_event") {
            usage.add_from_value(raw);
        } else {
            usage.add_from_value(&event.data);
        }
    }
    metrics.turns = (turns > 0).then_some(turns);
    let (usage, cost) = usage.finish();
    metrics.usage = usage;
    metrics.cost = cost;
    metrics
}

#[derive(Default)]
pub(crate) struct UsageAccumulator {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    reasoning_tokens: u64,
    total_tokens: u64,
    cost_usd: f64,
    has_input: bool,
    has_output: bool,
    has_cache_read: bool,
    has_cache_write: bool,
    has_reasoning: bool,
    has_total: bool,
    has_cost: bool,
}

impl UsageAccumulator {
    pub(crate) fn add_from_value(&mut self, value: &Value) {
        if let Some(accounting) = value.get("accounting") {
            self.add_tokens(accounting);
            self.add_cost(accounting);
        }
        if let Some(usage) = value.get("usage") {
            self.add_tokens(usage);
            self.add_cost(usage);
        }
        self.add_cost(value);
    }

    pub(crate) fn add_tokens(&mut self, value: &Value) {
        self.add_field(value, "billable_input_tokens", "input");
        self.add_field(value, "input_tokens", "input");
        self.add_field(value, "prompt_tokens", "input");
        self.add_field(value, "billable_output_tokens", "output");
        self.add_field(value, "output_tokens", "output");
        self.add_field(value, "completion_tokens", "output");
        self.add_field(value, "cache_read_tokens", "cache_read");
        self.add_field(value, "cached_input_tokens", "cache_read");
        self.add_field(value, "cache_write_tokens", "cache_write");
        self.add_field(value, "reasoning_tokens", "reasoning");
        self.add_field(value, "reported_total_tokens", "total");
        self.add_field(value, "total_tokens", "total");
    }

    pub(crate) fn add_field(&mut self, value: &Value, field: &str, target: &str) {
        let Some(amount) = value.get(field).and_then(Value::as_u64) else {
            return;
        };
        match target {
            "input" => {
                self.input_tokens += amount;
                self.has_input = true;
            }
            "output" => {
                self.output_tokens += amount;
                self.has_output = true;
            }
            "cache_read" => {
                self.cache_read_tokens += amount;
                self.has_cache_read = true;
            }
            "cache_write" => {
                self.cache_write_tokens += amount;
                self.has_cache_write = true;
            }
            "reasoning" => {
                self.reasoning_tokens += amount;
                self.has_reasoning = true;
            }
            "total" => {
                self.total_tokens += amount;
                self.has_total = true;
            }
            _ => {}
        }
    }

    pub(crate) fn add_cost(&mut self, value: &Value) {
        for field in ["amount_usd", "cost_usd", "total_cost_usd"] {
            if let Some(amount) = value.get(field).and_then(Value::as_f64) {
                self.cost_usd += amount;
                self.has_cost = true;
            }
        }
    }

    pub(crate) fn finish(self) -> (UsageMetrics, CostMetrics) {
        let computed_total = self.input_tokens
            + self.output_tokens
            + self.cache_read_tokens
            + self.cache_write_tokens
            + self.reasoning_tokens;
        (
            UsageMetrics {
                input_tokens: self.has_input.then_some(self.input_tokens),
                output_tokens: self.has_output.then_some(self.output_tokens),
                cache_read_tokens: self.has_cache_read.then_some(self.cache_read_tokens),
                cache_write_tokens: self.has_cache_write.then_some(self.cache_write_tokens),
                reasoning_tokens: self.has_reasoning.then_some(self.reasoning_tokens),
                total_tokens: if self.has_total {
                    Some(self.total_tokens)
                } else if self.has_input
                    || self.has_output
                    || self.has_cache_read
                    || self.has_cache_write
                    || self.has_reasoning
                {
                    Some(computed_total)
                } else {
                    None
                },
            },
            CostMetrics {
                amount_usd: self.has_cost.then_some(self.cost_usd),
                source: self.has_cost.then(|| "event_usage".to_string()),
            },
        )
    }
}

pub(crate) fn event_indicates_tool_error(event: &TrajectoryEvent) -> bool {
    let raw = event.data.get("raw_event").unwrap_or(&event.data);
    raw.get("outcome")
        .and_then(Value::as_str)
        .is_some_and(|value| value != "normal" && value != "ok" && value != "success")
        || raw
            .get("result")
            .and_then(|result| result.get("error"))
            .is_some_and(|value| !value.is_null())
        || raw
            .get("result")
            .and_then(|result| result.get("exit_code"))
            .and_then(Value::as_i64)
            .is_some_and(|code| code != 0)
}

pub(crate) fn aggregate_run_metrics(cases: &[CaseResult], duration_ms: u128) -> RunMetrics {
    let mut usage = UsageMetrics::default();
    let mut has_input = false;
    let mut has_output = false;
    let mut has_cache_read = false;
    let mut has_cache_write = false;
    let mut has_reasoning = false;
    let mut has_total = false;
    let mut total_turns = 0_u64;
    let mut has_turns = false;
    let mut metrics = RunMetrics {
        duration_ms,
        ..RunMetrics::default()
    };
    for case in cases {
        metrics.total_tool_calls += case.metrics.tool_calls;
        metrics.total_tool_errors += case.metrics.tool_errors;
        if let Some(turns) = case.metrics.turns {
            has_turns = true;
            total_turns += turns;
        }
        add_optional_u64(
            &mut usage.input_tokens,
            &mut has_input,
            case.metrics.usage.input_tokens,
        );
        add_optional_u64(
            &mut usage.output_tokens,
            &mut has_output,
            case.metrics.usage.output_tokens,
        );
        add_optional_u64(
            &mut usage.cache_read_tokens,
            &mut has_cache_read,
            case.metrics.usage.cache_read_tokens,
        );
        add_optional_u64(
            &mut usage.cache_write_tokens,
            &mut has_cache_write,
            case.metrics.usage.cache_write_tokens,
        );
        add_optional_u64(
            &mut usage.reasoning_tokens,
            &mut has_reasoning,
            case.metrics.usage.reasoning_tokens,
        );
        add_optional_u64(
            &mut usage.total_tokens,
            &mut has_total,
            case.metrics.usage.total_tokens,
        );
        if let Some(amount) = case.metrics.cost.amount_usd {
            metrics.cost.amount_usd = Some(metrics.cost.amount_usd.unwrap_or_default() + amount);
            metrics.cost.source = Some("case_metrics".to_string());
        }
    }
    metrics.total_turns = has_turns.then_some(total_turns);
    metrics.usage = usage;
    metrics
}

pub(crate) fn add_optional_u64(target: &mut Option<u64>, seen: &mut bool, value: Option<u64>) {
    if let Some(value) = value {
        *target = Some(target.unwrap_or_default() + value);
        *seen = true;
    }
}

pub(crate) fn run_agent(
    case: &CasePlan,
    workspace: &Path,
    events: &mut Vec<TrajectoryEvent>,
) -> Result<()> {
    match case.agent.kind {
        AgentKind::Fake => {
            if case.agent.fake.behavior == FakeBehavior::Fail {
                push_event(
                    events,
                    &case.case_id,
                    "fake_agent_noop",
                    "fake fail agent made no workspace changes",
                    json!({ "behavior": case.agent.fake.behavior }),
                );
                return Ok(());
            }
            let changed = apply_fake_pass_fixes(&case.task, workspace)?;
            push_event(
                events,
                &case.case_id,
                "fake_agent_finished",
                "fake pass agent applied deterministic workspace changes",
                json!({
                    "behavior": case.agent.fake.behavior,
                    "changed_files": changed,
                }),
            );
            Ok(())
        }
        AgentKind::Psychevo => {
            let prompt = task_prompt(&case.task)?;
            push_event(
                events,
                &case.case_id,
                "psychevo_agent_started",
                "Psychevo live adapter command started",
                json!({
                    "agent": case.agent.id,
                    "task": case.task.id,
                }),
            );
            let output = run_psychevo_agent(&case.agent, &case.task.dir, workspace, &prompt)?;
            let observation = collect_psychevo_observation_output(workspace, &output);
            append_psychevo_process_events(events, &case.case_id, &observation);
            push_event(
                events,
                &case.case_id,
                "psychevo_agent_finished",
                "Psychevo live adapter command finished",
                json!({
                    "exit_code": output.code,
                    "stdout_bytes": output.stdout.len(),
                    "stderr_bytes": output.stderr.len(),
                    "timed_out": output.timed_out,
                }),
            );
            if output.success {
                Ok(())
            } else {
                bail!("Psychevo agent `{}` failed", case.agent.id)
            }
        }
        AgentKind::Opencode => run_wrapper_agent(
            "opencode",
            &case.agent,
            &case.task,
            workspace,
            events,
            &case.case_id,
        ),
        AgentKind::Hermes => run_wrapper_agent(
            "hermes",
            &case.agent,
            &case.task,
            workspace,
            events,
            &case.case_id,
        ),
    }
}

pub(crate) fn apply_fake_pass_fixes(task: &TaskManifest, workspace: &Path) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    for check in &task.test_spec.checks {
        if let LocalCodingCheck::ExactFile { path, expected } = check {
            let target = resolve_relative(workspace, path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(&target, expected)
                .with_context(|| format!("failed to write {}", target.display()))?;
            changed.push(path.clone());
        }
    }
    Ok(changed)
}

pub(crate) fn run_wrapper_agent(
    adapter: &str,
    agent: &AgentManifest,
    task: &TaskManifest,
    workspace: &Path,
    events: &mut Vec<TrajectoryEvent>,
    case_id: &str,
) -> Result<()> {
    let prompt = task_prompt(task)?;
    push_event(
        events,
        case_id,
        &format!("{adapter}_agent_started"),
        &format!("{adapter} wrapper command started"),
        json!({ "agent": agent.id, "task": task.id }),
    );
    let output = run_named_wrapper_agent(adapter, agent, &task.dir, workspace, &prompt)?;
    append_wrapper_process_events(events, case_id, adapter, &output);
    push_event(
        events,
        case_id,
        &format!("{adapter}_agent_finished"),
        &format!("{adapter} wrapper command finished"),
        json!({
            "exit_code": output.code,
            "stdout_bytes": output.stdout.len(),
            "stderr_bytes": output.stderr.len(),
            "timed_out": output.timed_out,
        }),
    );
    if output.success {
        Ok(())
    } else {
        bail!("{adapter} agent `{}` failed", agent.id)
    }
}

pub(crate) fn run_named_wrapper_agent(
    adapter: &str,
    agent: &AgentManifest,
    task_dir: &Path,
    workspace: &Path,
    prompt: &str,
) -> Result<ProcessOutcome> {
    let options = match adapter {
        "opencode" => &agent.opencode,
        "hermes" => &agent.hermes,
        _ => bail!("unsupported wrapper adapter `{adapter}`"),
    };
    let command = options
        .command
        .clone()
        .unwrap_or_else(|| adapter.to_string());
    let mut args = if options.args.is_empty() {
        vec![prompt.to_string()]
    } else {
        options
            .args
            .iter()
            .map(|arg| {
                arg.replace("{workspace}", &workspace.display().to_string())
                    .replace("{prompt}", prompt)
            })
            .collect()
    };
    let spec = CommandManifest {
        command: {
            let mut command_and_args = vec![command];
            command_and_args.append(&mut args);
            command_and_args
        },
        timeout_seconds: Some(600),
    };
    run_process(&spec, task_dir, workspace)
}

pub(crate) fn append_wrapper_process_events(
    events: &mut Vec<TrajectoryEvent>,
    case_id: &str,
    adapter: &str,
    output: &ProcessOutcome,
) {
    for line in output.stdout.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(raw_event) => {
                let event_type = raw_event
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("event");
                push_event(
                    events,
                    case_id,
                    &format!("{adapter}_{}", event_kind_suffix(event_type)),
                    &format!("{adapter} event: {event_type}"),
                    json!({ "raw_event": raw_event }),
                );
            }
            Err(err) => push_event(
                events,
                case_id,
                &format!("{adapter}_stdout_line"),
                &format!("{adapter} stdout line"),
                json!({ "line": line, "parse_error": err.to_string() }),
            ),
        }
    }
    for line in output.stderr.lines().filter(|line| !line.trim().is_empty()) {
        push_event(
            events,
            case_id,
            &format!("{adapter}_stderr_line"),
            &format!("{adapter} stderr line"),
            json!({ "line": line }),
        );
    }
}

pub(crate) fn run_psychevo_agent(
    agent: &AgentManifest,
    task_dir: &Path,
    workspace: &Path,
    prompt: &str,
) -> Result<ProcessOutcome> {
    let command = agent
        .psychevo
        .command
        .clone()
        .unwrap_or_else(|| "pevo".to_string());
    let mut args = if agent.psychevo.args.is_empty() {
        let mut args = vec![
            "run".to_string(),
            "--dir".to_string(),
            workspace.display().to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--dangerously-skip-permissions".to_string(),
            "--no-skills".to_string(),
            "--no-agents".to_string(),
        ];
        if let Some(model) = &agent.psychevo.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }
        args.push(prompt.to_string());
        args
    } else {
        agent
            .psychevo
            .args
            .iter()
            .map(|arg| {
                arg.replace("{workspace}", &workspace.display().to_string())
                    .replace("{prompt}", prompt)
            })
            .collect()
    };
    let spec = CommandManifest {
        command: {
            let mut command_and_args = vec![command];
            command_and_args.append(&mut args);
            command_and_args
        },
        timeout_seconds: Some(600),
    };
    run_process(&spec, task_dir, workspace)
}

#[derive(Debug)]
pub(crate) struct PsychevoObservationOutput {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) fn collect_psychevo_observation_output(
    workspace: &Path,
    output: &ProcessOutcome,
) -> PsychevoObservationOutput {
    let stdout = read_optional_string(&workspace.join("pevo-live.stdout"))
        .unwrap_or_else(|| output.stdout.clone());
    let stderr = match read_optional_string(&workspace.join("pevo-live.stderr")) {
        Some(file_stderr) if output.stderr.trim().is_empty() => file_stderr,
        Some(file_stderr) if file_stderr.trim().is_empty() => output.stderr.clone(),
        Some(file_stderr) => format!("{}\n{}", file_stderr.trim_end(), output.stderr.trim_end()),
        None => output.stderr.clone(),
    };
    PsychevoObservationOutput { stdout, stderr }
}

pub(crate) fn read_optional_string(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

pub(crate) fn append_psychevo_process_events(
    events: &mut Vec<TrajectoryEvent>,
    case_id: &str,
    output: &PsychevoObservationOutput,
) {
    for line in output.stdout.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(raw_event) => {
                let event_type = raw_event
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("event");
                let kind = format!("psychevo_{}", event_kind_suffix(event_type));
                push_event(
                    events,
                    case_id,
                    &kind,
                    &format!("Psychevo runtime event: {event_type}"),
                    json!({ "raw_event": raw_event }),
                );
            }
            Err(err) => push_event(
                events,
                case_id,
                "psychevo_stdout_line",
                "Psychevo adapter stdout line",
                json!({
                    "line": line,
                    "parse_error": err.to_string(),
                }),
            ),
        }
    }
    for line in output.stderr.lines().filter(|line| !line.trim().is_empty()) {
        push_event(
            events,
            case_id,
            "psychevo_stderr_line",
            "Psychevo adapter stderr line",
            json!({ "line": line }),
        );
    }
}

pub(crate) fn event_kind_suffix(value: &str) -> String {
    let normalized = sanitize_id(&value.to_ascii_lowercase());
    let trimmed = normalized.trim_matches('_');
    if trimmed.is_empty() {
        "event".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn run_process(
    spec: &CommandManifest,
    task_dir: &Path,
    workspace: &Path,
) -> Result<ProcessOutcome> {
    if spec.command.is_empty() {
        bail!("command declaration is empty");
    }
    let program = resolve_command_part(&spec.command[0], task_dir);
    let mut command = Command::new(program);
    for arg in &spec.command[1..] {
        command.arg(resolve_command_part(arg, task_dir));
    }
    command
        .current_dir(workspace)
        .env("PEVAL_WORKSPACE", workspace)
        .env("PEVAL_TASK_DIR", task_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    wait_for_command(
        command,
        spec.timeout_seconds.map(Duration::from_secs),
        workspace,
    )
}

pub(crate) fn wait_for_command(
    mut command: Command,
    timeout: Option<Duration>,
    workspace: &Path,
) -> Result<ProcessOutcome> {
    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to spawn evaluator command in {}",
            workspace.display()
        )
    })?;
    if let Some(timeout) = timeout {
        let started = Instant::now();
        loop {
            if child.try_wait()?.is_some() {
                let output = child.wait_with_output()?;
                return Ok(ProcessOutcome {
                    success: output.status.success(),
                    code: output.status.code(),
                    timed_out: false,
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                let output = child.wait_with_output()?;
                return Ok(ProcessOutcome {
                    success: false,
                    code: output.status.code(),
                    timed_out: true,
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    let output = child.wait_with_output()?;
    Ok(ProcessOutcome {
        success: output.status.success(),
        code: output.status.code(),
        timed_out: false,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

pub(crate) fn validate_case(case: &CasePlan) -> Result<()> {
    reject_unsupported(case.task_set.schema_version, &case.task_set.manifest_path)?;
    reject_unsupported(case.agent.schema_version, &case.agent.manifest_path)?;
    reject_unsupported(case.task.schema_version, &case.task.manifest_path)?;
    match case.agent.kind {
        AgentKind::Opencode => {
            validate_wrapper_command("opencode", &case.agent.opencode, &case.task.dir)?
        }
        AgentKind::Hermes => {
            validate_wrapper_command("hermes", &case.agent.hermes, &case.task.dir)?
        }
        AgentKind::Fake | AgentKind::Psychevo => {}
    }
    let workspace_source = resolve_relative(&case.task.dir, &case.task.workspace.source);
    if !workspace_source.is_dir() {
        bail!(
            "task `{}` workspace source does not exist: {}",
            case.task.id,
            workspace_source.display()
        );
    }
    validate_local_coding_test_spec(&case.task)?;
    Ok(())
}

pub(crate) fn validate_local_coding_test_spec(task: &TaskManifest) -> Result<()> {
    if task.test_spec.checks.is_empty() {
        bail!("task `{}` test_spec declares no checks", task.id);
    }
    for check in &task.test_spec.checks {
        match check {
            LocalCodingCheck::PythonFunctionCases {
                module,
                function,
                cases,
                ..
            } => {
                if function.trim().is_empty() {
                    bail!(
                        "task `{}` python_function_cases has empty function",
                        task.id
                    );
                }
                if cases.is_empty() {
                    bail!("task `{}` python_function_cases declares no cases", task.id);
                }
                let path = resolve_relative(&task.dir, &task.workspace.source).join(module);
                if !path.is_file() {
                    bail!(
                        "task `{}` python module does not exist: {}",
                        task.id,
                        path.display()
                    );
                }
            }
            LocalCodingCheck::ExactFile { path, .. } => {
                if path.as_os_str().is_empty() {
                    bail!("task `{}` exact_file path is empty", task.id);
                }
            }
            LocalCodingCheck::CargoTest { .. } => {
                let manifest =
                    resolve_relative(&task.dir, &task.workspace.source).join("Cargo.toml");
                if !manifest.is_file() {
                    bail!(
                        "task `{}` cargo_test requires Cargo.toml at {}",
                        task.id,
                        manifest.display()
                    );
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_wrapper_command(
    adapter: &str,
    options: &WrapperAgentOptions,
    dir: &Path,
) -> Result<()> {
    if options.command.is_none() && options.args.is_empty() {
        return Ok(());
    }
    let command = options
        .command
        .clone()
        .unwrap_or_else(|| adapter.to_string());
    let mut parts = vec![command];
    parts.extend(options.args.clone());
    validate_command(
        &CommandManifest {
            command: parts,
            timeout_seconds: Some(600),
        },
        dir,
        &format!("{adapter} wrapper command"),
    )
}

pub(crate) fn validate_command(command: &CommandManifest, dir: &Path, label: &str) -> Result<()> {
    if command.command.is_empty() {
        bail!("{label} declaration is empty");
    }
    let program = &command.command[0];
    if is_declared_path(program, dir) {
        let path = resolve_relative(dir, Path::new(program));
        if !path.exists() {
            bail!("{label} path does not exist: {}", path.display());
        }
    }
    for arg in &command.command[1..] {
        if is_declared_path(arg, dir) {
            let path = resolve_relative(dir, Path::new(arg));
            if !path.exists() {
                bail!("{label} argument path does not exist: {}", path.display());
            }
        }
    }
    Ok(())
}

pub(crate) fn selected_task_sets(
    project: &EvalProject,
    task_set_filter: Option<&str>,
) -> Result<Vec<TaskSetManifest>> {
    if let Some(id) = task_set_filter {
        return Ok(vec![
            project
                .task_sets
                .get(id)
                .with_context(|| format!("unknown task set `{id}`"))?
                .clone(),
        ]);
    }
    Ok(project.task_sets.values().cloned().collect())
}

pub(crate) fn selected_agent_ids(
    project: &EvalProject,
    agent_filter: Option<&str>,
) -> Result<Vec<String>> {
    if let Some(agent_id) = agent_filter {
        if !project.agents.contains_key(agent_id) {
            bail!("unknown agent `{agent_id}`");
        }
        return Ok(vec![agent_id.to_string()]);
    }
    Ok(project.agents.keys().cloned().collect())
}

pub(crate) fn validate_direct_benchmark_selection(
    benchmark: Option<&str>,
    agent: Option<&str>,
    task_set: Option<&str>,
    task: Option<&str>,
) -> Result<()> {
    if benchmark.is_some() {
        if agent.is_none() {
            bail!("--benchmark requires an explicit --agent");
        }
        if task_set.is_none() && task.is_none() {
            bail!("--benchmark requires an explicit --task-set or --task");
        }
    }
    Ok(())
}

pub(crate) fn load_eval_config(path: &Path, store_root: Option<PathBuf>) -> Result<EvalProject> {
    let manifest_path = discover_manifest(path)?;
    let eval_root = manifest_path
        .parent()
        .context("eval config TOML has no parent directory")?
        .to_path_buf();
    let config = read_eval_config_manifest(&manifest_path)?;
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let store = resolve_optional_store(store_root)?;
    let registry = ResolvedRegistry::load(
        Some((
            &config.agents,
            &config.benchmarks,
            &eval_root,
            &manifest_path,
        )),
        store.as_ref().map(|store| store.root.as_path()),
        &home,
    )?;
    let benchmark = resolve_benchmark_reference(&config.benchmark, &registry, &eval_root)?;
    let agents = resolve_selected_agents(&config.selection.agents, &registry)?;
    let (task_sets, tasks) = select_benchmark_tasks(&benchmark, &config.selection)?;
    Ok(EvalProject {
        eval_root: Some(eval_root),
        eval_manifest_path: Some(manifest_path),
        id: config.id,
        name: config.name,
        benchmark_root: benchmark.root,
        benchmark_manifest_path: benchmark.manifest_path,
        benchmark_id: benchmark.id,
        benchmark_name: benchmark.name,
        schema_version: config.schema_version,
        output_root: config.output_root,
        evaluator: benchmark.evaluator,
        agents,
        task_sets,
        tasks,
        selection: config.selection,
    })
}

pub(crate) fn load_one_off_benchmark(
    benchmark_ref: &str,
    store_root: Option<PathBuf>,
) -> Result<EvalProject> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let store = resolve_optional_store(store_root)?;
    let registry = ResolvedRegistry::load(
        None,
        store.as_ref().map(|store| store.root.as_path()),
        &home,
    )?;
    let reference = if Path::new(benchmark_ref).exists()
        || benchmark_ref.ends_with(".toml")
        || benchmark_ref.contains('/')
    {
        BenchmarkReference {
            id: None,
            path: Some(PathBuf::from(benchmark_ref)),
        }
    } else {
        BenchmarkReference {
            id: Some(benchmark_ref.to_string()),
            path: None,
        }
    };
    let benchmark = resolve_benchmark_reference(&reference, &registry, &cwd)?;
    Ok(EvalProject {
        eval_root: None,
        eval_manifest_path: None,
        id: format!("{}-one-off", benchmark.id),
        name: format!("{} one-off", benchmark.name),
        benchmark_root: benchmark.root,
        benchmark_manifest_path: benchmark.manifest_path,
        benchmark_id: benchmark.id,
        benchmark_name: benchmark.name,
        schema_version: MANIFEST_SCHEMA_VERSION,
        output_root: None,
        evaluator: benchmark.evaluator,
        agents: registry.agents,
        task_sets: benchmark.task_sets,
        tasks: benchmark.tasks,
        selection: EvalSelection::default(),
    })
}

pub(crate) fn resolve_optional_store(store_root: Option<PathBuf>) -> Result<Option<EvalStore>> {
    match store_root {
        Some(root) => EvalStore::resolve(Some(root)).map(Some),
        None => Ok(EvalStore::resolve(None).ok()),
    }
}

pub(crate) struct ResolvedRegistry {
    pub agents: BTreeMap<String, AgentManifest>,
    pub benchmarks: BTreeMap<String, RegistryBenchmark>,
}

impl ResolvedRegistry {
    pub(crate) fn load(
        eval_layer: Option<(&[AgentManifest], &[RegistryBenchmark], &Path, &Path)>,
        workspace_root: Option<&Path>,
        home: &Path,
    ) -> Result<Self> {
        let mut agents = BTreeMap::new();
        let mut benchmarks = BTreeMap::new();

        let global = read_global_peval_config(home)?;
        merge_agents(&mut agents, global.agents, &global_peval_config_path(home))?;
        merge_benchmarks(&mut benchmarks, global.benchmarks, home)?;

        if let Some(root) = workspace_root {
            let workspace = read_workspace_config(root)?;
            merge_agents(&mut agents, workspace.agents, &workspace_config_path(root))?;
            merge_benchmarks(&mut benchmarks, workspace.benchmarks, root)?;
        }

        if let Some((eval_agents, eval_benchmarks, eval_root, eval_path)) = eval_layer {
            merge_agents(&mut agents, eval_agents.to_vec(), eval_path)?;
            merge_benchmarks(&mut benchmarks, eval_benchmarks.to_vec(), eval_root)?;
        }

        Ok(Self { agents, benchmarks })
    }
}

pub(crate) fn merge_agents(
    target: &mut BTreeMap<String, AgentManifest>,
    agents: Vec<AgentManifest>,
    path: &Path,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for mut agent in agents {
        reject_unsupported(agent.schema_version, path)?;
        agent.id = slugify(&agent.id);
        if !seen.insert(agent.id.clone()) {
            bail!("duplicate agent id `{}` in {}", agent.id, path.display());
        }
        agent.manifest_path = path.to_path_buf();
        target.insert(agent.id.clone(), agent);
    }
    Ok(())
}

pub(crate) fn merge_benchmarks(
    target: &mut BTreeMap<String, RegistryBenchmark>,
    benchmarks: Vec<RegistryBenchmark>,
    base: &Path,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for mut benchmark in benchmarks {
        benchmark.id = slugify(&benchmark.id);
        if !seen.insert(benchmark.id.clone()) {
            bail!(
                "duplicate benchmark id `{}` in registry config rooted at {}",
                benchmark.id,
                base.display()
            );
        }
        benchmark.path = resolve_relative(base, &benchmark.path);
        target.insert(benchmark.id.clone(), benchmark);
    }
    Ok(())
}

pub(crate) fn resolve_selected_agents(
    selected: &[String],
    registry: &ResolvedRegistry,
) -> Result<BTreeMap<String, AgentManifest>> {
    let mut agents = BTreeMap::new();
    for id in selected {
        let id = slugify(id);
        let agent = registry
            .agents
            .get(&id)
            .with_context(|| format!("unknown agent `{id}`"))?
            .clone();
        agents.insert(id, agent);
    }
    Ok(agents)
}

pub(crate) fn resolve_benchmark_reference(
    reference: &BenchmarkReference,
    registry: &ResolvedRegistry,
    base: &Path,
) -> Result<BenchmarkManifest> {
    match (&reference.id, &reference.path) {
        (Some(_), Some(_)) => bail!("benchmark reference must use either id or path, not both"),
        (None, None) => bail!("benchmark reference must declare id or path"),
        (Some(id), None) => {
            let id = slugify(id);
            let entry = registry
                .benchmarks
                .get(&id)
                .with_context(|| format!("unknown benchmark `{id}`"))?;
            BenchmarkManifest::load(&entry.path)
        }
        (None, Some(path)) => BenchmarkManifest::load(resolve_relative(base, path)),
    }
}

pub(crate) fn select_benchmark_tasks(
    benchmark: &BenchmarkManifest,
    selection: &EvalSelection,
) -> Result<(
    BTreeMap<String, TaskSetManifest>,
    BTreeMap<String, TaskManifest>,
)> {
    let selected_tasks = selection.tasks.iter().cloned().collect::<BTreeSet<_>>();
    let mut task_sets = if selection.task_sets.is_empty() {
        benchmark.task_sets.clone()
    } else {
        let mut out = BTreeMap::new();
        for id in &selection.task_sets {
            let task_set = benchmark
                .task_sets
                .get(id)
                .with_context(|| format!("unknown task set `{id}`"))?
                .clone();
            out.insert(task_set.id.clone(), task_set);
        }
        out
    };
    if !selected_tasks.is_empty() {
        for task_set in task_sets.values_mut() {
            task_set
                .tasks
                .retain(|task_id| selected_tasks.contains(task_id));
        }
        task_sets.retain(|_, task_set| !task_set.tasks.is_empty());
    }

    let mut tasks = BTreeMap::new();
    for task_set in task_sets.values() {
        for task_id in &task_set.tasks {
            if !selected_tasks.is_empty() && !selected_tasks.contains(task_id) {
                continue;
            }
            let task = benchmark
                .tasks
                .get(task_id)
                .with_context(|| {
                    format!(
                        "task set `{}` references unknown task `{task_id}`",
                        task_set.id
                    )
                })?
                .clone();
            tasks.insert(task.id.clone(), task);
        }
    }
    for task_id in &selected_tasks {
        if !tasks.contains_key(task_id) {
            bail!("selected task `{task_id}` is not present in selected task sets");
        }
    }
    if task_sets.is_empty() {
        bail!("no task sets selected");
    }
    if tasks.is_empty() {
        bail!("no tasks selected");
    }
    Ok((task_sets, tasks))
}

pub(crate) fn load_task_set_tasks(
    project: &EvalProject,
    task_set: &TaskSetManifest,
    task_filter: Option<&str>,
) -> Result<Vec<TaskManifest>> {
    if task_set.tasks.is_empty() {
        bail!("task set `{}` does not declare any tasks", task_set.id);
    }
    let tasks = task_set
        .tasks
        .iter()
        .map(|id| {
            project.tasks.get(id).cloned().with_context(|| {
                format!("task set `{}` references unknown task `{id}`", task_set.id)
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if let Some(task_id) = task_filter {
        let selected = tasks
            .into_iter()
            .filter(|task| task.id == task_id)
            .collect::<Vec<_>>();
        if selected.is_empty() {
            bail!(
                "task set `{}` does not include selected task `{task_id}`",
                task_set.id
            );
        }
        return Ok(selected);
    }
    Ok(tasks)
}

pub(crate) fn read_eval_config_manifest(path: &Path) -> Result<EvalConfigManifest> {
    let raw: RawEvalConfigManifest = read_toml(path)?;
    reject_unsupported(raw.schema_version, path)?;
    if let Some(output_root) = raw.output_root.as_ref() {
        validate_store_namespace(output_root)
            .with_context(|| format!("invalid output_root in {}", path.display()))?;
    }
    validate_eval_selection(&raw.select, path)?;
    Ok(EvalConfigManifest {
        schema_version: raw.schema_version,
        id: slugify(&raw.id),
        name: raw.name.unwrap_or_else(|| raw.id.clone()),
        output_root: raw.output_root,
        benchmark: raw.benchmark,
        selection: raw.select,
        agents: raw.agents,
        benchmarks: raw.benchmarks,
    })
}

pub(crate) fn validate_eval_selection(selection: &EvalSelection, path: &Path) -> Result<()> {
    if selection.agents.is_empty() {
        bail!("{} select.agents must not be empty", path.display());
    }
    if selection.task_sets.is_empty() && selection.tasks.is_empty() {
        bail!(
            "{} select.task_sets or select.tasks must declare at least one item",
            path.display()
        );
    }
    Ok(())
}

pub(crate) fn load_task_sources(
    root: &Path,
    sources: &[TaskSourceManifest],
) -> Result<BTreeMap<String, TaskManifest>> {
    if sources.is_empty() {
        bail!(
            "no task_sources declared in {}",
            root.join("benchmark.toml").display()
        );
    }
    let mut tasks = BTreeMap::new();
    for source in sources {
        match source.format {
            TaskSourceFormat::Jsonl => {
                let path = resolve_relative(root, &source.path);
                load_jsonl_tasks(&path, &mut tasks)?;
            }
        }
    }
    if tasks.is_empty() {
        bail!("task sources did not declare any tasks");
    }
    Ok(tasks)
}

pub(crate) fn load_jsonl_tasks(
    path: &Path,
    tasks: &mut BTreeMap<String, TaskManifest>,
) -> Result<()> {
    let file =
        fs::File::open(path).with_context(|| format!("failed to read {}", path.display()))?;
    let source_dir = path
        .parent()
        .with_context(|| format!("task source has no parent: {}", path.display()))?;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = index + 1;
        let line =
            line.with_context(|| format!("failed to read {} line {line_number}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        let raw: RawTaskRecord = serde_json::from_str(&line)
            .with_context(|| format!("failed to parse {} line {line_number}", path.display()))?;
        reject_unsupported(raw.schema_version, path)
            .with_context(|| format!("invalid task `{}` in {}", raw.task_id, path.display()))?;
        let task_dir = raw
            .dir
            .as_ref()
            .map(|dir| resolve_relative(source_dir, dir))
            .unwrap_or_else(|| source_dir.to_path_buf());
        let task = TaskManifest {
            schema_version: raw.schema_version,
            id: raw.task_id,
            name: raw.name,
            kind: raw.kind,
            problem_statement: raw.problem_statement,
            workspace: raw.workspace,
            test_spec: raw.test_spec,
            manifest_path: path.to_path_buf(),
            dir: task_dir,
        };
        if tasks.insert(task.id.clone(), task).is_some() {
            bail!("duplicate task id in task sources");
        }
    }
    Ok(())
}

pub(crate) fn collect_task_set_manifests(
    raw_task_sets: Vec<TaskSetManifest>,
    manifest_path: &Path,
    tasks: &BTreeMap<String, TaskManifest>,
) -> Result<BTreeMap<String, TaskSetManifest>> {
    let mut task_sets = BTreeMap::new();
    for mut task_set in raw_task_sets {
        reject_unsupported(task_set.schema_version, manifest_path)?;
        task_set.manifest_path = manifest_path.to_path_buf();
        if task_set.tasks.is_empty() {
            bail!("task set `{}` does not declare any tasks", task_set.id);
        }
        for task_id in &task_set.tasks {
            if !tasks.contains_key(task_id) {
                bail!(
                    "task set `{}` references unknown task `{task_id}`",
                    task_set.id
                );
            }
        }
        if task_sets.insert(task_set.id.clone(), task_set).is_some() {
            bail!("duplicate task set id");
        }
    }
    if task_sets.is_empty() {
        bail!("no task_sets declared in {}", manifest_path.display());
    }
    Ok(task_sets)
}
