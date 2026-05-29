#[allow(unused_imports)]
use super::*;

pub(crate) fn run_harbor_container_case(
    case: &CasePlan,
    artifact_root: &Path,
    logs_dir: &Path,
    artifact_includes: &BTreeSet<String>,
    events: &mut Vec<TrajectoryEvent>,
) -> Result<(CaseStatus, ScoreResult, String, String)> {
    let environment = harbor_container_environment(&case.task)?;
    let runtime = prepare_harbor_compose(case, artifact_root, logs_dir, &environment)?;
    push_event(
        events,
        &case.case_id,
        "container_compose_prepared",
        "Docker Compose task environment prepared",
        json!({
            "project": runtime.project_name,
            "compose": "container/docker-compose.yml",
            "workdir": runtime.workdir,
            "image": environment.docker_image,
            "allow_internet": environment.allow_internet,
        }),
    );

    let up = docker_compose_command(&runtime, &["up", "-d", "--build"])?;
    let up = wait_for_command(
        up,
        Some(Duration::from_secs(environment.build_timeout_seconds)),
        artifact_root,
    )?;
    if !up.success {
        push_event(
            events,
            &case.case_id,
            "container_setup_failed",
            "Docker Compose failed to start task environment",
            json!({
                "exit_code": up.code,
                "timed_out": up.timed_out,
                "stdout_bytes": up.stdout.len(),
                "stderr_bytes": up.stderr.len(),
                "project": runtime.project_name,
            }),
        );
        return Ok(container_case_failure(
            CaseStatus::SetupFailed,
            "Docker Compose failed to start task environment",
            Some(&up),
        ));
    }

    let install_result = install_container_acp_agent(case, &runtime, artifact_root);
    if let Err(err) = install_result {
        push_event(
            events,
            &case.case_id,
            "agent_install_failed",
            "ACP agent installation in container failed",
            json!({ "error": format!("{err:#}"), "project": runtime.project_name }),
        );
        return Ok(container_case_failure(
            CaseStatus::RuntimeFailed,
            &format!("{err:#}"),
            None,
        ));
    }

    if let Err(err) = run_acp_agent_in_container(case, &runtime, logs_dir, artifact_root, events) {
        push_event(
            events,
            &case.case_id,
            "agent_failed",
            "container ACP agent failed",
            json!({ "error": format!("{err:#}"), "project": runtime.project_name }),
        );
        return Ok(container_case_failure(
            CaseStatus::RuntimeFailed,
            &format!("{err:#}"),
            None,
        ));
    }

    let tests_dir = case.task.dir.join("tests");
    copy_into_container(&runtime, &tests_dir, "/tests")?;
    let verifier_timeout = case
        .task
        .verifier_timeout_seconds
        .unwrap_or(default_agent_timeout_seconds());
    let verifier = docker_compose_exec_shell(
        &runtime,
        &runtime.workdir,
        &BTreeMap::from([
            ("PEVAL_WORKSPACE".to_string(), runtime.workdir.clone()),
            ("PEVAL_TASK_DIR".to_string(), "/task".to_string()),
            ("PEVAL_LOGS".to_string(), "/logs".to_string()),
            ("PEVAL_TASK_ID".to_string(), case.task.id.clone()),
            (
                "PEVAL_NATIVE_TASK_ID".to_string(),
                case.task.native_id.clone(),
            ),
            ("PEVAL_SOURCE_ID".to_string(), case.task.source_id.clone()),
        ]),
        "sh /tests/test.sh",
    )?;
    let verifier = wait_for_command(
        verifier,
        Some(Duration::from_secs(verifier_timeout)),
        artifact_root,
    )?;
    let status = if verifier.timed_out {
        CaseStatus::Timeout
    } else if verifier.success {
        CaseStatus::Passed
    } else {
        CaseStatus::Failed
    };
    let default_message = match status {
        CaseStatus::Passed => "verifier passed".to_string(),
        CaseStatus::Timeout => "verifier timed out".to_string(),
        _ => "verifier failed".to_string(),
    };
    let mut score = import_verifier_score(
        &logs_dir.join("verifier"),
        status,
        default_message,
        &verifier,
    )?;
    if status != CaseStatus::Passed {
        score.passed = false;
    }
    if score.score.is_none() {
        score.score = Some(if score.passed { 1.0 } else { 0.0 });
    }
    push_event(
        events,
        &case.case_id,
        "evaluator_finished",
        &score.message,
        json!({
            "status": status,
            "passed": score.passed,
            "project": runtime.project_name,
        }),
    );

    if artifact_includes.contains("workspace") {
        let retained_workspace = artifact_root.join("workspace");
        if retained_workspace.exists() {
            fs::remove_dir_all(&retained_workspace)
                .with_context(|| format!("failed to remove {}", retained_workspace.display()))?;
        }
        copy_from_container(&runtime, &runtime.workdir, &retained_workspace)
            .with_context(|| "failed to retain container workspace artifact")?;
    }

    if status == CaseStatus::Passed {
        let down = docker_compose_command(&runtime, &["down", "--volumes", "--remove-orphans"])?;
        let down = wait_for_command(down, Some(Duration::from_secs(120)), artifact_root)?;
        push_event(
            events,
            &case.case_id,
            "container_cleanup_finished",
            "Docker Compose task environment cleaned up",
            json!({
                "project": runtime.project_name,
                "exit_code": down.code,
                "success": down.success,
            }),
        );
    } else {
        push_event(
            events,
            &case.case_id,
            "container_retained",
            "failed Docker Compose task environment retained",
            json!({
                "project": runtime.project_name,
                "compose": "container/docker-compose.yml",
            }),
        );
    }
    let evaluator_stdout = serde_json::to_string(&score)?;
    Ok((status, score, evaluator_stdout, verifier.stderr))
}
