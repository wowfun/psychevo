#[allow(unused_imports)]
use super::*;

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
pub(crate) fn event_kind_suffix(value: &str) -> String {
    let normalized = sanitize_id(&value.to_ascii_lowercase());
    let trimmed = normalized.trim_matches('_');
    if trimmed.is_empty() {
        "event".to_string()
    } else {
        trimmed.to_string()
    }
}
