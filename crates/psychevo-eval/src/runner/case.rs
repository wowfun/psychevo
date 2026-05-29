#[allow(unused_imports)]
use super::*;

pub(crate) fn run_case(
    artifact_root: &Path,
    case: CasePlan,
    artifact_includes: &BTreeSet<String>,
) -> Result<CaseResult> {
    if effective_execution_backend(&case.task) == ExecutionBackend::Container {
        return run_container_case(artifact_root, case, artifact_includes);
    }
    let started = Instant::now();
    fs::create_dir_all(artifact_root)
        .with_context(|| format!("failed to create {}", artifact_root.display()))?;
    let result_rel = PathBuf::from("run.json");
    let trajectory_rel = PathBuf::from("trajectory.jsonl");
    let prompt_rel = PathBuf::from("prompt.md");
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
    let prompt = task_prompt(&case.task)?;
    fs::write(artifact_root.join(&prompt_rel), prompt.as_bytes()).with_context(|| {
        format!(
            "failed to write {}",
            artifact_root.join(&prompt_rel).display()
        )
    })?;

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
        TaskSourceKind::Harbor
            if effective_execution_backend(&case.task) == ExecutionBackend::Local =>
        {
            run_peval_agent_verifier(case, workspace, logs_dir)
        }
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
        AgentKind::Acp | AgentKind::PsychevoAcp | AgentKind::OpencodeAcp | AgentKind::HermesAcp => {
            agent.acp.model.clone()
        }
        AgentKind::Psychevo => agent.psychevo.model.clone(),
        AgentKind::Opencode => agent.opencode.model.clone(),
        AgentKind::Hermes => agent.hermes.model.clone(),
        AgentKind::Fake | AgentKind::HumanInLoop => None,
    }
}
