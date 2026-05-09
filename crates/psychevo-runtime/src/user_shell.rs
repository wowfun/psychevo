use std::time::{Instant, SystemTime, UNIX_EPOCH};

use psychevo_ai::Outcome;
use serde_json::json;

use crate::error::{Error, Result};
use crate::paths::canonical_workdir;
use crate::tools::{default_bash_timeout_secs, run_bash_command};
use crate::types::{RunControl, RunStreamEvent, RunStreamSink, UserShellOptions, UserShellResult};

pub async fn run_user_shell_command_streaming_controlled(
    options: UserShellOptions,
    stream: RunStreamSink,
    control: RunControl,
) -> Result<UserShellResult> {
    let workdir = canonical_workdir(&options.workdir)?;
    let command = options.command;
    if command.trim().is_empty() {
        return Err(Error::Message("shell command is empty".to_string()));
    }

    let tool_call_id = "user_shell".to_string();
    stream(RunStreamEvent::Event(json!({
        "type": "tool_execution_start",
        "tool_call_id": tool_call_id,
        "tool_name": "bash",
        "args": {"command": command.clone()},
        "started_at_ms": now_ms(),
        "source": "user_shell",
    })));

    let started = Instant::now();
    let (result, is_error) = match run_bash_command(
        workdir.clone(),
        command.clone(),
        default_bash_timeout_secs(),
        control.receivers.abort_signal(),
    )
    .await
    {
        Ok((result, is_error)) => (result, is_error),
        Err(err) => (
            json!({
                "output": "(no output)",
                "exit_code": null,
                "error": err.to_string(),
                "exit_code_meaning": null,
                "truncated": false
            }),
            true,
        ),
    };

    let aborted = result.get("error").and_then(serde_json::Value::as_str) == Some("aborted");
    let outcome = if aborted {
        Outcome::Aborted
    } else if is_error {
        Outcome::Failed
    } else {
        Outcome::Normal
    };
    stream(RunStreamEvent::Event(json!({
        "type": "tool_execution_end",
        "tool_call_id": tool_call_id,
        "tool_name": "bash",
        "result": result.clone(),
        "outcome": outcome.as_str(),
        "elapsed_ms": started.elapsed().as_millis() as u64,
        "source": "user_shell",
    })));

    Ok(UserShellResult {
        command,
        workdir,
        outcome,
        tool_failures: usize::from(is_error && !aborted),
        result,
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}
