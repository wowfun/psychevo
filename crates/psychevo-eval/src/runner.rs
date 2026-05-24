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

pub fn expand_matrix(
    project: &EvalProject,
    suite_filter: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<Vec<CasePlan>> {
    let mut plans = Vec::new();
    let suites = selected_suites(project, suite_filter)?;
    for suite in suites {
        let agent_ids = selected_agent_ids(project, &suite, agent_filter)?;
        let tasks = load_suite_tasks(&suite)?;
        for task in tasks {
            for agent_id in &agent_ids {
                let agent = project
                    .agents
                    .get(agent_id)
                    .with_context(|| format!("unknown agent `{agent_id}` in suite `{}`", suite.id))?
                    .clone();
                let case_id = sanitize_id(&format!("{}__{}__{}", suite.id, task.id, agent.id));
                plans.push(CasePlan {
                    case_id,
                    suite: suite.clone(),
                    task: task.clone(),
                    agent,
                });
            }
        }
    }
    Ok(plans)
}

pub fn run_evaluation(request: RunRequest) -> Result<RunSummary> {
    let project = load_project_from_config(request.config.as_deref())?;
    let cases = expand_matrix(&project, request.suite.as_deref(), request.agent.as_deref())?;
    if cases.is_empty() {
        bail!("no cases selected");
    }
    for case in &cases {
        validate_case(&project, case)?;
    }

    let run_id = request.run_id.unwrap_or_else(generate_run_id);
    let explicit_output = request.output_root.is_some();
    let store = if explicit_output {
        None
    } else {
        Some(EvalStore::resolve(request.store_root)?)
    };
    let output_base = if let Some(path) = request.output_root {
        resolve_cli_path(&path)?
    } else if let Some(store) = &store {
        store.run_output_base(&project)?
    } else {
        unreachable!("explicit output-root is the only non-store run path")
    };
    let artifact_root = output_base.join(&run_id);
    fs::create_dir_all(&artifact_root)
        .with_context(|| format!("failed to create {}", artifact_root.display()))?;

    let started_at_ms = now_ms();
    let mut results = Vec::new();
    for case in cases {
        let result = run_case(&project, &artifact_root, case)?;
        results.push(result);
    }
    let finished_at_ms = now_ms();
    let passed_cases = results
        .iter()
        .filter(|case| case.status == CaseStatus::Passed)
        .count();
    let failed_cases = results.len().saturating_sub(passed_cases);
    let status = if failed_cases == 0 {
        RunStatus::Passed
    } else {
        RunStatus::Failed
    };
    let summary = RunSummary {
        schema_version: SCHEMA_VERSION,
        run_id,
        project: project.name,
        artifact_root: artifact_root.clone(),
        started_at_ms,
        finished_at_ms,
        total_cases: results.len(),
        passed_cases,
        failed_cases,
        status,
        cases: results,
    };
    write_json_pretty(&artifact_root.join("summary.json"), &summary)?;
    write_run_reports(&summary)?;
    if let Some(store) = store {
        store.register_run(&summary)?;
    }
    Ok(summary)
}

pub fn render_report(request: ReportRequest) -> Result<String> {
    let summary = read_run_summary(&request.run_root)?;
    render_summary_report(&summary, request.format)
}

pub fn compare_runs(request: CompareRequest) -> Result<CompareReport> {
    if request.run_roots.len() < 2 {
        bail!("compare requires at least two run artifact roots");
    }
    let summaries = request
        .run_roots
        .iter()
        .map(|root| read_run_summary(root))
        .collect::<Result<Vec<_>>>()?;
    let runs = summaries
        .iter()
        .map(|summary| CompareRun {
            run_id: summary.run_id.clone(),
            artifact_root: summary.artifact_root.clone(),
            status: summary.status,
        })
        .collect::<Vec<_>>();
    let mut keys = BTreeSet::new();
    for summary in &summaries {
        for case in &summary.cases {
            keys.insert(compare_key(case));
        }
    }
    let mut cases = Vec::new();
    for key in keys {
        let mut statuses = BTreeMap::new();
        for summary in &summaries {
            if let Some(case) = summary.cases.iter().find(|case| compare_key(case) == key) {
                statuses.insert(summary.run_id.clone(), case.status);
            }
        }
        cases.push(CompareCase { key, statuses });
    }
    Ok(CompareReport {
        schema_version: SCHEMA_VERSION,
        runs,
        cases,
    })
}

pub fn replay_run(request: ReplayRequest) -> Result<ReplayReport> {
    let summary = read_run_summary(&request.run_root)?;
    let mut events = Vec::new();
    for case in &summary.cases {
        if request
            .case_id
            .as_deref()
            .is_some_and(|id| id != case.case_id)
        {
            continue;
        }
        let path = request.run_root.join(&case.artifacts.trajectory);
        let file =
            fs::File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        for line in BufReader::new(file).lines() {
            let line = line.with_context(|| format!("failed to read {}", path.display()))?;
            if line.trim().is_empty() {
                continue;
            }
            let event: TrajectoryEvent = serde_json::from_str(&line)
                .with_context(|| format!("invalid trajectory event in {}", path.display()))?;
            reject_unsupported(event.schema_version, &path)?;
            events.push(event);
        }
    }
    if events.is_empty() && request.case_id.is_some() {
        bail!("no trajectory events matched requested case");
    }
    Ok(ReplayReport {
        schema_version: SCHEMA_VERSION,
        run_id: summary.run_id,
        events,
    })
}

pub fn import_dataset(request: DatasetImportRequest) -> Result<DatasetEntry> {
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
        schema_version: SCHEMA_VERSION,
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
    project: &EvalProject,
    artifact_root: &Path,
    case: CasePlan,
) -> Result<CaseResult> {
    let started = Instant::now();
    let case_dir = artifact_root.join("cases").join(&case.case_id);
    fs::create_dir_all(&case_dir)
        .with_context(|| format!("failed to create {}", case_dir.display()))?;
    let result_rel = PathBuf::from("cases")
        .join(&case.case_id)
        .join("result.json");
    let trajectory_rel = PathBuf::from("cases")
        .join(&case.case_id)
        .join("trajectory.jsonl");
    let stdout_rel = PathBuf::from("cases")
        .join(&case.case_id)
        .join("scorer.stdout");
    let stderr_rel = PathBuf::from("cases")
        .join(&case.case_id)
        .join("scorer.stderr");

    let mut events = Vec::new();
    push_event(
        &mut events,
        &case.case_id,
        "case_started",
        "case execution started",
        json!({
            "suite": case.suite.id,
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
        if let Err(err) = run_agent(project, &case, workspace_temp.path(), &mut events) {
            let score = ScoreResult {
                schema_version: SCHEMA_VERSION,
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
            let scorer = run_process(&case.task.scorer, &case.task.dir, workspace_temp.path())
                .context("failed to run scorer")?;
            fs::write(artifact_root.join(&stdout_rel), scorer.stdout.as_bytes()).with_context(
                || {
                    format!(
                        "failed to write {}",
                        artifact_root.join(&stdout_rel).display()
                    )
                },
            )?;
            fs::write(artifact_root.join(&stderr_rel), scorer.stderr.as_bytes()).with_context(
                || {
                    format!(
                        "failed to write {}",
                        artifact_root.join(&stderr_rel).display()
                    )
                },
            )?;
            let (case_status, scorer_score) = parse_scorer_result(&scorer);
            push_event(
                &mut events,
                &case.case_id,
                "scorer_finished",
                &scorer_score.message,
                json!({
                    "status": case_status,
                    "passed": scorer_score.passed,
                    "exit_code": scorer.code,
                    "timed_out": scorer.timed_out,
                }),
            );
            (case_status, scorer_score)
        };

    push_event(
        &mut events,
        &case.case_id,
        "case_finished",
        "case execution finished",
        json!({ "status": status }),
    );
    write_jsonl(&artifact_root.join(&trajectory_rel), &events)?;

    let result = CaseResult {
        schema_version: SCHEMA_VERSION,
        case_id: case.case_id,
        suite_id: case.suite.id,
        task_id: case.task.id,
        agent_id: case.agent.id,
        status,
        score,
        duration_ms: started.elapsed().as_millis(),
        artifacts: CaseArtifacts {
            result: result_rel.clone(),
            trajectory: trajectory_rel,
            scorer_stdout: stdout_rel,
            scorer_stderr: stderr_rel,
        },
    };
    write_json_pretty(&artifact_root.join(&result_rel), &result)?;
    Ok(result)
}

pub(crate) fn run_agent(
    project: &EvalProject,
    case: &CasePlan,
    workspace: &Path,
    events: &mut Vec<TrajectoryEvent>,
) -> Result<()> {
    match case.agent.kind {
        AgentKind::Fake => {
            let spec = match case.agent.fake.behavior {
                FakeBehavior::Pass => case.task.fake.pass.as_ref(),
                FakeBehavior::Fail => case.task.fake.fail.as_ref(),
            };
            if let Some(command) = spec {
                let output = run_process(command, &case.task.dir, workspace)?;
                let kind = if output.success {
                    "fake_agent_finished"
                } else {
                    "fake_agent_failed"
                };
                push_event(
                    events,
                    &case.case_id,
                    kind,
                    "fake agent command finished",
                    json!({
                        "behavior": case.agent.fake.behavior,
                        "exit_code": output.code,
                        "stdout": output.stdout,
                        "stderr": output.stderr,
                        "timed_out": output.timed_out,
                    }),
                );
                if !output.success {
                    bail!("fake agent `{}` failed", case.agent.id);
                }
            } else {
                push_event(
                    events,
                    &case.case_id,
                    "fake_agent_noop",
                    "fake agent made no workspace changes",
                    json!({ "behavior": case.agent.fake.behavior }),
                );
            }
            Ok(())
        }
        AgentKind::Psychevo => {
            if !project.allow_live {
                bail!(
                    "agent `{}` uses the Psychevo live adapter, but allow_live is false",
                    case.agent.id
                );
            }
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

pub(crate) fn parse_scorer_result(outcome: &ProcessOutcome) -> (CaseStatus, ScoreResult) {
    if outcome.timed_out {
        return (
            CaseStatus::Timeout,
            ScoreResult {
                schema_version: SCHEMA_VERSION,
                passed: false,
                score: Some(0.0),
                message: "scorer timed out".to_string(),
                details: json!({ "stderr": outcome.stderr }),
            },
        );
    }
    if !outcome.success {
        return (
            CaseStatus::ScorerFailed,
            ScoreResult {
                schema_version: SCHEMA_VERSION,
                passed: false,
                score: Some(0.0),
                message: format!("scorer exited with code {:?}", outcome.code),
                details: json!({
                    "stdout": outcome.stdout,
                    "stderr": outcome.stderr,
                }),
            },
        );
    }
    match serde_json::from_str::<ScoreResult>(&outcome.stdout) {
        Ok(mut score) => {
            if let Err(err) = reject_unsupported_result_schema(score.schema_version) {
                score.passed = false;
                score.score = Some(0.0);
                score.message = err.to_string();
                return (CaseStatus::ScorerFailed, score);
            }
            let status = if score.passed {
                CaseStatus::Passed
            } else {
                CaseStatus::Failed
            };
            (status, score)
        }
        Err(err) => (
            CaseStatus::ScorerFailed,
            ScoreResult {
                schema_version: SCHEMA_VERSION,
                passed: false,
                score: Some(0.0),
                message: format!("malformed scorer JSON: {err}"),
                details: json!({ "stdout": outcome.stdout }),
            },
        ),
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

    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to spawn command `{}` in {}",
            spec.command.join(" "),
            workspace.display()
        )
    })?;
    let timeout = spec.timeout_seconds.map(Duration::from_secs);
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

pub(crate) fn validate_case(project: &EvalProject, case: &CasePlan) -> Result<()> {
    reject_unsupported(case.suite.schema_version, &case.suite.manifest_path)?;
    reject_unsupported(case.agent.schema_version, &case.agent.manifest_path)?;
    reject_unsupported(case.task.schema_version, &case.task.manifest_path)?;
    if case.agent.kind == AgentKind::Psychevo && !project.allow_live {
        bail!(
            "agent `{}` uses Psychevo live execution, but {} has allow_live = false",
            case.agent.id,
            project.manifest_path.display()
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
    validate_command(&case.task.scorer, &case.task.dir, "scorer")?;
    if let Some(command) = &case.task.fake.pass {
        validate_command(command, &case.task.dir, "fake pass command")?;
    }
    if let Some(command) = &case.task.fake.fail {
        validate_command(command, &case.task.dir, "fake fail command")?;
    }
    Ok(())
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

pub(crate) fn selected_suites(
    project: &EvalProject,
    suite_filter: Option<&str>,
) -> Result<Vec<SuiteManifest>> {
    if let Some(id) = suite_filter {
        return Ok(vec![
            project
                .suites
                .get(id)
                .with_context(|| format!("unknown suite `{id}`"))?
                .clone(),
        ]);
    }
    Ok(project.suites.values().cloned().collect())
}

pub(crate) fn selected_agent_ids(
    project: &EvalProject,
    suite: &SuiteManifest,
    agent_filter: Option<&str>,
) -> Result<Vec<String>> {
    if let Some(agent_id) = agent_filter {
        if !project.agents.contains_key(agent_id) {
            bail!("unknown agent `{agent_id}`");
        }
        return Ok(vec![agent_id.to_string()]);
    }
    if suite.agents.is_empty() {
        return Ok(project.agents.keys().cloned().collect());
    }
    Ok(suite.agents.clone())
}

pub(crate) fn load_suite_tasks(suite: &SuiteManifest) -> Result<Vec<TaskManifest>> {
    if suite.tasks.is_empty() {
        bail!("suite `{}` does not declare any tasks", suite.id);
    }
    suite
        .tasks
        .iter()
        .map(|path| {
            let path = resolve_relative(&suite.dir, path);
            read_task_manifest(&path)
        })
        .collect()
}

pub(crate) fn read_project_manifest(path: &Path) -> Result<ProjectManifest> {
    let raw: RawProjectManifest = read_toml(path)?;
    reject_unsupported(raw.schema_version, path)?;
    if let Some(output_root) = raw.output_root.as_ref() {
        validate_store_namespace(output_root)
            .with_context(|| format!("invalid output_root in {}", path.display()))?;
    }
    Ok(ProjectManifest {
        schema_version: raw.schema_version,
        name: raw.name,
        output_root: raw.output_root,
        allow_live: raw.allow_live,
    })
}

pub(crate) fn load_agent_manifests(root: &Path) -> Result<BTreeMap<String, AgentManifest>> {
    let mut agents = BTreeMap::new();
    for path in sorted_toml_files(&root.join("agents"))? {
        let mut manifest: AgentManifest = read_toml(&path)?;
        reject_unsupported(manifest.schema_version, &path)?;
        manifest.manifest_path = path;
        if agents.insert(manifest.id.clone(), manifest).is_some() {
            bail!("duplicate agent id");
        }
    }
    if agents.is_empty() {
        bail!(
            "no agent manifests found under {}",
            root.join("agents").display()
        );
    }
    Ok(agents)
}
