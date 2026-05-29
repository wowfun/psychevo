#[allow(unused_imports)]
use super::*;

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
