#[allow(unused_imports)]
use super::*;

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
