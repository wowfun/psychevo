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
    let output_store = if let Some(path) = request.output_root {
        EvalStore::new(resolve_cli_path(&path)?)
    } else {
        EvalStore::resolve(request.store_root)?
    };
    let output_base = output_store.cell_runs_root(&project);
    fs::create_dir_all(&output_base)
        .with_context(|| format!("failed to create {}", output_base.display()))?;
    let artifact_includes = resolved_artifact_includes(&project, &request.include_artifacts);

    let mut cells = Vec::new();
    for case in cases {
        let fingerprint = cell_fingerprint(&project, &case)?;
        let cell_key = cell_key(&fingerprint);
        let cell_root = output_store.cell_root(&project, &case, &cell_key);
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
        let cell = execute_cell(
            &project,
            case,
            &cell_root,
            &cell_key,
            &fingerprint,
            &artifact_includes,
        )?;
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

pub(crate) fn resolved_artifact_includes(
    project: &EvalProject,
    cli_includes: &[String],
) -> BTreeSet<String> {
    project
        .artifacts
        .include
        .iter()
        .chain(cli_includes.iter())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

pub(crate) fn execute_cell(
    project: &EvalProject,
    case: CasePlan,
    cell_root: &Path,
    cell_key: &str,
    fingerprint: &str,
    artifact_includes: &BTreeSet<String>,
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
    let result = run_case(&temp, case, artifact_includes)?;
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

pub(crate) fn cell_key(fingerprint: &str) -> String {
    fingerprint.chars().take(16).collect::<String>()
}

pub(crate) fn cell_fingerprint(project: &EvalProject, case: &CasePlan) -> Result<String> {
    let workspace_source = resolve_relative(&case.task.dir, &case.task.workspace.source);
    let payload = json!({
        "artifact_schema_version": ARTIFACT_SCHEMA_VERSION,
        "runner": "psychevo-eval-cell-v8",
        "benchmark": {
            "id": &project.benchmark_id,
            "name": &project.benchmark_name,
            "slug": project.slug(),
        },
        "task_set": {
            "id": &case.task_set.id,
        },
        "task": {
            "id": &case.task.id,
            "kind": &case.task.kind,
            "source_kind": case.task.source_kind,
            "source_id": &case.task.source_id,
            "native_id": &case.task.native_id,
            "definition": serde_json::to_value(&case.task)?,
            "prompt": task_prompt(&case.task)?,
            "workspace": workspace_tree_hash(&workspace_source)?,
        },
        "agent": {
            "id": &case.agent.id,
            "kind": case.agent.kind,
            "model": agent_model(&case.agent),
            "fake": &case.agent.fake,
            "command": &case.agent.command,
            "acp": &case.agent.acp,
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

pub(crate) fn run_case(
    artifact_root: &Path,
    case: CasePlan,
    artifact_includes: &BTreeSet<String>,
) -> Result<CaseResult> {
    let started = Instant::now();
    fs::create_dir_all(artifact_root)
        .with_context(|| format!("failed to create {}", artifact_root.display()))?;
    let result_rel = PathBuf::from("run.json");
    let trajectory_rel = PathBuf::from("trajectory.jsonl");
    let stdout_rel = PathBuf::from("evaluator.stdout");
    let stderr_rel = PathBuf::from("evaluator.stderr");
    let logs_dir = artifact_root.join("logs");
    fs::create_dir_all(logs_dir.join("agent"))
        .with_context(|| format!("failed to create {}", logs_dir.join("agent").display()))?;
    fs::create_dir_all(logs_dir.join("verifier"))
        .with_context(|| format!("failed to create {}", logs_dir.join("verifier").display()))?;

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

    let (status, score) =
        if let Err(err) = run_agent(&case, workspace_temp.path(), &logs_dir, &mut events) {
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
                run_task_verifier(&case, workspace_temp.path(), &logs_dir)
                    .context("failed to run task verifier")?;
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
    let observed = collect_case_observability(&events, duration_ms);
    let metrics = observed.metrics;
    let warnings = observed.warnings;
    write_jsonl(&artifact_root.join(&trajectory_rel), &events)?;
    if artifact_includes.contains("workspace") {
        let retained_workspace = artifact_root.join("workspace");
        if retained_workspace.exists() {
            fs::remove_dir_all(&retained_workspace)
                .with_context(|| format!("failed to remove {}", retained_workspace.display()))?;
        }
        copy_dir(workspace_temp.path(), &retained_workspace).with_context(|| {
            format!(
                "failed to retain workspace artifact at {}",
                retained_workspace.display()
            )
        })?;
    }

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
        warnings,
        artifacts: CaseArtifacts {
            result: result_rel.clone(),
            trajectory: trajectory_rel,
            evaluator_stdout: stdout_rel,
            evaluator_stderr: stderr_rel,
        },
    };
    Ok(result)
}

pub(crate) fn run_task_verifier(
    case: &CasePlan,
    workspace: &Path,
    logs_dir: &Path,
) -> Result<(CaseStatus, ScoreResult, String, String)> {
    match case.task.source_kind {
        TaskSourceKind::PevalAgent => run_peval_agent_verifier(case, workspace, logs_dir),
        TaskSourceKind::Harbor | TaskSourceKind::SweBench | TaskSourceKind::Tau2 => {
            bail!(
                "source kind `{:?}` requires an official bridge and is not executable by the local runner",
                case.task.source_kind
            )
        }
    }
}

pub(crate) fn run_peval_agent_verifier(
    case: &CasePlan,
    workspace: &Path,
    logs_dir: &Path,
) -> Result<(CaseStatus, ScoreResult, String, String)> {
    let script = case.task.dir.join("tests").join("test.sh");
    if !script.is_file() {
        bail!(
            "task `{}` verifier script does not exist: {}",
            case.task.id,
            script.display()
        );
    }
    let verifier_logs = logs_dir.join("verifier");
    fs::create_dir_all(&verifier_logs)
        .with_context(|| format!("failed to create {}", verifier_logs.display()))?;
    let mut command = Command::new("sh");
    command
        .arg(&script)
        .current_dir(workspace)
        .env("PEVAL_WORKSPACE", workspace)
        .env("PEVAL_TASK_DIR", &case.task.dir)
        .env("PEVAL_LOGS", logs_dir)
        .env("PEVAL_TASK_ID", &case.task.id)
        .env("PEVAL_NATIVE_TASK_ID", &case.task.native_id)
        .env("PEVAL_SOURCE_ID", &case.task.source_id)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let timeout = case
        .task
        .verifier_timeout_seconds
        .unwrap_or(default_agent_timeout_seconds());
    let outcome = wait_for_command(command, Some(Duration::from_secs(timeout)), workspace)?;
    let status = if outcome.timed_out {
        CaseStatus::Timeout
    } else if outcome.success {
        CaseStatus::Passed
    } else {
        CaseStatus::Failed
    };
    let default_message = match status {
        CaseStatus::Passed => "verifier passed".to_string(),
        CaseStatus::Timeout => "verifier timed out".to_string(),
        _ => "verifier failed".to_string(),
    };
    let mut score = import_verifier_score(&verifier_logs, status, default_message, &outcome)?;
    if status != CaseStatus::Passed {
        score.passed = false;
    }
    if score.score.is_none() {
        score.score = Some(if score.passed { 1.0 } else { 0.0 });
    }
    let stdout = serde_json::to_string(&score)?;
    Ok((status, score, stdout, outcome.stderr))
}

pub(crate) fn import_verifier_score(
    verifier_logs: &Path,
    status: CaseStatus,
    default_message: String,
    outcome: &ProcessOutcome,
) -> Result<ScoreResult> {
    let result_json = verifier_logs.join("result.json");
    if result_json.is_file() {
        let raw = fs::read_to_string(&result_json)
            .with_context(|| format!("failed to read {}", result_json.display()))?;
        let value: Value = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", result_json.display()))?;
        let passed = value
            .get("passed")
            .and_then(Value::as_bool)
            .unwrap_or(status == CaseStatus::Passed);
        let score = value.get("score").and_then(Value::as_f64);
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(&default_message)
            .to_string();
        let details = value
            .get("details")
            .cloned()
            .unwrap_or_else(|| json!({ "imported_from": result_json }));
        return Ok(ScoreResult {
            schema_version: EVALUATOR_RESULT_SCHEMA_VERSION,
            passed,
            score,
            message,
            details,
        });
    }

    let reward = verifier_logs.join("reward.txt");
    let score = if reward.is_file() {
        fs::read_to_string(&reward)
            .ok()
            .and_then(|raw| raw.trim().parse::<f64>().ok())
    } else {
        None
    };
    Ok(ScoreResult {
        schema_version: EVALUATOR_RESULT_SCHEMA_VERSION,
        passed: status == CaseStatus::Passed,
        score,
        message: default_message,
        details: json!({
            "exit_code": outcome.code,
            "timed_out": outcome.timed_out,
            "stdout_bytes": outcome.stdout.len(),
            "stderr_bytes": outcome.stderr.len(),
        }),
    })
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
        AgentKind::Command => agent.command.model.clone(),
        AgentKind::Acp => agent.acp.model.clone(),
        AgentKind::Psychevo => agent.psychevo.model.clone(),
        AgentKind::Opencode => agent.opencode.model.clone(),
        AgentKind::Hermes => agent.hermes.model.clone(),
        AgentKind::Fake => None,
    }
}

pub(crate) struct CaseObservability {
    pub(crate) metrics: CaseMetrics,
    pub(crate) warnings: Vec<String>,
}

#[allow(dead_code)]
pub(crate) fn collect_case_metrics(events: &[TrajectoryEvent], duration_ms: u128) -> CaseMetrics {
    collect_case_observability(events, duration_ms).metrics
}

pub(crate) fn collect_case_observability(
    events: &[TrajectoryEvent],
    duration_ms: u128,
) -> CaseObservability {
    let mut metrics = CaseMetrics {
        duration_ms,
        ..CaseMetrics::default()
    };
    let mut turns = 0_u64;
    let mut usage = UsageAccumulator::default();
    let mut warnings = Vec::new();
    let mut tool_error_ids = BTreeSet::new();
    let acp_windowed = events
        .iter()
        .any(|event| event.kind == "acp_agent_prompt_started");
    let mut in_acp_prompt = !acp_windowed;
    for event in events {
        if event.kind == "acp_agent_prompt_started" {
            in_acp_prompt = true;
            continue;
        }
        if acp_windowed && !in_acp_prompt {
            continue;
        }
        if event.kind == "acp_session_update" {
            collect_acp_session_update_metrics(
                event,
                &mut metrics,
                &mut usage,
                &mut warnings,
                &mut tool_error_ids,
            );
        } else if event.kind == "acp_agent_prompt_finished" {
            if let Some(prompt_result) = event.data.get("prompt_result") {
                usage.add_from_value(prompt_result);
                collect_psychevo_meta(prompt_result, &mut warnings, &mut turns);
            }
            in_acp_prompt = false;
        } else if event.kind.ends_with("turn_start") {
            turns += 1;
        } else if event.kind.ends_with("tool_execution_start") {
            metrics.tool_calls += 1;
        } else if event.kind.ends_with("tool_execution_end") && event_indicates_tool_error(event) {
            if let Some(id) = tool_call_id_for_event(event) {
                if tool_error_ids.insert(id) {
                    metrics.tool_errors += 1;
                }
            } else {
                metrics.tool_errors += 1;
            }
        }
        if let Some(raw) = event.data.get("raw_event") {
            usage.add_from_value(raw);
            collect_warning(raw, &mut warnings);
        } else {
            usage.add_from_value(&event.data);
            collect_warning(&event.data, &mut warnings);
        }
    }
    metrics.turns = (turns > 0).then_some(turns);
    let (usage, accounting, cost) = usage.finish();
    metrics.usage = usage;
    metrics.accounting = accounting;
    metrics.cost = cost;
    CaseObservability { metrics, warnings }
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
    has_cumulative_cost: bool,
    accounting: AccountingAccumulator,
}

impl UsageAccumulator {
    pub(crate) fn add_from_value(&mut self, value: &Value) {
        let mut saw_usage = false;
        if let Some(accounting) = value.get("accounting") {
            self.add_accounting(accounting);
        }
        if let Some(usage) = value.get("usage") {
            saw_usage = self.has_usage_tokens();
            self.add_usage_tokens(usage);
            saw_usage = saw_usage || self.has_usage_tokens();
            self.add_cost(usage);
        }
        if let Some(psychevo) = value.get("_meta").and_then(|meta| meta.get("psychevo"))
            && let Some(accounting) = psychevo.get("accounting")
        {
            self.add_accounting(accounting);
            if !saw_usage {
                self.add_usage_from_accounting(accounting);
            }
        }
        if let Some(accounting) = value.get("accounting")
            && !saw_usage
        {
            self.add_usage_from_accounting(accounting);
        }
        if let Some(cost) = value.get("cost") {
            self.add_cumulative_cost(cost);
        }
        self.add_cost(value);
    }

    pub(crate) fn add_usage_tokens(&mut self, value: &Value) {
        self.add_field(value, "input_tokens", "input");
        self.add_field(value, "inputTokens", "input");
        self.add_field(value, "prompt_tokens", "input");
        self.add_field(value, "output_tokens", "output");
        self.add_field(value, "outputTokens", "output");
        self.add_field(value, "completion_tokens", "output");
        self.add_field(value, "cached_read_tokens", "cache_read");
        self.add_field(value, "cachedReadTokens", "cache_read");
        self.add_field(value, "cache_read_tokens", "cache_read");
        self.add_field(value, "cached_input_tokens", "cache_read");
        self.add_field(value, "cached_tokens", "cache_read");
        self.add_field(value, "cached_write_tokens", "cache_write");
        self.add_field(value, "cachedWriteTokens", "cache_write");
        self.add_field(value, "cache_write_tokens", "cache_write");
        self.add_field(value, "thought_tokens", "reasoning");
        self.add_field(value, "thoughtTokens", "reasoning");
        self.add_field(value, "reasoning_tokens", "reasoning");
        self.add_field(value, "reported_total_tokens", "total");
        self.add_field(value, "total_tokens", "total");
        self.add_field(value, "totalTokens", "total");
    }

    pub(crate) fn add_usage_from_accounting(&mut self, value: &Value) {
        let cache_read = value.get("cache_read_tokens").and_then(json_u64);
        let cache_write = value.get("cache_write_tokens").and_then(json_u64);
        let reasoning = value.get("reasoning_tokens").and_then(json_u64);
        let input = value
            .get("context_input_tokens")
            .and_then(json_u64)
            .or_else(|| {
                value
                    .get("billable_input_tokens")
                    .and_then(json_u64)
                    .map(|amount| {
                        amount
                            .saturating_add(cache_read.unwrap_or(0))
                            .saturating_add(cache_write.unwrap_or(0))
                    })
            });
        let output = value
            .get("billable_output_tokens")
            .and_then(json_u64)
            .map(|amount| amount.saturating_add(reasoning.unwrap_or(0)));
        self.add_amount(input, "input");
        self.add_amount(output, "output");
        self.add_amount(cache_read, "cache_read");
        self.add_amount(cache_write, "cache_write");
        self.add_amount(reasoning, "reasoning");
        self.add_amount(
            value.get("reported_total_tokens").and_then(json_u64),
            "total",
        );
    }

    pub(crate) fn add_accounting(&mut self, value: &Value) {
        self.accounting.add(value);
        if let Some(nanodollars) = value.get("estimated_cost_nanodollars").and_then(json_i64) {
            let amount = nanodollars as f64 / 1_000_000_000.0;
            if self.has_cumulative_cost {
                self.cost_usd = self.cost_usd.max(amount);
            } else {
                self.cost_usd += amount;
            }
            self.has_cost = true;
        }
    }

    pub(crate) fn add_field(&mut self, value: &Value, field: &str, target: &str) {
        let Some(amount) = value.get(field).and_then(json_u64) else {
            return;
        };
        self.add_amount(Some(amount), target);
    }

    fn add_amount(&mut self, amount: Option<u64>, target: &str) {
        let Some(amount) = amount else {
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

    fn has_usage_tokens(&self) -> bool {
        self.has_input
            || self.has_output
            || self.has_cache_read
            || self.has_cache_write
            || self.has_reasoning
            || self.has_total
    }

    pub(crate) fn add_cost(&mut self, value: &Value) {
        for field in ["amount_usd", "cost_usd", "total_cost_usd"] {
            if let Some(amount) = value.get(field).and_then(Value::as_f64) {
                self.cost_usd += amount;
                self.has_cost = true;
            }
        }
        if value
            .get("currency")
            .and_then(Value::as_str)
            .is_none_or(|currency| currency.eq_ignore_ascii_case("USD"))
            && let Some(amount) = value.get("amount").and_then(Value::as_f64)
        {
            self.cost_usd += amount;
            self.has_cost = true;
        }
    }

    pub(crate) fn add_cumulative_cost(&mut self, value: &Value) {
        if value
            .get("currency")
            .and_then(Value::as_str)
            .is_none_or(|currency| currency.eq_ignore_ascii_case("USD"))
            && let Some(amount) = value.get("amount").and_then(Value::as_f64)
        {
            self.cost_usd = if self.has_cost {
                self.cost_usd.max(amount)
            } else {
                amount
            };
            self.has_cost = true;
            self.has_cumulative_cost = true;
        }
    }

    pub(crate) fn finish(self) -> (UsageMetrics, AccountingMetrics, CostMetrics) {
        let computed_total = self.input_tokens + self.output_tokens;
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
            self.accounting.finish(),
            CostMetrics {
                amount_usd: self.has_cost.then_some(self.cost_usd),
                source: self.has_cost.then(|| "event_usage".to_string()),
            },
        )
    }
}

#[derive(Default)]
pub(crate) struct AccountingAccumulator {
    metrics: AccountingMetrics,
}

impl AccountingAccumulator {
    pub(crate) fn add(&mut self, value: &Value) {
        add_accounting_u64(
            &mut self.metrics.context_input_tokens,
            value.get("context_input_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.billable_input_tokens,
            value.get("billable_input_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.billable_output_tokens,
            value.get("billable_output_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.reasoning_tokens,
            value.get("reasoning_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.cache_read_tokens,
            value.get("cache_read_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.cache_write_tokens,
            value.get("cache_write_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.reported_total_tokens,
            value.get("reported_total_tokens").and_then(json_u64),
        );
        add_accounting_i64(
            &mut self.metrics.estimated_cost_nanodollars,
            value.get("estimated_cost_nanodollars").and_then(json_i64),
        );
        merge_accounting_string(
            &mut self.metrics.pricing_source,
            value.get("pricing_source").and_then(Value::as_str),
        );
        merge_accounting_string(
            &mut self.metrics.pricing_tier,
            value.get("pricing_tier").and_then(Value::as_str),
        );
    }

    pub(crate) fn finish(self) -> AccountingMetrics {
        self.metrics
    }
}

pub(crate) fn collect_acp_session_update_metrics(
    event: &TrajectoryEvent,
    metrics: &mut CaseMetrics,
    usage: &mut UsageAccumulator,
    warnings: &mut Vec<String>,
    tool_error_ids: &mut BTreeSet<String>,
) {
    let Some(update) = acp_update_value(event) else {
        return;
    };
    match update.get("sessionUpdate").and_then(Value::as_str) {
        Some("tool_call") => metrics.tool_calls += 1,
        Some("tool_call_update") => {
            if update
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.eq_ignore_ascii_case("failed"))
            {
                let id = update
                    .get("toolCallId")
                    .or_else(|| update.get("tool_call_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                if tool_error_ids.insert(id) {
                    metrics.tool_errors += 1;
                }
            }
        }
        Some("usage_update") => usage.add_from_value(update),
        _ => collect_warning(update, warnings),
    }
}

pub(crate) fn acp_update_value(event: &TrajectoryEvent) -> Option<&Value> {
    event.data.get("raw_event")?.get("params")?.get("update")
}

pub(crate) fn collect_psychevo_meta(value: &Value, warnings: &mut Vec<String>, turns: &mut u64) {
    let Some(psychevo) = value.get("_meta").and_then(|meta| meta.get("psychevo")) else {
        return;
    };
    if let Some(meta_turns) = psychevo.get("turns").and_then(json_u64) {
        *turns = turns.saturating_add(meta_turns);
    }
    if let Some(items) = psychevo.get("warnings").and_then(Value::as_array) {
        for item in items {
            if let Some(message) = item.as_str() {
                push_warning(warnings, message);
            } else if let Some(message) = item.get("message").and_then(Value::as_str) {
                push_warning(warnings, message);
            }
        }
    }
}

pub(crate) fn collect_warning(value: &Value, warnings: &mut Vec<String>) {
    let event_type = value.get("type").and_then(Value::as_str);
    if matches!(event_type, Some("warning"))
        && let Some(message) = value.get("message").and_then(Value::as_str)
    {
        push_warning(warnings, message);
    }
}

pub(crate) fn push_warning(warnings: &mut Vec<String>, message: &str) {
    let message = message.trim();
    if !message.is_empty() && !warnings.iter().any(|warning| warning == message) {
        warnings.push(message.to_string());
    }
}

pub(crate) fn tool_call_id_for_event(event: &TrajectoryEvent) -> Option<String> {
    let raw = event.data.get("raw_event").unwrap_or(&event.data);
    raw.get("tool_call_id")
        .or_else(|| raw.get("toolCallId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn json_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
        .or_else(|| value.as_str().and_then(|value| value.parse::<u64>().ok()))
}

pub(crate) fn json_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_str().and_then(|value| value.parse::<i64>().ok()))
}

pub(crate) fn add_accounting_u64(target: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        *target = Some(target.unwrap_or_default() + value);
    }
}

pub(crate) fn add_accounting_i64(target: &mut Option<i64>, value: Option<i64>) {
    if let Some(value) = value {
        *target = Some(target.unwrap_or_default() + value);
    }
}

pub(crate) fn merge_accounting_string(target: &mut Option<String>, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    match target {
        None => *target = Some(value.to_string()),
        Some(current) if current == value || current == "mixed" => {}
        Some(current) => *current = "mixed".to_string(),
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
    let mut accounting = AccountingAccumulator::default();
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
        accounting.add(&serde_json::to_value(&case.metrics.accounting).unwrap_or_default());
        if let Some(amount) = case.metrics.cost.amount_usd {
            metrics.cost.amount_usd = Some(metrics.cost.amount_usd.unwrap_or_default() + amount);
            metrics.cost.source = Some("case_metrics".to_string());
        }
    }
    metrics.total_turns = has_turns.then_some(total_turns);
    metrics.usage = usage;
    metrics.accounting = accounting.finish();
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
    logs_dir: &Path,
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
        AgentKind::Command => run_command_agent(case, workspace, logs_dir, events),
        AgentKind::Acp => run_acp_agent(case, workspace, logs_dir, events),
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

pub(crate) fn run_command_agent(
    case: &CasePlan,
    workspace: &Path,
    logs_dir: &Path,
    events: &mut Vec<TrajectoryEvent>,
) -> Result<()> {
    let prompt = task_prompt(&case.task)?;
    let command =
        case.agent.command.command.clone().with_context(|| {
            format!("command agent `{}` does not declare command", case.agent.id)
        })?;
    let prompt_dir = workspace.join(".peval");
    fs::create_dir_all(&prompt_dir)
        .with_context(|| format!("failed to create {}", prompt_dir.display()))?;
    let prompt_file = prompt_dir.join("prompt.md");
    fs::write(&prompt_file, prompt.as_bytes())
        .with_context(|| format!("failed to write {}", prompt_file.display()))?;
    let args = case
        .agent
        .command
        .args
        .iter()
        .map(|arg| render_agent_template(arg, workspace, &case.task.dir, &prompt, &prompt_file))
        .collect::<Vec<_>>();
    push_event(
        events,
        &case.case_id,
        "command_agent_started",
        "command agent started",
        json!({
            "agent": case.agent.id,
            "task": case.task.id,
        }),
    );
    let mut process = Command::new(resolve_command_part(&command, &case.task.dir));
    for arg in args {
        process.arg(resolve_command_part(&arg, &case.task.dir));
    }
    process
        .current_dir(workspace)
        .env("PEVAL_WORKSPACE", workspace)
        .env("PEVAL_TASK_DIR", &case.task.dir)
        .env("PEVAL_LOGS", logs_dir)
        .env("PEVAL_TASK_ID", &case.task.id)
        .env("PEVAL_NATIVE_TASK_ID", &case.task.native_id)
        .env("PEVAL_SOURCE_ID", &case.task.source_id)
        .env("PEVAL_PROMPT_FILE", &prompt_file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = wait_for_command(
        process,
        Some(Duration::from_secs(case.agent.command.timeout_seconds)),
        workspace,
    )?;
    append_wrapper_process_events(events, &case.case_id, "command", &output);
    push_event(
        events,
        &case.case_id,
        "command_agent_finished",
        "command agent finished",
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
        bail!("command agent `{}` failed", case.agent.id)
    }
}

pub(crate) fn run_acp_agent(
    case: &CasePlan,
    workspace: &Path,
    logs_dir: &Path,
    events: &mut Vec<TrajectoryEvent>,
) -> Result<()> {
    let prompt = task_prompt(&case.task)?;
    let command = case
        .agent
        .acp
        .command
        .clone()
        .with_context(|| format!("ACP agent `{}` does not declare command", case.agent.id))?;
    let prompt_dir = workspace.join(".peval");
    fs::create_dir_all(&prompt_dir)
        .with_context(|| format!("failed to create {}", prompt_dir.display()))?;
    let prompt_file = prompt_dir.join("prompt.md");
    fs::write(&prompt_file, prompt.as_bytes())
        .with_context(|| format!("failed to write {}", prompt_file.display()))?;
    let args = case
        .agent
        .acp
        .args
        .iter()
        .map(|arg| render_agent_template(arg, workspace, &case.task.dir, &prompt, &prompt_file))
        .collect::<Vec<_>>();

    push_event(
        events,
        &case.case_id,
        "acp_agent_started",
        "ACP agent stdio session started",
        json!({
            "agent": case.agent.id,
            "task": case.task.id,
        }),
    );

    let mut process = Command::new(resolve_command_part(&command, &case.task.dir));
    for arg in args {
        process.arg(resolve_command_part(&arg, &case.task.dir));
    }
    process
        .current_dir(workspace)
        .env("PEVAL_WORKSPACE", workspace)
        .env("PEVAL_TASK_DIR", &case.task.dir)
        .env("PEVAL_LOGS", logs_dir)
        .env("PEVAL_TASK_ID", &case.task.id)
        .env("PEVAL_NATIVE_TASK_ID", &case.task.native_id)
        .env("PEVAL_SOURCE_ID", &case.task.source_id)
        .env("PEVAL_PROMPT_FILE", &prompt_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process.spawn().with_context(|| {
        format!(
            "failed to spawn ACP agent `{}` in {}",
            case.agent.id,
            workspace.display()
        )
    })?;
    let mut stdin = child.stdin.take().context("ACP agent stdin unavailable")?;
    let stdout = child
        .stdout
        .take()
        .context("ACP agent stdout unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("ACP agent stderr unavailable")?;
    let (stdout_tx, stdout_rx) = std::sync::mpsc::channel::<String>();
    let stdout_reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap_or_default();
            if stdout_tx.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_reader = thread::spawn(move || {
        let mut stderr = stderr;
        let mut content = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut content);
        content
    });

    let deadline = Instant::now() + Duration::from_secs(case.agent.acp.timeout_seconds);
    let mut next_id = 1_u64;
    acp_send(
        &mut stdin,
        next_id,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": {
                "name": "psychevo-eval",
                "version": env!("CARGO_PKG_VERSION"),
            },
        }),
    )?;
    let _ = acp_recv_response(
        &mut stdin,
        &stdout_rx,
        next_id,
        deadline,
        events,
        &case.case_id,
    )?;
    next_id += 1;

    acp_send(
        &mut stdin,
        next_id,
        "session/new",
        json!({
            "cwd": workspace,
            "mcpServers": [],
        }),
    )?;
    let session = acp_recv_response(
        &mut stdin,
        &stdout_rx,
        next_id,
        deadline,
        events,
        &case.case_id,
    )?;
    let session_id = session
        .get("sessionId")
        .and_then(Value::as_str)
        .context("ACP session/new response missing sessionId")?
        .to_string();
    next_id += 1;

    if let Some(mode) = &case.agent.acp.mode {
        acp_send(
            &mut stdin,
            next_id,
            "session/set_mode",
            json!({
                "sessionId": session_id,
                "modeId": mode,
            }),
        )?;
        let _ = acp_recv_response(
            &mut stdin,
            &stdout_rx,
            next_id,
            deadline,
            events,
            &case.case_id,
        )?;
        next_id += 1;
    }
    if let Some(model) = &case.agent.acp.model {
        acp_send(
            &mut stdin,
            next_id,
            "session/set_model",
            json!({
                "sessionId": session_id,
                "modelId": model,
            }),
        )?;
        let _ = acp_recv_response(
            &mut stdin,
            &stdout_rx,
            next_id,
            deadline,
            events,
            &case.case_id,
        )?;
        next_id += 1;
    }

    push_event(
        events,
        &case.case_id,
        "acp_agent_prompt_started",
        "ACP agent prompt started",
        json!({
            "session_id": session_id,
            "prompt_bytes": prompt.len(),
        }),
    );
    acp_send(
        &mut stdin,
        next_id,
        "session/prompt",
        json!({
            "sessionId": session_id,
            "prompt": [
                {
                    "type": "text",
                    "text": prompt,
                }
            ],
        }),
    )?;
    let prompt_result = acp_recv_response(
        &mut stdin,
        &stdout_rx,
        next_id,
        deadline,
        events,
        &case.case_id,
    )?;
    push_event(
        events,
        &case.case_id,
        "acp_agent_prompt_finished",
        "ACP agent prompt finished",
        json!({ "prompt_result": prompt_result.clone() }),
    );
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let _ = stdout_reader.join();
    let stderr = stderr_reader.join().unwrap_or_default();
    for line in stderr.lines().filter(|line| !line.trim().is_empty()) {
        push_event(
            events,
            &case.case_id,
            "acp_stderr_line",
            "ACP agent stderr line",
            json!({ "line": line }),
        );
    }
    push_event(
        events,
        &case.case_id,
        "acp_agent_finished",
        "ACP agent stdio session finished",
        json!({ "prompt_result": prompt_result }),
    );
    Ok(())
}

pub(crate) fn acp_send(
    stdin: &mut std::process::ChildStdin,
    id: u64,
    method: &str,
    params: Value,
) -> Result<()> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    std::io::Write::write_all(stdin, serde_json::to_string(&request)?.as_bytes())?;
    std::io::Write::write_all(stdin, b"\n")?;
    std::io::Write::flush(stdin)?;
    Ok(())
}

pub(crate) fn acp_recv_response(
    stdin: &mut std::process::ChildStdin,
    stdout_rx: &std::sync::mpsc::Receiver<String>,
    id: u64,
    deadline: Instant,
    events: &mut Vec<TrajectoryEvent>,
    case_id: &str,
) -> Result<Value> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            bail!("ACP agent timed out waiting for response id {id}");
        }
        let line = stdout_rx
            .recv_timeout(deadline.saturating_duration_since(now))
            .with_context(|| format!("ACP agent did not produce response id {id}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let raw = match serde_json::from_str::<Value>(&line) {
            Ok(raw) => raw,
            Err(err) => {
                push_event(
                    events,
                    case_id,
                    "acp_stdout_line",
                    "ACP agent stdout line",
                    json!({ "line": line, "parse_error": err.to_string() }),
                );
                continue;
            }
        };
        if raw.get("id").and_then(Value::as_u64) == Some(id) && raw.get("method").is_none() {
            if let Some(error) = raw.get("error") {
                bail!("ACP agent returned error for id {id}: {error}");
            }
            return Ok(raw.get("result").cloned().unwrap_or(Value::Null));
        }
        if raw.get("method").and_then(Value::as_str) == Some("session/request_permission")
            && raw.get("id").is_some()
        {
            acp_send_response(
                stdin,
                raw.get("id").cloned().unwrap_or(Value::Null),
                json!({
                    "outcome": {
                        "outcome": "selected",
                        "optionId": "allow_once",
                    }
                }),
            )?;
            push_event(
                events,
                case_id,
                "acp_permission_allowed",
                "ACP permission request allowed once",
                json!({ "raw_event": raw }),
            );
            continue;
        }
        let kind = if raw.get("method").and_then(Value::as_str) == Some("session/update") {
            "acp_session_update"
        } else if raw.get("method").is_some() {
            "acp_notification"
        } else {
            "acp_unexpected_response"
        };
        push_event(
            events,
            case_id,
            kind,
            "ACP agent protocol message",
            json!({ "raw_event": raw }),
        );
    }
}

pub(crate) fn acp_send_response(
    stdin: &mut std::process::ChildStdin,
    id: Value,
    result: Value,
) -> Result<()> {
    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    std::io::Write::write_all(stdin, serde_json::to_string(&response)?.as_bytes())?;
    std::io::Write::write_all(stdin, b"\n")?;
    std::io::Write::flush(stdin)?;
    Ok(())
}

pub(crate) fn render_agent_template(
    value: &str,
    workspace: &Path,
    task_dir: &Path,
    prompt: &str,
    prompt_file: &Path,
) -> String {
    value
        .replace("{workspace}", &workspace.display().to_string())
        .replace("{task_dir}", &task_dir.display().to_string())
        .replace("{prompt_file}", &prompt_file.display().to_string())
        .replace("{prompt}", prompt)
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
        AgentKind::Command => validate_command_agent(&case.agent, &case.task.dir)?,
        AgentKind::Acp => validate_acp_agent(&case.agent, &case.task.dir)?,
        AgentKind::Opencode => {
            validate_wrapper_command("opencode", &case.agent.opencode, &case.task.dir)?
        }
        AgentKind::Hermes => {
            validate_wrapper_command("hermes", &case.agent.hermes, &case.task.dir)?
        }
        AgentKind::Fake | AgentKind::Psychevo => {}
    }
    if case.task.source_kind != TaskSourceKind::PevalAgent {
        bail!(
            "incompatible_source_agent: task `{}` comes from source kind `{:?}`, which requires an official bridge and is incompatible with local agent kind `{:?}`",
            case.task.id,
            case.task.source_kind,
            case.agent.kind
        );
    }
    let workspace_source = resolve_relative(&case.task.dir, &case.task.workspace.source);
    if !workspace_source.is_dir() {
        bail!(
            "task `{}` workspace source does not exist: {}",
            case.task.id,
            workspace_source.display()
        );
    }
    validate_peval_agent_task(&case.task)?;
    Ok(())
}

pub(crate) fn validate_command_agent(agent: &AgentManifest, dir: &Path) -> Result<()> {
    let command = agent.command.command.clone().with_context(|| {
        format!(
            "command agent `{}` must declare [agents.command].command",
            agent.id
        )
    })?;
    let mut parts = vec![command];
    parts.extend(agent.command.args.clone());
    validate_command(
        &CommandManifest {
            command: parts,
            timeout_seconds: Some(agent.command.timeout_seconds),
        },
        dir,
        "command agent command",
    )
}

pub(crate) fn validate_acp_agent(agent: &AgentManifest, dir: &Path) -> Result<()> {
    let command =
        agent.acp.command.clone().with_context(|| {
            format!("ACP agent `{}` must declare [agents.acp].command", agent.id)
        })?;
    let mut parts = vec![command];
    parts.extend(agent.acp.args.clone());
    validate_command(
        &CommandManifest {
            command: parts,
            timeout_seconds: Some(agent.acp.timeout_seconds),
        },
        dir,
        "ACP agent command",
    )
}

pub(crate) fn validate_peval_agent_task(task: &TaskManifest) -> Result<()> {
    let task_toml = task.dir.join("task.toml");
    let instruction = task.dir.join("instruction.md");
    let environment = task.dir.join("environment");
    let verifier = task.dir.join("tests").join("test.sh");
    if !task_toml.is_file() {
        bail!("task `{}` missing task.toml", task.id);
    }
    let raw = fs::read_to_string(&task_toml)
        .with_context(|| format!("failed to read {}", task_toml.display()))?;
    let _: toml::Value =
        toml::from_str(&raw).with_context(|| format!("failed to parse {}", task_toml.display()))?;
    if !instruction.is_file() {
        bail!("task `{}` missing instruction.md", task.id);
    }
    if !environment.is_dir() {
        bail!("task `{}` missing environment/", task.id);
    }
    if !verifier.is_file() {
        bail!("task `{}` missing tests/test.sh", task.id);
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
        artifacts: config.artifacts,
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
        artifacts: ArtifactSelection::default(),
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
    let mut task_sets = if selection.sets.is_empty() {
        benchmark.task_sets.clone()
    } else {
        let mut out = BTreeMap::new();
        for id in &selection.sets {
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
        artifacts: raw.artifacts,
        analysis: raw.analysis,
        reports: raw.reports,
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
    if selection.sets.is_empty() && selection.tasks.is_empty() {
        bail!(
            "{} select.sets or select.tasks must declare at least one item",
            path.display()
        );
    }
    Ok(())
}

type LoadedBenchmarkSources = (
    Vec<BenchmarkSourceSummary>,
    BTreeMap<String, TaskSetManifest>,
    BTreeMap<String, TaskManifest>,
);

pub(crate) fn load_benchmark_sources(
    root: &Path,
    manifest_path: &Path,
    sources: BenchmarkSources,
) -> Result<LoadedBenchmarkSources> {
    if sources.is_empty() {
        bail!("no sources declared in {}", manifest_path.display());
    }
    let mut seen_sources = BTreeSet::new();
    let mut summaries = Vec::new();
    let mut task_sets = BTreeMap::new();
    let mut tasks = BTreeMap::new();

    for source in sources.peval_agent {
        let source_id = unique_source_id(&source.id, &mut seen_sources, manifest_path)?;
        summaries.push(BenchmarkSourceSummary {
            id: source_id.clone(),
            kind: TaskSourceKind::PevalAgent,
        });
        let source_root = resolve_relative(root, &source.path);
        let loaded = load_directory_source(
            &source_id,
            TaskSourceKind::PevalAgent,
            &source_root,
            manifest_path,
            source.verifier_timeout_seconds,
        )
        .with_context(|| format!("failed to load peval_agent source `{source_id}`"))?;
        insert_loaded_source(
            &mut task_sets,
            &mut tasks,
            &source_id,
            manifest_path,
            loaded,
            source.sets,
        )?;
    }

    for source in sources.harbor {
        let source_id = unique_source_id(&source.id, &mut seen_sources, manifest_path)?;
        summaries.push(BenchmarkSourceSummary {
            id: source_id.clone(),
            kind: TaskSourceKind::Harbor,
        });
        let source_root = resolve_relative(&resolve_relative(root, &source.root), &source.path);
        let loaded = load_directory_source(
            &source_id,
            TaskSourceKind::Harbor,
            &source_root,
            manifest_path,
            None,
        )
        .with_context(|| format!("failed to load harbor source `{source_id}`"))?;
        insert_loaded_source(
            &mut task_sets,
            &mut tasks,
            &source_id,
            manifest_path,
            loaded,
            source.sets,
        )?;
    }

    for source in sources.swe_bench {
        let source_id = unique_source_id(&source.id, &mut seen_sources, manifest_path)?;
        summaries.push(BenchmarkSourceSummary {
            id: source_id.clone(),
            kind: TaskSourceKind::SweBench,
        });
        let loaded = load_declared_official_source(
            &source_id,
            TaskSourceKind::SweBench,
            manifest_path,
            resolve_relative(root, &source.root),
            format!("{}:{}", source.dataset, source.split),
        )?;
        insert_loaded_source(
            &mut task_sets,
            &mut tasks,
            &source_id,
            manifest_path,
            loaded,
            source.sets,
        )?;
    }

    for source in sources.tau2 {
        let source_id = unique_source_id(&source.id, &mut seen_sources, manifest_path)?;
        summaries.push(BenchmarkSourceSummary {
            id: source_id.clone(),
            kind: TaskSourceKind::Tau2,
        });
        let mut native = source.domain.clone();
        if let Some(split) = source.split {
            native.push('-');
            native.push_str(&split);
        }
        if let Some(task_set) = source.task_set {
            native.push('-');
            native.push_str(&task_set);
        }
        let loaded = load_declared_official_source(
            &source_id,
            TaskSourceKind::Tau2,
            manifest_path,
            resolve_relative(root, &source.root),
            native,
        )?;
        insert_loaded_source(
            &mut task_sets,
            &mut tasks,
            &source_id,
            manifest_path,
            loaded,
            source.sets,
        )?;
    }

    if task_sets.is_empty() {
        bail!(
            "sources did not declare any sets in {}",
            manifest_path.display()
        );
    }
    if tasks.is_empty() {
        bail!(
            "sources did not declare any tasks in {}",
            manifest_path.display()
        );
    }
    Ok((summaries, task_sets, tasks))
}

pub(crate) struct LoadedSource {
    native_ids: Vec<String>,
    tasks: BTreeMap<String, TaskManifest>,
}

pub(crate) fn unique_source_id(
    id: &str,
    seen_sources: &mut BTreeSet<String>,
    manifest_path: &Path,
) -> Result<String> {
    let source_id = slugify(id);
    if !seen_sources.insert(source_id.clone()) {
        bail!(
            "duplicate source id `{}` in {}",
            source_id,
            manifest_path.display()
        );
    }
    Ok(source_id)
}

pub(crate) fn load_directory_source(
    source_id: &str,
    source_kind: TaskSourceKind,
    source_root: &Path,
    manifest_path: &Path,
    default_timeout: Option<u64>,
) -> Result<LoadedSource> {
    if !source_root.is_dir() {
        bail!("source directory does not exist: {}", source_root.display());
    }
    let mut native_ids = Vec::new();
    let mut tasks = BTreeMap::new();
    for entry in fs::read_dir(source_root)
        .with_context(|| format!("failed to read {}", source_root.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let native_id = entry.file_name().to_string_lossy().to_string();
        let task_dir = entry.path();
        if source_kind == TaskSourceKind::PevalAgent {
            validate_task_directory_files(&task_dir)
                .with_context(|| format!("invalid task `{native_id}`"))?;
        }
        let task_toml = task_dir.join("task.toml");
        let task_value = if task_toml.is_file() {
            let raw = fs::read_to_string(&task_toml)
                .with_context(|| format!("failed to read {}", task_toml.display()))?;
            Some(
                toml::from_str::<toml::Value>(&raw)
                    .with_context(|| format!("failed to parse {}", task_toml.display()))?,
            )
        } else {
            None
        };
        let instruction = task_dir.join("instruction.md");
        let problem_statement = if instruction.is_file() {
            fs::read_to_string(&instruction)
                .with_context(|| format!("failed to read {}", instruction.display()))?
        } else {
            format!("Run official source task `{native_id}`.")
        };
        let canonical_id = canonical_task_id(source_id, &native_id);
        let task = TaskManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            id: canonical_id.clone(),
            name: task_value
                .as_ref()
                .and_then(|value| value.get("name"))
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            kind: task_value
                .as_ref()
                .and_then(|value| value.get("kind"))
                .and_then(toml::Value::as_str)
                .unwrap_or(match source_kind {
                    TaskSourceKind::PevalAgent => "coding",
                    TaskSourceKind::Harbor => "harbor",
                    TaskSourceKind::SweBench => "swe-bench",
                    TaskSourceKind::Tau2 => "tau2",
                })
                .to_string(),
            problem_statement,
            workspace: WorkspaceManifest {
                source: PathBuf::from("environment"),
            },
            test_spec: TestSpecManifest { checks: Vec::new() },
            source_kind,
            source_id: source_id.to_string(),
            native_id: native_id.clone(),
            verifier_timeout_seconds: task_value
                .as_ref()
                .and_then(read_task_verifier_timeout)
                .or(default_timeout),
            manifest_path: manifest_path.to_path_buf(),
            dir: task_dir,
        };
        native_ids.push(native_id);
        if tasks.insert(canonical_id, task).is_some() {
            bail!("duplicate task id in source `{source_id}`");
        }
    }
    native_ids.sort();
    if native_ids.is_empty() {
        bail!("source `{source_id}` did not declare any task directories");
    }
    Ok(LoadedSource { native_ids, tasks })
}

pub(crate) fn load_declared_official_source(
    source_id: &str,
    source_kind: TaskSourceKind,
    manifest_path: &Path,
    root: PathBuf,
    native_id: String,
) -> Result<LoadedSource> {
    let native_id = slugify(&native_id);
    let canonical_id = canonical_task_id(source_id, &native_id);
    let task = TaskManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        id: canonical_id.clone(),
        name: None,
        kind: match source_kind {
            TaskSourceKind::PevalAgent => "coding",
            TaskSourceKind::Harbor => "harbor",
            TaskSourceKind::SweBench => "swe-bench",
            TaskSourceKind::Tau2 => "tau2",
        }
        .to_string(),
        problem_statement: format!("Run official {:?} task `{native_id}`.", source_kind),
        workspace: WorkspaceManifest {
            source: PathBuf::from("."),
        },
        test_spec: TestSpecManifest { checks: Vec::new() },
        source_kind,
        source_id: source_id.to_string(),
        native_id: native_id.clone(),
        verifier_timeout_seconds: None,
        manifest_path: manifest_path.to_path_buf(),
        dir: root,
    };
    Ok(LoadedSource {
        native_ids: vec![native_id],
        tasks: BTreeMap::from([(canonical_id, task)]),
    })
}

pub(crate) fn insert_loaded_source(
    task_sets: &mut BTreeMap<String, TaskSetManifest>,
    tasks: &mut BTreeMap<String, TaskManifest>,
    source_id: &str,
    manifest_path: &Path,
    loaded: LoadedSource,
    sets: Vec<SourceSetManifest>,
) -> Result<()> {
    let native_ids = loaded.native_ids;
    for (task_id, task) in loaded.tasks {
        if tasks.insert(task_id.clone(), task).is_some() {
            bail!("duplicate canonical task id `{task_id}`");
        }
    }
    let all_tasks = native_ids
        .iter()
        .map(|native_id| canonical_task_id(source_id, native_id))
        .collect::<Vec<_>>();
    insert_task_set(
        task_sets,
        TaskSetManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            id: source_id.to_string(),
            name: Some(source_id.to_string()),
            description: None,
            tasks: all_tasks,
            manifest_path: manifest_path.to_path_buf(),
        },
    )?;
    for set in sets {
        let set_id = format!("{source_id}/{}", slugify(&set.id));
        let selected = filter_source_set(source_id, &native_ids, &set);
        if selected.is_empty() {
            bail!("source set `{set_id}` selected no tasks");
        }
        insert_task_set(
            task_sets,
            TaskSetManifest {
                schema_version: MANIFEST_SCHEMA_VERSION,
                id: set_id,
                name: set.name,
                description: set.description,
                tasks: selected,
                manifest_path: manifest_path.to_path_buf(),
            },
        )?;
    }
    Ok(())
}

pub(crate) fn insert_task_set(
    task_sets: &mut BTreeMap<String, TaskSetManifest>,
    task_set: TaskSetManifest,
) -> Result<()> {
    if task_sets.insert(task_set.id.clone(), task_set).is_some() {
        bail!("duplicate set id");
    }
    Ok(())
}

pub(crate) fn filter_source_set(
    source_id: &str,
    native_ids: &[String],
    set: &SourceSetManifest,
) -> Vec<String> {
    let include_all = set.include.is_empty();
    let mut selected = native_ids
        .iter()
        .filter(|native_id| {
            (include_all
                || set
                    .include
                    .iter()
                    .any(|pattern| glob_match(pattern, native_id)))
                && !set
                    .exclude
                    .iter()
                    .any(|pattern| glob_match(pattern, native_id))
        })
        .map(|native_id| canonical_task_id(source_id, native_id))
        .collect::<Vec<_>>();
    selected.sort();
    if let Some(limit) = set.limit {
        selected.truncate(limit);
    }
    selected
}

pub(crate) fn canonical_task_id(source_id: &str, native_id: &str) -> String {
    format!("{}/{}", source_id, native_id)
}

pub(crate) fn validate_task_directory_files(task_dir: &Path) -> Result<()> {
    let task_toml = task_dir.join("task.toml");
    let instruction = task_dir.join("instruction.md");
    let environment = task_dir.join("environment");
    let verifier = task_dir.join("tests").join("test.sh");
    if !task_toml.is_file() {
        bail!("missing task.toml");
    }
    let raw = fs::read_to_string(&task_toml)
        .with_context(|| format!("failed to read {}", task_toml.display()))?;
    let _: toml::Value =
        toml::from_str(&raw).with_context(|| format!("failed to parse {}", task_toml.display()))?;
    if !instruction.is_file() {
        bail!("missing instruction.md");
    }
    if !environment.is_dir() {
        bail!("missing environment/");
    }
    if !verifier.is_file() {
        bail!("missing tests/test.sh");
    }
    Ok(())
}

pub(crate) fn read_task_verifier_timeout(value: &toml::Value) -> Option<u64> {
    value
        .get("verifier")
        .and_then(|verifier| {
            verifier
                .get("timeout_seconds")
                .or_else(|| verifier.get("timeout_sec"))
        })
        .and_then(toml::Value::as_integer)
        .and_then(|value| u64::try_from(value).ok())
}

pub(crate) fn glob_match(pattern: &str, value: &str) -> bool {
    fn inner(pattern: &[u8], value: &[u8]) -> bool {
        match pattern.split_first() {
            None => value.is_empty(),
            Some((&b'*', rest)) => {
                inner(rest, value) || (!value.is_empty() && inner(pattern, &value[1..]))
            }
            Some((&b'?', rest)) => !value.is_empty() && inner(rest, &value[1..]),
            Some((&expected, rest)) => value
                .split_first()
                .is_some_and(|(&actual, tail)| actual == expected && inner(rest, tail)),
        }
    }
    inner(pattern.as_bytes(), value.as_bytes())
}

#[cfg(test)]
mod metrics_tests {
    use super::*;

    fn event(sequence: u64, kind: &str, data: Value) -> TrajectoryEvent {
        TrajectoryEvent {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            sequence,
            case_id: "case".to_string(),
            kind: kind.to_string(),
            message: kind.to_string(),
            timestamp_ms: 0,
            data,
        }
    }

    fn acp_update(sequence: u64, update: Value) -> TrajectoryEvent {
        event(
            sequence,
            "acp_session_update",
            json!({
                "raw_event": {
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {
                        "sessionId": "s",
                        "update": update,
                    },
                },
            }),
        )
    }

    #[test]
    fn acp_metrics_collect_prompt_response_usage_accounting_and_warnings() {
        let events = vec![
            event(0, "acp_agent_started", json!({})),
            event(1, "acp_agent_prompt_started", json!({})),
            acp_update(
                2,
                json!({
                    "sessionUpdate": "tool_call",
                    "toolCallId": "call-1",
                    "title": "Tool",
                }),
            ),
            acp_update(
                3,
                json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "call-1",
                    "status": "failed",
                }),
            ),
            acp_update(
                4,
                json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "call-1",
                    "status": "failed",
                }),
            ),
            acp_update(
                5,
                json!({
                    "sessionUpdate": "usage_update",
                    "used": 512,
                    "size": 4096,
                    "cost": {
                        "amount": 0.12,
                        "currency": "USD",
                    },
                }),
            ),
            event(
                6,
                "acp_agent_prompt_finished",
                json!({
                    "prompt_result": {
                        "stopReason": "end_turn",
                        "usage": {
                            "inputTokens": 10,
                            "outputTokens": 5,
                            "cachedReadTokens": 2,
                            "totalTokens": 15,
                        },
                        "_meta": {
                            "psychevo": {
                                "turns": 2,
                                "warnings": ["MCP server degraded"],
                                "accounting": {
                                    "context_input_tokens": 10,
                                    "billable_input_tokens": 8,
                                    "billable_output_tokens": 5,
                                    "cache_read_tokens": 2,
                                    "reported_total_tokens": 15,
                                    "estimated_cost_nanodollars": 120000000,
                                    "pricing_source": "fixture",
                                    "pricing_tier": "standard",
                                },
                            },
                        },
                    },
                }),
            ),
            event(7, "acp_agent_finished", json!({ "ignored": true })),
        ];

        let observed = collect_case_observability(&events, 123);
        assert_eq!(observed.metrics.duration_ms, 123);
        assert_eq!(observed.metrics.tool_calls, 1);
        assert_eq!(observed.metrics.tool_errors, 1);
        assert_eq!(observed.metrics.turns, Some(2));
        assert_eq!(observed.metrics.usage.input_tokens, Some(10));
        assert_eq!(observed.metrics.usage.output_tokens, Some(5));
        assert_eq!(observed.metrics.usage.cache_read_tokens, Some(2));
        assert_eq!(observed.metrics.usage.total_tokens, Some(15));
        assert_eq!(
            observed.metrics.accounting.estimated_cost_nanodollars,
            Some(120000000)
        );
        assert_eq!(
            observed.metrics.accounting.pricing_source.as_deref(),
            Some("fixture")
        );
        assert_eq!(observed.metrics.cost.amount_usd, Some(0.12));
        assert_eq!(observed.warnings, vec!["MCP server degraded"]);
    }

    #[test]
    fn acp_metrics_synthesizes_usage_from_accounting_without_prompt_usage() {
        let events = vec![
            event(0, "acp_agent_prompt_started", json!({})),
            event(
                1,
                "acp_agent_prompt_finished",
                json!({
                    "prompt_result": {
                        "stopReason": "end_turn",
                        "_meta": {
                            "psychevo": {
                                "accounting": {
                                    "billable_input_tokens": 8,
                                    "billable_output_tokens": 5,
                                    "cache_read_tokens": 2,
                                    "reasoning_tokens": 1,
                                    "reported_total_tokens": 16,
                                },
                            },
                        },
                    },
                }),
            ),
        ];

        let observed = collect_case_observability(&events, 50);
        assert_eq!(observed.metrics.usage.input_tokens, Some(10));
        assert_eq!(observed.metrics.usage.output_tokens, Some(6));
        assert_eq!(observed.metrics.usage.cache_read_tokens, Some(2));
        assert_eq!(observed.metrics.usage.reasoning_tokens, Some(1));
        assert_eq!(observed.metrics.usage.total_tokens, Some(16));
    }
}
