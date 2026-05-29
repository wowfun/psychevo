#[allow(unused_imports)]
use super::*;

mod acp;
mod compose;
mod docker;
mod env;
mod harbor;

pub(crate) use acp::*;
pub(crate) use compose::*;
pub(crate) use docker::*;
pub(crate) use env::*;
pub(crate) use harbor::*;

pub(crate) fn run_container_case(
    artifact_root: &Path,
    case: CasePlan,
    artifact_includes: &BTreeSet<String>,
) -> Result<CaseResult> {
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
        "container case execution started",
        json!({
            "task_set": case.task_set.id,
            "task": case.task.id,
            "agent": case.agent.id,
            "execution": "container",
        }),
    );
    let prompt = task_prompt(&case.task)?;
    fs::write(artifact_root.join(&prompt_rel), prompt.as_bytes()).with_context(|| {
        format!(
            "failed to write {}",
            artifact_root.join(&prompt_rel).display()
        )
    })?;

    let run_result = match case.task.source_kind {
        TaskSourceKind::Harbor => run_harbor_container_case(
            &case,
            artifact_root,
            &logs_dir,
            artifact_includes,
            &mut events,
        ),
        TaskSourceKind::SweBench => Ok(container_case_failure(
            CaseStatus::SetupFailed,
            "SWE-bench container execution is not implemented yet",
            None,
        )),
        other => Ok(container_case_failure(
            CaseStatus::SetupFailed,
            &format!("source kind `{other:?}` does not support container execution"),
            None,
        )),
    };
    let (status, score, evaluator_stdout, evaluator_stderr) = match run_result {
        Ok(result) => result,
        Err(err) => container_case_failure(CaseStatus::SetupFailed, &format!("{err:#}"), None),
    };
    fs::write(artifact_root.join(&stdout_rel), evaluator_stdout.as_bytes()).with_context(|| {
        format!(
            "failed to write {}",
            artifact_root.join(&stdout_rel).display()
        )
    })?;
    fs::write(artifact_root.join(&stderr_rel), evaluator_stderr.as_bytes()).with_context(|| {
        format!(
            "failed to write {}",
            artifact_root.join(&stderr_rel).display()
        )
    })?;
    push_event(
        &mut events,
        &case.case_id,
        "case_finished",
        "container case execution finished",
        json!({ "status": status }),
    );
    let duration_ms = started.elapsed().as_millis();
    let observed = collect_case_observability(&events, duration_ms);
    let metrics = observed.metrics;
    let warnings = observed.warnings;
    write_jsonl(&artifact_root.join(&trajectory_rel), &events)?;

    Ok(CaseResult {
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
            result: result_rel,
            trajectory: trajectory_rel,
            evaluator_stdout: stdout_rel,
            evaluator_stderr: stderr_rel,
        },
    })
}

pub(crate) fn container_case_failure(
    status: CaseStatus,
    message: &str,
    outcome: Option<&ProcessOutcome>,
) -> (CaseStatus, ScoreResult, String, String) {
    let details = outcome
        .map(|outcome| {
            json!({
                "exit_code": outcome.code,
                "timed_out": outcome.timed_out,
                "stdout_bytes": outcome.stdout.len(),
                "stderr_bytes": outcome.stderr.len(),
            })
        })
        .unwrap_or(Value::Null);
    let score = ScoreResult {
        schema_version: EVALUATOR_RESULT_SCHEMA_VERSION,
        passed: false,
        score: Some(0.0),
        message: message.to_string(),
        details,
    };
    let stdout = outcome
        .map(|outcome| outcome.stdout.clone())
        .unwrap_or_default();
    let stderr = outcome
        .map(|outcome| {
            if outcome.stderr.trim().is_empty() {
                message.to_string()
            } else {
                outcome.stderr.clone()
            }
        })
        .unwrap_or_else(|| message.to_string());
    (status, score, stdout, stderr)
}

#[derive(Debug, Clone)]
pub(crate) struct HarborContainerEnvironment {
    pub(crate) docker_image: Option<String>,
    pub(crate) allow_internet: bool,
    pub(crate) build_timeout_seconds: u64,
    pub(crate) cpus: Option<f64>,
    pub(crate) memory_mb: Option<u64>,
    pub(crate) workdir: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ContainerRuntime {
    pub(crate) project_name: String,
    pub(crate) compose_path: PathBuf,
    pub(crate) workdir: String,
}
